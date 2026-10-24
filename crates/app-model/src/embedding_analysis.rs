//! Dimension reduction of rows into a low-dimensional map, bound to the row
//! state: classical and metric (SMACOF) multidimensional scaling, t-SNE and
//! UMAP ([`stat_multivariate::mds`], [`stat_multivariate::tsne`],
//! [`stat_multivariate::umap`]).
//!
//! The objects are the dataset rows. Their dissimilarities are either the
//! Euclidean distances between their values in the chosen numeric columns
//! (optionally standardized), or a distance matrix stored as columns: with
//! `n` chosen columns the table has `n` rows, and row `i` / column `i` are
//! the same object. Excluded rows (and their matching columns) are left
//! out. The report shows the fit, the map (points keep their dataset rows
//! for brushing, optionally coloured and marked by a column) and, for the
//! scaling methods, a Shepard diagram; the coordinates can be saved as
//! columns. Every method is seeded and reproducible.

use stat_core::rng::Rng;
use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_multivariate::{
    Proximity, SmacofOptions, TsneOptions, UmapOptions, classical_scaling, metric_scaling, standardize_columns,
    trustworthiness, tsne, umap,
};

use crate::format::format_number;
use crate::report::{
    Annotations, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use crate::report_columns::ReportColumn;

/// Largest number of objects for metric scaling (it keeps the full distance matrix).
pub const SCALING_LIMIT: usize = 3000;

/// Most pairs drawn in a Shepard diagram.
const SHEPARD_PAIRS: usize = 1500;

/// Which embedding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmbeddingMethod {
    /// Classical (Torgerson) multidimensional scaling.
    ClassicalScaling,
    /// Metric multidimensional scaling by SMACOF.
    MetricScaling,
    /// t-distributed stochastic neighbour embedding.
    #[default]
    Tsne,
    /// Uniform manifold approximation and projection.
    Umap,
}

impl EmbeddingMethod {
    /// Every method, in menu order.
    pub const ALL: [Self; 4] = [Self::ClassicalScaling, Self::MetricScaling, Self::Tsne, Self::Umap];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ClassicalScaling => "Classical scaling",
            Self::MetricScaling => "Metric scaling (SMACOF)",
            Self::Tsne => "t-SNE",
            Self::Umap => "UMAP",
        }
    }

    /// Prefix of saved coordinate columns, e.g. `"t-SNE"`.
    #[must_use]
    pub fn short(self) -> &'static str {
        match self {
            Self::ClassicalScaling | Self::MetricScaling => "MDS",
            Self::Tsne => "t-SNE",
            Self::Umap => "UMAP",
        }
    }

    /// Stable key used in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::ClassicalScaling => "classical-mds",
            Self::MetricScaling => "metric-mds",
            Self::Tsne => "t-sne",
            Self::Umap => "umap",
        }
    }

    /// The method with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }

    /// Whether this is a scaling method (stress and Shepard diagram).
    #[must_use]
    pub fn scaling(self) -> bool {
        matches!(self, Self::ClassicalScaling | Self::MetricScaling)
    }
}

/// Method, input and settings of an embedding.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingRequest {
    /// The method.
    pub method: EmbeddingMethod,
    /// Numeric columns: the variables (rows input) or the distance matrix.
    pub columns: Vec<usize>,
    /// The columns hold a distance matrix rather than variables.
    pub distances: bool,
    /// Standardize the variables before computing distances.
    pub standardize: bool,
    /// Dimensions of the map (1–3).
    pub dims: usize,
    /// t-SNE perplexity.
    pub perplexity: f64,
    /// UMAP neighbourhood size.
    pub neighbours: usize,
    /// UMAP minimum distance.
    pub min_dist: f64,
    /// Iterations (t-SNE), epochs (UMAP) or most iterations (SMACOF);
    /// `None` for the method's default (1000, automatic, 300).
    pub iterations: Option<usize>,
    /// Seed of the random parts.
    pub seed: u64,
    /// Column that colours and marks the points (`None`: plain).
    pub color: Option<usize>,
}

impl EmbeddingRequest {
    /// `method` on standardized variables `columns`, two dimensions,
    /// perplexity 30, 15 neighbours, minimum distance 0.1, default
    /// iterations, seed 1.
    #[must_use]
    pub fn new(method: EmbeddingMethod, columns: Vec<usize>) -> Self {
        Self {
            method,
            columns,
            distances: false,
            standardize: true,
            dims: 2,
            perplexity: 30.0,
            neighbours: 15,
            min_dist: 0.1,
            iterations: None,
            seed: 1,
            color: None,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.columns.iter().copied().chain(self.color).collect()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.columns.iter_mut().for_each(|c| *c = f(*c));
        self.color = self.color.map(f);
    }

    /// The project-file form (`"analysis": "embedding"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        use crate::project::{obj, s};
        let name = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        obj([
            ("analysis", s("embedding")),
            ("method", s(self.method.key())),
            ("columns", crate::correlation_analysis::columns_json(&self.columns, table)),
            ("distances", JsonValue::Bool(self.distances)),
            ("standardize", JsonValue::Bool(self.standardize)),
            ("dimensions", JsonValue::Number(self.dims as f64)),
            ("perplexity", JsonValue::number(self.perplexity)),
            ("neighbours", JsonValue::Number(self.neighbours as f64)),
            ("min_dist", JsonValue::number(self.min_dist)),
            ("iterations", self.iterations.map_or(JsonValue::Null, |i| JsonValue::Number(i as f64))),
            ("seed", JsonValue::Number(self.seed as f64)),
            ("color", self.color.map_or(JsonValue::Null, name)),
        ])
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, an unknown method.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let method = match v.get("method").and_then(JsonValue::as_str) {
            None => EmbeddingMethod::default(),
            Some(k) => {
                EmbeddingMethod::from_key(k).ok_or_else(|| invalid(format!("embedding: unknown method {k:?}")))?
            }
        };
        let d = Self::new(method, crate::correlation_analysis::json_columns(v, "columns", table)?);
        let color = match v.get("color") {
            None | Some(JsonValue::Null) => None,
            Some(c) => {
                let name = c.as_str().ok_or_else(|| invalid("embedding: \"color\" must be a column name"))?;
                Some(table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))?)
            }
        };
        let num = |key: &str, default: f64| v.get(key).and_then(JsonValue::as_f64).unwrap_or(default);
        let int = |key: &str, default: usize| v.get(key).and_then(JsonValue::as_usize).unwrap_or(default);
        Ok(Self {
            distances: v.get("distances").and_then(JsonValue::as_bool).unwrap_or(d.distances),
            standardize: v.get("standardize").and_then(JsonValue::as_bool).unwrap_or(d.standardize),
            dims: int("dimensions", d.dims),
            perplexity: num("perplexity", d.perplexity),
            neighbours: int("neighbours", d.neighbours),
            min_dist: num("min_dist", d.min_dist),
            iterations: v.get("iterations").and_then(JsonValue::as_usize),
            seed: v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(1),
            color,
            ..d
        })
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

/// How well the map fits, by method.
#[derive(Debug, Clone, PartialEq)]
enum Fit {
    Classical {
        eigenvalues: Vec<f64>,
        proportion: f64,
    },
    Metric {
        stress1: f64,
        iterations: usize,
        converged: bool,
        start: usize,
    },
    Tsne {
        kl: f64,
        method: stat_multivariate::TsneMethod,
        rate: f64,
        iterations: usize,
        /// Iterations with early exaggeration, and its factor.
        exaggerated: usize,
        exaggeration: f64,
    },
    Umap {
        a: f64,
        b: f64,
        edges: usize,
        epochs: usize,
        spectral: bool,
    },
}

/// A computed embedding.
#[derive(Debug, Clone)]
pub struct EmbeddingAnalysis {
    request: EmbeddingRequest,
    names: Vec<String>,
    n_rows: usize,
    /// Dataset row of each object.
    rows: Vec<usize>,
    coordinates: Vec<Vec<f64>>,
    fit: Fit,
    /// Trustworthiness and its neighbourhood size.
    trust: Option<(f64, usize)>,
    /// Shepard diagram pairs: (dissimilarity, map distance).
    shepard: Vec<(f64, f64)>,
    n_excluded: usize,
    n_missing: usize,
    /// Colour groups: labels and the group of each object.
    groups: Option<(String, Vec<String>, Vec<usize>)>,
}

impl EmbeddingAnalysis {
    /// Compute the embedding.
    ///
    /// # Errors
    /// No columns, a missing or non-numeric column, a distance matrix whose
    /// size differs from the number of rows or that has a missing value,
    /// too few rows, more than [`SCALING_LIMIT`] rows for metric scaling, a
    /// constant column when standardizing, and the methods' own errors
    /// (perplexity at least the number of rows less one, too many
    /// neighbours, invalid settings).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &EmbeddingRequest) -> Result<Self> {
        if request.columns.is_empty() {
            return Err(invalid("choose at least one column"));
        }
        if !(1..=3).contains(&request.dims) {
            return Err(invalid("the map can have 1, 2 or 3 dimensions"));
        }
        let (names, rows, data, n_excluded, n_missing, distance_matrix) = if request.distances {
            let n = request.columns.len();
            if table.n_rows() != n {
                return Err(invalid(format!(
                    "a distance matrix of {n} columns needs {n} rows (one per object), but the table has {}",
                    table.n_rows()
                )));
            }
            let mut names = Vec::new();
            let mut values = Vec::new();
            for &i in &request.columns {
                let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
                values.push(c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?);
                names.push(c.name.clone());
            }
            if let Some((r, c)) =
                (0..n).flat_map(|r| (0..n).map(move |c| (r, c))).find(|&(r, c)| !values[c][r].is_finite())
            {
                return Err(invalid(format!("the distance matrix has no value in row {}, column {}", r + 1, names[c])));
            }
            let rows: Vec<usize> = (0..n).filter(|&r| !states.is_excluded(r)).collect();
            let matrix: Vec<Vec<f64>> = rows.iter().map(|&r| rows.iter().map(|&c| values[c][r]).collect()).collect();
            (names, rows.clone(), Vec::new(), n - rows.len(), 0, Some(matrix))
        } else {
            let (names, rows, data, ex, miss) =
                crate::gaussian_mixture_analysis::row_data(table, states, &request.columns)?;
            let data = if request.standardize {
                standardize_columns(&data).map_err(|_| {
                    let j = (0..names.len()).find(|&j| data.iter().all(|r| r[j] == data[0][j])).unwrap_or_default();
                    invalid(format!("column {:?} is constant, so it cannot be standardized", names[j]))
                })?
            } else {
                data
            };
            (names, rows, data, ex, miss, None)
        };
        let n = rows.len();
        if n < 4 {
            return Err(StatError::InsufficientData { required: 4, found: n });
        }
        let proximity = match &distance_matrix {
            Some(m) => Proximity::Distances(m),
            None => Proximity::Points(&data),
        };
        proximity.validate(4)?;
        let (coordinates, fit) = match request.method {
            EmbeddingMethod::ClassicalScaling => {
                let c = classical_scaling(&proximity, request.dims)?;
                let proportion = c.proportion();
                let mut eigenvalues = c.eigenvalues;
                eigenvalues.truncate(n);
                (c.coordinates, Fit::Classical { eigenvalues, proportion })
            }
            EmbeddingMethod::MetricScaling => {
                if n > SCALING_LIMIT {
                    return Err(invalid(format!(
                        "metric scaling handles up to {SCALING_LIMIT} rows; use t-SNE or UMAP for {n}"
                    )));
                }
                let mut o = SmacofOptions { seed: request.seed, ..SmacofOptions::new(request.dims) };
                if let Some(i) = request.iterations {
                    o.max_iterations = i;
                }
                let s = metric_scaling(&proximity, &o)?;
                let fit = Fit::Metric {
                    stress1: s.stress1,
                    iterations: s.iterations,
                    converged: s.converged,
                    start: s.start,
                };
                (s.coordinates, fit)
            }
            EmbeddingMethod::Tsne => {
                let mut o = TsneOptions {
                    dims: request.dims,
                    perplexity: request.perplexity,
                    seed: request.seed,
                    ..TsneOptions::new()
                };
                if let Some(i) = request.iterations {
                    o.iterations = i;
                    o.exaggeration_iterations = o.exaggeration_iterations.min(i / 4);
                }
                if request.perplexity >= (n - 1) as f64 {
                    return Err(invalid(format!(
                        "the perplexity ({}) must be below the number of rows less one ({})",
                        format_number(request.perplexity, 4),
                        n - 1
                    )));
                }
                let t = tsne(&proximity, &o)?;
                let fit = Fit::Tsne {
                    kl: t.kl_divergence,
                    method: t.method,
                    rate: t.learning_rate,
                    iterations: o.iterations,
                    exaggerated: o.exaggeration_iterations.min(o.iterations),
                    exaggeration: o.exaggeration,
                };
                (t.coordinates, fit)
            }
            EmbeddingMethod::Umap => {
                let o = UmapOptions {
                    dims: request.dims,
                    neighbours: request.neighbours,
                    min_dist: request.min_dist,
                    epochs: request.iterations,
                    seed: request.seed,
                    ..UmapOptions::new()
                };
                if request.neighbours > n {
                    return Err(invalid(format!(
                        "the number of neighbours ({}) cannot exceed the number of rows ({n})",
                        request.neighbours
                    )));
                }
                let u = umap(&proximity, &o)?;
                let fit = Fit::Umap { a: u.a, b: u.b, edges: u.edges, epochs: u.epochs, spectral: u.spectral };
                (u.coordinates, fit)
            }
        };
        let k = 10.min((n - 1) / 2);
        let trust =
            (k >= 1 && 2 * k < n).then(|| trustworthiness(&proximity, &coordinates, k).ok().map(|t| (t, k))).flatten();
        let shepard = if request.method.scaling() { shepard_pairs(&proximity, &coordinates) } else { Vec::new() };
        let groups = request.color.map(|c| colour_groups(table, c, &rows)).transpose()?;
        Ok(Self {
            request: request.clone(),
            names,
            n_rows: table.n_rows(),
            rows,
            coordinates,
            fit,
            trust,
            shepard,
            n_excluded,
            n_missing,
            groups,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &EmbeddingRequest {
        &self.request
    }

    /// Map coordinates of the analysed rows, in row order.
    #[must_use]
    pub fn coordinates(&self) -> &[Vec<f64>] {
        &self.coordinates
    }

    /// Dataset rows analysed, in order.
    #[must_use]
    pub fn rows(&self) -> &[usize] {
        &self.rows
    }

    /// Trustworthiness of the map and its neighbourhood size.
    #[must_use]
    pub fn trustworthiness(&self) -> Option<(f64, usize)> {
        self.trust
    }

    /// Report title, e.g. `"t-SNE: x, y, z"`.
    #[must_use]
    pub fn title(&self) -> String {
        let what = if self.request.distances { "distances" } else { "" };
        let label = match self.request.method {
            EmbeddingMethod::ClassicalScaling => "Classical Scaling",
            EmbeddingMethod::MetricScaling => "Metric Scaling (SMACOF)",
            EmbeddingMethod::Tsne => "t-SNE",
            EmbeddingMethod::Umap => "UMAP",
        };
        let cols = if self.names.len() > 6 {
            format!("{}, … ({} columns)", self.names[..3].join(", "), self.names.len())
        } else {
            self.names.join(", ")
        };
        if what.is_empty() { format!("{label}: {cols}") } else { format!("{label} of distances: {cols}") }
    }

    /// Columns the report can add: `dim1`, `dim2`, … (the map coordinates).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let short = self.request.method.short();
        (0..self.request.dims)
            .map(|k| {
                let mut values = vec![f64::NAN; self.n_rows];
                for (c, &r) in self.coordinates.iter().zip(&self.rows) {
                    values[r] = c[k];
                }
                ReportColumn::continuous(
                    &format!("dim{}", k + 1),
                    &format!("Save {short} dimension {}", k + 1),
                    format!("{short} {}", k + 1),
                    values,
                )
            })
            .collect()
    }

    /// Report sections: summary, eigenvalues (classical scaling), map,
    /// Shepard diagram (scaling methods).
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        if let Fit::Classical { eigenvalues, .. } = &self.fit {
            out.push(self.eigenvalue_table(eigenvalues));
        }
        out.push(self.map());
        if !self.shepard.is_empty() {
            out.push(self.shepard_plot());
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let r = &self.request;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        push("Method", r.method.label().to_owned());
        if r.distances {
            push("Input", format!("distance matrix of {} columns", self.names.len()));
        } else {
            let how = if r.standardize { "standardized" } else { "unstandardized" };
            push("Input", format!("Euclidean distances between rows on {} {how} columns", self.names.len()));
        }
        push("Rows mapped", self.rows.len().to_string());
        if self.n_excluded > 0 {
            push("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            push("Rows with a missing value", self.n_missing.to_string());
        }
        push("Dimensions", r.dims.to_string());
        match &self.fit {
            Fit::Classical { proportion, eigenvalues } => {
                push("Share of the positive eigenvalues kept", format!("{:.2}%", 100.0 * proportion));
                let negative = eigenvalues.iter().filter(|v| **v < -1e-9 * eigenvalues[0].abs()).count();
                if negative > 0 {
                    push("Negative eigenvalues (non-Euclidean distances)", negative.to_string());
                }
            }
            Fit::Metric { stress1, iterations, converged, start } => {
                push("Stress-1", format_number(*stress1, 4));
                push("Iterations", format!("{iterations}{}", if *converged { "" } else { " (limit reached)" }));
                let from = if *start == 0 { "classical scaling".to_owned() } else { format!("random start {start}") };
                push("Best start", format!("{from} (seed {})", r.seed));
            }
            Fit::Tsne { kl, method, rate, iterations, exaggerated, exaggeration } => {
                push("Perplexity", format_number(r.perplexity, 4));
                push("Gradient", method.label().to_owned());
                push(
                    "Iterations",
                    format!(
                        "{iterations} (the first {exaggerated} with early exaggeration {})",
                        format_number(*exaggeration, 4)
                    ),
                );
                push("Learning rate", format_number(*rate, 4));
                push("KL divergence", format_number(*kl, 4));
                push("Seed", r.seed.to_string());
            }
            Fit::Umap { a, b, edges, epochs, spectral } => {
                push("Neighbours", r.neighbours.to_string());
                push("Minimum distance", format_number(r.min_dist, 4));
                push("Curve parameters a, b", format!("{}, {}", format_number(*a, 5), format_number(*b, 5)));
                push("Graph edges", edges.to_string());
                push("Epochs", epochs.to_string());
                let start = if *spectral { "spectral layout" } else { "random" };
                push("Start", format!("{start} (seed {})", r.seed));
            }
        }
        if let Some((t, k)) = self.trust {
            push(&format!("Trustworthiness ({k} neighbours)"), format_number(t, 4));
        }
        section(SectionKind::EmbeddingSummary, "Fit", SectionContent::Table(t))
    }

    fn eigenvalue_table(&self, eigenvalues: &[f64]) -> ReportSection {
        let positive: f64 = eigenvalues.iter().filter(|v| **v > 0.0).sum();
        let mut t = TextTable::new(["Dimension", "Eigenvalue", "Percent of positive total", "Cumulative percent"]);
        let mut cum = 0.0;
        for (k, &l) in eigenvalues.iter().enumerate().take(10) {
            let share = if positive > 0.0 { l.max(0.0) / positive } else { 0.0 };
            cum += share;
            t.push([
                (k + 1).to_string(),
                format_number(l, 6),
                format!("{:.2}", 100.0 * share),
                format!("{:.2}", 100.0 * cum),
            ]);
        }
        section(SectionKind::EmbeddingEigenvalues, "Eigenvalues", SectionContent::Table(t))
    }

    fn map(&self) -> ReportSection {
        let short = self.request.method.short();
        let two = self.request.dims >= 2;
        let points: Vec<RowPoint> =
            self.rows
                .iter()
                .zip(&self.coordinates)
                .map(|(&row, c)| {
                    if two {
                        RowPoint { row, x: c[0], y: c[1] }
                    } else {
                        RowPoint { row, x: c[0], y: (row + 1) as f64 }
                    }
                })
                .collect();
        let groups = self.groups.as_ref().map(|(title, labels, membership)| PointGroups {
            title: title.clone(),
            labels: labels.clone(),
            membership: membership.clone(),
            lines: vec![Vec::new(); labels.len()],
        });
        let plot = PlotSpec {
            x_label: format!("{short} 1"),
            y_label: if two { format!("{short} 2") } else { "Row".to_owned() },
            points,
            groups,
            ..PlotSpec::default()
        };
        let title = match &self.groups {
            Some((title, ..)) => format!("Map (coloured and marked by {title})"),
            None => "Map".to_owned(),
        };
        section(SectionKind::EmbeddingMap, &title, SectionContent::Plot(Box::new(plot)))
    }

    fn shepard_plot(&self) -> ReportSection {
        let top = self.shepard.iter().map(|p| p.0.max(p.1)).fold(0.0, f64::max);
        let items = self.shepard.iter().map(|&(x, y)| (x, y, String::new(), 0)).collect();
        let plot = PlotSpec {
            x_label: "Dissimilarity".to_owned(),
            y_label: "Distance on the map".to_owned(),
            line: Some(vec![[0.0, 0.0], [top, top]]),
            line_name: Some("Equal".to_owned()),
            annotations: Some(Annotations { groups: vec!["Pairs of rows".to_owned()], items, labels: Vec::new() }),
            ..PlotSpec::default()
        };
        let title = if self.shepard.len() >= SHEPARD_PAIRS {
            format!("Shepard Diagram ({SHEPARD_PAIRS} pairs drawn at random)")
        } else {
            "Shepard Diagram (every pair)".to_owned()
        };
        section(SectionKind::ShepardDiagram, &title, SectionContent::Plot(Box::new(plot)))
    }
}

/// Dissimilarity and map distance of every pair, or of [`SHEPARD_PAIRS`]
/// pairs drawn at random (seeded) when there are more.
fn shepard_pairs(proximity: &Proximity<'_>, map: &[Vec<f64>]) -> Vec<(f64, f64)> {
    let n = map.len();
    let dist = |i: usize, j: usize| map[i].iter().zip(&map[j]).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt();
    if n * (n - 1) / 2 <= SHEPARD_PAIRS {
        return (0..n)
            .flat_map(|i| (0..i).map(move |j| (i, j)))
            .map(|(i, j)| (proximity.distance(i, j), dist(i, j)))
            .collect();
    }
    let mut rng = Rng::new(0x5EED);
    (0..SHEPARD_PAIRS)
        .map(|_| {
            let i = rng.below(n);
            let mut j = rng.below(n - 1);
            if j >= i {
                j += 1;
            }
            (proximity.distance(i, j), dist(i, j))
        })
        .collect()
}

/// Colour groups from column `c` for `rows`: its distinct values when it is
/// text, nominal or ordinal, or has at most 12 distinct values; otherwise
/// four quartile bins.
fn colour_groups(table: &DataTable, c: usize, rows: &[usize]) -> Result<(String, Vec<String>, Vec<usize>)> {
    let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
    let name = col.name.clone();
    if let Some(v) = col.as_numeric() {
        let mut distinct: Vec<f64> = rows.iter().map(|&r| v[r]).filter(|x| x.is_finite()).collect();
        distinct.sort_by(f64::total_cmp);
        distinct.dedup();
        let categorical = col.level != MeasurementLevel::Continuous || distinct.len() <= 12;
        if categorical {
            let mut labels: Vec<String> = distinct.iter().map(|x| format_number(*x, 6)).collect();
            let missing = labels.len();
            let membership =
                rows.iter().map(|&r| distinct.iter().position(|d| *d == v[r]).unwrap_or(missing)).collect::<Vec<_>>();
            if membership.contains(&missing) {
                labels.push("Missing".to_owned());
            }
            return Ok((name, labels, membership));
        }
        // Quartile bins.
        let q = |p: f64| distinct[((distinct.len() - 1) as f64 * p).round() as usize];
        let cuts = [q(0.25), q(0.5), q(0.75)];
        let f = |x: f64| format_number(x, 4);
        let mut labels = vec![
            format!("≤ {}", f(cuts[0])),
            format!("{}–{}", f(cuts[0]), f(cuts[1])),
            format!("{}–{}", f(cuts[1]), f(cuts[2])),
            format!("> {}", f(cuts[2])),
        ];
        let membership: Vec<usize> = rows
            .iter()
            .map(|&r| if v[r].is_finite() { cuts.iter().filter(|c| v[r] > **c).count() } else { 4 })
            .collect();
        if membership.contains(&4) {
            labels.push("Missing".to_owned());
        }
        return Ok((name, labels, membership));
    }
    let stat_data::ColumnData::Text(text) = &col.data else {
        return Err(invalid(format!("column {name:?} cannot colour the map")));
    };
    let mut labels: Vec<String> = rows.iter().map(|&r| text[r].clone()).collect();
    labels.sort();
    labels.dedup();
    let membership = rows.iter().map(|&r| labels.iter().position(|l| *l == text[r]).unwrap_or(0)).collect();
    Ok((name, labels, membership))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> crate::sample::Sample {
        crate::sample::catalog()
            .into_iter()
            .find(|s| s.name.starts_with("Expression profiles"))
            .map(|s| (s.build)())
            .unwrap()
    }

    fn genes(t: &DataTable) -> Vec<usize> {
        (0..t.n_cols()).filter(|&i| t.column(i).is_some_and(|c| c.name.starts_with("Gene"))).collect()
    }

    #[test]
    fn every_method_maps_the_rows() {
        let s = sample();
        let t = &s.table;
        let cols = genes(t);
        let color = t.find("Cell type");
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([2], true);
        for method in EmbeddingMethod::ALL {
            let req = EmbeddingRequest { color, iterations: Some(300), ..EmbeddingRequest::new(method, cols.clone()) };
            let a = EmbeddingAnalysis::run(t, &rows, &req).unwrap();
            assert_eq!(a.rows().len(), t.n_rows() - 1);
            let sections = a.sections();
            let map = sections.iter().find(|s| s.kind == SectionKind::EmbeddingMap).unwrap();
            let SectionContent::Plot(p) = &map.content else { panic!("plot") };
            assert!(p.points.iter().all(|q| q.row != 2));
            assert_eq!(p.groups.as_ref().unwrap().labels.len(), 4);
            assert_eq!(method.scaling(), sections.iter().any(|s| s.kind == SectionKind::ShepardDiagram));
            let saved = a.saveable_columns();
            assert_eq!(saved.len(), 2);
            assert!(saved[0].values[2].is_nan() && saved[1].values[0].is_finite());
            // Clustered data map well.
            let (trust, k) = a.trustworthiness().unwrap();
            assert_eq!(k, 10);
            assert!(trust > 0.8, "{method:?}: {trust}");
            assert_eq!(EmbeddingMethod::from_key(method.key()), Some(method));
            if method == EmbeddingMethod::Tsne {
                // 300 iterations: early exaggeration for the first quarter (75).
                let SectionContent::Table(fit) = &sections[0].content else { panic!() };
                let it = fit.rows.iter().find(|r| r[0] == "Iterations").unwrap();
                assert_eq!(it[1], "300 (the first 75 with early exaggeration 12)");
            }
        }
    }

    #[test]
    fn distance_matrix_input_matches_rows_input() {
        // Five points on a line; their distance matrix as columns.
        let xs = [0.0, 1.0, 3.0, 6.0, 10.0];
        let mut columns: Vec<stat_data::Column> = (0..5)
            .map(|j| {
                stat_data::Column::numeric(format!("d{}", j + 1), xs.iter().map(|x: &f64| (x - xs[j]).abs()).collect())
            })
            .collect();
        columns.push(stat_data::Column::numeric("x", xs.to_vec()));
        let t = DataTable::new("line", columns).unwrap();
        let rows = RowStateManager::new(5);
        let mut req = EmbeddingRequest {
            distances: true,
            dims: 1,
            ..EmbeddingRequest::new(EmbeddingMethod::ClassicalScaling, (0..5).collect())
        };
        let a = EmbeddingAnalysis::run(&t, &rows, &req).unwrap();
        let c: Vec<f64> = a.coordinates().iter().map(|r| r[0]).collect();
        // Recovers the line up to a shift and a sign.
        for i in 0..5 {
            for j in 0..5 {
                assert!(((c[i] - c[j]).abs() - (xs[i] - xs[j]).abs()).abs() < 1e-9);
            }
        }
        req.method = EmbeddingMethod::MetricScaling;
        let m = EmbeddingAnalysis::run(&t, &rows, &req).unwrap();
        let SectionContent::Table(fit) = &m.sections()[0].content else { panic!() };
        let stress = fit.rows.iter().find(|r| r[0] == "Stress-1").unwrap();
        assert!(stress[1].parse::<f64>().unwrap() < 1e-3);
        // Excluding a row drops its column too.
        let mut ex = RowStateManager::new(5);
        ex.set_excluded([4], true);
        assert_eq!(EmbeddingAnalysis::run(&t, &ex, &req).unwrap().rows(), [0, 1, 2, 3]);
        // A matrix whose size differs from the rows is refused.
        let bad = EmbeddingRequest { columns: vec![0, 1, 2], ..req.clone() };
        assert!(EmbeddingAnalysis::run(&t, &rows, &bad).is_err());
        let back = EmbeddingRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
        let full = EmbeddingRequest {
            method: EmbeddingMethod::Umap,
            perplexity: 12.0,
            neighbours: 7,
            min_dist: 0.3,
            iterations: Some(200),
            seed: 5,
            color: Some(5),
            standardize: false,
            ..EmbeddingRequest::new(EmbeddingMethod::Umap, vec![5])
        };
        assert_eq!(EmbeddingRequest::from_json(&full.to_json(&t), &t).unwrap(), full);
        // t-SNE needs a perplexity below n − 1.
        let tsne_req = EmbeddingRequest { perplexity: 30.0, ..EmbeddingRequest::new(EmbeddingMethod::Tsne, vec![5]) };
        assert!(EmbeddingAnalysis::run(&t, &rows, &tsne_req).is_err());
    }
}
