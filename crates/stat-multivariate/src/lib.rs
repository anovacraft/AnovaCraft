//! # stat-multivariate
//!
//! Methods for several variables at once:
//!
//! * [`pca`] — principal component analysis via the singular value decomposition.
//! * [`correspondence`] — simple correspondence analysis of a two-way table.
//! * [`kmeans`] — k-means clustering with k-means++ seeding and several starts.
//! * [`cluster_number`] — gap statistic and average silhouette widths for
//!   choosing the number of k-means clusters; silhouette widths of any
//!   clustering.
//! * [`distance`] — distance matrices between rows: Euclidean, Manhattan,
//!   Gower (mixed types, missing values allowed), Jaccard, Bray–Curtis,
//!   Hamming and simple matching.
//! * [`hierarchical`] — agglomerative clustering (single, complete, average,
//!   Ward linkage) of data rows or of a distance matrix, with dendrogram
//!   cutting and drawing coordinates.
//! * [`correlations`] — Pearson and Spearman correlation matrices with tests
//!   and Fisher-z intervals, pairwise or listwise rows, partial correlations.
//! * [`outliers`] — Mahalanobis, jackknifed Mahalanobis and Hotelling T²
//!   distances with their beta and F reference limits.
//! * [`imputation`] — missing-value patterns and imputation: column mean or median,
//!   multivariate normal by the EM algorithm, and iterative low-rank SVD.
//! * [`multiple_imputation`] — `m` completed data sets by chained equations
//!   (predictive mean matching, Bayesian linear, logistic and multinomial
//!   regression draws) or by data augmentation under a multivariate normal
//!   model, with convergence traces.
//! * [`reliability`] — item analysis: Cronbach's alpha (raw and
//!   standardized) with Feldt's interval and item statistics.
//! * [`discriminant`] — linear and quadratic discriminant analysis with
//!   Fisher's canonical discriminant functions.
//! * [`factor`] — exploratory factor analysis: maximum-likelihood and
//!   principal-axis extraction, varimax and promax rotation, factor scores.
//! * [`mixture`] — Gaussian mixture clustering by EM with six covariance
//!   structures and BIC across structures and numbers of components.
//! * [`latent_class`] — latent class analysis of categorical items by EM,
//!   with standard errors, pattern goodness of fit and BIC across numbers
//!   of classes.
//! * [`embedding`], [`mds`], [`tsne`], [`umap`] — dimension reduction:
//!   classical and SMACOF metric scaling, t-SNE (exact or Barnes–Hut) and
//!   UMAP, with trustworthiness of an embedding.
//! * [`pcoa`] — principal coordinates analysis of any distance matrix, with
//!   Lingoes or Cailliez corrections for negative eigenvalues.

pub mod cluster_number;
pub mod correlations;
pub mod correspondence;
pub mod discriminant;
pub mod distance;
pub mod embedding;
pub mod factor;
pub mod hierarchical;
pub mod imputation;
pub mod kmeans;
pub mod latent_class;
pub mod mds;
pub mod mixture;
pub mod multiple_imputation;
pub mod outliers;
pub mod pca;
pub mod pcoa;
pub mod reliability;
mod spd;
mod sym_eigen;
pub mod tsne;
pub mod umap;

pub use cluster_number::{
    ClusterNumber, ClusterNumberRow, GapOptions, GapReference, Silhouette, choose_cluster_number, silhouette,
};
pub use correlations::{CorrelationMatrix, MissingRows, PairCorrelation, PartialCorrelations, correlation_matrix};
pub use correspondence::{CorrespondenceAnalysis, correspondence_analysis};
pub use discriminant::{CanonicalFunctions, DiscriminantFit, DiscriminantRule, WilksTest, fit_discriminant};
pub use distance::{DistanceColumn, DistanceMatrix, DistanceMetric, VariableKind, distance_matrix};
pub use embedding::{Proximity, standardize_columns, trustworthiness};
pub use factor::{Extraction, FactorFit, FactorOptions, FitTest, Rotation, ScoreMethod, factor_analysis};
pub use hierarchical::{Dendrogram, Linkage, Merge, hierarchical_clustering, hierarchical_from_distances};
pub use kmeans::{KMeans, KMeansOptions, k_means, k_means_from_centers};
pub use latent_class::{
    LatentClassCandidate, LatentClassFit, LatentClassOptions, LatentClassSelection, PatternFit, fit_latent_classes,
    select_latent_classes,
};
pub use mds::{ClassicalScaling, Smacof, SmacofOptions, classical_scaling, metric_scaling};
pub use mixture::{
    CovarianceStructure, GaussianMixture, MixtureCandidate, MixtureOptions, MixtureSelection, MixtureStart,
    fit_gaussian_mixture, select_gaussian_mixture,
};
pub use outliers::{DistanceLimits, OutlierDistances, outlier_distances};
pub use pca::{Pca, principal_components};
pub use pcoa::{NegativeCorrection, PrincipalCoordinates, principal_coordinates};
pub use reliability::{ItemAnalysis, ItemStatistics, feldt_interval, item_analysis};
pub use tsne::{Tsne, TsneMethod, TsneOptions, tsne};
pub use umap::{Umap, UmapOptions, umap};
