//! # stat-predictive
//!
//! Predictive modelling:
//!
//! * [`acquisition`]: the next runs of an experiment by expected improvement
//!   or an upper confidence bound, in batches by the constant liar;
//! * [`boost`]: gradient-boosted trees (squared, absolute, logistic and
//!   multinomial losses; histogram splits, Newton leaves, subsampling and
//!   early stopping);
//! * [`elastic_net`]: lasso, ridge and elastic net regression by coordinate
//!   descent along a path of penalties, the penalty chosen by k-fold
//!   cross-validation;
//! * [`gaussian_process`]: Gaussian-process regression (squared-exponential
//!   and Matérn 5/2 kernels with one length scale per input, nugget,
//!   maximum likelihood from several starts);
//! * [`mars`]: multivariate adaptive regression splines (forward pass of
//!   hinge pairs and their products, backward pruning by GCV or held-out
//!   rows);
//! * [`neural`]: fully connected neural networks with one or two hidden
//!   layers (L-BFGS or Adam, weight decay, several random starts);
//! * [`pls`]: partial least squares regression (NIPALS and SIMPLS) with
//!   cross-validated PRESS and VIP scores;
//! * [`svm`]: support vector classification and ε-regression by SMO
//!   (linear, radial basis and polynomial kernels; one-against-one classes;
//!   Platt probabilities with pairwise coupling; grid tuning);
//! * [`tree`]: classification and regression trees with surrogate splits
//!   and cost-complexity pruning;
//! * [`forest`]: random forests with out-of-bag error and permutation and
//!   impurity importance;
//! * [`validation`]: training / validation / test roles and k-fold
//!   assignments;
//! * [`measures`]: fit measures (RMSE, MAE, R², misclassification, log loss,
//!   AUC, confusion matrix) and ROC curves.

pub mod acquisition;
pub mod boost;
pub mod elastic_net;
pub mod forest;
pub mod gaussian_process;
pub mod mars;
pub mod measures;
pub mod neural;
pub mod pls;
pub mod svm;
pub mod tree;
pub mod validation;

pub use boost::{BoostLoss, BoostOptions, Boosted, RegressionLoss, cross_validate_rounds, fit_boosted};
pub use elastic_net::{ElasticNetOptions, ElasticNetPath, fit_elastic_net, fit_elastic_net_at};
pub use acquisition::{
    AcquisitionCriterion, Candidate, Goal, Objective, Proposal, ProposalOptions, SearchSpace, expected_improvement, propose_batch,
    upper_confidence_bound,
};
pub use forest::{Forest, ForestOptions, fit_forest};
pub use gaussian_process::{
    CovarianceKernel, GaussianProcess, GpHyperparameters, GpOptions, Nugget, fit_gaussian_process, log_marginal_likelihood,
};
pub use mars::{MarsFit, MarsOptions, fit_mars};
pub use measures::{ClassFit, ContinuousFit, auc, roc_curve};
pub use neural::{Activation, Layer, Network, NetworkOptions, NetworkTask, Output, Solver, fit_network};
pub use pls::{PlsAlgorithm, PlsFit, choose_factors, cross_validate_pls, fit_pls};
pub use svm::{
    Kernel, SvmClassifier, SvmMachine, SvmOptions, SvmTask, TuningSplit, fit_classifier, fit_svc, fit_svr, tune,
};
pub use tree::{
    Criterion, CrossValidation, Feature, PruneStep, Rule, Target, Tree, TreeOptions, Value, choose_step,
    cross_validate_pruning, fit_tree,
};
pub use validation::{Role, fold_assignment, grouped_roles, random_roles, stratified_roles};
