//! Multivariate analyses bound to a dataset: principal components and
//! clustering ([`stat_multivariate`]).
//!
//! Both use the rows that have a value in every chosen column and are not
//! excluded. Score and cluster plots keep each point's dataset row, so they
//! take part in linked brushing like every other plot.

use crate::format::format_number;
use crate::report::Annotations;
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use stat_core::{Matrix, Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_multivariate::{
    ClusterNumber, CorrespondenceAnalysis, Dendrogram, GapOptions, GapReference, KMeansOptions, Linkage, Pca,
    choose_cluster_number, correspondence_analysis, hierarchical_clustering, k_means, k_means_from_centers,
    principal_components, silhouette,
};

const DIGITS: usize = 5;
/// Most components shown in the loadings table.
const MAX_SHOWN_COMPONENTS: usize = 8;

/// Columns and scaling for a principal component analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcaRequest {
    /// Numeric columns (at least two).
    pub columns: Vec<usize>,
    /// Standardize (correlation matrix) rather than centre only (covariance).
    pub standardize: bool,
}

/// Clustering method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterMethod {
    /// K-means with `k` clusters.
    KMeans {
        /// Number of clusters.
        k: usize,
        /// Seed of the random starts.
        seed: u64,
    },
    /// Agglomerative clustering cut into `k` clusters.
    Hierarchical {
        /// Linkage criterion.
        linkage: Linkage,
        /// Number of clusters.
        k: usize,
    },
}

/// Settings of the "Number of Clusters" section of a k-means report: the gap
/// statistic and average silhouette width for `k = 1, …, max_k`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChooseClusters {
    /// Largest number of clusters examined.
    pub max_k: usize,
    /// Reference data sets drawn for the gap statistic.
    pub references: usize,
    /// Box the reference data are drawn from.
    pub reference: GapReference,
}

impl Default for ChooseClusters {
    fn default() -> Self {
        Self { max_k: 8, references: 50, reference: GapReference::DataRange }
    }
}

/// Starting centers given for k-means (Hartigan & Wong 1979), in the units
/// of the data (standardized like the data when the request standardizes).
#[derive(Debug, Clone, PartialEq)]
pub struct StartingCenters {
    /// One center per cluster, one coordinate per column (in column order).
    pub centers: Vec<Vec<f64>>,
    /// Also run the seeded k-means++ starts and keep the best solution
    /// (`false`: only the given start).
    pub also_seeded: bool,
}

/// Columns and method for a cluster analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterRequest {
    /// Numeric columns (at least one).
    pub columns: Vec<usize>,
    /// Method and number of clusters.
    pub method: ClusterMethod,
    /// Standardize columns to unit variance first (recommended when units differ).
    pub standardize: bool,
    /// K-means only: also compare numbers of clusters (gap statistic and
    /// silhouettes); `None` leaves the section out.
    pub choose: Option<ChooseClusters>,
    /// K-means only: starting centers given instead of (or beside) the
    /// seeded starts; their number sets `k`.
    pub centers: Option<StartingCenters>,
    /// How rows are compared (hierarchical clustering can use any metric or
    /// a distance matrix; k-means is Euclidean).
    pub distance: ClusterDistance,
}

/// How a cluster analysis measures the distance between rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClusterDistance {
    /// Euclidean distance of the (optionally standardized) columns.
    #[default]
    Euclidean,
    /// Another metric of [`crate::distance_analysis::measure`] (Gower for
    /// mixed column types, Manhattan, Jaccard, Bray–Curtis…).
    Metric(stat_multivariate::distance::DistanceMetric),
    /// The columns hold a distance matrix (one row per column).
    Matrix,
}

/// Silhouettes need every pairwise distance, so larger data sets skip them.
const MAX_SILHOUETTE_ROWS: usize = 5000;

/// Column names, rows used, the data as row vectors, and the number excluded.
type CompleteData = (Vec<String>, Vec<usize>, Vec<Vec<f64>>, usize);

/// Rows complete in every column and not excluded, the column names, and the
/// data as row vectors.
fn complete_data(
    table: &DataTable,
    states: &RowStateManager,
    columns: &[usize],
    min_columns: usize,
) -> Result<CompleteData> {
    if columns.len() < min_columns {
        return Err(StatError::InvalidArgument { message: format!("choose at least {min_columns} numeric columns") });
    }
    let mut names = Vec::with_capacity(columns.len());
    let mut values = Vec::with_capacity(columns.len());
    for &c in columns {
        let col = table.column(c).ok_or_else(|| StatError::InvalidArgument { message: format!("no column {c}") })?;
        let v = col
            .as_numeric()
            .ok_or_else(|| StatError::InvalidArgument { message: format!("column {:?} is not numeric", col.name) })?;
        names.push(col.name.clone());
        values.push(v);
    }
    let mut excluded = 0;
    let rows: Vec<usize> = (0..table.n_rows())
        .filter(|&r| values.iter().all(|v| v[r].is_finite()))
        .filter(|&r| {
            let keep = !states.is_excluded(r);
            if !keep {
                excluded += 1;
            }
            keep
        })
        .collect();
    let data = rows.iter().map(|&r| values.iter().map(|v| v[r]).collect()).collect();
    Ok((names, rows, data, excluded))
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

fn empty_plot(x_label: &str, y_label: &str) -> PlotSpec {
    PlotSpec {
        x_time: None,
        y_time: None,
        x_label: x_label.to_owned(),
        y_label: y_label.to_owned(),
        points: Vec::new(),
        line: None,
        band: None,
        groups: None,
        x_ticks: None,
        panes: Vec::new(),
        annotations: None,
        segments: Vec::new(),
        ..PlotSpec::default()
    }
}

/// A computed principal component analysis.
#[derive(Debug, Clone)]
pub struct PcaAnalysis {
    request: PcaRequest,
    n_rows: usize,
    names: Vec<String>,
    rows: Vec<usize>,
    pca: Pca,
}

impl PcaAnalysis {
    /// Run the analysis on the complete, included rows.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for fewer than two numeric columns or a
    /// non-numeric column; errors from [`principal_components`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &PcaRequest) -> Result<Self> {
        let (names, rows, data, _) = complete_data(table, states, &request.columns, 2)?;
        let x = Matrix::from_rows(&data)?;
        let pca = principal_components(&x, request.standardize)?;
        Ok(Self { request: request.clone(), n_rows: table.n_rows(), names, rows, pca })
    }

    /// The fitted components.
    #[must_use]
    pub fn pca(&self) -> &Pca {
        &self.pca
    }

    /// Columns a report can add to the table: the scores on each component
    /// (`"pc1"`, `"pc2"`, …).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        (0..self.pca.n_components())
            .map(|k| {
                ReportColumn::continuous(
                    &format!("pc{}", k + 1),
                    &format!("Save PC{} scores to a new column", k + 1),
                    format!("PC{} score", k + 1),
                    self.score_column(k),
                )
            })
            .collect()
    }

    /// Scores on component `k` (0-based) for every dataset row; NaN for rows
    /// that were not analysed.
    #[must_use]
    pub fn score_column(&self, k: usize) -> Vec<f64> {
        let mut out = vec![f64::NAN; self.n_rows];
        if k < self.pca.n_components() {
            for (i, &r) in self.rows.iter().enumerate() {
                out[r] = self.pca.scores[(i, k)];
            }
        }
        out
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!(
            "Principal Components ({}): {}",
            if self.request.standardize { "correlations" } else { "covariances" },
            self.names.join(", ")
        )
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let p = &self.pca;
        let r = p.n_components();
        let mut eig = TextTable::new(["Component", "Eigenvalue", "Percent", "Cumulative percent"]);
        for (k, ((l, prop), cum)) in p.eigenvalues.iter().zip(p.proportions()).zip(p.cumulative()).enumerate() {
            eig.push([
                format!("PC{}", k + 1),
                format_number(*l, DIGITS),
                format!("{:.2}", 100.0 * prop),
                format!("{:.2}", 100.0 * cum),
            ]);
        }
        let mut out = vec![section(
            SectionKind::Eigenvalues,
            &format!("Eigenvalues ({} rows)", self.rows.len()),
            SectionContent::Table(eig),
        )];
        let mut scree = empty_plot("Component", "Eigenvalue");
        scree.line = Some(p.eigenvalues.iter().enumerate().map(|(k, &l)| [(k + 1) as f64, l]).collect());
        scree.line_name = Some("Eigenvalue".to_owned());
        scree.point_name = Some("Eigenvalue".to_owned());
        scree.x_ticks = Some((1..=r).map(|k| (k as f64, format!("PC{k}"))).collect());
        if self.request.standardize {
            // Kaiser's guideline: components with eigenvalue above 1.
            scree.segments.push(vec![[1.0, 1.0], [r as f64, 1.0]]);
        }
        out.push(section(SectionKind::ScreePlot, "Scree Plot", SectionContent::Plot(Box::new(scree))));

        let shown = r.min(MAX_SHOWN_COMPONENTS);
        let corr = p.scaled_loadings();
        let mut heads = vec!["Variable".to_owned()];
        heads.extend((1..=shown).map(|k| format!("PC{k}")));
        if self.request.standardize {
            heads.extend((1..=shown.min(2)).map(|k| format!("Correlation with PC{k}")));
        }
        let mut load = TextTable::new(heads);
        for (j, name) in self.names.iter().enumerate() {
            let mut row = vec![name.clone()];
            row.extend((0..shown).map(|k| format_number(p.loadings[(j, k)], 4)));
            if self.request.standardize {
                row.extend((0..shown.min(2)).map(|k| format_number(corr[(j, k)], 4)));
            }
            load.push(row);
        }
        out.push(section(SectionKind::Loadings, "Eigenvectors (loadings)", SectionContent::Table(load)));

        if r >= 2 {
            let mut plot = empty_plot("PC1 score", "PC2 score");
            plot.points = self
                .rows
                .iter()
                .enumerate()
                .map(|(i, &row)| RowPoint { row, x: p.scores[(i, 0)], y: p.scores[(i, 1)] })
                .collect();
            // Loading arrows, scaled to the score range, from the origin.
            let reach =
                (0..self.rows.len()).map(|i| p.scores[(i, 0)].abs().max(p.scores[(i, 1)].abs())).fold(0.0, f64::max);
            let longest =
                (0..self.names.len()).map(|j| p.loadings[(j, 0)].hypot(p.loadings[(j, 1)])).fold(0.0, f64::max);
            if longest > 0.0 {
                let s = 0.8 * reach / longest;
                plot.segments = (0..self.names.len())
                    .map(|j| vec![[0.0, 0.0], [s * p.loadings[(j, 0)], s * p.loadings[(j, 1)]]])
                    .collect();
            }
            let title = format!(
                "Score Plot (PC1 {:.1}%, PC2 {:.1}%; lines show the variables' loadings)",
                100.0 * p.proportions()[0],
                100.0 * p.proportions()[1]
            );
            out.push(section(SectionKind::ScorePlot, &title, SectionContent::Plot(Box::new(plot))));
        }
        out
    }
}

/// A computed cluster analysis.
#[derive(Debug, Clone)]
pub struct ClusterAnalysis {
    request: ClusterRequest,
    n_rows: usize,
    names: Vec<String>,
    rows: Vec<usize>,
    data: Vec<Vec<f64>>,
    labels: Vec<usize>,
    k: usize,
    within_ss: Option<(Vec<f64>, f64)>,
    dendrogram: Option<Dendrogram>,
    /// Average silhouette width per cluster and overall (k ≥ 2, not too many rows).
    silhouette: Option<(Vec<f64>, f64)>,
    /// Gap statistic and silhouettes across k, or why they are unavailable.
    choose: Option<std::result::Result<ClusterNumber, String>>,
    /// The k-means solution came from the given starting centers.
    from_given: bool,
    /// Map of the objects for the cluster plot when the distances are not
    /// Euclidean: their first two principal coordinates.
    map: Option<Vec<(f64, f64)>>,
}

impl ClusterAnalysis {
    /// Run the clustering on the complete, included rows.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for no columns, a non-numeric column,
    /// a constant column when standardizing, or an invalid `k`; errors from the
    /// clustering routines.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ClusterRequest) -> Result<Self> {
        if request.distance != ClusterDistance::Euclidean {
            return Self::run_on_distances(table, states, request);
        }
        let (names, rows, data, _) = complete_data(table, states, &request.columns, 1)?;
        let mut work = data.clone();
        let mut given = match (&request.centers, request.method) {
            (Some(c), ClusterMethod::KMeans { .. }) => {
                if let Some(bad) = c.centers.iter().find(|r| r.len() != names.len()) {
                    return Err(StatError::InvalidArgument {
                        message: format!(
                            "each starting center needs {} values, one per column (got {})",
                            names.len(),
                            bad.len()
                        ),
                    });
                }
                Some(c.centers.clone())
            }
            _ => None,
        };
        if request.standardize {
            let n = work.len() as f64;
            for j in 0..names.len() {
                let m = work.iter().map(|r| r[j]).sum::<f64>() / n;
                let sd = (work.iter().map(|r| (r[j] - m) * (r[j] - m)).sum::<f64>() / (n - 1.0)).sqrt();
                if sd.is_nan() || sd <= 0.0 {
                    return Err(StatError::InvalidArgument {
                        message: format!("column {:?} is constant, so it cannot be standardized", names[j]),
                    });
                }
                for r in work.iter_mut().chain(given.iter_mut().flatten()) {
                    r[j] = (r[j] - m) / sd;
                }
            }
        }
        let mut from_given = false;
        let (labels, k, within_ss, dendrogram) = match request.method {
            ClusterMethod::KMeans { k, seed } => {
                let options = KMeansOptions { seed, ..KMeansOptions::new(k) };
                let km = match (&given, &request.centers) {
                    (Some(g), Some(c)) => k_means_from_centers(&work, g, &options, c.also_seeded)?,
                    _ => k_means(&work, &options)?,
                };
                from_given = km.from_given_centers;
                let total = km.total_ss;
                (km.assignments.clone(), km.centers.len(), Some((km.within_ss.clone(), total)), None)
            }
            ClusterMethod::Hierarchical { linkage, k } => {
                let d = hierarchical_clustering(&work, linkage)?;
                (d.cut(k)?, k, None, Some(d))
            }
        };
        let silhouette = (k >= 2 && work.len() <= MAX_SILHOUETTE_ROWS)
            .then(|| silhouette(&work, &labels, k).ok().map(|s| (s.cluster_average, s.average)))
            .flatten();
        let choose = match (request.method, request.choose) {
            (ClusterMethod::KMeans { seed, .. }, Some(c)) => Some(
                choose_cluster_number(
                    &work,
                    &GapOptions {
                        max_k: c.max_k.min(work.len()),
                        references: c.references,
                        reference: c.reference,
                        seed,
                        max_silhouette_points: MAX_SILHOUETTE_ROWS,
                        ..GapOptions::default()
                    },
                )
                .map_err(|e| e.to_string()),
            ),
            _ => None,
        };
        Ok(Self {
            request: request.clone(),
            n_rows: table.n_rows(),
            names,
            rows,
            data,
            labels,
            k,
            within_ss,
            dendrogram,
            silhouette,
            choose,
            from_given,
            map: None,
        })
    }

    /// Hierarchical clustering on another metric or a distance matrix.
    fn run_on_distances(table: &DataTable, states: &RowStateManager, request: &ClusterRequest) -> Result<Self> {
        let ClusterMethod::Hierarchical { linkage, k } = request.method else {
            return Err(StatError::InvalidArgument {
                message: "k-means uses Euclidean distances; choose hierarchical clustering for other distances or a \
                          distance matrix"
                    .to_owned(),
            });
        };
        let measured = match request.distance {
            ClusterDistance::Matrix => crate::distance_analysis::matrix_from_columns(table, states, &request.columns)?,
            ClusterDistance::Metric(metric) => crate::distance_analysis::measure(
                table,
                states,
                &crate::distance_analysis::DistanceSpec {
                    columns: request.columns.clone(),
                    metric,
                    standardize: request.standardize,
                },
            )?,
            ClusterDistance::Euclidean => unreachable!("handled by run"),
        };
        let d = stat_multivariate::hierarchical::hierarchical_from_distances(&measured.matrix, linkage)?;
        let labels = d.cut(k)?;
        // Cluster means of the numeric columns (distance columns for a matrix).
        let numeric: Vec<(String, &[f64])> = request
            .columns
            .iter()
            .filter_map(|&c| table.column(c))
            .filter_map(|c| c.as_numeric().map(|v| (c.name.clone(), v)))
            .collect();
        let names: Vec<String> = numeric.iter().map(|(n, _)| n.clone()).collect();
        let data: Vec<Vec<f64>> = measured.rows.iter().map(|&r| numeric.iter().map(|(_, v)| v[r]).collect()).collect();
        let map = stat_multivariate::pcoa::principal_coordinates(
            &measured.matrix,
            stat_multivariate::pcoa::NegativeCorrection::None,
            2,
        )
        .ok()
        .filter(|p| p.coordinates.first().is_some_and(|c| c.len() >= 2))
        .map(|p| p.coordinates.iter().map(|c| (c[0], c[1])).collect());
        Ok(Self {
            request: request.clone(),
            n_rows: table.n_rows(),
            names,
            rows: measured.rows,
            data,
            labels,
            k,
            within_ss: None,
            dendrogram: Some(d),
            silhouette: None,
            choose: None,
            map,
            from_given: false,
        })
    }

    /// "Number of Clusters": the gap table and plots of the gap statistic
    /// and the average silhouette width against k.
    fn cluster_number_sections(&self, choose: &std::result::Result<ClusterNumber, String>) -> Vec<ReportSection> {
        let c = match choose {
            Err(e) => {
                return vec![section(
                    SectionKind::ClusterNumber,
                    "Number of Clusters",
                    SectionContent::Note(format!("Not available: {e}")),
                )];
            }
            Ok(c) => c,
        };
        let settings = self.request.choose.unwrap_or_default();
        let mut table =
            TextTable::new(["k", "log W", "Expected log W", "Gap", "Std error", "Average silhouette", "Suggested by"]);
        for r in &c.rows {
            let mut by = Vec::new();
            if r.k == c.gap_choice {
                by.push("gap");
            }
            if c.silhouette_choice == Some(r.k) {
                by.push("silhouette");
            }
            table.push([
                r.k.to_string(),
                format_number(r.log_w, DIGITS),
                format_number(r.expected_log_w, DIGITS),
                format_number(r.gap, DIGITS),
                format_number(r.std_error, 4),
                r.silhouette.map_or_else(|| "—".to_owned(), |s| format_number(s, 4)),
                by.join(", "),
            ]);
        }
        let box_name = match settings.reference {
            GapReference::DataRange => "uniform over the data range",
            GapReference::PrincipalComponents => "uniform over the principal-component box",
        };
        let silhouette_note = match c.silhouette_choice {
            Some(k) => format!("; largest average silhouette at k = {k}"),
            None => String::new(),
        };
        let mut out = vec![section(
            SectionKind::ClusterNumber,
            &format!(
                "Number of Clusters (gap statistic: {} reference sets {box_name}; smallest k with Gap(k) ≥ \
                 Gap(k+1) − s(k+1) is {}{silhouette_note})",
                settings.references, c.gap_choice
            ),
            SectionContent::Table(table),
        )];
        let ticks: Vec<(f64, String)> = c.rows.iter().map(|r| (r.k as f64, r.k.to_string())).collect();
        let mut gap = empty_plot("Number of clusters k", "Gap statistic");
        gap.line = Some(c.rows.iter().map(|r| [r.k as f64, r.gap]).collect());
        gap.line_name = Some("Gap(k)".to_owned());
        gap.segments =
            c.rows.iter().map(|r| vec![[r.k as f64, r.gap - r.std_error], [r.k as f64, r.gap + r.std_error]]).collect();
        gap.x_ticks = Some(ticks.clone());
        out.push(section(
            SectionKind::GapPlot,
            "Gap Statistic by k (vertical bars: ± one standard error)",
            SectionContent::Plot(Box::new(gap)),
        ));
        let sil: Polyline = c.rows.iter().filter_map(|r| r.silhouette.map(|s| [r.k as f64, s])).collect();
        if !sil.is_empty() {
            let mut plot = empty_plot("Number of clusters k", "Average silhouette width");
            plot.line = Some(sil);
            plot.line_name = Some("Average silhouette".to_owned());
            plot.x_ticks = Some(ticks);
            out.push(section(
                SectionKind::SilhouettePlot,
                "Average Silhouette Width by k (higher: better separated clusters)",
                SectionContent::Plot(Box::new(plot)),
            ));
        }
        out
    }

    /// The gap statistic and silhouettes across k, when requested.
    #[must_use]
    pub fn cluster_number(&self) -> Option<&std::result::Result<ClusterNumber, String>> {
        self.choose.as_ref()
    }

    /// Cluster (1-based) of each dataset row, `None` for rows not clustered.
    #[must_use]
    pub fn row_clusters(&self, n_rows: usize) -> Vec<Option<usize>> {
        let mut out = vec![None; n_rows];
        for (&r, &l) in self.rows.iter().zip(&self.labels) {
            if r < n_rows {
                out[r] = Some(l + 1);
            }
        }
        out
    }

    /// Columns a report can add to the table: the cluster numbers (nominal).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let mut c = ReportColumn::continuous(
            "cluster",
            "Save cluster numbers to a new column",
            "Cluster".to_owned(),
            self.cluster_column(),
        );
        c.level = stat_data::MeasurementLevel::Nominal;
        vec![c]
    }

    /// Cluster number (1-based) for every dataset row as a numeric column;
    /// NaN for rows that were not clustered.
    #[must_use]
    pub fn cluster_column(&self) -> Vec<f64> {
        self.row_clusters(self.n_rows).into_iter().map(|c| c.map_or(f64::NAN, |c| c as f64)).collect()
    }

    /// The "Starting Centers" table: the given centers, and which start won.
    fn starting_centers(&self, c: &StartingCenters) -> ReportSection {
        let mut heads = vec!["Center".to_owned()];
        heads.extend(self.names.iter().cloned());
        let mut t = TextTable::new(heads);
        for (i, center) in c.centers.iter().enumerate() {
            let mut row = vec![(i + 1).to_string()];
            row.extend(center.iter().map(|&v| format_number(v, DIGITS)));
            t.push(row);
        }
        let starts = KMeansOptions::new(1).starts;
        let how = match (c.also_seeded, self.from_given) {
            (false, _) => "Starting Centers (given; k-means ran from these only)".to_owned(),
            (true, true) => format!(
                "Starting Centers (given, beside {starts} seeded starts; the given start gave the smallest \
                 within-cluster SS)"
            ),
            (true, false) => format!(
                "Starting Centers (given, beside {starts} seeded starts; a seeded start gave a smaller \
                 within-cluster SS)"
            ),
        };
        section(SectionKind::ClusterStartingCenters, &how, SectionContent::Table(t))
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let method = match self.request.method {
            ClusterMethod::KMeans { k, .. } if self.request.centers.is_some() => {
                format!("k-means from given centers, k = {k}")
            }
            ClusterMethod::KMeans { k, .. } => format!("k-means, k = {k}"),
            ClusterMethod::Hierarchical { linkage, k } => format!("hierarchical, {} linkage, k = {k}", linkage.label()),
        };
        let distance = match self.request.distance {
            ClusterDistance::Euclidean => String::new(),
            ClusterDistance::Metric(m) => format!(", {} distances", m.label()),
            ClusterDistance::Matrix => ", distance matrix".to_owned(),
        };
        if self.request.distance == ClusterDistance::Euclidean {
            format!("Clusters ({method}{distance}): {}", self.names.join(", "))
        } else {
            format!("Clusters ({method}{distance}): {} objects", self.rows.len())
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let k = self.k;
        let sizes: Vec<usize> = (0..k).map(|c| self.labels.iter().filter(|&&l| l == c).count()).collect();
        let mut heads = vec!["Cluster", "Rows"];
        if self.within_ss.is_some() {
            heads.push("Within-cluster SS");
        }
        if self.silhouette.is_some() {
            heads.push("Average silhouette");
        }
        let mut summary = TextTable::new(heads);
        for c in 0..k {
            let mut row = vec![(c + 1).to_string(), sizes[c].to_string()];
            if let Some((w, _)) = &self.within_ss {
                row.push(format_number(w[c], DIGITS));
            }
            if let Some((s, _)) = &self.silhouette {
                row.push(format_number(s[c], 4));
            }
            summary.push(row);
        }
        let mut title = format!("Cluster Summary ({} rows)", self.rows.len());
        if let Some((w, total)) = &self.within_ss {
            let within: f64 = w.iter().sum();
            title = format!(
                "{title}: between-cluster SS is {:.1}% of the total{}",
                100.0 * (1.0 - within / total),
                if self.request.standardize { " (standardized units)" } else { "" }
            );
        }
        if let Some((_, avg)) = &self.silhouette {
            title = format!("{title}; average silhouette width {}", format_number(*avg, 3));
        }
        let mut out = vec![section(SectionKind::ClusterSummary, &title, SectionContent::Table(summary))];
        if let Some(c) = &self.request.centers {
            out.push(self.starting_centers(c));
        }
        if let Some(choose) = &self.choose {
            out.extend(self.cluster_number_sections(choose));
        }

        let mut heads = vec!["Cluster".to_owned()];
        heads.extend(self.names.iter().cloned());
        let mut means = TextTable::new(heads);
        for c in 0..k {
            let members: Vec<&Vec<f64>> =
                self.data.iter().zip(&self.labels).filter(|(_, l)| **l == c).map(|(d, _)| d).collect();
            let mut row = vec![(c + 1).to_string()];
            for j in 0..self.names.len() {
                let present: Vec<f64> = members.iter().map(|r| r[j]).filter(|v| v.is_finite()).collect();
                let m = present.iter().sum::<f64>() / present.len() as f64;
                row.push(format_number(m, DIGITS));
            }
            means.push(row);
        }
        if !self.names.is_empty() {
            out.push(section(SectionKind::ClusterMeans, "Cluster Means", SectionContent::Table(means)));
        }

        // Points on the first two columns (or the only column against row
        // order); for other distances, on the first two principal coordinates.
        let (x_label, y_label, coords): (String, String, Vec<(f64, f64)>) = if let Some(map) = &self.map {
            ("PCo 1".to_owned(), "PCo 2".to_owned(), map.clone())
        } else if self.names.is_empty() {
            ("Row".to_owned(), "Cluster".to_owned(), self.rows.iter().zip(&self.labels).map(|(&r, &l)| ((r + 1) as f64, (l + 1) as f64)).collect())
        } else if self.names.len() >= 2 {
            (self.names[0].clone(), self.names[1].clone(), self.data.iter().map(|r| (r[0], r[1])).collect())
        } else {
            (
                "Row".to_owned(),
                self.names[0].clone(),
                self.rows.iter().zip(&self.data).map(|(&r, d)| ((r + 1) as f64, d[0])).collect(),
            )
        };
        let mut plot = empty_plot(&x_label, &y_label);
        let keep: Vec<usize> =
            (0..coords.len()).filter(|&i| coords[i].0.is_finite() && coords[i].1.is_finite()).collect();
        plot.points = keep.iter().map(|&i| RowPoint { row: self.rows[i], x: coords[i].0, y: coords[i].1 }).collect();
        plot.groups = Some(PointGroups {
            title: "Cluster".to_owned(),
            labels: (1..=k).map(|c| c.to_string()).collect(),
            membership: keep.iter().map(|&i| self.labels[i]).collect(),
            lines: vec![Vec::new(); k],
        });
        let plot_title = if self.map.is_some() { "Clusters (on the first two principal coordinates)" } else { "Clusters" };
        out.push(section(SectionKind::ClusterPlot, plot_title, SectionContent::Plot(Box::new(plot))));

        if let Some(d) = &self.dendrogram {
            let mut plot = empty_plot("Rows (in tree order)", "Merge distance");
            plot.segments = d.segments();
            // Leaves as points at height 0, so selecting them brushes rows.
            let order = d.leaf_order();
            plot.points = order
                .iter()
                .enumerate()
                .map(|(x, &leaf)| RowPoint { row: self.rows[leaf], x: (x + 1) as f64, y: 0.0 })
                .collect();
            plot.groups = Some(PointGroups {
                title: "Cluster".to_owned(),
                labels: (1..=k).map(|c| c.to_string()).collect(),
                membership: order.iter().map(|&leaf| self.labels[leaf]).collect(),
                lines: vec![Vec::new(); k],
            });
            // The cut: a line between the heights of the last k merges.
            let n = d.merges.len();
            if k >= 2 && k <= n + 1 {
                let below = d.merges[n + 1 - k].height;
                let above = if k >= 2 && n + 1 - k > 0 { d.merges[n - k].height } else { 0.0 };
                let h = (below + above) / 2.0;
                let cut: Polyline = vec![[0.5, h], [d.n as f64 + 0.5, h]];
                plot.line = Some(cut);
                plot.line_name = Some(format!("Cut height ({k} clusters)"));
            }
            out.push(section(
                SectionKind::Dendrogram,
                &format!("Dendrogram ({} linkage; the line marks the cut into {k} clusters)", d.linkage.label()),
                SectionContent::Plot(Box::new(plot)),
            ));
        }
        out
    }
}

/// Two categorical columns for a correspondence analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrespondenceRequest {
    /// Column whose levels form the table's rows.
    pub row: usize,
    /// Column whose levels form the table's columns.
    pub column: usize,
}

/// A computed correspondence analysis of two columns' cross-tabulation.
#[derive(Debug, Clone)]
pub struct CorrespondenceReport {
    row_name: String,
    column_name: String,
    row_levels: Vec<String>,
    column_levels: Vec<String>,
    ca: CorrespondenceAnalysis,
    n_rows_used: usize,
}

impl CorrespondenceReport {
    /// Cross-tabulate the two columns (rows with both values, not excluded)
    /// and analyse the table. Levels are in order of first appearance.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for the same column twice, an invalid
    /// column, or a table smaller than 2 × 2; errors from
    /// [`correspondence_analysis`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &CorrespondenceRequest) -> Result<Self> {
        if request.row == request.column {
            return Err(StatError::InvalidArgument { message: "choose two different columns".into() });
        }
        let col =
            |i: usize| table.column(i).ok_or_else(|| StatError::InvalidArgument { message: format!("no column {i}") });
        let (rc, cc) = (col(request.row)?, col(request.column)?);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for r in 0..table.n_rows() {
            let (x, y) = (rc.level_text(r), cc.level_text(r));
            if x.trim().is_empty() || y.trim().is_empty() || states.is_excluded(r) {
                continue;
            }
            a.push(x);
            b.push(y);
        }
        let ct = stat_discrete::ContingencyTable::from_pairs(&a, &b)?;
        let counts: Vec<Vec<f64>> = ct.counts.iter().map(|row| row.iter().map(|&v| v as f64).collect()).collect();
        let ca = correspondence_analysis(&counts)?;
        // Independent columns leave nothing to map (and percentages of a zero
        // inertia would read NaN).
        if ca.total_inertia().is_nan() || ca.total_inertia() <= 1e-12 {
            return Err(StatError::InvalidArgument {
                message: format!(
                    "{} and {} show no association (total inertia is zero), so there is nothing to map",
                    rc.name, cc.name
                ),
            });
        }
        Ok(Self {
            row_name: rc.name.clone(),
            column_name: cc.name.clone(),
            row_levels: ct.row_labels,
            column_levels: ct.column_labels,
            ca,
            n_rows_used: a.len(),
        })
    }

    /// The analysis.
    #[must_use]
    pub fn analysis(&self) -> &CorrespondenceAnalysis {
        &self.ca
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Correspondence Analysis: {} by {}", self.row_name, self.column_name)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let ca = &self.ca;
        let dims = ca.inertias.len();
        let mut t = TextTable::new(["Dimension", "Inertia", "Percent", "Cumulative percent"]);
        let mut cum = 0.0;
        for (k, (l, p)) in ca.inertias.iter().zip(ca.proportions()).enumerate() {
            cum += p;
            t.push([
                format!("{}", k + 1),
                format_number(*l, DIGITS),
                format!("{:.2}", 100.0 * p),
                format!("{:.2}", 100.0 * cum),
            ]);
        }
        let mut out = vec![section(
            SectionKind::Inertia,
            &format!(
                "Inertia (total inertia {} = χ² / n = {} / {})",
                format_number(ca.total_inertia(), DIGITS),
                format_number(ca.chi_square(), DIGITS),
                self.n_rows_used
            ),
            SectionContent::Table(t),
        )];
        let shown = dims.min(2);
        let coords = |name: &str, levels: &[String], masses: &[f64], m: &stat_core::Matrix| {
            let mut heads = vec![name.to_owned(), "Mass".to_owned()];
            heads.extend((1..=shown).map(|k| format!("Dimension {k}")));
            let mut t = TextTable::new(heads);
            for (i, l) in levels.iter().enumerate() {
                let mut row = vec![l.clone(), format_number(masses[i], 4)];
                row.extend((0..shown).map(|k| format_number(m[(i, k)], 4)));
                t.push(row);
            }
            t
        };
        out.push(section(
            SectionKind::RowCoordinates,
            &format!("{} Coordinates (principal)", self.row_name),
            SectionContent::Table(coords(&self.row_name, &self.row_levels, &ca.row_masses, &ca.row_coordinates)),
        ));
        out.push(section(
            SectionKind::ColumnCoordinates,
            &format!("{} Coordinates (principal)", self.column_name),
            SectionContent::Table(coords(
                &self.column_name,
                &self.column_levels,
                &ca.column_masses,
                &ca.column_coordinates,
            )),
        ));
        if dims >= 1 {
            let y = |m: &stat_core::Matrix, i: usize| if dims >= 2 { m[(i, 1)] } else { 0.0 };
            let mut items: Vec<(f64, f64, String, usize)> = self
                .row_levels
                .iter()
                .enumerate()
                .map(|(i, l)| (ca.row_coordinates[(i, 0)], y(&ca.row_coordinates, i), l.clone(), 0))
                .collect();
            items.extend(
                self.column_levels
                    .iter()
                    .enumerate()
                    .map(|(j, l)| (ca.column_coordinates[(j, 0)], y(&ca.column_coordinates, j), l.clone(), 1)),
            );
            let mut plot = empty_plot(
                &format!("Dimension 1 ({:.1}%)", 100.0 * ca.proportions()[0]),
                &if dims >= 2 {
                    format!("Dimension 2 ({:.1}%)", 100.0 * ca.proportions()[1])
                } else {
                    "Dimension 2".to_owned()
                },
            );
            let (lo, hi) =
                items.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), it| (a.min(it.0), b.max(it.0)));
            plot.segments = vec![vec![[lo, 0.0], [hi, 0.0]]];
            plot.annotations = Some(Annotations {
                groups: vec![self.row_name.clone(), self.column_name.clone()],
                items,
                labels: Vec::new(),
            });
            out.push(section(
                SectionKind::CorrespondenceMap,
                "Symmetric Map (categories with similar profiles lie close together)",
                SectionContent::Plot(Box::new(plot)),
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        // Two clear groups in (a, b); c is noise-free in a.
        let a = vec![1.0, 1.2, 0.9, 1.1, 5.0, 5.2, 4.9, 5.1, f64::NAN];
        let b = vec![2.0, 2.1, 1.9, 2.2, 8.0, 8.1, 7.9, 8.3, 1.0];
        let c: Vec<f64> = a.iter().map(|v| 3.0 * v + 1.0).collect();
        DataTable::new("t", vec![Column::numeric("a", a), Column::numeric("b", b), Column::numeric("c", c)]).unwrap()
    }

    #[test]
    fn pca_report() {
        let t = table();
        let a =
            PcaAnalysis::run(&t, &RowStateManager::new(9), &PcaRequest { columns: vec![0, 1, 2], standardize: true })
                .unwrap();
        assert_eq!(a.rows, (0..8).collect::<Vec<_>>(), "row with a missing value left out");
        let s = a.sections();
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [SectionKind::Eigenvalues, SectionKind::ScreePlot, SectionKind::Loadings, SectionKind::ScorePlot]
        );
        let SectionContent::Table(e) = &s[0].content else { panic!() };
        assert_eq!(e.cell(2, "Cumulative percent"), Some("100.00"));
        let SectionContent::Plot(scree) = &s[1].content else { panic!() };
        assert_eq!((scree.line_label(), scree.point_label()), ("Eigenvalue", "Eigenvalue"));
        let total: f64 = scree.line.as_ref().unwrap().iter().map(|p| p[1]).sum();
        assert!((total - 3.0).abs() < 1e-9, "standardized: eigenvalues sum to the number of variables");
        let SectionContent::Plot(sp) = &s[3].content else { panic!() };
        assert_eq!(sp.points.len(), 8);
        assert_eq!(sp.segments.len(), 3, "one loading arrow per variable");
        assert!(
            PcaAnalysis::run(&t, &RowStateManager::new(9), &PcaRequest { columns: vec![0], standardize: true })
                .is_err()
        );
    }

    #[test]
    fn correspondence_without_association_is_refused() {
        // Every row/column combination equally often: inertia zero, which
        // showed NaN percentages and an empty map.
        let a: Vec<String> = ["x", "x", "y", "y"].map(str::to_owned).to_vec();
        let b: Vec<String> = ["p", "q", "p", "q"].map(str::to_owned).to_vec();
        let t = DataTable::new("s", vec![Column::text("A", a), Column::text("B", b)]).unwrap();
        let e = CorrespondenceReport::run(&t, &RowStateManager::new(4), &CorrespondenceRequest { row: 0, column: 1 })
            .unwrap_err()
            .to_string();
        assert!(e.contains("no association"), "{e}");
    }

    #[test]
    fn correspondence_report() {
        // Greenacre's staff × smoking table expanded to one row per person.
        let counts = [[4, 2, 3, 2], [4, 3, 7, 4], [25, 10, 12, 4], [18, 24, 33, 13], [10, 6, 7, 2]];
        let (staff, smoke) = (["SM", "JM", "SE", "JE", "SC"], ["None", "Light", "Medium", "Heavy"]);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for (i, row) in counts.iter().enumerate() {
            for (j, &n) in row.iter().enumerate() {
                for _ in 0..n {
                    a.push(staff[i].to_owned());
                    b.push(smoke[j].to_owned());
                }
            }
        }
        let n = a.len();
        let t = DataTable::new("s", vec![Column::text("Staff", a), Column::text("Smoking", b)]).unwrap();
        let r = CorrespondenceReport::run(&t, &RowStateManager::new(n), &CorrespondenceRequest { row: 0, column: 1 })
            .unwrap();
        assert!((r.analysis().inertias[0] - 0.074_759_1).abs() < 1e-7);
        let s = r.sections();
        assert_eq!(s.len(), 4);
        let SectionContent::Plot(p) = &s[3].content else { panic!() };
        assert_eq!(p.annotations.as_ref().unwrap().items.len(), 9);
        assert!(
            CorrespondenceReport::run(&t, &RowStateManager::new(n), &CorrespondenceRequest { row: 0, column: 0 })
                .is_err()
        );
    }

    #[test]
    fn clustering_reports() {
        let t = table();
        let states = RowStateManager::new(9);
        for method in
            [ClusterMethod::KMeans { k: 2, seed: 1 }, ClusterMethod::Hierarchical { linkage: Linkage::Ward, k: 2 }]
        {
            let req = ClusterRequest {
                columns: vec![0, 1],
                method,
                standardize: true,
                choose: None,
                centers: None,
                distance: Default::default(),
            };
            let a = ClusterAnalysis::run(&t, &states, &req).unwrap();
            assert_eq!(a.labels, [0, 0, 0, 0, 1, 1, 1, 1], "{method:?}");
            let clusters = a.row_clusters(9);
            assert!(a.cluster_column()[8].is_nan() && a.cluster_column()[4] == 2.0);
            assert_eq!(clusters[0], Some(1));
            assert_eq!(clusters[8], None);
            let s = a.sections();
            let SectionContent::Table(m) = &s[1].content else { panic!() };
            assert_eq!(m.cell(1, "a"), Some("5.05"), "means in original units");
            if let ClusterMethod::Hierarchical { .. } = method {
                let SectionContent::Plot(d) = &s[3].content else { panic!() };
                assert_eq!(d.segments.len(), 7);
                assert!(d.line.is_some(), "cut line");
                assert_eq!(d.line_label(), "Cut height (2 clusters)");
                // Numeric cluster labels are named by the grouping title.
                let g = d.groups.as_ref().unwrap();
                assert_eq!((g.key_label(0), g.key_label(1)), ("Cluster 1".to_owned(), "Cluster 2".to_owned()));
            } else {
                assert!(s[0].title.contains("between-cluster SS"));
            }
            // Average silhouette per cluster in the summary.
            let SectionContent::Table(sum) = &s[0].content else { panic!() };
            assert_eq!(sum.columns.last().map(String::as_str), Some("Average silhouette"));
        }
    }

    /// Given starting centers, in the data's units, are standardized with the
    /// data; their number sets k, and the report lists them.
    #[test]
    fn k_means_from_given_centers() {
        let t = table();
        let states = RowStateManager::new(9);
        // Start the second group's center first, so cluster numbers follow the
        // rows, not the order of the centers.
        let centers = vec![vec![5.0, 8.0], vec![1.0, 2.0]];
        let req = ClusterRequest {
            columns: vec![0, 1],
            method: ClusterMethod::KMeans { k: 2, seed: 1 },
            standardize: true,
            choose: None,
            centers: Some(StartingCenters { centers: centers.clone(), also_seeded: false }),
            distance: Default::default(),
        };
        let a = ClusterAnalysis::run(&t, &states, &req).unwrap();
        assert_eq!(a.labels, [0, 0, 0, 0, 1, 1, 1, 1]);
        assert_eq!(a.title(), "Clusters (k-means from given centers, k = 2): a, b");
        let s = a.sections();
        let start = s.iter().find(|x| x.kind == SectionKind::ClusterStartingCenters).unwrap();
        assert!(start.title.contains("from these only"), "{}", start.title);
        let SectionContent::Table(c) = &start.content else { panic!() };
        assert_eq!(c.cell(0, "a"), Some("5"));
        let seeded = ClusterRequest { centers: Some(StartingCenters { centers, also_seeded: true }), ..req.clone() };
        let b = ClusterAnalysis::run(&t, &states, &seeded).unwrap();
        assert!(b.sections().iter().any(|x| x.title.contains("the given start gave")));
        let wrong =
            ClusterRequest { centers: Some(StartingCenters { centers: vec![vec![1.0]], also_seeded: false }), ..req };
        assert!(ClusterAnalysis::run(&t, &states, &wrong).is_err());
    }

    /// Three separated groups: the "Number of Clusters" section suggests 3
    /// by both criteria, with a gap plot and a silhouette plot; hierarchical
    /// clustering ignores the setting.
    #[test]
    fn choose_number_of_clusters() {
        let mut rng = stat_core::rng::Rng::new(4);
        let centres = [[0.0, 0.0], [5.0, 0.0], [2.5, 4.0]];
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in 0..60 {
            a.push(centres[i % 3][0] + 0.5 * rng.normal());
            b.push(centres[i % 3][1] + 0.5 * rng.normal());
        }
        let t = DataTable::new("t", vec![Column::numeric("a", a), Column::numeric("b", b)]).unwrap();
        let states = RowStateManager::new(60);
        let choose = Some(ChooseClusters { max_k: 5, references: 20, ..ChooseClusters::default() });
        let req = ClusterRequest {
            distance: Default::default(),
            columns: vec![0, 1],
            method: ClusterMethod::KMeans { k: 3, seed: 1 },
            standardize: false,
            choose,
            centers: None,
        };
        let a = ClusterAnalysis::run(&t, &states, &req).unwrap();
        let c = a.cluster_number().unwrap().as_ref().unwrap();
        assert_eq!((c.gap_choice, c.silhouette_choice), (3, Some(3)));
        let s = a.sections();
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            &kinds[..4],
            [
                SectionKind::ClusterSummary,
                SectionKind::ClusterNumber,
                SectionKind::GapPlot,
                SectionKind::SilhouettePlot
            ]
        );
        let SectionContent::Table(tab) = &s[1].content else { panic!() };
        assert_eq!(tab.rows.len(), 5);
        assert_eq!(tab.rows[2][6], "gap, silhouette");
        assert!(s[1].title.contains("is 3; largest average silhouette at k = 3"), "{}", s[1].title);
        let hier = ClusterRequest { method: ClusterMethod::Hierarchical { linkage: Linkage::Ward, k: 3 }, ..req };
        let h = ClusterAnalysis::run(&t, &states, &hier).unwrap();
        assert!(h.cluster_number().is_none());
    }
}
