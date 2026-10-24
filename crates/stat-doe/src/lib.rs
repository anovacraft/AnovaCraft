//! # stat-doe
//!
//! Design of experiments. Every generator returns a [`Design`]: factors with
//! low/high settings, runs in coded units in standard order, the role of each
//! run, and notes (aliasing, efficiency). [`Design::to_table`] exports a design
//! as a [`stat_data::DataTable`] in coded or actual units, optionally in
//! randomized run order, with empty response columns ready for results.
//!
//! * [`factorial`] — full factorials (`2^k`, `3^k`, mixed levels) and two-level
//!   fractional factorials `2^(k−p)` with defining relation, resolution and
//!   aliases.
//! * [`factorial_analysis`] — effects, normal and half-normal plots, Lenth's method, dispersion
//!   effects and means of two-level factorial experiments.
//! * [`screening`] — Plackett–Burman designs and definitive screening designs
//!   built from conference matrices (with extra runs and center runs).
//! * [`categorical_screening`] — D-optimal screening designs mixing two-level
//!   continuous factors with categorical factors of any number of levels.
//! * [`simulate`] — simulated responses from random coefficients with effect
//!   sparsity, hierarchy and heredity.
//! * [`response_surface`] — central composite (circumscribed, inscribed,
//!   face-centred) and Box–Behnken designs.
//! * [`optimal`] — model formulas and D- and I-optimal designs by coordinate exchange.
//! * [`augment`] — fold-over, centre and axial runs, and D-optimal augmentation of an existing design.
//! * [`constraints`] — design regions cut by linear constraints and disallowed combinations.
//! * [`split_plot`] — D-optimal split-plot and split-split-plot designs for hard-to-change factors.
//! * [`mixture`] — simplex-lattice, simplex-centroid and extreme-vertices mixture designs and Scheffé models.
//! * [`orthogonal`] — standard orthogonal arrays (L4 … L27), signal-to-noise ratios and response tables.
//! * [`design`] — the design type, factor coding, export, seeded randomization.
//! * [`evaluation`] — design evaluation before data collection: power of each
//!   term, prediction variance (profile, maximum, average, fraction of design
//!   space), alias matrix, correlations, D/A/G/I efficiencies, comparing two
//!   designs.
//! * [`desirability`] — Derringer–Suich desirability functions for optimizing
//!   several responses at once.
//! * [`space_filling`] — space-filling designs for computer experiments:
//!   maximin Latin hypercubes, maximin and minimax distance designs, uniform designs.

pub mod augment;
pub mod categorical_screening;
pub mod constraints;
pub mod design;
pub mod desirability;
pub mod evaluation;
pub mod factorial;
pub mod factorial_analysis;
pub mod mixture;
pub mod optimal;
pub mod orthogonal;
pub mod response_surface;
pub mod screening;
pub mod simulate;
pub mod space_filling;
pub mod split_plot;
pub mod surface;

pub use categorical_screening::{
    CategoricalScreening, CategoricalScreeningOptions, ScreeningFactor, categorical_screening,
};
pub use augment::{Augmented, add_axial_runs, add_center_runs, augment_optimal, fold_over};
pub use constraints::Region;
pub use design::{Design, Factor, PointType, Rng, TableOptions, Units, default_factor_names};
pub use desirability::{Desirability, Goal, overall_desirability};
pub use evaluation::{
    DesignComparison, DesignEvaluation, EvaluationOptions, FractionOfDesignSpace, TermPower, compare_designs,
    default_alias_terms, evaluate_design, evaluate_design_in,
};
pub use factorial::{
    FractionalFactorial, fractional_factorial, fractional_factorial_with_generators, full_factorial, smallest_fraction,
    three_level_factorial, two_level_factorial,
};
pub use mixture::{BlendType, MixtureDesign, ScheffeModel, extreme_vertices, simplex_centroid, simplex_lattice};
pub use optimal::{
    ModelFormula, OptimalDesign, OptimalOptions, Term, average_prediction_variance, d_efficiency, d_optimal, i_optimal,
    log_det_information, region_moments,
};
pub use optimal::{d_optimal_in, i_optimal_in, region_moments_in};
pub use orthogonal::{LevelMeans, SignalToNoise, StandardArray, best_levels, is_orthogonal, level_means};
pub use response_surface::{AxialDistance, CcdOptions, box_behnken, central_composite, uniform_precision_centers};
pub use screening::{
    DsdOptions, DsdSummary, compare_definitive_screening, conference_matrix, definitive_screening,
    definitive_screening_with, dsd_comparison_choices, dsd_order, plackett_burman, plackett_burman_runs,
};
pub use space_filling::{SpaceFillingCriteria, SpaceFillingKind, space_filling_design};
pub use split_plot::{
    AugmentedSplitPlot, ChangeLevel, PlotStructure, SplitPlotAugmentOptions, SplitPlotDesign, SplitPlotOptions,
    augment_split_plot, split_plot_design, split_plot_log_det,
};
pub use surface::{CanonicalAnalysis, QuadraticSurface, RidgeDirection, RidgePoint, StationaryKind, symmetric_eigen};
