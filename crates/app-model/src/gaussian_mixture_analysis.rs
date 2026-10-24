//! Gaussian mixture clustering of several numeric columns, bound to the row
//! state ([`stat_multivariate::mixture`]).
//!
//! Rows complete in every chosen column and not excluded are clustered.
//! Every requested covariance structure is fitted with every number of
//! components in the requested range; the smallest BIC chooses the model.
//! The report shows the BIC table and plot, the chosen model's mixing
//! proportions, means and covariance matrices (in the columns' own units,
//! also when the columns were standardized for fitting), and the rows on
//! the first two columns marked by most likely component with each
//! component's 95% ellipse. Every point keeps its dataset row for brushing.
//! Membership probabilities, the most likely component and the
//! classification uncertainty can be saved as columns.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_multivariate::{
    CovarianceStructure, GaussianMixture, MixtureOptions, MixtureSelection, MixtureStart, select_gaussian_mixture,
};

use crate::format::format_number;
use crate::report::{
    Annotations, PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use crate::report_columns::ReportColumn;

const DIGITS: usize = 5;

/// Columns, candidate models and starts of a Gaussian mixture analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct GaussianMixtureRequest {
    /// Numeric columns (one or more).
    pub columns: Vec<usize>,
    /// Covariance structures compared (for one column only the spherical
    /// ones differ: EII is "equal variance", VII "unequal variance").
    pub structures: Vec<CovarianceStructure>,
    /// Smallest number of components tried.
    pub min_components: usize,
    /// Largest number of components tried.
    pub max_components: usize,
    /// Standardize the columns before fitting (results are reported in the
    /// original units).
    pub standardize: bool,
    /// Partition of the first EM start.
    pub start: MixtureStart,
    /// EM starts per model.
    pub starts: usize,
    /// Seed of the random starts.
    pub seed: u64,
}

impl GaussianMixtureRequest {
    /// Default largest number of components.
    pub const DEFAULT_MAX: usize = 6;

    /// All six structures, 1 to 6 components, unstandardized, 5 k-means
    /// starts, seed 1.
    #[must_use]
    pub fn new(columns: Vec<usize>) -> Self {
        Self {
            columns,
            structures: CovarianceStructure::ALL.to_vec(),
            min_components: 1,
            max_components: Self::DEFAULT_MAX,
            standardize: false,
            start: MixtureStart::KMeans,
            starts: 5,
            seed: 1,
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

    /// The project-file form (`"analysis": "gaussian-mixture"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        use crate::project::{obj, s};
        obj([
            ("analysis", s("gaussian-mixture")),
            ("columns", crate::correlation_analysis::columns_json(&self.columns, table)),
            ("structures", JsonValue::Array(self.structures.iter().map(|m| s(m.key())).collect())),
            ("min_components", JsonValue::Number(self.min_components as f64)),
            ("max_components", JsonValue::Number(self.max_components as f64)),
            ("standardize", JsonValue::Bool(self.standardize)),
            ("start", s(self.start.key())),
            ("starts", JsonValue::Number(self.starts as f64)),
            ("seed", JsonValue::Number(self.seed as f64)),
        ])
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, an unknown structure or start.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let d = Self::new(crate::correlation_analysis::json_columns(v, "columns", table)?);
        let structures = match v.get("structures").and_then(JsonValue::as_array) {
            None => d.structures.clone(),
            Some(list) => list
                .iter()
                .map(|m| {
                    m.as_str()
                        .and_then(CovarianceStructure::from_key)
                        .ok_or_else(|| invalid(format!("gaussian mixture: unknown covariance structure {m:?}")))
                })
                .collect::<Result<_>>()?,
        };
        let start = match v.get("start").and_then(JsonValue::as_str) {
            None => d.start,
            Some(k) => {
                MixtureStart::from_key(k).ok_or_else(|| invalid(format!("gaussian mixture: unknown start {k:?}")))?
            }
        };
        let usize_or = |key: &str, default: usize| v.get(key).and_then(JsonValue::as_usize).unwrap_or(default);
        Ok(Self {
            structures,
            min_components: usize_or("min_components", d.min_components),
            max_components: usize_or("max_components", d.max_components),
            standardize: v.get("standardize").and_then(JsonValue::as_bool).unwrap_or(d.standardize),
            start,
            starts: usize_or("starts", d.starts),
            seed: v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(1),
            ..d
        })
    }
}

/// A computed Gaussian mixture analysis.
#[derive(Debug, Clone)]
pub struct GaussianMixtureAnalysis {
    request: GaussianMixtureRequest,
    names: Vec<String>,
    n_rows: usize,
    rows: Vec<usize>,
    /// The analysed data (original units).
    data: Vec<Vec<f64>>,
    selection: MixtureSelection,
    /// Chosen means and covariances in the original units.
    means: Vec<Vec<f64>>,
    covariances: Vec<Vec<Vec<f64>>>,
    /// `n Σ ln sⱼ` when the columns were standardized for fitting (else 0):
    /// the log-likelihood in the original units is the fitted one less this.
    log_scale: f64,
    n_excluded: usize,
    n_missing: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

/// Numeric columns' values on the rows complete in all of them and not
/// excluded: names, rows, data, excluded and missing counts.
pub(crate) type RowData = (Vec<String>, Vec<usize>, Vec<Vec<f64>>, usize, usize);

/// See [`RowData`].
pub(crate) fn row_data(table: &DataTable, states: &RowStateManager, columns: &[usize]) -> Result<RowData> {
    let mut names = Vec::new();
    let mut values = Vec::new();
    for &i in columns {
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
    let data = rows.iter().map(|&r| values.iter().map(|v| v[r]).collect()).collect();
    Ok((names, rows, data, n_excluded, n_missing))
}

/// Column means and SDs (divisor `n − 1`).
fn moments(data: &[Vec<f64>], p: usize) -> (Vec<f64>, Vec<f64>) {
    let n = data.len() as f64;
    let mean: Vec<f64> = (0..p).map(|j| data.iter().map(|r| r[j]).sum::<f64>() / n).collect();
    let sd = (0..p).map(|j| (data.iter().map(|r| (r[j] - mean[j]).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()).collect();
    (mean, sd)
}

impl GaussianMixtureAnalysis {
    /// Fit every candidate model and choose by BIC.
    ///
    /// # Errors
    /// No columns or structures, a missing or non-numeric column, an
    /// invalid range of components (at least 1, at most the number of
    /// rows), a constant column when standardizing, no starts, or every fit
    /// failing.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &GaussianMixtureRequest) -> Result<Self> {
        if request.columns.is_empty() {
            return Err(invalid("choose at least one column"));
        }
        if request.structures.is_empty() {
            return Err(invalid("choose at least one covariance structure"));
        }
        if request.min_components == 0 || request.min_components > request.max_components {
            return Err(invalid("the numbers of components must run from at least 1 up to a larger or equal number"));
        }
        if request.starts == 0 {
            return Err(invalid("use at least one start"));
        }
        let (names, rows, data, n_excluded, n_missing) = row_data(table, states, &request.columns)?;
        let p = names.len();
        if rows.len() < request.max_components.max(2) {
            return Err(StatError::InsufficientData { required: request.max_components.max(2), found: rows.len() });
        }
        let (mean, sd) = moments(&data, p);
        let work = if request.standardize {
            stat_multivariate::standardize_columns(&data).map_err(|_| {
                let j = sd.iter().position(|s| s.is_nan() || *s <= 0.0).unwrap_or(0);
                invalid(format!("column {:?} is constant, so it cannot be standardized", names[j]))
            })?
        } else {
            data.clone()
        };
        // A small ridge keeps covariance estimates positive definite: 10⁻⁶
        // of the average variance of the fitted columns.
        let scale = if request.standardize { 1.0 } else { sd.iter().map(|s| s * s).sum::<f64>() / p as f64 };
        let base = MixtureOptions {
            starts: request.starts,
            start: request.start,
            seed: request.seed,
            ridge: 1e-6 * scale.max(f64::MIN_POSITIVE),
            ..MixtureOptions::new(1, CovarianceStructure::Vvv)
        };
        let ks: Vec<usize> = (request.min_components..=request.max_components).collect();
        // With one column the diagonal and full structures equal the spherical ones.
        let mut structures: Vec<CovarianceStructure> = request.structures.clone();
        if p == 1 {
            structures.retain(|s| matches!(s, CovarianceStructure::Eii | CovarianceStructure::Vii));
            if structures.is_empty() {
                structures = vec![CovarianceStructure::Eii, CovarianceStructure::Vii];
            }
        }
        let selection = select_gaussian_mixture(&work, &ks, &structures, &base)?;
        let chosen = selection.chosen();
        let (means, covariances) = if request.standardize {
            (
                chosen.means.iter().map(|m| m.iter().enumerate().map(|(j, v)| mean[j] + sd[j] * v).collect()).collect(),
                chosen
                    .covariances
                    .iter()
                    .map(|c| (0..p).map(|a| (0..p).map(|b| c[a][b] * sd[a] * sd[b]).collect()).collect())
                    .collect(),
            )
        } else {
            (chosen.means.clone(), chosen.covariances.clone())
        };
        let log_scale =
            if request.standardize { rows.len() as f64 * sd.iter().map(|s| s.ln()).sum::<f64>() } else { 0.0 };
        Ok(Self {
            request: request.clone(),
            names,
            n_rows: table.n_rows(),
            rows,
            data,
            selection,
            means,
            covariances,
            log_scale,
            n_excluded,
            n_missing,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &GaussianMixtureRequest {
        &self.request
    }

    /// The chosen mixture (in the fitting units).
    #[must_use]
    pub fn chosen(&self) -> &GaussianMixture {
        self.selection.chosen()
    }

    /// Every candidate fitted.
    #[must_use]
    pub fn selection(&self) -> &MixtureSelection {
        &self.selection
    }

    /// Component means in the columns' units.
    #[must_use]
    pub fn means(&self) -> &[Vec<f64>] {
        &self.means
    }

    /// Report title, e.g. `"Gaussian Mixture (VVV, 3 components): x, y"`.
    #[must_use]
    pub fn title(&self) -> String {
        let c = self.chosen();
        format!(
            "Gaussian Mixture ({}, {}): {}",
            c.structure.key(),
            if c.k() == 1 { "1 component".to_owned() } else { format!("{} components", c.k()) },
            self.names.join(", ")
        )
    }

    /// Log-likelihood of the chosen model in the columns' own units (also
    /// when they were standardized for fitting: the density of the original
    /// values is that of the standardized ones divided by `Π sⱼ`).
    #[must_use]
    pub fn log_likelihood(&self) -> f64 {
        self.chosen().log_likelihood - self.log_scale
    }

    /// A candidate's BIC in the columns' own units (see [`Self::log_likelihood`]).
    fn bic_of(&self, c: &stat_multivariate::MixtureCandidate) -> Option<f64> {
        c.bic().map(|b| b + 2.0 * self.log_scale)
    }

    /// Dataset rows clustered, in order.
    #[must_use]
    pub fn rows(&self) -> &[usize] {
        &self.rows
    }

    fn per_row(&self, values: impl Iterator<Item = f64>) -> Vec<f64> {
        let mut out = vec![f64::NAN; self.n_rows];
        for (v, &r) in values.zip(&self.rows) {
            out[r] = v;
        }
        out
    }

    /// Columns the report can add: `probability_1`, `probability_2`, …,
    /// `cluster` (most likely component, nominal) and `uncertainty`.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let c = self.chosen();
        let mut out: Vec<ReportColumn> = (0..c.k())
            .map(|k| {
                ReportColumn::continuous(
                    &format!("probability_{}", k + 1),
                    &format!("Save probability of component {}", k + 1),
                    format!("Probability component {}", k + 1),
                    self.per_row(c.probabilities.iter().map(|z| z[k])),
                )
            })
            .collect();
        let mut cluster = ReportColumn::continuous(
            "cluster",
            "Save most likely component",
            "Mixture component".to_owned(),
            self.per_row(c.classification().into_iter().map(|k| (k + 1) as f64)),
        );
        cluster.level = MeasurementLevel::Nominal;
        out.push(cluster);
        out.push(ReportColumn::continuous(
            "uncertainty",
            "Save classification uncertainty",
            "Mixture uncertainty".to_owned(),
            self.per_row(c.uncertainty().into_iter()),
        ));
        out
    }

    /// Report sections: summary, BIC table, BIC plot, components, covariance
    /// matrices, cluster plot.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![
            self.summary(),
            self.bic_table(),
            self.bic_plot(),
            self.components(),
            self.covariance_table(),
            self.cluster_plot(),
        ]
    }

    fn structures(&self) -> Vec<CovarianceStructure> {
        let mut out: Vec<CovarianceStructure> = Vec::new();
        for c in &self.selection.candidates {
            if !out.contains(&c.structure) {
                out.push(c.structure);
            }
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let c = self.chosen();
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        push("Columns", self.names.join(", "));
        push("Rows clustered", self.rows.len().to_string());
        if self.n_excluded > 0 {
            push("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            push("Rows with a missing value", self.n_missing.to_string());
        }
        push("Chosen covariance structure", format!("{} ({})", c.structure.key(), c.structure.label()));
        push("Components", c.k().to_string());
        push("Log-likelihood", format_number(self.log_likelihood(), 8));
        push("Parameters", c.n_parameters().to_string());
        push("BIC (smaller is better)", format_number(c.bic() + 2.0 * self.log_scale, 8));
        let failed = self.selection.candidates.iter().filter(|m| m.fit.is_err()).count();
        push("Models compared", format!("{} ({} could not be fitted)", self.selection.candidates.len(), failed));
        push(
            "Starts per model",
            format!("{} (first from {}; seed {})", self.request.starts, self.request.start.label(), self.request.seed),
        );
        if self.request.standardize {
            push("Columns standardized for fitting", "yes (estimates shown in the original units)".to_owned());
        }
        let uncertain = c.uncertainty().iter().filter(|u| **u > 0.2).count();
        push("Rows with uncertainty above 0.2", uncertain.to_string());
        if !c.converged {
            push("Note", "EM stopped at its iteration limit before converging".to_owned());
        }
        section(SectionKind::GaussianMixtureSummary, "Chosen Model", SectionContent::Table(t))
    }

    fn bic_table(&self) -> ReportSection {
        let structures = self.structures();
        let mut header = vec!["Components".to_owned()];
        header.extend(structures.iter().map(|s| s.key().to_owned()));
        let mut t = TextTable::new(header);
        for k in self.request.min_components..=self.request.max_components {
            let mut row = vec![k.to_string()];
            for s in &structures {
                let pos = self.selection.candidates.iter().position(|c| c.k == k && c.structure == *s);
                let cell = match pos {
                    Some(i) => match self.bic_of(&self.selection.candidates[i]) {
                        Some(b) if i == self.selection.best => format!("{} (chosen)", format_number(b, 7)),
                        Some(b) => format_number(b, 7),
                        None => "not fitted".to_owned(),
                    },
                    None => String::new(),
                };
                row.push(cell);
            }
            t.push(row);
        }
        let keys: Vec<String> =
            structures.iter().map(|s| format!("{} {}", s.key(), s.label().to_lowercase())).collect();
        let title = format!("BIC by Model ({})", keys.join("; "));
        section(SectionKind::GaussianMixtureBic, &title, SectionContent::Table(t))
    }

    fn bic_plot(&self) -> ReportSection {
        let structures = self.structures();
        let labels: Vec<String> = structures.iter().map(|s| s.key().to_owned()).collect();
        let lines: Vec<Polyline> = structures
            .iter()
            .map(|s| {
                self.selection
                    .candidates
                    .iter()
                    .filter(|c| c.structure == *s)
                    .filter_map(|c| self.bic_of(c).map(|b| [c.k as f64, b]))
                    .collect()
            })
            .collect();
        let mut items: Vec<(f64, f64, String, usize)> = Vec::new();
        for (g, line) in lines.iter().enumerate() {
            for &[x, y] in line {
                items.push((x, y, String::new(), g));
            }
        }
        if let Some(chosen) = self.selection.candidates.get(self.selection.best) {
            if let (Some(b), Some(g)) = (self.bic_of(chosen), structures.iter().position(|s| *s == chosen.structure)) {
                items.push((chosen.k as f64, b, "chosen".to_owned(), g));
            }
        }
        let ks = self.request.min_components..=self.request.max_components;
        let plot = PlotSpec {
            x_label: "Components".to_owned(),
            y_label: "BIC".to_owned(),
            groups: Some(PointGroups {
                title: "Covariance structure".to_owned(),
                labels: labels.clone(),
                membership: Vec::new(),
                lines,
            }),
            annotations: Some(Annotations { groups: labels, items, labels: Vec::new() }),
            x_ticks: Some(ks.map(|k| (k as f64, k.to_string())).collect()),
            ..PlotSpec::default()
        };
        section(
            SectionKind::GaussianMixtureBicPlot,
            "BIC by Number of Components (smaller is better)",
            SectionContent::Plot(Box::new(plot)),
        )
    }

    fn components(&self) -> ReportSection {
        let c = self.chosen();
        let class = c.classification();
        let mut header = vec!["Component".to_owned(), "Proportion".to_owned(), "Rows (most likely)".to_owned()];
        header.extend(self.names.iter().map(|n| format!("Mean {n}")));
        let mut t = TextTable::new(header);
        for k in 0..c.k() {
            let mut row = vec![
                (k + 1).to_string(),
                format_number(c.weights[k], 4),
                class.iter().filter(|&&m| m == k).count().to_string(),
            ];
            row.extend(self.means[k].iter().map(|v| format_number(*v, DIGITS)));
            t.push(row);
        }
        section(SectionKind::GaussianMixtureComponents, "Components", SectionContent::Table(t))
    }

    fn covariance_table(&self) -> ReportSection {
        let c = self.chosen();
        let mut header = vec!["Component".to_owned(), "Column".to_owned()];
        header.extend(self.names.iter().map(|n| format!("Cov. with {n}")));
        let mut t = TextTable::new(header);
        let shared = c.structure.shared();
        let shown = if shared { 1 } else { c.k() };
        for k in 0..shown {
            for (a, name) in self.names.iter().enumerate() {
                let label = if shared { "All (shared)".to_owned() } else { (k + 1).to_string() };
                let mut row = vec![if a == 0 { label } else { String::new() }, name.clone()];
                row.extend(self.covariances[k][a].iter().map(|v| format_number(*v, DIGITS)));
                t.push(row);
            }
        }
        let title = format!("Covariance Matrices ({})", c.structure.label().to_lowercase());
        section(SectionKind::GaussianMixtureCovariances, &title, SectionContent::Table(t))
    }

    fn cluster_plot(&self) -> ReportSection {
        let c = self.chosen();
        let class = c.classification();
        let k = c.k();
        let two = self.names.len() >= 2;
        let points: Vec<RowPoint> =
            self.rows
                .iter()
                .zip(&self.data)
                .map(|(&row, d)| {
                    if two {
                        RowPoint { row, x: d[0], y: d[1] }
                    } else {
                        RowPoint { row, x: (row + 1) as f64, y: d[0] }
                    }
                })
                .collect();
        // 95% ellipse of each component on the first two columns.
        let lines: Vec<Polyline> = (0..k)
            .map(|m| {
                if !two {
                    return Vec::new();
                }
                let s = &self.covariances[m];
                ellipse(self.means[m][0], self.means[m][1], s[0][0], s[0][1], s[1][1], CHI2_2_95)
            })
            .collect();
        let (x_label, y_label) = if two {
            (self.names[0].clone(), self.names[1].clone())
        } else {
            ("Row".to_owned(), self.names[0].clone())
        };
        let plot = PlotSpec {
            x_label,
            y_label,
            points,
            groups: Some(PointGroups {
                title: "Component".to_owned(),
                labels: (1..=k).map(|m| m.to_string()).collect(),
                membership: class,
                lines,
            }),
            ..PlotSpec::default()
        };
        let title = if two {
            "Rows by Most Likely Component (with each component's 95% ellipse)"
        } else {
            "Rows by Most Likely Component"
        };
        section(SectionKind::GaussianMixtureClusterPlot, title, SectionContent::Plot(Box::new(plot)))
    }
}

/// Upper 5% point of χ² on 2 degrees of freedom, `−2 ln 0.05`.
const CHI2_2_95: f64 = 5.991_464_547_107_979;

/// The ellipse `(x − μ)ᵀ Σ⁻¹ (x − μ) = c` as a closed polyline.
fn ellipse(mx: f64, my: f64, sxx: f64, sxy: f64, syy: f64, c: f64) -> Polyline {
    // Eigen-decomposition of the 2 × 2 covariance.
    let tr = 0.5 * (sxx + syy);
    let det = sxx * syy - sxy * sxy;
    let disc = (tr * tr - det).max(0.0).sqrt();
    let (l1, l2) = ((tr + disc).max(0.0), (tr - disc).max(0.0));
    let angle = if sxy.abs() < 1e-300 {
        if sxx >= syy { 0.0 } else { std::f64::consts::FRAC_PI_2 }
    } else {
        (l1 - sxx).atan2(sxy)
    };
    let (ca, sa) = (angle.cos(), angle.sin());
    let (r1, r2) = ((c * l1).sqrt(), (c * l2).sqrt());
    (0..=72)
        .map(|i| {
            let t = f64::from(i) * std::f64::consts::TAU / 72.0;
            let (u, v) = (r1 * t.cos(), r2 * t.sin());
            [mx + u * ca - v * sa, my + u * sa + v * ca]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> crate::sample::Sample {
        crate::sample::catalog()
            .into_iter()
            .find(|s| s.name.starts_with("Cell populations"))
            .map(|s| (s.build)())
            .unwrap()
    }

    #[test]
    fn chooses_a_model_and_saves_columns() {
        let s = sample();
        let t = &s.table;
        let cols = vec![t.find("CD4 marker").unwrap(), t.find("CD8 marker").unwrap()];
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([0], true);
        let req = GaussianMixtureRequest { max_components: 5, ..GaussianMixtureRequest::new(cols.clone()) };
        let a = GaussianMixtureAnalysis::run(t, &rows, &req).unwrap();
        // Three elongated populations of unequal shape: full, varying covariances.
        assert_eq!(a.chosen().k(), 3);
        assert_eq!(a.chosen().structure, CovarianceStructure::Vvv);
        let s = a.sections();
        assert_eq!(s.len(), 6);
        let SectionContent::Table(bic) = &s[1].content else { panic!("table") };
        assert_eq!(bic.rows.len(), 5);
        assert!(bic.rows[2].iter().any(|c| c.ends_with("(chosen)")));
        let SectionContent::Plot(p) = &s[5].content else { panic!("plot") };
        assert_eq!(p.points.len(), t.n_rows() - 1);
        assert!(p.points.iter().all(|q| q.row != 0));
        let g = p.groups.as_ref().unwrap();
        assert_eq!(g.lines.len(), 3);
        assert!(g.lines.iter().all(|l| l.len() == 73));
        let saved = a.saveable_columns();
        let keys: Vec<&str> = saved.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["probability_1", "probability_2", "probability_3", "cluster", "uncertainty"]);
        assert!(saved[0].values[0].is_nan());
        let r = 5;
        let sum: f64 = (0..3).map(|k| saved[k].values[r]).sum();
        assert!((sum - 1.0).abs() < 1e-12);
        // Standardizing reports the same model in the original units.
        let st = GaussianMixtureAnalysis::run(t, &rows, &GaussianMixtureRequest { standardize: true, ..req.clone() })
            .unwrap();
        assert_eq!(st.chosen().k(), 3);
        // ... with the log-likelihood and BIC of the original values.
        let ll = |x: &GaussianMixtureAnalysis| x.log_likelihood();
        assert!((ll(&st) - ll(&a)).abs() < 1e-5 * ll(&a).abs(), "{} vs {}", ll(&st), ll(&a));
        let cell = |x: &GaussianMixtureAnalysis, label: &str| -> f64 {
            let SectionContent::Table(t) = &x.sections()[0].content else { panic!() };
            t.rows.iter().find(|r| r[0] == label).unwrap()[1].replace('−', "-").parse().unwrap()
        };
        for label in ["Log-likelihood", "BIC (smaller is better)"] {
            let (u, v) = (cell(&st, label), cell(&a, label));
            assert!((u - v).abs() < 1e-5 * v.abs(), "{label}: {u} vs {v}");
        }
        for (m, n) in st.means().iter().zip(a.means()) {
            for (u, v) in m.iter().zip(n) {
                assert!((u - v).abs() < 1e-3 * (1.0 + v.abs()), "{u} vs {v}");
            }
        }
    }

    #[test]
    fn one_column_json_and_errors() {
        let s = sample();
        let t = &s.table;
        let rows = RowStateManager::new(t.n_rows());
        let c = t.find("CD4 marker").unwrap();
        let req = GaussianMixtureRequest { max_components: 3, ..GaussianMixtureRequest::new(vec![c]) };
        let a = GaussianMixtureAnalysis::run(t, &rows, &req).unwrap();
        assert!(matches!(a.chosen().structure, CovarianceStructure::Eii | CovarianceStructure::Vii));
        assert_eq!(a.selection().candidates.len(), 6);
        let full = GaussianMixtureRequest {
            structures: vec![CovarianceStructure::Eee, CovarianceStructure::Vvi],
            min_components: 2,
            max_components: 4,
            standardize: true,
            start: MixtureStart::Hierarchical,
            starts: 3,
            seed: 7,
            ..GaussianMixtureRequest::new(vec![c, c + 1])
        };
        let back = GaussianMixtureRequest::from_json(&full.to_json(t), t).unwrap();
        assert_eq!(back, full);
        assert!(GaussianMixtureAnalysis::run(t, &rows, &GaussianMixtureRequest::new(vec![])).is_err());
        let bad = GaussianMixtureRequest { min_components: 3, max_components: 2, ..req.clone() };
        assert!(GaussianMixtureAnalysis::run(t, &rows, &bad).is_err());
        assert!(
            GaussianMixtureAnalysis::run(t, &rows, &GaussianMixtureRequest::new(vec![t.find("Population").unwrap()]))
                .is_err()
        );
    }

    #[test]
    fn ellipse_of_a_diagonal_covariance() {
        let e = ellipse(1.0, 2.0, 4.0, 0.0, 1.0, 1.0);
        assert!((e[0][0] - 3.0).abs() < 1e-12 && (e[0][1] - 2.0).abs() < 1e-12);
        let top = e.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        assert!((top - 3.0).abs() < 1e-3);
    }
}
