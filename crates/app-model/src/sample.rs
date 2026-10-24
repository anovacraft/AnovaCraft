//! Built-in sample datasets, chosen so that every analysis in the application
//! has data to try it on. [`catalog`] lists them with a description and the
//! reports each one opens with; [`project_samples`] lists sample projects of
//! several tables (with a dashboard).
//!
//! Real data come from the NIST Statistical Reference Datasets (public domain).
//! Every other dataset is **simulated** from a stated model with a fixed seed,
//! so it is identical every time and the analyses have known answers; these are
//! marked "(simulated)" and are not real measurements.

use crate::project::SavedReport;
use crate::{
    BivariateRequest, ControlChartOptions, ControlChartRequest, DistributionOptions, DistributionRequest,
    GaugeReportOptions, GaugeStudyRequest, LinearModelSpec, ReportOptions,
};
use stat_core::descriptive::QuantileMethod;
use stat_data::{Column, DataTable, MeasurementLevel, RowStateManager};
use stat_quality::ChartKind;
use stat_strd::{anova_datasets, linear_regression_datasets, univariate_datasets};

mod assay_templates;
mod bayes;
mod categorical_responses;
mod clinical_dropout;
mod clinical_visits;
mod competing_warranty;
mod curve_fitting;
mod dashboard_project;
mod forecasting;
mod functional;
mod latent_structure;
mod mixed_extras;
mod neural;
mod sequential;
pub(crate) mod nonlinear;
mod regression_extras;
mod shewhart;
mod splines;
mod tensile_tests;
pub use tensile_tests::{tensile_batch_a, tensile_project};
pub use categorical_responses::{SERVICE_RATING_SCALE, commute_modes, service_ratings};
pub use clinical_dropout::{TRIAL_COLUMNS, clinical_dropout_project, clinical_trial};
pub use competing_warranty::{monthly_warranty_returns, pump_seal_failures};
pub use dashboard_project::{ProjectSampleInfo, greenhouse_climate, greenhouse_project, project_samples};
pub use forecasting::{hotel_occupancy, kiln_fuel_feed, reactor_temperature};
pub use functional::exotherm_profiles;
pub use latent_structure::{ABILITY_TESTS, ability_tests, honey_origins};
pub use nonlinear::seedling_growth;
pub use mixed_extras::{capsule_stability, wine_bitterness};
pub use regression_extras::{glucose_methods, rotenone, tablet_stability};
pub use shewhart::{coil_thickness, short_run_machining, tablet_hardness, ward_safety};
mod accelerated;
mod monitoring;
pub use monitoring::{clean_room, sensor_batches};
pub use neural::polymer_batches;
pub use sequential::{HEAT_BUDGET, IMPURITY_FORMULA, YIELD_FORMULA, reaction_round_one};
pub use splines::fuel_consumption;
mod agreement;
pub use accelerated::{LASER_CURRENT, adhesive_bonds, device_a, laser_degradation};
pub use agreement::{GO_NO_GO_LIMIT, go_no_go_gauge, visual_inspection};
mod factorial;
mod gauge_designs;
mod studies;
pub use gauge_designs::{bore_diameter, weld_pull_strength};
mod charts;
mod design_comparison;
mod imputation;
mod latent_classes;
mod maps_and_mixtures;
mod multiple_testing;
mod resampling;
mod reviews;
mod screening;
mod survey;
pub use charts::{harbour_weather, household_budget, soil_texture};
mod tabulation;
mod visitor_survey;
pub use design_comparison::design_comparison;
pub use factorial::{coating_thickness, filtration_rate};
pub use imputation::{AIR_QUALITY_COLUMNS, air_quality};
pub use latent_classes::{SYMPTOM_ITEMS, symptom_checklist};
pub use maps_and_mixtures::{GENES, cell_populations, expression_profiles, town_distances};
pub use multiple_testing::{SCREEN_GENES, gene_screen, plasma_concentration, trial_endpoints};
pub use resampling::repair_times;
pub use reviews::espresso_reviews;
pub use screening::{COUNT_GENES, transcript_counts};
pub use studies::{gauge_linearity, gauge_repeatability, moulding_shrinkage};
pub use survey::hillside_survey;
pub use tabulation::multicentre_trial;
pub use visitor_survey::visitor_survey;
mod computer_experiment;
pub use computer_experiment::{BOREHOLE_INPUTS, borehole, borehole_flow};
mod split_plot;
pub use split_plot::furnace_request;
mod survival_models;
pub use survival_models::{capacitor_life_test, leukemia_covariates};
mod test_bench;
mod images;
mod time_series;
pub use test_bench::test_bench_signals;
pub use images::{blurred_shifted_scene, machined_surface_scan};
pub use time_series::{advertising_sales, electricity_demand, product_adoption};
mod signals;
pub use signals::{ecg_like, machine_vibration, raman_bands};
mod graph_examples;
mod treatment_study;
mod censored_values;
pub use censored_values::well_arsenic;
mod pk_profiles;
mod distances;
mod fatigue;
pub use fatigue::steel_fatigue;
pub use distances::shrub_traits;
pub use pk_profiles::pk_profiles;
pub use graph_examples::{parcel_flows, reading_trial, regional_climate};
pub use treatment_study::{treatment_study, treatment_study_sample};

/// A sample dataset with the reports it opens with.
#[derive(Debug, Clone)]
pub struct Sample {
    /// The data.
    pub table: DataTable,
    /// Reports to open, in display order.
    pub reports: Vec<SavedReport>,
}

/// One entry of the sample catalog.
#[derive(Debug, Clone, Copy)]
pub struct SampleInfo {
    /// Menu name.
    pub name: &'static str,
    /// What the data are and which features they exercise.
    pub description: &'static str,
    /// Build the dataset and its starting reports.
    pub build: fn() -> Sample,
    /// The menu group the sample is listed under.
    pub topic: SampleTopic,
}

/// Groups of the sample catalog, named after the menus whose analyses the
/// samples show, in menu order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleTopic {
    /// Describe and Compare: distributions, tests and tables.
    DescribeCompare,
    /// Model: regression, generalized and predictive models.
    Model,
    /// Curve fitting: nonlinear regression and dose–response.
    Curves,
    /// Bayesian estimation, Bayes factors and Bayesian models.
    Bayesian,
    /// Multivariate methods.
    Multivariate,
    /// Designed experiments.
    Design,
    /// Control charts, capability and other quality tools.
    Quality,
    /// Measurement systems analysis.
    Measurement,
    /// Reliability and survival.
    Reliability,
    /// Time series.
    TimeSeries,
    /// Graphs.
    Graphs,
}

impl SampleTopic {
    /// Every group, in menu order.
    pub const ALL: [Self; 11] = [
        Self::DescribeCompare,
        Self::Model,
        Self::Curves,
        Self::Bayesian,
        Self::Multivariate,
        Self::Design,
        Self::Quality,
        Self::Measurement,
        Self::Reliability,
        Self::TimeSeries,
        Self::Graphs,
    ];

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::DescribeCompare => "Describe and compare",
            Self::Model => "Models",
            Self::Curves => "Curve fitting",
            Self::Bayesian => "Bayesian analysis",
            Self::Multivariate => "Multivariate",
            Self::Design => "Designed experiments",
            Self::Quality => "Quality and control charts",
            Self::Measurement => "Measurement systems",
            Self::Reliability => "Reliability and survival",
            Self::TimeSeries => "Series and signals",
            Self::Graphs => "Graphs",
        }
    }
}

/// Every sample dataset, in menu order.
#[must_use]
pub fn catalog() -> Vec<SampleInfo> {
    vec![
        SampleInfo {
            name: "Longley economy (NIST) — multiple regression, PCA, lasso",
            description: "NIST StRD. Multiple regression with highly collinear predictors; parameter estimates, \
                          effect tests, diagnostics and the Response Explorer.",
            build: longley_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Norris calibration (NIST) — simple regression, diagnostics",
            description: "NIST StRD. Simple linear regression of 36 instrument readings on reference values: \
                          fitted line, confidence band, residuals and influence diagnostics.",
            build: norris_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Plant growth (simulated) — ANCOVA, outliers",
            description: "Analysis of covariance: height by fertilizer (text), light hours (continuous) and \
                          greenhouse (numeric codes marked nominal), with two outliers to find in the diagnostics; \
                          mixed models with greenhouses as random, one with a residual variance for each \
                          fertilizer and robust standard errors clustered by greenhouse.",
            build: plant_growth_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Catalyst factorial (simulated) — interactions, quadratic terms, 3-D profiler",
            description: "A replicated 2³ factorial with center points: yield by temperature, pressure and \
                          catalyst, fitted with a temperature × pressure interaction and a squared temperature \
                          term. Explore it with the Response Explorer.",
            build: catalyst_sample,
            topic: SampleTopic::Design,
        },
        SampleInfo {
            name: "Michelson light speed (NIST) — distribution, one-sample tests",
            description: "NIST StRD. 100 measurements of the speed of light: summary statistics, quantiles, \
                          normal fit, normality tests, the normal quantile plot, and one-sample tests against \
                          the modern value (299.792 458 thousand km/s), and a tolerance interval for 90% of \
                          measurements.",
            build: michelson_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Clinic wait times (simulated) — skewed data, Box–Cox, Welch & rank tests, formulas",
            description: "Right-skewed waiting times with missing values at three clinics and two shifts: \
                          non-normal distributions, a Box–Cox transformation, unequal variances, Welch and rank \
                          tests. Two formula columns compute the log wait and each wait relative to its \
                          clinic's median.",
            build: wait_times_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Silicon resistivity (NIST) — one-way ANOVA",
            description: "NIST StRD SiRstv: five readings on each of five instruments. One-way ANOVA, Welch's \
                          test, equal-variance and rank tests, and Mood's median test (exclude three instruments for \
                          two-sample t-tests).",
            build: resistivity_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Customer survey (simulated) — contingency table, correspondence",
            description: "Categorical data: region by preferred channel (contingency table, χ² tests, \
                          correspondence analysis), an \
                          ordinal satisfaction score, and spending by age.",
            build: survey_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Fill weights (simulated) — X̄/R, phases, EWMA, CUSUM, capability",
            description: "30 subgroups of 5 bottle weights; the mean shifts upward from subgroup 21, when the \
                          filler setting changed, and one subgroup has a spike. X̄ and R chart with run rules, \
                          the same chart with limits per filler setting, EWMA and CUSUM charts against the \
                          500 g target, and capability against 495–505 g.",
            build: fill_weights_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Beam deflection (NIST) — individuals chart, runs tests",
            description: "NIST StRD Lew: 200 strongly oscillating deflection readings. On the individuals and \
                          moving range chart the swings inflate the moving ranges, so the limits are wide and no \
                          run rule fires; the distribution's test of runs up and down detects the oscillation \
                          that the chart misses.",
            build: lew_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Gauge study (simulated) — gauge R&R, intraclass correlation",
            description: "10 parts × 3 operators × 3 replicates: ANOVA gauge R&R with variability plots, and \
                          the intraclass-correlation evaluation of the same study.",
            build: gauge_sample,
            topic: SampleTopic::Measurement,
        },
        SampleInfo {
            name: "Flower measurements (simulated) — PCA, clustering, PLS-DA, correlations, outlier distances, MANOVA",
            description: "Four size measurements on 90 flowers of three varieties: principal components, \
                          k-means and hierarchical clustering (compare the clusters with the Variety column), \
                          correlations with partial correlations, a scatterplot matrix coloured by variety, \
                          Mahalanobis outlier distances, a MANOVA of the four measurements by variety with a \
                          contrast and canonical plot, and a PLS discriminant analysis predicting the variety by \
                          10-fold cross-validation.",
            build: flowers_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Customer churn (simulated) — logistic & Poisson GLM, lasso, profilers, proportions",
            description: "400 customers: logistic regression of churn on tenure, monthly charge and contract \
                          type, Poisson regression of support calls, and a lasso for spending.",
            build: churn_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Leukemia remission (1963) — Kaplan–Meier, log-rank",
            description: "Weeks in remission for 42 patients on 6-MP or placebo, with right censoring: \
                          Kaplan–Meier curves, median survival, log-rank test and Weibull fits.",
            build: leukemia_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Bearing life test (simulated) — Weibull & lognormal life fits",
            description: "60 fan bearings from two suppliers run until failure or the 3000-hour cut-off \
                          (some removed early): survival curves, Weibull and lognormal fits.",
            build: bearings_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Sleep drug trial (1908) — paired t, two-group tests, power",
            description: "Extra hours of sleep for 10 patients on each of two drugs, stacked in one column with \
                          Drug and Patient columns: paired and two-group comparisons, and a sample-size plan.",
            build: sleep_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Reaction yield — response surface, contours, profilers",
            description: "Yield of a chemical process at 13 settings of reaction time and temperature from a \
                          rotatable central composite design: response-surface fit, stationary point (a maximum), \
                          canonical and ridge analysis, and a contour plot.",
            build: reaction_yield_sample,
            topic: SampleTopic::Design,
        },
        SampleInfo {
            name: "Reaction yield (simulated) — sequential optimization, Gaussian processes",
            description: "A reaction optimized in rounds: temperature (60–120 °C), time (10–90 min), catalyst \
                          (0.5–3%) and solvent, within a heat budget (Temperature + 0.5·Time ≤ 125). Round 1 is a \
                          12-run Latin hypercube; round 2 is the four runs the yield report proposed after it. \
                          Yield and Impurity are formula columns simulating the reaction, so choosing Add proposed \
                          runs in the report's menu fills in their results at once and the report proposes the \
                          next round: expected improvement of yield, the yield–impurity trade-off by desirability, \
                          and a Gaussian-process model of yield with its Response Explorer.",
            build: sequential::reaction_yield_sample,
            topic: SampleTopic::Design,
        },
        SampleInfo {
            name: "Coating formulation (simulated) — several responses, stepwise terms, desirability",
            description: "Hardness, gloss and cost of a coating at 20 settings of cure temperature, cure time \
                          and catalyst from a rotatable central composite design: quadratic models of all three \
                          responses explored together, desirability (hardness up, gloss on target, cost down), \
                          the best settings, and a simulated defect rate under process variation; a second \
                          report selects each response's own terms stepwise and keeps the same goals.",
            build: coating_sample,
            topic: SampleTopic::Design,
        },
        SampleInfo {
            name: "Yarn elongation — mixture model, mixture profiler, ternary plot",
            description: "Elongation of yarn spun from blends of three polymers on a replicated simplex-lattice \
                          {3, 2} design: quadratic Scheffé model, nonlinear-blending tests and a ternary contour plot.",
            build: yarn_sample,
            topic: SampleTopic::Design,
        },
        SampleInfo {
            name: "Daily inspection (simulated) — p, u and c attribute charts",
            description: "30 days of final inspection with varying lot sizes: defective units (p chart with \
                          limits that vary with lot size; day 18 is out of control), surface flaws per unit \
                          inspected (u chart) and flaws on a fixed audit panel (c chart).",
            build: inspection_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Hald cement (1932) — stepwise selection, best subsets, profilers, collinearity",
            description: "Heat evolved by 13 cement batches against four ingredient percentages that nearly sum \
                          to a constant: mixed stepwise selection by p-value, the best models of each size by \
                          best subsets, and the full model whose estimates are unstable because the predictors \
                          are collinear.",
            build: hald_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Loan applications (simulated) — trees, forest, boosting, model comparison",
            description: "600 loan applicants with credit history, employment, income (some missing), debt \
                          ratio, age and loan amount, split into training, validation and test rows: a \
                          classification tree of the outcome (repaid, late or default) sized on the validation \
                          rows, a random forest and boosted trees of the same outcome, the three models' saved \
                          default probabilities compared on the test rows (ROC, lift, gains, calibration), and a regression tree of the \
                          interest rate sized by 10-fold cross-validation.",
            build: loans_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Fuel spectra (simulated) — partial least squares",
            description: "Near-infrared absorbance of 60 fuel samples at 40 wavelengths, built from three \
                          constituents with overlapping absorption bands and a varying baseline, with each \
                          sample's octane number: partial least squares picks a few factors from the 40 \
                          strongly correlated predictors by 10-fold cross-validation, with VIP, loadings and \
                          score plots.",
            build: fuel_spectra_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Pump sensors (simulated) — support vector machines",
            description: "400 pump readings (temperature, vibration, pressure, production line) with a fault \
                          flag that is set outside an elliptical safe zone, and an efficiency that curves with \
                          temperature: support vector machines with radial basis and linear kernels tuned on the \
                          validation rows and compared on the test rows (the curved boundary needs the radial \
                          kernel), and an ε-support vector regression of efficiency tuned by 5-fold \
                          cross-validation.",
            build: pumps_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Fuel consumption (simulated) — multivariate adaptive regression splines",
            description: "500 test drives (speed, load, ambient temperature, tyre pressure, fuel type) with fuel \
                          consumption that bends at 50 and 80 km/h, rises with load faster above 80 km/h and in the \
                          cold, and falls for diesel and hybrid cars: adaptive spline models with and without interactions \
                          sized on the validation rows, boosted trees, and the three models' predictions compared \
                          on the test rows.",
            build: splines::fuel_consumption_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Polymer batches (simulated) — neural networks and model search",
            description: "600 polymer batches (cure temperature, pressure, catalyst, mixing time, resin supplier) \
                          with a tensile strength that rises smoothly through a temperature threshold, peaks at a \
                          middle pressure and gains most from the catalyst when the cure is cool, and a grade \
                          (Reject, Standard, Premium) judged from it: neural networks of strength and grade with \
                          the penalty tuned on the validation rows, boosted trees of strength, the two strength \
                          models compared on the test rows, and a search that fits every applicable model of \
                          strength and ranks them on the validation rows.",
            build: neural::polymer_batches_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Rosner's measurements (1983) — outlier tests",
            description: "The 54 values of Rosner's generalized ESD example (NIST e-Handbook §1.3.5.17): the \
                          distribution with the generalized ESD test, which finds three outliers among the \
                          largest values where Grubbs' single-outlier test finds none.",
            build: rosner_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Blood counts (Dunnett 1955) — comparisons with a control",
            description: "Blood counts (millions of cells per mm³) of animals given a control or one of two \
                          drugs, from Dunnett's 1955 paper: each drug compared with the control by Dunnett's \
                          method, with simultaneous intervals.",
            build: blood_counts_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Plastic film (Johnson & Wichern) — MANOVA",
            description: "Tear resistance, gloss and opacity of 20 plastic film samples made at two extrusion \
                          rates with two amounts of an additive (Johnson & Wichern, Example 6.12): a two-way \
                          multivariate analysis of variance with Wilks, Pillai, Hotelling–Lawley and Roy tests, \
                          each response's own F-tests and the canonical analysis of each effect.",
            build: plastic_film_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Seal strength (simulated) — distribution identification",
            description: "Peel strength of 150 heat seals on pouches, from three shifts: skewed data with a \
                          floor of about 20 N. Fourteen distributions fitted and ranked, where the Weibull with \
                          a threshold fits best and the normal is rejected, with the Johnson and Box–Cox \
                          transformations alongside.",
            build: seal_strength_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Coating batches (simulated) — between/within capability",
            description: "Coating thickness (µm) of 5 panels from each of 25 batches, whose means drift from \
                          batch to batch: between/within capability separates the batch-to-batch from the \
                          within-batch variation, and the ordinary capability study beside it shows how the \
                          within-batch σ alone overstates the process.",
            build: coating_batches_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Supplier lots (simulated) — attribute capability, acceptance sampling",
            description: "40 incoming lots with the units inspected, defective units and cosmetic defects: \
                          binomial capability (percent defective with an exact interval and process Z) and \
                          Poisson capability (defects per unit), and an acceptance sampling plan designed from \
                          risk points of 1% and 6% defective, compared with two hand-picked plans.",
            build: supplier_lots_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Solder defects (simulated) — Pareto chart, cause-and-effect diagram",
            description: "Solder defects on two assembly lines over a month, each traced to a cause (with its \
                          category and sub-cause): a Pareto chart of the causes per line, where nozzle wear \
                          leads on line A and the reflow profile on line B and the rarest causes pool into \
                          \"Other\", and the cause-and-effect diagram the causes were collected on.",
            build: solder_defects_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Shaft diameters (simulated) — multi-vari chart",
            description: "Diameters of turned shafts measured at three positions, five shafts from each of \
                          three batches: multi-vari charts showing that batch-to-batch (time) variation \
                          dominates, with a steady taper along each shaft and little difference between \
                          shafts of a batch.",
            build: shaft_diameters_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Coil thickness (simulated) — three-way, moving-average and zone charts",
            description: "Strip thickness, 4 readings on each of 30 coils whose means differ more than the \
                          readings within a coil: the X̄ and R chart flags most coils, while the three-way \
                          (between/within) chart judges the coil means against their own variation. Line-speed \
                          readings with a sustained shift from reading 101 on a moving-average chart and a zone \
                          chart.",
            build: shewhart::coil_thickness_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Short-run machining (simulated) — short-run Z and moving range chart",
            description: "Bore diameters of three part numbers with different nominal sizes and spreads, \
                          machined in batches on one lathe: each value standardized by its own part's mean and \
                          σ so that all three share one chart, where the parts made after a tool slipped stand \
                          out.",
            build: shewhart::short_run_machining_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Ward safety (simulated) — Laney p′ and u′, g and t charts",
            description: "30 weeks on a hospital ward: readmissions among thousands of discharges (the p chart \
                          flags ordinary week-to-week variation; Laney's p′ chart does not), medication errors \
                          per patient day (u′ chart), procedures between infections (g chart, with one long \
                          run) and days between pressure injuries (t chart, whose run rules catch them becoming \
                          more frequent).",
            build: shewhart::ward_safety_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Tablet hardness (simulated) — capability summary page, bootstrap intervals",
            description: "Crushing strength of 100 tablets from 25 press samples, skewed to the right, against \
                          a 5–13 kp specification: the capability summary page with a lognormal model, control \
                          charts, the last samples, histogram and probability plot, and bootstrap intervals for \
                          the percentile indices; and a graph of the tablets by press sample with the \
                          specification as a shaded band and a reference line at the mean.",
            build: shewhart::tablet_hardness_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Bottle filling (simulated) — multivariate control charts",
            description: "Fill volume, cap torque and wall thickness of 100 bottles in 25 subgroups of four; the \
                          three are correlated. The first 20 subgroups are the in-control reference: a Hotelling \
                          T² chart of the subgroups flags subgroup 23, whose volume ran high without the torque \
                          that usually goes with it (volume contributes most), the generalized variance |S| chart \
                          flags subgroup 25, whose spread grew, and a MEWMA chart of the individual bottles \
                          follows the same shifts.",
            build: bottle_filling_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Reactor sensors (simulated) — principal-components control chart",
            description: "Eight correlated sensors on a batch reactor, 100 readings in ten batches; the first 60 \
                          are normal operation. A principal-components model of the reference (two components, \
                          chosen by cross-validation) charts T² and the squared prediction error: the coolant \
                          sensor drifts from reading 71 without the others following, which the SPE chart and its \
                          contributions point at, and the production rate runs high from reading 86, which the \
                          T² chart catches; the change-point estimate dates the shift.",
            build: monitoring::sensor_batches_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Clean-room monitoring (simulated) — alert and action limits",
            description: "Forty days of environmental monitoring at ten locations in grade A–D rooms (one room \
                          grade missing): air colony counts, contact plates positive out of plates taken, and \
                          particle counts. Alert and action levels by room grade from the best-fitting count \
                          distribution, binomial levels for the contact plates, and 2σ/3σ levels for particles \
                          by location; the operator station has two action excursions, the material airlock's \
                          counts rise for six days, and the corridor's particle counts jump.",
            build: monitoring::clean_room_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Heat treatment (simulated) — analysis of means",
            description: "60 lots from four furnaces on three shifts: hardness, parts cracked out of those \
                          inspected, and surface pits over the area inspected. Analysis of means of hardness by \
                          furnace (furnace 3 runs hard, furnace 4 soft) and by furnace and shift (furnace 2 on \
                          the night shift stands out as an interaction), of the cracked proportion by furnace \
                          (furnace 2 above the limits, furnace 1 below) and of the pit rate by shift (night \
                          above).",
            build: heat_treatment_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Ability tests (simulated) — factor analysis",
            description: "Scores of 240 students on nine ability tests driven by three correlated abilities \
                          (verbal, quantitative, spatial): maximum-likelihood factor analysis with the number of \
                          factors from the eigenvalues above 1, a varimax rotation and the chi-square test of fit, \
                          and a principal-axis solution with an oblique promax rotation and its factor \
                          correlations.",
            build: latent_structure::ability_tests_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Honey origins (simulated) — discriminant analysis",
            description: "Moisture, sugars, conductivity, proline and pH of 180 honey samples from three origins, \
                          with a Validation column (training, validation and test rows): linear discriminant \
                          analysis with its canonical plot, classification tables by role and posterior \
                          probabilities, and a quadratic rule with equal priors for comparison.",
            build: latent_structure::honey_origins_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Visitor survey (simulated) — multiple response",
            description: "160 museum visitors in three age groups ticked every way they had heard of an \
                          exhibition (a check-all-that-apply question kept as one column of lists): how often \
                          each item was ticked as a share of visitors and of ticks, the cross-tab by age group \
                          and a chi-square test per item with a Bonferroni-adjusted overall test. The comments \
                          column has long text for the data grid's text pane.",
            build: visitor_survey::visitor_survey_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Course evaluation (simulated) — item analysis, reverse-scored items",
            description: "Six 1–5 agreement items from 120 students, two of them worded negatively: Cronbach's \
                          alpha with the negative items reverse-scored, item–rest correlations, alpha if an item \
                          is deleted, and the inter-item correlations (Spearman's ρ for the ordinal answers).",
            build: course_evaluation_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Gauge linearity (simulated) — bias and linearity",
            description: "Five reference parts (2 to 10 mm) each measured 12 times by a gauge that reads high at \
                          the low end and low at the high end: bias at each reference value, the linearity line \
                          with its band (which excludes zero), %linearity and %bias.",
            build: studies::gauge_linearity_sample,
            topic: SampleTopic::Measurement,
        },
        SampleInfo {
            name: "Gauge repeatability (simulated) — Cg and Cgk",
            description: "A 25 mm master part measured 50 times on one gauge that reads slightly high: Cg, Cgk \
                          and the bias test against a tolerance of 0.1 mm, and the run chart of the readings.",
            build: studies::gauge_repeatability_sample,
            topic: SampleTopic::Measurement,
        },
        SampleInfo {
            name: "Weld pull test (simulated) — nested (destructive) gauge R&R",
            description: "A destructive test: three operators each pull three coupons from five batches of \
                          their own (batches numbered 1–5 within each operator). Nested gauge R&R with parts \
                          within operators, the gauge run chart in test order and the variability chart by \
                          operator.",
            build: gauge_designs::weld_pull_sample,
            topic: SampleTopic::Measurement,
        },
        SampleInfo {
            name: "Bore gauges (simulated) — expanded gauge R&R, unbalanced",
            description: "Eight parts measured twice by three operators on each of two bore gauges, with seven \
                          readings lost. Expanded gauge R&R by REML with the gauge as a random factor and all \
                          two-factor interactions, variance components with intervals, and the gauge run chart.",
            build: gauge_designs::bore_diameter_sample,
            topic: SampleTopic::Measurement,
        },
        SampleInfo {
            name: "Moulding shrinkage (simulated) — orthogonal array, signal-to-noise",
            description: "An L8 inner array of four control factors crossed with an L4 outer array of two noise \
                          factors (8 runs × 4 noise conditions). The smaller-the-better signal-to-noise analysis \
                          finds that high pressure makes shrinkage insensitive to humidity; the table is also \
                          declared as a design to show its orthogonal factors.",
            build: studies::moulding_shrinkage_sample,
            topic: SampleTopic::Design,
        },
        SampleInfo {
            name: "Rotenone (Martin 1942) — probit analysis",
            description: "Aphids sprayed with five concentrations of rotenone and the number killed, Finney's \
                          worked example of probit analysis: the probit line on log₁₀ concentration, the \
                          concentrations killing 10%, 50% and 90% with fiducial limits, and a χ² test of fit. \
                          The unsprayed control is left out on the log scale.",
            build: regression_extras::rotenone_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Glucose methods (simulated) — Deming and Passing–Bablok regression",
            description: "Glucose in 40 specimens by a laboratory analyzer and a point-of-care meter: Deming \
                          regression with jackknife intervals and Passing–Bablok regression with its linearity \
                          test, both showing a proportional difference of about 3.5%, and a difference plot. \
                          One specimen has a gross meter error.",
            build: regression_extras::glucose_methods_sample,
            topic: SampleTopic::Measurement,
        },
        SampleInfo {
            name: "Tablet stability (simulated) — shelf-life estimation",
            description: "Assay of three tablet batches over 24 months against a lower limit of 95% of label \
                          claim: the poolability tests keep a common slope but separate intercepts, and the \
                          shelf life is set by batch C, where its one-sided 95% lower confidence bound meets \
                          the limit.",
            build: regression_extras::tablet_stability_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Wine bitterness (Randall 1989) — ordinal mixed model",
            description: "Nine judges rated the bitterness of eight wines on a five-point scale; the wines were \
                          made cold or warm, with or without skin contact. A cumulative logit model with a random \
                          intercept for each judge: thresholds, odds ratios of a more bitter rating for warm \
                          fermentation and skin contact, the judge-to-judge variance and the predicted \
                          probability of each rating.",
            build: mixed_extras::wine_bitterness_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Capsule stability, six lots (simulated) — shelf life with random lots",
            description: "Assay of six capsule lots over 24 months against a lower limit of 95% of label claim, \
                          with the lots treated as a random sample of production: random intercepts and slopes, \
                          the shelf life where the one-sided 95% lower prediction bound for a future lot meets \
                          the limit (shorter than the bound for the average lot), beside the fixed-lot ICH Q1E \
                          analysis.",
            build: mixed_extras::capsule_stability_sample,
            topic: SampleTopic::Quality,
        },
        SampleInfo {
            name: "Service ratings (simulated) — ordinal logistic regression",
            description: "Invented five-point satisfaction ratings (Poor to Excellent) of 150 customer contacts \
                          with the wait in minutes and the channel used: a proportional-odds model in which \
                          longer waits lower the odds of a better rating, odds ratios per minute and by channel, \
                          the score test of parallel lines, and the predicted probability of each rating \
                          against the wait.",
            build: categorical_responses::service_ratings_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Commute modes (simulated) — nominal logistic regression",
            description: "Invented usual commuting modes (walk, bicycle, bus, car) of 200 people with the \
                          distance travelled and their age: baseline-category logits against car, odds ratios \
                          for each mode, likelihood-ratio tests of distance and age, and the predicted share of \
                          each mode by distance.",
            build: categorical_responses::commute_modes_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Seedling growth (simulated) — nonlinear regression",
            description: "Heights of two seedlings every three days for 45 days (invented data from a logistic \
                          curve). A three-parameter logistic fit gives the final height, growth rate and the day \
                          of fastest growth with asymptotic and profile intervals, the fitted curve with \
                          confidence and prediction bands, and residuals that spread out as the plants grow.",
            build: nonlinear::seedling_growth_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Compound potency (simulated) — global curve fit",
            description: "Responses of two compounds at eleven log concentrations in triplicate (invented data). \
                          A log-dose response curve fitted to both compounds at once, with the bottom, top and \
                          Hill slope shared and one LogEC50 per compound; the model comparison tests whether the \
                          compounds differ at all and whether their LogEC50 values differ.",
            build: curve_fitting::compound_potency_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Assay standard curve (simulated) — inverse prediction",
            description: "Absorbances of eight standards in duplicate and six unknown samples (invented data). \
                          A dose–response curve with 1/Y² weights; a robust fit finds the mis-pipetted standard, \
                          which is removed before the final fit, and the unknowns' concentrations are read off \
                          the curve with intervals.",
            build: curve_fitting::standard_curve_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Substrate depletion (simulated) — differential equation fit",
            description: "Substrate concentration of an enzyme reaction every 2.5 minutes (invented data). The \
                          Michaelis–Menten rate equation dS/dt = −Vmax·S/(Km + S) has no closed-form solution in \
                          time, so it is fitted by solving the equation at every step of the fit.",
            build: curve_fitting::substrate_depletion_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "ELISA plate (simulated) — standard curve and sample concentrations",
            description: "Eight standards in duplicate, two blanks and twelve diluted samples on one plate. The \
                          four-parameter logistic standard-curve template (1/Y² weights) back-calculates each \
                          standard against the ICH M10 limits, draws the precision profile with the quantitation \
                          range where the CV is at most 20%, and reads the samples off the curve times their \
                          dilution factor; one sample is above the range and one below it.",
            build: assay_templates::elisa_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Receptor saturation (simulated) — total and nonspecific binding",
            description: "Total and nonspecific binding of a radioligand at eight concentrations in triplicate, \
                          in one table with a Measured column. The saturation-binding template fits both curves \
                          at once and gives Bmax and Kd of the specific binding.",
            build: assay_templates::saturation_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Competition binding (simulated) — IC50 and Ki",
            description: "Bound radioligand (1 nM, Kd 0.5 nM) against thirteen log concentrations of an \
                          unlabelled competitor in triplicate. The one-site competition template gives the IC50 \
                          and the inhibition constant Ki by the Cheng–Prusoff equation, with intervals.",
            build: assay_templates::competition_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Enzyme inhibition (simulated) — inhibition mechanism",
            description: "Initial rates at seven substrate and four inhibitor concentrations. The mixed-inhibition \
                          template fits all the curves together and compares competitive, uncompetitive, \
                          noncompetitive and mixed inhibition by AICc; the data favour a mostly competitive \
                          inhibitor.",
            build: assay_templates::inhibition_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Antagonist shift (simulated) — Schild analysis",
            description: "Agonist concentration–response curves with no antagonist and at three antagonist \
                          concentrations. The Gaddum–Schild template fits pA2 and the Schild slope to all curves \
                          at once, tests whether the slope is 1, and draws the Schild plot of dose ratios.",
            build: assay_templates::schild_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Bacterial growth (simulated) — modified Gompertz growth curves",
            description: "ln(N/N0) of two strains every hour for a day. The modified Gompertz template fits each \
                          strain's lag time, maximum specific growth rate and asymptote, with doubling times, and \
                          the model comparison tests whether the strains differ.",
            build: assay_templates::growth_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Clonogenic survival (simulated) — linear-quadratic model",
            description: "Colonies counted from cells plated at six radiation doses in triplicate. The \
                          linear-quadratic template fits the plating efficiency, α and β (1/Y weights for \
                          counts) and gives α/β, the surviving fraction at 2 Gy and the dose for 10% survival.",
            build: assay_templates::clonogenic_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Potency assay (simulated) — relative potency",
            description: "A reference standard and a test lot at ten log concentrations in duplicate. The \
                          relative-potency template fits parallel curves, tests parallelism against separate \
                          curves, and gives the test lot's potency relative to the reference (about 0.56) with \
                          its interval.",
            build: assay_templates::potency_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Bioequivalence crossover (simulated) — crossover equivalence",
            description: "24 subjects each take a reference (R) and a test (T) tablet in two periods, half in \
                          the order R then T and half T then R; the area under the concentration curve (AUC) \
                          is recorded. The crossover report estimates the treatment, period and carryover \
                          effects and shows the 90% interval for the T/R ratio of geometric means inside \
                          0.80–1.25 (average bioequivalence).",
            build: bioequivalence_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Reaction times by task and cue (simulated) — factorial repeated measures",
            description: "20 people, 10 younger and 10 older, each do three tasks with a valid or an invalid cue; \
                          one row per person, task and cue holds the mean reaction time (ms). The repeated-measures \
                          report tests age group (between subjects), task and cue (within) and their \
                          interactions, with Mauchly's test and Greenhouse–Geisser and Huynh–Feldt corrections \
                          for task, and the task effect within each age group (older people slow down more on the \
                          choice task).",
            build: reaction_times_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Dendrite lengths per mouse (simulated) — nested comparison",
            description: "Dendrite lengths (µm) of 6 neurons from each of 4 mice per treatment (control and \
                          two drugs). Neurons from one mouse are not independent, so the nested comparison tests \
                          treatments against the variation between mice (a mixed model with Satterthwaite degrees \
                          of freedom, and the classical nested ANOVA, since the design is balanced) and shows how \
                          much of the variance lies between mice.",
            build: dendrites_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Horse-kick deaths (von Bortkiewicz 1898) — goodness of fit for counts",
            description: "Deaths from horse kicks in 10 Prussian army corps over 20 years (200 corps-years) as a \
                          frequency table. The goodness-of-fit report compares the counts with a Poisson \
                          distribution with the mean estimated (0.61 per corps-year); the fit is close.",
            build: horse_kicks_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Paint drying times (simulated) — comparisons with the best",
            description: "Drying time of five paint formulations on six panels each. Hsu's comparisons with the \
                          best (smaller is better) rule out the slow formulations and leave the fastest ones \
                          as candidates, in the group comparison and in the one-way model.",
            build: drying_times_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Prototype failures (simulated) — reliability growth",
            description: "Cumulative test hours at each failure of a prototype in two phases, with end-of-phase \
                          records: a power-law (Crow–AMSAA) fit of the whole test and of each phase (the redesign \
                          at 1500 hours lowers the failure intensity), with intensity and MTBF plots and the \
                          Cramér–von Mises test.",
            build: reliability_growth_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Pump repairs (simulated) — repairable systems, mean cumulative function",
            description: "Repair ages of 24 pumps of an old and a new design, each with its end of observation: \
                          mean cumulative repairs per pump with confidence limits, the difference between the \
                          designs, constant-rate and power-law fits and trend tests (old pumps wear out; new ones \
                          hold a steady rate).",
            build: repairable_systems_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Seal inspections (invented) — interval-censored life distributions",
            description: "Invented data: seals from two suppliers inspected every 250 hours up to 2000, each row \
                          counting the units found failed between two inspections (or before the first, or still \
                          sound at the end), plus a few failures seen exactly in service. Censored maximum \
                          likelihood fits compared by AIC and Anderson–Darling, Turnbull's estimate, a Weibull \
                          probability plot with limits, percentiles and failure probabilities per supplier.",
            build: seal_inspection_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Bearing endurance test (invented) — Weibull with known shape",
            description: "Invented data: twelve bearings run to 1500 hours, two failing. With so few failures the \
                          Weibull with a known shape (β = 2) gives a lower bound on the scale and on the time to \
                          10% failing, alongside the ordinary fits.",
            build: bearing_test_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Pump failure modes (simulated) — competing risks",
            description: "120 water pumps at two sites, each failing from the first of three modes (seal leak, \
                          bearing wear, impeller crack) or still running at the end of its follow-up: cumulative \
                          incidence of each mode per site with limits, Gray's test (cracks come sooner at the \
                          South site), Fine–Gray regression on load and site (load hastens bearing wear), and a \
                          Weibull per mode combined into the pump's failure probability.",
            build: competing_warranty::pump_failure_modes_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Monthly warranty returns (simulated) — return forecast",
            description: "Units shipped each month of a year and the returns from each month's shipments by \
                          calendar month (a triangular table): the counts as interval- and right-censored life \
                          data, Weibull and other fits compared, and returns forecast for the next 12 months from \
                          the units in service plus planned shipments, with prediction limits.",
            build: competing_warranty::warranty_returns_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Hotel occupancy (simulated) — exponential smoothing",
            description: "Nine years of monthly average rooms occupied per night at a 220-room hotel whose level, growth \
                          and summer peak drift over time, the peak scaling with the level. Automatic exponential smoothing compares \
                          the ETS models by AICc and picks multiplicative seasonality, then forecasts the next 12 \
                          months with 80% and 95% prediction limits, with residual checks and accuracy.",
            build: forecasting::hotel_occupancy_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "Reactor temperature (simulated) — ARIMA",
            description: "Ten days of hourly reactor temperatures that wander without a fixed mean, simulated from \
                          an ARIMA(1,1,1) process. The automatic search differences once (KPSS tests) and picks \
                          the orders by AICc; the report shows the search, coefficients with standard errors, \
                          forecasts for the next day with 80% and 95% limits, and residual checks.",
            build: forecasting::reactor_temperature_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "Kiln fuel feed (simulated) — ARIMA with an input",
            description: "Twelve days of hourly burning-zone temperatures of a lime kiln and the fuel feed that \
                          drives them two hours later through a first-order lag, then 24 hours of planned feed. \
                          ARIMA with the feed as a transfer-function input finds the delay from the prewhitened \
                          cross-correlation, estimates the weights and the steady-state gain (about 45 °C per \
                          t/h), and forecasts the planned day from the planned feed.",
            build: forecasting::kiln_fuel_feed_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "Device-A accelerated life test — life–stress model",
            description: "Published data (Meeker & Escobar 1998, Table C.10): hours to failure of devices run at \
                          10, 40, 60 and 80 °C, survivors removed at 5000 hours. A lognormal–Arrhenius model gives \
                          the activation energy (about 0.63 eV), acceleration factors, life at 10 °C, the \
                          life–stress and probability plots, and likelihood-ratio checks against separate fits.",
            build: accelerated::device_a_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Laser degradation — pseudo failure times",
            description: "Published data (Meeker & Escobar 1998, Table C.17): percent increase in operating current \
                          of 15 lasers every 250 hours to 4000. Each laser's straight-line path is extended to a 10% \
                          increase; the pseudo failure times (three reached within the test) get a Weibull fit, \
                          probability plot and percentiles.",
            build: accelerated::laser_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Adhesive bond strength (simulated) — destructive degradation",
            description: "Simulated data: bonds broken after 0–32 weeks at 50, 60 and 70 °C. Log strength falls \
                          linearly in √weeks at an Arrhenius rate; the fit gives the activation energy of the rate \
                          and the distribution of the time for strength to fall below 50 N at 40 °C.",
            build: accelerated::adhesive_bond_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Leukemia with covariates (Freireich 1963) — proportional hazards",
            description: "Weeks in remission for 42 leukemia patients on 6-mercaptopurine or placebo, with log \
                          white blood cell count and sex: Cox regression with Efron ties, hazard ratios with \
                          profile-likelihood limits, the Schoenfeld-residual test of proportional hazards and \
                          survival curves for each treatment.",
            build: survival_models::leukemia_covariates_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Capacitor life test (simulated) — parametric survival regression",
            description: "Capacitors from two lots inspected at fixed times at 85, 105 and 125 °C, so every life \
                          is interval- or right-censored: a Weibull accelerated failure time model with \
                          temperature and lot, time ratios, percentiles and failure probabilities at each \
                          temperature, and a Cox–Snell residual check.",
            build: survival_models::capacitor_life_test_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Visual inspection (simulated) — attribute agreement",
            description: "Three inspectors grade 30 parts twice, pass/fail and on a 1–5 scale, against known \
                          standard ratings: agreement within each inspector, with the standard and between \
                          inspectors, with exact intervals, Cohen's and Fleiss' kappa, weighted kappas, \
                          Kendall's W and τ_b for the ordered grades, and the agreement plot.",
            build: agreement::visual_inspection_sample,
            topic: SampleTopic::Measurement,
        },
        SampleInfo {
            name: "Go/no-go gauge (simulated) — attribute gauge bias and repeatability",
            description: "Sixteen reference parts near a 12 mm lower limit, each checked 20 times with a plug \
                          gauge: adjusted probabilities of acceptance, the fitted normal gauge curve, bias, \
                          repeatability and the bias t test (AIAG analytic method).",
            build: agreement::go_no_go_gauge_sample,
            topic: SampleTopic::Measurement,
        },
        SampleInfo {
            name: "Multi-centre trial (invented) — multi-way table, stratified 2 × 2 tables",
            description: "Invented data: 200 patients in five centres given a drug or placebo, with the outcome, \
                          baseline severity, age and change in a symptom score. A multi-way table of outcome \
                          counts, row percentages and mean score change by centre and treatment, and the \
                          stratified analysis by centre: Cochran–Mantel–Haenszel tests, the Mantel–Haenszel \
                          common odds ratio, Breslow–Day homogeneity and Fisher's exact test in each centre.",
            build: tabulation::multicentre_trial_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Monthly electricity demand (simulated) — series description, decomposition",
            description: "Ten years of monthly electricity demand on a regional grid: a rising trend, a winter \
                          heating peak and a smaller summer cooling peak whose size grows with the level \
                          (multiplicative seasonality), and autocorrelated noise. The series description shows \
                          the seasonal autocorrelation, Ljung–Box and unit-root tests; the multiplicative \
                          decomposition gives the seasonal indices and strengths of trend and seasonality.",
            build: time_series::electricity_demand_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "Product adoption (simulated) — logistic S-curve trend",
            description: "Cumulative customers of a new subscription service over its first four years, \
                          following an S-shaped adoption curve. A logistic trend model estimates the saturation \
                          level (about 52 000) and the month of fastest growth, with accuracy measures and a year \
                          of forecasts; compare it with a linear or exponential trend.",
            build: time_series::product_adoption_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "Advertising and sales (simulated) — cross-correlation",
            description: "Two years of weekly advertising spend and sales; sales respond to advertising two \
                          weeks later, and both series carry their own autocorrelation. The cross-correlation \
                          after prewhitening the advertising series shows a clear peak at lag 2.",
            build: time_series::advertising_sales_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "Raman bands (invented) — baseline, peak finding, peak fitting",
            description: "Invented Raman spectra of four mixtures from 1450 to 1800 cm⁻¹: two overlapping bands \
                          near 1585 and 1604 cm⁻¹ that trade height with the mixture and a broader band at 1655 \
                          cm⁻¹, on a curved fluorescence background. arPLS removes the background, peak finding \
                          locates the bands, and a three-band pseudo-Voigt fit to every spectrum gives centres, \
                          heights, widths and areas with standard errors, compares one to three bands by AICc \
                          and F tests, and tabulates the parameters by sample.",
            build: signals::raman_bands_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "Machine vibration (simulated) — smoothing, derivatives and integrals",
            description: "Two seconds of accelerometer readings at 500 Hz from a bearing housing: a 12 Hz shaft \
                          vibration with a 36 Hz harmonic, noise and short impacts every 0.45 s. Savitzky–Golay \
                          smoothing keeps the waveform while removing noise, a running median removes the \
                          impacts, and the derivatives and integrals report integrates acceleration to velocity \
                          (Simpson's rule).",
            build: signals::machine_vibration_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "ECG-like signal (simulated) — smoothing, peak finding",
            description: "Ten seconds of a simulated ECG-like trace at 250 Hz (not a recording): P, QRS and T \
                          waves at about 72 beats a minute on a slow baseline wander, with 50 Hz mains hum and \
                          noise. A five-point moving average removes the hum; peak finding after a SNIP baseline \
                          picks out the R waves (prominent and at least 0.3 s apart) and gives the beat times.",
            build: signals::ecg_like_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "Test bench signals (simulated) — spectrum, filter, correlation, wavelets",
            description: "Three 2-second runs of a vibration test at 500 samples per second, stacked by run: a \
                          microphone picking up 50 Hz mains hum, a 120 Hz motor tone and a random excitation; an \
                          accelerometer feeling the same excitation 10 ms later; and a strain gauge recording a \
                          step with a decaying ringing. The reports find the two tones in the spectrum (with \
                          Welch's average and a spectrogram), filter out the 120 Hz tone, measure the 10 ms delay \
                          by cross-correlation, and denoise the strain with wavelets, with a scalogram.",
            build: test_bench::test_bench_signals_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "Machined surface scan (simulated) — image spectrum and notch filter",
            description: "A simulated 256 × 256 height map of a machined steel surface (2 µm pixels), one row \
                          per pixel: two sets of tool marks 16 µm and 24 µm apart at different angles, a slow \
                          waviness, sensor noise and a shallow pit. The image spectrum shows the tool marks as \
                          two pairs of bright spots and lists their spacing and direction; notching those two \
                          frequencies out leaves the pit plainly visible in the filtered image.",
            build: images::machined_surface_scan_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "Blurred and shifted scene (simulated) — deblurring and registration",
            description: "Three simulated 128 × 128 photographs of one scene of discs and bars, one row per \
                          pixel: sharp, blurred by a Gaussian of 2 pixels, and moved 5.3 pixels right and 3.6 \
                          up. The reports restore the blurred picture with a Wiener filter and find the shift \
                          of the moved picture to a tenth of a pixel by phase correlation, then align it.",
            build: images::blurred_shifted_scene_sample,
            topic: SampleTopic::TimeSeries,
        },
        SampleInfo {
            name: "Hillside survey (invented) — 3-D scatter, contour and surface plots",
            description: "Invented data: elevation, soil depth and soil type at 140 irregularly placed survey \
                          points on a hillside. Contour and surface plots of elevation (Delaunay triangulation \
                          through the points) and of soil depth (a thin-plate spline smoothed by \
                          cross-validation), and a rotatable 3-D scatter plot coloured by soil type and sized by \
                          depth, with drop lines.",
            build: survey::hillside_survey_sample,
            topic: SampleTopic::Graphs,
        },
        SampleInfo {
            name: "Reading trial (invented) — estimation plots, significance brackets",
            description: "Invented reading scores of 80 pupils in a 2 × 2 trial of phonics lessons and \
                          tutoring. A Gardner–Altman estimation plot of tutored against untutored pupils and a \
                          Cumming plot of the four arms against the control arm (mean differences with BCa \
                          bootstrap intervals), and a swarm plot of the four arms with Tukey–Kramer significance \
                          brackets.",
            build: graph_examples::reading_trial_sample,
            topic: SampleTopic::Graphs,
        },
        SampleInfo {
            name: "Parcel flows (invented) — Sankey, chord and network charts",
            description: "Invented consignments from four regions through two sorting hubs to four regions, \
                          with their parcel counts: a Sankey diagram of origin, hub and destination sized by \
                          parcels, a chord diagram of flows between regions and a network of regions and hubs. \
                          Click a flow, arc or node to select its consignments.",
            build: graph_examples::parcel_flows_sample,
            topic: SampleTopic::Graphs,
        },
        SampleInfo {
            name: "Regional climate (invented) — clustered heat map, convex hulls",
            description: "Invented climate normals of 24 towns in four regions (temperatures, rainfall, \
                          sunshine, wind and frost days): a clustered heat map of the standardized measures with \
                          town and measure dendrograms, and a scatter of sunshine against rainfall with a convex \
                          hull around each region.",
            build: graph_examples::regional_climate_sample,
            topic: SampleTopic::Graphs,
        },
        SampleInfo {
            name: "Harbour weather (invented) — wind rose, vector field, radar, ridgeline and donut charts",
            description: "Invented three-hourly readings over a fortnight at a harbour: wind direction and speed \
                          with their east and north components, temperature, humidity, pressure and sky. A wind \
                          rose of speed by direction, a vector field of the wind by hour and day, a radar chart \
                          of the means by time of day, ridgelines of temperature by time of day, and a donut \
                          chart of the sky.",
            build: charts::harbour_weather_sample,
            topic: SampleTopic::Graphs,
        },
        SampleInfo {
            name: "Household budget (invented) — waterfall, donut and lollipop charts",
            description: "Invented income and spending of a household over three months. A waterfall chart from \
                          income to what is left, a donut chart of spending by category, and lollipops of each \
                          item's monthly mean.",
            build: charts::household_budget_sample,
            topic: SampleTopic::Graphs,
        },
        SampleInfo {
            name: "Soil texture (invented) — ternary plot",
            description: "Invented sand, silt and clay percentages of 48 soil samples from three fields, on a \
                          ternary plot coloured and shaped by field.",
            build: charts::soil_texture_sample,
            topic: SampleTopic::Graphs,
        },
        SampleInfo {
            name: "Clinical visits (invented) — value labels, row colours",
            description: "An invented blood-pressure trial of 72 patients with the arm coded 0/1 and sex coded \
                          1/2, each with value labels, so the model of the change in pressure names Placebo and \
                          Active and the grid shows the labels (Stored values shows the codes). Colour rows by Arm \
                          from Data ▸ Row states to see the scatter plot keyed by arm; Patient is shown on \
                          hover.",
            build: clinical_visits::clinical_visits_sample,
            topic: SampleTopic::Graphs,
        },
        SampleInfo {
            name: "Repair times (invented) — bootstrap, permutation tests",
            description: "Invented, skewed repair times of 24 jobs by two crews, with the parts replaced. A \
                          bootstrap of the distribution report's mean, standard deviation and coefficient of \
                          variation (percentile and BCa intervals with their bootstrap distributions), a \
                          permutation test of the crews' difference in median time, and a permutation test of \
                          the Spearman correlation between parts replaced and repair time.",
            build: resampling::repair_times_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Cell populations (simulated) — Gaussian mixture clustering",
            description: "CD4 and CD8 marker intensities of 360 cells from three populations of unequal size and \
                          shape (one elongated, one tilted the other way, one small and round): Gaussian mixture \
                          clustering choosing the covariance structure and number of components by BIC, with \
                          k-means for comparison (compare both with the Population column).",
            build: maps_and_mixtures::cell_populations_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Symptom checklist (simulated) — latent class analysis",
            description: "Answers of 600 respondents to seven symptom items (three Absent/Mild/Severe, four \
                          No/Yes, a few left blank) from three hidden profiles: latent class analysis choosing the \
                          number of classes by BIC, with class sizes, response probabilities and a profile plot \
                          (compare the saved classes with the Simulated profile column).",
            build: latent_classes::symptom_checklist_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Espresso machine reviews (simulated) — text analysis, topics, sentiment",
            description: "Star ratings and short review texts for 300 espresso machines of three models, each \
                          review about one or two of six themes (taste, milk frothing, cleaning, noise, delivery, \
                          service): term frequencies, a word cloud and phrases, semantic dimensions with document \
                          clusters, a six-topic model, and lexicon sentiment scores (compare the topics with the \
                          Simulated main theme column and the scores with the Rating).",
            build: reviews::espresso_reviews_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Expression profiles (simulated) — t-SNE, UMAP, multidimensional scaling",
            description: "Twenty gene expression values for 300 cells of four types, with a gradient shared \
                          across types: t-SNE, UMAP and classical scaling maps coloured by cell type.",
            build: maps_and_mixtures::expression_profiles_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Exotherm profiles (simulated) — functional data, registration, functional PCA",
            description: "Temperature curves of 24 exothermic batches from a factorial in catalyst, feed rate and \
                          agitator, read every 2 minutes: smoothed curves, functional principal components, the \
                          scores modelled on the three factors with a profiler of the predicted curve, and the \
                          same curves aligned on their peaks by landmark and continuous registration.",
            build: functional::exotherm_profiles_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Town distances (invented) — scaling from a distance matrix",
            description: "Road distances between 12 fictional towns, one column per town: classical and metric \
                          (SMACOF) multidimensional scaling recover a map of the towns from the distances alone, \
                          and principal coordinates show how far the road distances are from a flat map.",
            build: maps_and_mixtures::town_distances_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Shrub traits (invented) — Gower distances, principal coordinates",
            description: "Sixteen shrubs described by measurements, an ordinal shade tolerance, a growth form and \
                          thorns, with missing values: Gower distances of the mixed traits, principal coordinates \
                          with their negative eigenvalues, and hierarchical clustering of the same distances.",
            build: distances::shrub_traits_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Design comparison (planned) — design evaluation, two candidate designs",
            description: "Two 15-run plans for a quadratic model in temperature, time and pressure, stacked with \
                          a Design column and no responses yet: a Box–Behnken design with three centre runs and \
                          an I-optimal design. The design evaluation compares them: power of each term, prediction \
                          variance profiles, fraction-of-design-space curves, aliasing, correlations of terms and \
                          D, A, G and I efficiencies.",
            build: design_comparison::design_comparison_sample,
            topic: SampleTopic::Design,
        },
        SampleInfo {
            name: "Filtration rate (Montgomery) — unreplicated 2⁴, normal plot of effects, Lenth's method",
            description: "Published data (Montgomery, Design and Analysis of Experiments, Example 6.2): filtration \
                          rate in an unreplicated 2⁴ factorial. The factorial analysis estimates all 15 effects, \
                          judges them by Lenth's pseudo standard error on normal and half-normal plots, and shows \
                          main-effect, interaction and cube plots and Bergman–Hynén dispersion tests.",
            build: factorial::filtration_rate_sample,
            topic: SampleTopic::Design,
        },
        SampleInfo {
            name: "Coating thickness (simulated) — replicated 2³, dispersion effects",
            description: "A replicated 2³ factorial (three runs per setting) in which the oven temperature \
                          changes the variability but not the mean: t tests of the effects, and the effects of \
                          ln s on a normal plot (Box & Meyer).",
            build: factorial::coating_thickness_sample,
            topic: SampleTopic::Design,
        },
        SampleInfo {
            name: "Borehole water flow (Worley 1987) — computer experiment, space-filling design",
            description: "A deterministic computer model: water flow (m³/yr) through a borehole between two \
                          aquifers as a function of eight inputs (borehole radius rw, radius of influence r, \
                          transmissivities Tu and Tl, potentiometric heads Hu and Hl, borehole length L and \
                          hydraulic conductivity Kw), computed exactly from Worley's formula on a 40-run \
                          maximin Latin hypercube (Design > Space filling) over the input ranges of Morris, \
                          Mitchell & Ylvisaker (1993). Opens with a surrogate linear model (main effects and rw²) \
                          with the Response Explorer.",
            build: computer_experiment::borehole_sample,
            topic: SampleTopic::Design,
        },
        SampleInfo {
            name: "Furnace coatings (simulated) — split-plot design",
            description: "A D-optimal split-plot design (Design > Optimal > D-optimal with the furnace temperature \
                          hard to change): 24 runs in 8 furnace heats of 3, each heat at one temperature, with \
                          coating thickness and rack position reset on every run, and simulated corrosion \
                          resistance including a heat-to-heat effect. Opens with the design declaration (whole \
                          plots and model) and the split-plot mixed model fitted by REML, which tests the \
                          temperature against heat-to-heat (whole-plot) error.",
            build: split_plot::furnace_sample,
            topic: SampleTopic::Design,
        },
        SampleInfo {
            name: "Air quality (simulated) — missing-value imputation",
            description: "Daily readings from one monitoring station over 120 days with gaps: a PM2.5 sensor \
                          outage, ozone lost on humid days and scattered dropouts. The missing-data patterns, \
                          imputation by a multivariate normal fitted with the EM algorithm, and by a low-rank \
                          SVD fit with its rank chosen on held-out values.",
            build: imputation::air_quality_sample,
            topic: SampleTopic::Multivariate,
        },
        SampleInfo {
            name: "Cardiac rehabilitation (invented) — treatment effects from observational data",
            description: "An observational study of 400 patients, 170 of whom enrolled in a rehabilitation \
                          programme. Enrolment depended on age, sex, disease severity, smoking and region, which \
                          also predict the outcomes, so a plain comparison is confounded. Opens with a doubly \
                          robust (augmented weighting) estimate of the effect on fitness gain with covariate \
                          balance, propensity-score overlap and subgroup effects by age group and sex, and with \
                          1:1 propensity-score matching for the effect on readmission among those enrolled.",
            build: treatment_study::treatment_study_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Gene screen (simulated) — many two-group tests, volcano plot",
            description: "Expression of 30 genes in 8 untreated and 8 treated tissue samples, seven of which \
                          respond: a Welch t test per gene with Benjamini–Hochberg adjusted p-values, the table \
                          ordered by p-value, and a volcano plot of each gene's change against −log₁₀ p.",
            build: multiple_testing::gene_screen_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Transcript counts (simulated) — screening many responses",
            description: "Read counts of 40 genes in 24 tissue samples (control or treated, liver or lung, three \
                          batches): a negative binomial regression per gene on treatment and batch with \
                          Benjamini–Hochberg q-values, volcano plots, and a forest plot of each gene's rate ratio; \
                          then the same screen within each tissue, where three genes respond in liver only.",
            build: screening::transcript_counts_sample,
            topic: SampleTopic::Model,
        },
        SampleInfo {
            name: "Trial endpoints (invented) — adjusted and combined p-values",
            description: "p-values of twelve secondary endpoints of a trial: Bonferroni, Šidák, Holm, Hochberg, \
                          Benjamini–Hochberg and Benjamini–Yekutieli adjustments and Storey q-values, the number \
                          of endpoints each declares significant, and Fisher's and Stouffer's combined tests \
                          (weighted by the patients measured).",
            build: multiple_testing::trial_endpoints_sample,
            topic: SampleTopic::DescribeCompare,
        },
        SampleInfo {
            name: "Plasma concentration (simulated) — area under a curve",
            description: "Drug concentration in plasma after one dose of two formulations, three animals sampled \
                          at each of nine times (each animal once): the area under each mean curve with Bailer's \
                          standard error, and the peak; save the curves as a percentage of their range.",
            build: multiple_testing::plasma_concentration_sample,
            topic: SampleTopic::Curves,
        },
        SampleInfo {
            name: "Response rates by site (simulated) — Bayesian estimation, hierarchical logistic model",
            description: "Patients at six sites randomized to Active or Placebo: the posterior of each arm's \
                          response rate and of their difference with a region of practical equivalence, the \
                          adverse-event rate ratio, and a Bayesian logistic model with a random intercept per site.",
            build: bayes::response_rates_sample,
            topic: SampleTopic::Bayesian,
        },
        SampleInfo {
            name: "Stroop reaction times (simulated) — Bayes factors, Bayesian ANOVA",
            description: "Colour-naming times of 40 younger and older participants: Bayes factor paired and \
                          two-sample t tests, a correlation and a contingency-table Bayes factor, with robustness \
                          plots over the prior scale, and a Bayesian ANOVA of the interference effect.",
            build: bayes::stroop_sample,
            topic: SampleTopic::Bayesian,
        },
        SampleInfo {
            name: "Eight schools (Rubin 1981) — Bayesian hierarchical model",
            description: "Coaching effects estimated in eight schools with their standard errors: a hierarchical \
                          normal model sampled with NUTS shrinks each school toward the common mean, with trace, \
                          rank and autocorrelation plots of the between-school SD.",
            build: bayes::eight_schools_sample,
            topic: SampleTopic::Bayesian,
        },
        SampleInfo {
            name: "Well water arsenic (invented) — non-detects, censored distribution fits",
            description: "Arsenic in 24 wells, with values below the laboratory's detection limit (1, 2 or 5 \
                          µg/L) flagged as non-detects: the Distribution report treats them as left-censored, \
                          with Kaplan–Meier percentiles reversed for left censoring and maximum-likelihood fits.",
            build: censored_values::well_arsenic_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "Steel fatigue (invented) — S–N curves with run-outs",
            description: "Cycles to failure of 32 specimens at five stress amplitudes, two of them unbroken \
                          run-outs at 10⁷ cycles: a lognormal Basquin curve with B10 and median curves and a lower \
                          bound, fatigue strength at 10⁶ and 10⁷ cycles, and an endurance-limit curve.",
            build: fatigue::steel_fatigue_sample,
            topic: SampleTopic::Reliability,
        },
        SampleInfo {
            name: "PK profiles (invented) — non-compartmental pharmacokinetics",
            description: "Plasma concentrations of 12 subjects after a 100 mg oral dose of a reference and a test \
                          formulation, sampled over 24 h: Cmax, Tmax, AUC, terminal half-life, clearance and \
                          volume per profile, geometric means by formulation, and linear and semi-log profiles.",
            build: pk_profiles::pk_profiles_sample,
            topic: SampleTopic::Curves,
        },
    ]
}

/// **Simulated** 2×2 crossover bioequivalence trial: 24 subjects, 12 per
/// sequence (R→T, T→R). log AUC = log 100 + subject effect N(0, 0.3²) +
/// 0.03 in period 2 − 0.05 for the test tablet + within-subject noise
/// N(0, 0.15²); AUC recorded to 0.1.
#[must_use]
pub fn bioequivalence() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0xB10E);
    let (mut subject, mut sequence, mut period, mut treatment, mut auc) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for k in 1..=24 {
        let test_first = k % 2 == 0;
        let base = 100f64.ln() + 0.3 * rng.normal();
        for p in 1..=2 {
            let test = (p == 1) == test_first;
            let y = base + if p == 2 { 0.03 } else { 0.0 } + if test { -0.05 } else { 0.0 } + 0.15 * rng.normal();
            subject.push(format!("S{k:02}"));
            sequence.push(if test_first { "TR" } else { "RT" }.to_owned());
            period.push(f64::from(p));
            treatment.push(if test { "T" } else { "R" }.to_owned());
            auc.push(round_to(y.exp(), 1));
        }
    }
    let mut period = Column::numeric("Period", period);
    period.level = MeasurementLevel::Ordinal;
    table(
        "Bioequivalence crossover (simulated)",
        vec![
            text("Subject", subject),
            text("Sequence", sequence),
            period,
            text("Formulation", treatment),
            Column::numeric("AUC", auc),
        ],
    )
}

fn bioequivalence_sample() -> Sample {
    let t = bioequivalence();
    let reports = vec![SavedReport::Crossover {
        request: crate::CrossoverRequest {
            reference: Some("R".to_owned()),
            ..crate::CrossoverRequest::new(
                col(&t, "AUC"),
                col(&t, "Subject"),
                col(&t, "Period"),
                col(&t, "Formulation"),
            )
        },
    }];
    Sample { table: t, reports }
}

/// **Simulated** reaction times: 20 subjects (10 younger, 10 older), three
/// tasks × two cues, one row per subject and cell. RT = 350 + subject
/// N(0, 40²) + task (0, 80, 50) + 30 for an invalid cue + 60 for older
/// subjects + 40 more for older subjects on the choice task + noise whose
/// sd grows with the task (15, 25, 20) so that sphericity is in doubt; ms
/// to the nearest unit.
#[must_use]
pub fn reaction_times() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0x5EAC);
    let tasks = ["Simple", "Choice", "Go/no-go"];
    let (mut subject, mut age, mut task, mut cue, mut rt) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for k in 1..=20 {
        let older = k > 10;
        let base = 350.0 + 40.0 * rng.normal() + if older { 60.0 } else { 0.0 };
        for (t, name) in tasks.iter().enumerate() {
            let effect = [0.0, 80.0, 50.0][t] + if older && t == 1 { 40.0 } else { 0.0 };
            for invalid in [false, true] {
                let y = base + effect + if invalid { 30.0 } else { 0.0 } + [15.0, 25.0, 20.0][t] * rng.normal();
                subject.push(format!("P{k:02}"));
                age.push(if older { "Older" } else { "Younger" }.to_owned());
                task.push((*name).to_owned());
                cue.push(if invalid { "Invalid" } else { "Valid" }.to_owned());
                rt.push(y.round());
            }
        }
    }
    table(
        "Reaction times (simulated)",
        vec![
            text("Person", subject),
            text("Age group", age),
            text("Task", task),
            text("Cue", cue),
            Column::numeric("Reaction time", rt),
        ],
    )
}

fn reaction_times_sample() -> Sample {
    let t = reaction_times();
    let reports = vec![SavedReport::FactorialRepeated {
        request: crate::FactorialRepeatedRequest {
            response: col(&t, "Reaction time"),
            subject: col(&t, "Person"),
            within: vec![col(&t, "Task"), col(&t, "Cue")],
            between: vec![col(&t, "Age group")],
            simple: Some((col(&t, "Task"), col(&t, "Age group"))),
        },
    }];
    Sample { table: t, reports }
}

/// **Simulated** dendrite lengths: treatments Control, Drug A and Drug B,
/// mice "Mouse 1" … "Mouse 4" within each (the labels repeat across
/// treatments), 6 neurons per mouse. Length =
/// 120 + treatment (0, 25, 6) + mouse N(0, 10²) + neuron N(0, 12²), µm to
/// 0.1.
#[must_use]
pub fn dendrites() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0xDE7D);
    let (mut treatment, mut mouse, mut length) = (Vec::new(), Vec::new(), Vec::new());
    for (g, name) in ["Control", "Drug A", "Drug B"].iter().enumerate() {
        for m in 1..=4 {
            let animal = 120.0 + [0.0, 25.0, 6.0][g] + 10.0 * rng.normal();
            for _ in 0..6 {
                treatment.push((*name).to_owned());
                mouse.push(format!("Mouse {m}"));
                length.push(round_to(animal + 12.0 * rng.normal(), 1));
            }
        }
    }
    table(
        "Dendrite lengths (simulated)",
        vec![text("Treatment", treatment), text("Mouse", mouse), Column::numeric("Length", length)],
    )
}

fn dendrites_sample() -> Sample {
    let t = dendrites();
    let reports = vec![SavedReport::Nested {
        request: crate::NestedRequest::new(col(&t, "Length"), col(&t, "Treatment"), col(&t, "Mouse")),
    }];
    Sample { table: t, reports }
}

/// Deaths from horse kicks in 10 Prussian army corps over 20 years, as
/// tabulated by L. von Bortkiewicz, *Das Gesetz der kleinen Zahlen* (1898):
/// 109, 65, 22, 3 and 1 corps-years with 0–4 deaths.
#[must_use]
pub fn horse_kicks() -> DataTable {
    table(
        "Horse-kick deaths (von Bortkiewicz 1898)",
        vec![
            Column::numeric("Deaths", vec![0.0, 1.0, 2.0, 3.0, 4.0]),
            Column::numeric("Corps-years", vec![109.0, 65.0, 22.0, 3.0, 1.0]),
        ],
    )
}

fn horse_kicks_sample() -> Sample {
    let t = horse_kicks();
    let reports = vec![SavedReport::GoodnessOfFit {
        request: crate::GoodnessRequest {
            column: col(&t, "Deaths"),
            frequency: Some(col(&t, "Corps-years")),
            model: crate::GoodnessModel::Poisson { min_expected: 5.0 },
        },
    }];
    Sample { table: t, reports }
}

/// **Simulated** drying times (minutes) of five paint formulations, six
/// panels each: means 42, 44, 48, 51 and 43 minutes with σ = 2, recorded to
/// 0.1 minute.
#[must_use]
pub fn drying_times() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0xD12E);
    let (mut paint, mut minutes) = (Vec::new(), Vec::new());
    for (name, mean) in [("A", 42.0), ("B", 44.0), ("C", 48.0), ("D", 51.0), ("E", 43.0)] {
        for _ in 0..6 {
            paint.push(format!("Formulation {name}"));
            minutes.push(round_to(mean + 2.0 * rng.normal(), 1));
        }
    }
    table("Paint drying times (simulated)", vec![text("Paint", paint), Column::numeric("Drying time (min)", minutes)])
}

fn drying_times_sample() -> Sample {
    let t = drying_times();
    let (paint, minutes) = (col(&t, "Paint"), col(&t, "Drying time (min)"));
    let reports = vec![
        SavedReport::Groups {
            request: crate::GroupComparisonRequest {
                best: Some(crate::BestDirection::Smallest),
                ..crate::GroupComparisonRequest::new(crate::GroupLayout::Stacked { response: minutes, group: paint })
            },
        },
        SavedReport::Linear {
            spec: crate::LinearModelSpec {
                response: minutes,
                predictors: vec![paint],
                effects: vec![vec![(0, 1)]],
                splines: Vec::new(),
            },
            options: crate::ReportOptions {
                best: Some(crate::BestDirection::Smallest),
                pairwise: None,
                ..crate::ReportOptions::default()
            },
        },
    ];
    Sample { table: t, reports }
}

/// **Simulated** coating thickness (µm): 25 batches of 5 panels. Batch means
/// are normal around 25 µm with σ_between = 0.8 µm; panels within a batch
/// vary with σ_within = 0.5 µm; values rounded to 0.01 µm.
#[must_use]
pub fn coating_batches() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0xC0A7);
    let (mut batch, mut thickness) = (Vec::new(), Vec::new());
    for b in 1..=25 {
        let centre = 25.0 + 0.8 * rng.normal();
        for _ in 0..5 {
            batch.push(format!("B{b:02}"));
            thickness.push(((centre + 0.5 * rng.normal()) * 100.0).round() / 100.0);
        }
    }
    table("Coating batches (simulated)", vec![text("Batch", batch), Column::numeric("Thickness (µm)", thickness)])
}

fn coating_batches_sample() -> Sample {
    let t = coating_batches();
    let column = col(&t, "Thickness (µm)");
    let limits = crate::SpecLimits { lower: Some(22.0), upper: Some(28.0), target: Some(25.0) };
    let reports = vec![
        SavedReport::BetweenWithinCapability {
            request: crate::BetweenWithinRequest {
                column,
                subgroup: crate::SubgroupBy::Column(col(&t, "Batch")),
                limits,
                within: crate::WithinEstimate::PooledStdDev,
                between: crate::BetweenEstimate::MovingRange,
            },
        },
        SavedReport::Capability {
            request: crate::CapabilityRequest {
                column,
                limits,
                within: crate::WithinSigma::PooledStdDev { subgroup_size: 5 },
                box_cox: None,
                model: crate::CapabilityModel::Normal,
                summary: false,
                bootstrap: None,
                by: None,
                group_order: crate::GroupOrder::Labels,
                probability_plot: false,
            },
        },
    ];
    Sample { table: t, reports }
}

/// **Simulated** incoming inspection of 40 supplier lots: 150 to 400 units
/// inspected per lot, each defective with probability 0.02, and cosmetic
/// defects at 0.05 per unit (Poisson).
#[must_use]
pub fn supplier_lots() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0x5A11);
    let poisson = |mean: f64, rng: &mut stat_core::rng::Rng| {
        // Inversion by multiplying uniforms; the means here are small.
        let (limit, mut k, mut prod) = ((-mean).exp(), 0u32, 1.0 - rng.next_f64());
        while prod > limit {
            k += 1;
            prod *= 1.0 - rng.next_f64();
        }
        f64::from(k)
    };
    let (mut lot, mut inspected, mut defective, mut defects) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for l in 1..=40 {
        let n = (150.0 + 250.0 * rng.next_f64()).round();
        let bad = (0..n as usize).filter(|_| rng.next_f64() < 0.02).count();
        lot.push(format!("L{l:03}"));
        inspected.push(n);
        defective.push(bad as f64);
        defects.push(poisson(0.05 * n, &mut rng));
    }
    table(
        "Supplier lots (simulated)",
        vec![
            text("Lot", lot),
            Column::numeric("Units inspected", inspected),
            Column::numeric("Defective units", defective),
            Column::numeric("Cosmetic defects", defects),
        ],
    )
}

/// **Simulated** bottle filling: 25 subgroups of four bottles with fill
/// volume (mL), cap torque (N·m) and wall thickness (mm) built from three
/// standard normal scores `z₁, z₂, z₃` as volume `500 + 2z₁`, torque
/// `2 + 0.15(0.7z₁ + 0.714z₂)` and thickness `0.35 + 0.01(0.5z₁ + 0.3z₂ +
/// 0.81z₃)`, recorded to 0.01 mL, 0.001 N·m and 0.0001 mm. Subgroup 23 has
/// its volume raised by 3 mL with the torque unchanged (breaking the
/// correlation); subgroup 25 has every score scaled by 2.5 (more spread).
#[must_use]
pub fn bottle_filling() -> DataTable {
    let mut noise = Noise(0xB0_77_1E_F1_11_00_25_04);
    let (mut subgroup, mut volume, mut torque, mut thickness) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for g in 1..=25 {
        for _ in 0..4 {
            let scale = if g == 25 { 2.5 } else { 1.0 };
            let (z1, z2, z3) = (scale * noise.normal(), scale * noise.normal(), scale * noise.normal());
            let shift = if g == 23 { 3.0 } else { 0.0 };
            subgroup.push(f64::from(g));
            volume.push(round_to(500.0 + 2.0 * z1 + shift, 2));
            torque.push(round_to(2.0 + 0.15 * (0.7 * z1 + 0.714 * z2), 3));
            thickness.push(round_to(0.35 + 0.01 * (0.5 * z1 + 0.3 * z2 + 0.81 * z3), 4));
        }
    }
    let mut subgroup = Column::numeric("Subgroup", subgroup);
    subgroup.level = MeasurementLevel::Ordinal;
    table(
        "Bottle filling (simulated)",
        vec![
            subgroup,
            Column::numeric("Fill volume (mL)", volume),
            Column::numeric("Cap torque (N·m)", torque),
            Column::numeric("Wall thickness (mm)", thickness),
        ],
    )
}

fn supplier_lots_sample() -> Sample {
    let t = supplier_lots();
    let size = crate::SubgroupSize::Column(col(&t, "Units inspected"));
    let capability = |kind, count: &str| SavedReport::AttributeCapability {
        request: crate::AttributeCapabilityRequest::new(kind, col(&t, count), size),
    };
    let reports = vec![
        capability(crate::AttributeCapabilityKind::Binomial, "Defective units"),
        capability(crate::AttributeCapabilityKind::Poisson, "Cosmetic defects"),
        SavedReport::AcceptanceSampling {
            request: crate::AcceptanceRequest {
                plans: vec![
                    crate::SamplingPlan::Single { n: 89, c: 2 },
                    crate::SamplingPlan::Double { n1: 50, c1: 1, n2: 100, c2: 3 },
                ],
                ..crate::AcceptanceRequest::default()
            },
        },
    ];
    Sample { table: t, reports }
}

fn bottle_filling_sample() -> Sample {
    let t = bottle_filling();
    let measures = vec![col(&t, "Fill volume (mL)"), col(&t, "Cap torque (N·m)"), col(&t, "Wall thickness (mm)")];
    let chart = |kind, subgrouped: bool, reference| SavedReport::MultivariateChart {
        request: crate::MultivariateChartRequest {
            subgroup: subgrouped.then(|| col(&t, "Subgroup")),
            subgroup_size: 1,
            reference: Some(reference),
            ..crate::MultivariateChartRequest::new(measures.clone(), kind)
        },
    };
    let reports = vec![
        chart(crate::MultivariateChartKind::HotellingT2, true, 20),
        chart(crate::MultivariateChartKind::GeneralizedVariance, true, 20),
        chart(crate::MultivariateChartKind::Mewma, false, 80),
    ];
    Sample { reports, table: t }
}

/// **Simulated** heat treatment of 60 lots: four furnaces × three shifts ×
/// five lots. Hardness (HRC, to 0.1) is `60 + furnace + shift + 0.8ε` with
/// furnace effects 0, 0, +1.2, −0.2, shift effects 0, +0.3, −0.3, and an
/// extra +2.4 for furnace 2 on the night shift. Each lot has 150–250 parts
/// inspected, cracked with probability 0.03 (0.055 for furnace 2), and
/// 2–4 m² of surface inspected with pits at 1.5 per m² (2.1 on the night
/// shift).
#[must_use]
pub fn heat_treatment() -> DataTable {
    let mut noise = Noise(0x4EA7_7EA7_0000_0060);
    let poisson = |mean: f64, noise: &mut Noise| {
        let (limit, mut k, mut prod) = ((-mean).exp(), 0u32, noise.uniform());
        while prod > limit {
            k += 1;
            prod *= noise.uniform();
        }
        f64::from(k)
    };
    let shifts = ["Day", "Evening", "Night"];
    let (mut furnace, mut shift, mut hardness, mut inspected, mut cracked, mut area, mut pits) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for f in 0..4 {
        for (s, name) in shifts.iter().enumerate() {
            for _ in 0..5 {
                let effect = [0.0, 0.0, 1.2, -0.2][f] + [0.0, 0.3, -0.3][s] + if f == 1 && s == 2 { 2.4 } else { 0.0 };
                furnace.push(format!("Furnace {}", f + 1));
                shift.push((*name).to_owned());
                hardness.push(round_to(60.0 + effect + 0.8 * noise.normal(), 1));
                let n = (150.0 + 100.0 * noise.uniform()).round();
                let p = if f == 1 { 0.055 } else { 0.03 };
                inspected.push(n);
                cracked.push((0..n as usize).filter(|_| noise.uniform() < p).count() as f64);
                let a = round_to(2.0 + 2.0 * noise.uniform(), 2);
                area.push(a);
                pits.push(poisson(a * if s == 2 { 2.1 } else { 1.5 }, &mut noise));
            }
        }
    }
    table(
        "Heat treatment (simulated)",
        vec![
            text("Furnace", furnace),
            text("Shift", shift),
            Column::numeric("Hardness (HRC)", hardness),
            Column::numeric("Parts inspected", inspected),
            Column::numeric("Parts cracked", cracked),
            Column::numeric("Area inspected (m²)", area),
            Column::numeric("Surface pits", pits),
        ],
    )
}

fn heat_treatment_sample() -> Sample {
    let t = heat_treatment();
    let (furnace, shift) = (col(&t, "Furnace"), col(&t, "Shift"));
    let hardness = col(&t, "Hardness (HRC)");
    let reports = vec![
        SavedReport::Anom { request: crate::AnomRequest::means(hardness, furnace) },
        SavedReport::Anom {
            request: crate::AnomRequest { second: Some(shift), ..crate::AnomRequest::means(hardness, furnace) },
        },
        SavedReport::Anom {
            request: crate::AnomRequest {
                kind: crate::AnomKind::Proportions,
                response: col(&t, "Parts cracked"),
                size: crate::SubgroupSize::Column(col(&t, "Parts inspected")),
                group: Some(furnace),
                second: None,
                alpha: 0.05,
            },
        },
        SavedReport::Anom {
            request: crate::AnomRequest {
                kind: crate::AnomKind::Rates,
                response: col(&t, "Surface pits"),
                size: crate::SubgroupSize::Column(col(&t, "Area inspected (m²)")),
                group: Some(shift),
                second: None,
                alpha: 0.05,
            },
        },
    ];
    Sample { reports, table: t }
}

/// **Simulated** peel strengths (N) of 150 heat seals: a Weibull with
/// threshold 20, shape 1.8 and scale 8, rounded to 0.01 N; 50 seals from each
/// of three shifts in turn.
#[must_use]
pub fn seal_strength() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0x5EA1);
    let n = 150;
    let strength: Vec<f64> = (0..n)
        .map(|_| {
            let u = 1.0 - rng.next_f64();
            let v = 20.0 + 8.0 * (-u.ln()).powf(1.0 / 1.8);
            (v * 100.0).round() / 100.0
        })
        .collect();
    let shift: Vec<String> = (0..n).map(|i| ["Early", "Late", "Night"][i / 50].to_owned()).collect();
    table("Seal strength (simulated)", vec![text("Shift", shift), Column::numeric("Peel strength (N)", strength)])
}

fn seal_strength_sample() -> Sample {
    let t = seal_strength();
    let request = crate::DistributionFitRequest::new(col(&t, "Peel strength (N)"));
    // A minimum peel strength of 21 N, judged with the identified distribution.
    let capability = crate::CapabilityRequest {
        column: col(&t, "Peel strength (N)"),
        limits: crate::SpecLimits { lower: Some(21.0), upper: None, target: None },
        within: crate::WithinSigma::MovingRange,
        box_cox: None,
        model: crate::CapabilityModel::Distribution(crate::FitFamily::Weibull3),
        summary: false,
        bootstrap: None,
        by: None,
        group_order: crate::GroupOrder::Labels,
        probability_plot: false,
    };
    Sample {
        reports: vec![SavedReport::DistributionFit { request }, SavedReport::Capability { request: capability }],
        table: t,
    }
}

/// Rosner's (1983) 54 measurements, as listed in the NIST e-Handbook
/// §1.3.5.17.3 for the generalized ESD test.
#[must_use]
pub fn rosner() -> DataTable {
    let values = [
        -0.25, 0.68, 0.94, 1.15, 1.20, 1.26, 1.26, 1.34, 1.38, 1.43, 1.49, 1.49, 1.55, 1.56, 1.58, 1.65, 1.69, 1.70,
        1.76, 1.77, 1.81, 1.91, 1.94, 1.96, 1.99, 2.06, 2.09, 2.10, 2.14, 2.15, 2.23, 2.24, 2.26, 2.35, 2.37, 2.40,
        2.47, 2.54, 2.62, 2.64, 2.90, 2.92, 2.92, 2.93, 3.21, 3.26, 3.30, 3.59, 3.68, 4.30, 4.64, 5.34, 5.42, 6.01,
    ];
    table("Rosner's measurements (1983)", vec![Column::numeric("Measurement", values.to_vec())])
}

fn rosner_sample() -> Sample {
    let t = rosner();
    let options = DistributionOptions {
        outliers: Some(crate::OutlierSettings {
            method: crate::OutlierMethod::GeneralizedEsd,
            max_outliers: Some(10),
            ..crate::OutlierSettings::default()
        }),
        ..DistributionOptions::default()
    };
    Sample { reports: vec![distribution(&t, "Measurement", options)], table: t }
}

/// Dunnett's (1955, *JASA* 50, p. 1105) blood counts: a control group and two
/// drugs.
#[must_use]
pub fn blood_counts() -> DataTable {
    let groups: [(&str, &[f64]); 3] = [
        ("Control", &[7.40, 8.50, 7.20, 8.24, 9.84, 8.32]),
        ("Drug A", &[9.76, 8.80, 7.68, 9.36]),
        ("Drug B", &[12.80, 9.68, 12.16, 9.20, 10.55]),
    ];
    let (mut treatment, mut count) = (Vec::new(), Vec::new());
    for (name, values) in groups {
        for &v in values {
            treatment.push(name.to_owned());
            count.push(v);
        }
    }
    table(
        "Blood counts (Dunnett 1955)",
        vec![text("Treatment", treatment), Column::numeric("Blood count (millions/mm³)", count)],
    )
}

/// The plastic film data of Johnson & Wichern, *Applied Multivariate
/// Statistical Analysis* (6th ed.), Example 6.12: tear resistance, gloss and
/// opacity at two extrusion rates and two additive amounts, five films each.
#[must_use]
pub fn plastic_film() -> DataTable {
    let tear = [6.5, 6.2, 5.8, 6.5, 6.5, 6.9, 7.2, 6.9, 6.1, 6.3, 6.7, 6.6, 7.2, 7.1, 6.8, 7.1, 7.0, 7.2, 7.5, 7.6];
    let gloss = [9.5, 9.9, 9.6, 9.6, 9.2, 9.1, 10.0, 9.9, 9.5, 9.4, 9.1, 9.3, 8.3, 8.4, 8.5, 9.2, 8.8, 9.7, 10.1, 9.2];
    let opacity = [4.4, 6.4, 3.0, 4.1, 0.8, 5.7, 2.0, 3.9, 1.9, 5.7, 2.8, 4.1, 3.8, 1.6, 3.4, 8.4, 5.2, 6.9, 2.7, 1.9];
    let rate = (0..20).map(|i| if i < 10 { "Low" } else { "High" }.to_owned()).collect();
    let additive = (0..20).map(|i| if (i / 5) % 2 == 0 { "Low" } else { "High" }.to_owned()).collect();
    table(
        "Plastic film (Johnson & Wichern)",
        vec![
            text("Extrusion rate", rate),
            text("Additive", additive),
            Column::numeric("Tear resistance", tear.to_vec()),
            Column::numeric("Gloss", gloss.to_vec()),
            Column::numeric("Opacity", opacity.to_vec()),
        ],
    )
}

fn plastic_film_sample() -> Sample {
    let t = plastic_film();
    let responses = vec![col(&t, "Tear resistance"), col(&t, "Gloss"), col(&t, "Opacity")];
    let request = crate::ManovaRequest {
        effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
        ..crate::ManovaRequest::new(responses, vec![col(&t, "Extrusion rate"), col(&t, "Additive")])
    };
    Sample { reports: vec![SavedReport::Manova { request }], table: t }
}

fn blood_counts_sample() -> Sample {
    let t = blood_counts();
    let request = crate::GroupComparisonRequest {
        control: Some("Control".to_owned()),
        ..crate::GroupComparisonRequest::new(crate::GroupLayout::Stacked {
            response: col(&t, "Blood count (millions/mm³)"),
            group: col(&t, "Treatment"),
        })
    };
    Sample { reports: vec![SavedReport::Groups { request }], table: t }
}

/// **Simulated** pump sensor readings: temperature uniform on [50, 90] °C,
/// vibration on [1, 7] mm/s, pressure on [2, 6] bar and production line A,
/// B or C. A pump is faulty when ((T − 70)/12)² + ((V − 3.5)/1.8)² exceeds 1
/// (0.8 on line C), with 4% of the flags flipped at random. Efficiency is
/// 80 − 0.02(T − 70)² + 2 sin(V) + 1.5 on line B, plus normal noise (sd 0.8).
/// Roles: 60% training, 25% validation, 15% test, at random.
#[must_use]
pub fn pumps() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0x5B11);
    let n = 400;
    let (mut temp, mut vib, mut pressure, mut line, mut fault, mut eff, mut role) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..n {
        let t = round_to(50.0 + 40.0 * rng.next_f64(), 1);
        let v = round_to(1.0 + 6.0 * rng.next_f64(), 2);
        let p = round_to(2.0 + 4.0 * rng.next_f64(), 2);
        let l = ["A", "B", "C"][rng.below(3)];
        let s = ((t - 70.0) / 12.0).powi(2) + ((v - 3.5) / 1.8).powi(2);
        let mut f = s > if l == "C" { 0.8 } else { 1.0 };
        if rng.next_f64() < 0.04 {
            f = !f;
        }
        let e =
            80.0 - 0.02 * (t - 70.0).powi(2) + 2.0 * v.sin() + if l == "B" { 1.5 } else { 0.0 } + 0.8 * rng.normal();
        let u = rng.next_f64();
        temp.push(t);
        vib.push(v);
        pressure.push(p);
        line.push(l.to_owned());
        fault.push(if f { "Fault" } else { "OK" }.to_owned());
        eff.push(round_to(e, 2));
        role.push(
            if u < 0.6 {
                "Training"
            } else if u < 0.85 {
                "Validation"
            } else {
                "Test"
            }
            .to_owned(),
        );
    }
    table(
        "Pump sensors (simulated)",
        vec![
            Column::numeric("Temperature", temp),
            Column::numeric("Vibration", vib),
            Column::numeric("Pressure", pressure),
            text("Line", line),
            text("Status", fault),
            Column::numeric("Efficiency (%)", eff),
            text("Validation", role),
        ],
    )
}

fn pumps_sample() -> Sample {
    let mut t = pumps();
    let c = |n: &str| col(&t, n);
    let predictors = vec![c("Temperature"), c("Vibration"), c("Pressure"), c("Line")];
    let roles = crate::ValidationMethod::Column(c("Validation"));
    let radial =
        crate::SvmRequest { validation: roles.clone(), ..crate::SvmRequest::new(c("Status"), predictors.clone()) };
    let linear = crate::SvmRequest { kernel: crate::SvmKernel::Linear, ..radial.clone() };
    let regression = crate::SvmRequest {
        validation: crate::ValidationMethod::KFold { folds: 5, seed: 1 },
        ..crate::SvmRequest::new(c("Efficiency (%)"), predictors)
    };
    let rows = RowStateManager::new(t.n_rows());
    let fault_probability = |r: &crate::SvmRequest| {
        crate::SvmAnalysis::run(&t, &rows, r)
            .ok()
            .and_then(|a| a.saveable_columns().into_iter().find(|k| k.name == "Prob Status = Fault"))
            .map(|k| k.values)
    };
    let saved = [
        ("Prob Fault (radial SVM)", fault_probability(&radial)),
        ("Prob Fault (linear SVM)", fault_probability(&linear)),
    ];
    let profilers = crate::ProfilerOptions::default();
    let mut reports = vec![
        SavedReport::SupportVectorMachine { request: radial, profilers },
        SavedReport::SupportVectorMachine { request: linear, profilers },
    ];
    let mut models = Vec::new();
    for (name, values) in saved {
        let at = t.n_cols();
        if let Some(v) = values {
            if t.insert_column(at, Column::numeric(name, v)).is_ok() {
                models.push(at);
            }
        }
    }
    if !models.is_empty() {
        reports.push(SavedReport::ModelComparison {
            request: crate::ModelComparisonRequest {
                response: col(&t, "Status"),
                models,
                event: Some("Fault".to_owned()),
                validation: Some(col(&t, "Validation")),
            },
        });
    }
    reports.push(SavedReport::SupportVectorMachine { request: regression, profilers });
    Sample { reports, table: t }
}

/// Wavelengths (nm) of the fuel spectra columns.
const FUEL_WAVELENGTHS: std::ops::Range<usize> = 0..40;

/// **Simulated** near-infrared spectra of 60 fuel samples. Each sample mixes
/// three constituents (fractions uniform on [0, 1]) whose Gaussian
/// absorption bands, centred at 960, 1080 and 1200 nm with widths (sd) of
/// 45, 35 and 50 nm, overlap; a baseline offset (uniform on [0, 0.1]) and
/// slope add light-scatter effects, and every absorbance carries normal
/// noise (sd 0.002). Octane is 85 + 6c₁ + 3c₂ − 2c₃ plus normal noise (sd 0.15).
#[must_use]
pub fn fuel_spectra() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0xF0E1);
    let n = 60;
    let bands = [(960.0, 45.0), (1080.0, 35.0), (1200.0, 50.0)];
    let wavelength = |k: usize| 900.0 + 10.0 * k as f64;
    let mut spectra = vec![Vec::with_capacity(n); FUEL_WAVELENGTHS.len()];
    let (mut ids, mut octane) = (Vec::new(), Vec::new());
    for i in 0..n {
        let c = [rng.next_f64(), rng.next_f64(), rng.next_f64()];
        let offset = 0.1 * rng.next_f64();
        let slope = 0.05 * rng.next_f64();
        for k in FUEL_WAVELENGTHS {
            let w = wavelength(k);
            let bands_sum: f64 =
                bands.iter().zip(&c).map(|(&(mu, sd), ci)| ci * (-0.5 * ((w - mu) / sd).powi(2)).exp()).sum();
            let a = bands_sum + offset + slope * (w - 900.0) / 400.0 + 0.002 * rng.normal();
            spectra[k].push(round_to(a, 4));
        }
        octane.push(round_to(85.0 + 6.0 * c[0] + 3.0 * c[1] - 2.0 * c[2] + 0.15 * rng.normal(), 2));
        ids.push(format!("F{:02}", i + 1));
    }
    let mut columns = vec![text("Sample", ids), Column::numeric("Octane", octane)];
    for (k, values) in spectra.into_iter().enumerate() {
        columns.push(Column::numeric(format!("A{}", wavelength(k)), values));
    }
    table("Fuel spectra (simulated)", columns)
}

fn fuel_spectra_sample() -> Sample {
    let t = fuel_spectra();
    let predictors: Vec<usize> = (2..t.n_cols()).collect();
    let request = crate::PlsRequest {
        validation: crate::ValidationMethod::KFold { folds: 10, seed: 1 },
        ..crate::PlsRequest::new(col(&t, "Octane"), predictors)
    };
    Sample {
        reports: vec![SavedReport::PartialLeastSquares { request, profilers: crate::ProfilerOptions::default() }],
        table: t,
    }
}

/// **Simulated** loan applications: 600 applicants whose outcome follows
/// threshold rules a tree can find (default is most likely with a poor
/// history or a fair history and income below 35, late payment with a good
/// history and debt ratio above 0.45; unemployment raises the default risk), with about 6% of incomes missing (more often for the
/// self-employed). The interest rate steps with history and debt ratio,
/// falls for incomes above 80, and carries normal noise.
#[must_use]
pub fn loans() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0x10A2);
    let n = 600;
    let mut cols: [Vec<f64>; 5] = Default::default();
    let (mut history, mut employment, mut outcome) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..n {
        let h = match rng.next_f64() {
            u if u < 0.55 => "Good",
            u if u < 0.85 => "Fair",
            _ => "Poor",
        };
        let e = match rng.next_f64() {
            u if u < 0.7 => "Salaried",
            u if u < 0.9 => "Self-employed",
            _ => "Unemployed",
        };
        let age = (21.0 + 44.0 * rng.next_f64()).round();
        let base = if e == "Unemployed" { 18.0 } else { 30.0 + 0.6 * (age - 21.0) };
        let income = round_to((base * (0.6 + 0.8 * rng.next_f64())).max(8.0), 1);
        let debt = round_to((0.1 + 0.6 * rng.next_f64()).min(0.95), 2);
        let amount = (5.0 + 45.0 * rng.next_f64()).round() * 1000.0;
        let (mut default, late) = match h {
            "Poor" => (0.7, 0.15),
            "Good" if debt > 0.45 => (0.1, 0.7),
            "Good" => (0.02, 0.04),
            _ if income < 35.0 => (0.65, 0.15),
            _ => (0.05, 0.1),
        };
        if e == "Unemployed" {
            default += 0.1;
        }
        let u = rng.next_f64();
        let o = if u < default {
            "Default"
        } else if u < default + late {
            "Late"
        } else {
            "Repaid"
        };
        let rate =
            6.0 + match h {
                "Poor" => 6.0,
                "Fair" => 3.0,
                _ => 0.0,
            } + if debt > 0.4 { 4.0 } else { 0.0 }
                - if income > 80.0 { 1.5 } else { 0.0 }
                + amount / 50_000.0
                + 0.6 * rng.normal();
        let missing = rng.next_f64() < if e == "Self-employed" { 0.15 } else { 0.04 };
        cols[0].push(age);
        cols[1].push(if missing { f64::NAN } else { income });
        cols[2].push(debt);
        cols[3].push(amount);
        cols[4].push(round_to(rate, 2));
        history.push(h.to_owned());
        employment.push(e.to_owned());
        outcome.push(o.to_owned());
    }
    let [age_col, income_col, debt_col, amount_col, rate_col] = cols;
    // Fixed validation roles, stratified by outcome: 60% training, 25%
    // validation, 15% test of each class.
    let mut roles = vec![String::new(); n];
    let mut role_rng = stat_core::rng::Rng::new(0x10A3);
    for class in ["Default", "Late", "Repaid"] {
        let mut members: Vec<usize> = (0..n).filter(|&r| outcome[r] == class).collect();
        role_rng.shuffle(&mut members);
        let m = members.len() as f64;
        for (k, &r) in members.iter().enumerate() {
            let at = k as f64 / m;
            roles[r] = if at < 0.6 {
                "Training"
            } else if at < 0.85 {
                "Validation"
            } else {
                "Test"
            }
            .to_owned();
        }
    }
    table(
        "Loan applications (simulated)",
        vec![
            text("Credit history", history),
            text("Employment", employment),
            Column::numeric("Age", age_col),
            Column::numeric("Income (k)", income_col),
            Column::numeric("Debt ratio", debt_col),
            Column::numeric("Loan amount", amount_col),
            text("Outcome", outcome),
            Column::numeric("Interest rate (%)", rate_col),
            text("Validation", roles),
        ],
    )
}

fn loans_sample() -> Sample {
    let mut t = loans();
    let c = |n: &str| col(&t, n);
    let predictors =
        vec![c("Credit history"), c("Employment"), c("Age"), c("Income (k)"), c("Debt ratio"), c("Loan amount")];
    let roles = crate::ValidationMethod::Column(c("Validation"));
    let classification =
        crate::TreeRequest { validation: roles.clone(), ..crate::TreeRequest::new(c("Outcome"), predictors.clone()) };
    let regression = crate::TreeRequest {
        validation: crate::ValidationMethod::KFold { folds: 10, seed: 1 },
        ..crate::TreeRequest::new(c("Interest rate (%)"), predictors.clone())
    };
    let forest = crate::ForestRequest {
        validation: roles.clone(),
        ..crate::ForestRequest::new(c("Outcome"), predictors.clone())
    };
    let boosted =
        crate::BoostRequest { validation: roles, ..crate::BoostRequest::new(c("Outcome"), predictors.clone()) };
    // The default probability each model saves (as its options menu would),
    // for the comparison report.
    let rows = RowStateManager::new(t.n_rows());
    let default_probability = |columns: Vec<crate::ReportColumn>| {
        columns.into_iter().find(|k| k.name == "Prob Outcome = Default").map(|k| k.values)
    };
    let saved = [
        ("Prob Default (tree)", crate::TreeAnalysis::run(&t, &rows, &classification).map(|a| a.saveable_columns())),
        ("Prob Default (forest)", crate::ForestAnalysis::run(&t, &rows, &forest).map(|a| a.saveable_columns())),
        ("Prob Default (boosted)", crate::BoostAnalysis::run(&t, &rows, &boosted).map(|a| a.saveable_columns())),
    ];
    let profilers = crate::ProfilerOptions::default();
    let mut reports = vec![
        SavedReport::DecisionTree { request: classification, profilers },
        SavedReport::RandomForest { request: forest, profilers },
        SavedReport::BoostedTrees { request: boosted, profilers },
    ];
    let mut models = Vec::new();
    for (name, columns) in saved {
        let at = t.n_cols();
        if let Some(p) = columns.ok().and_then(default_probability) {
            if t.insert_column(at, Column::numeric(name, p)).is_ok() {
                models.push(at);
            }
        }
    }
    if !models.is_empty() {
        reports.push(SavedReport::ModelComparison {
            request: crate::ModelComparisonRequest {
                response: col(&t, "Outcome"),
                models,
                event: Some("Default".to_owned()),
                validation: Some(col(&t, "Validation")),
            },
        });
    }
    reports.push(SavedReport::DecisionTree { request: regression, profilers });
    Sample { reports, table: t }
}

/// Cornell, *Experiments with Mixtures* (§2.7): elongation of yarn made from
/// polyethylene, polystyrene and polypropylene blends.
#[must_use]
pub fn yarn() -> DataTable {
    let runs: [([f64; 3], &[f64]); 6] = [
        ([1.0, 0.0, 0.0], &[11.0, 12.4]),
        ([0.5, 0.5, 0.0], &[15.0, 14.8, 16.1]),
        ([0.0, 1.0, 0.0], &[8.8, 10.0]),
        ([0.0, 0.5, 0.5], &[10.0, 9.7, 11.8]),
        ([0.0, 0.0, 1.0], &[16.8, 16.0]),
        ([0.5, 0.0, 0.5], &[17.7, 16.4, 16.6]),
    ];
    let mut cols = vec![Vec::new(); 4];
    for (x, ys) in runs {
        for &y in ys {
            for k in 0..3 {
                cols[k].push(x[k]);
            }
            cols[3].push(y);
        }
    }
    let names = ["Polyethylene", "Polystyrene", "Polypropylene", "Elongation"];
    table("Yarn elongation (Cornell, mixture)", names.iter().zip(cols).map(|(n, v)| Column::numeric(*n, v)).collect())
}

fn yarn_sample() -> Sample {
    let t = yarn();
    let request = crate::MixtureRequest {
        response: col(&t, "Elongation"),
        components: vec![col(&t, "Polyethylene"), col(&t, "Polystyrene"), col(&t, "Polypropylene")],
        model: crate::ScheffeModel::Quadratic,
    };
    Sample { reports: vec![SavedReport::Mixture { request, profilers: crate::ProfilerOptions::default() }], table: t }
}

/// Simulated coating study: a rotatable central composite design in three
/// factors (8 factorial, 6 axial at ±1.682, 6 centre runs) with three
/// responses computed from known second-order equations in the coded
/// factors plus seeded normal noise.
#[must_use]
pub fn coating() -> DataTable {
    let a = 1.681_792_830_507_429;
    let mut coded: Vec<[f64; 3]> = Vec::new();
    for i in 0..8 {
        coded.push([
            if i & 1 == 0 { -1.0 } else { 1.0 },
            if i & 2 == 0 { -1.0 } else { 1.0 },
            if i & 4 == 0 { -1.0 } else { 1.0 },
        ]);
    }
    for k in 0..3 {
        for s in [-a, a] {
            let mut p = [0.0; 3];
            p[k] = s;
            coded.push(p);
        }
    }
    coded.extend(std::iter::repeat_n([0.0; 3], 6));
    let mut rng = stat_core::rng::Rng::new(2024);
    let mut cols = vec![Vec::new(); 6];
    for [t, m, c] in coded {
        cols[0].push(round_to(180.0 + 10.0 * t, 1));
        cols[1].push(round_to(20.0 + 5.0 * m, 1));
        cols[2].push(round_to(2.0 + 0.5 * c, 2));
        let hardness = 70.0 + 6.0 * t + 4.0 * m + 3.0 * c - 2.0 * t * t - 1.5 * m * m + 1.5 * t * m;
        let gloss = 88.0 + 3.0 * t - 4.0 * c - 2.5 * m * m - 2.0 * t * c;
        let cost = 12.0 + 1.5 * t + 1.0 * m + 2.0 * c;
        cols[3].push(round_to(hardness + rng.normal(), 1));
        cols[4].push(round_to(gloss + 1.2 * rng.normal(), 1));
        cols[5].push(round_to(cost + 0.3 * rng.normal(), 2));
    }
    let names = ["Temperature (°C)", "Time (min)", "Catalyst (%)", "Hardness", "Gloss", "Cost"];
    table("Coating formulation (simulated)", names.iter().zip(cols).map(|(n, v)| Column::numeric(*n, v)).collect())
}

fn coating_sample() -> Sample {
    use crate::{Desirability, FactorVariation, Goal};
    let t = coating();
    let responses = vec![col(&t, "Hardness"), col(&t, "Gloss"), col(&t, "Cost")];
    let predictors = vec![col(&t, "Temperature (°C)"), col(&t, "Time (min)"), col(&t, "Catalyst (%)")];
    let request = crate::MultipleResponsesRequest {
        responses,
        predictors,
        form: crate::ModelForm::Quadratic,
        goals: vec![
            Some(Desirability::new(Goal::Maximize, 65.0, 76.0)),
            Some(Desirability::new(Goal::Target(90.0), 82.0, 98.0)),
            Some(Desirability::new(Goal::Minimize, 10.0, 15.0)),
        ],
        settings: Vec::new(),
        simulation: Some(crate::SimulationSetup {
            runs: 10_000,
            seed: 1,
            factors: vec![
                FactorVariation::Normal { sd: 2.0 },
                FactorVariation::Normal { sd: 1.0 },
                FactorVariation::Uniform { half_width: 0.1 },
            ],
            noise: vec![1.0, 1.2, 0.3],
            limits: vec![(Some(66.0), None), (Some(85.0), Some(95.0)), (None, Some(14.0))],
            // An operating window of ±5 °C and ±2 minutes around 180 °C, 20 min.
            region: Some(vec![(175.0, 185.0), (18.0, 22.0), (2.0, 2.0)]),
        }),
        // The oven's energy budget limits hot, long cures together.
        constraints: "Temperature (°C) + 2*Time (min) <= 230".to_owned(),
        selection: None,
    };
    // The same goals with each response's terms selected stepwise from the
    // quadratic model: hardness keeps six terms, gloss three, cost three.
    let stepwise = crate::MultipleResponsesRequest {
        simulation: None,
        constraints: String::new(),
        selection: Some(crate::TermSelection::default()),
        ..request.clone()
    };
    Sample {
        reports: vec![
            SavedReport::MultipleResponses { request: Box::new(request) },
            SavedReport::MultipleResponses { request: Box::new(stepwise) },
        ],
        table: t,
    }
}

/// Montgomery, *Design and Analysis of Experiments* (Example 11.2): process
/// yield (%) from a central composite design in reaction time (min) and
/// temperature (°F), with five centre runs and axial distance √2.
#[must_use]
pub fn reaction_yield() -> DataTable {
    let r = 5.0 * std::f64::consts::SQRT_2;
    let round = |v: f64| round_to(v, 2);
    let time = [80.0, 90.0, 80.0, 90.0, 85.0, 85.0, 85.0, 85.0, 85.0, 85.0 + r, 85.0 - r, 85.0, 85.0];
    let temp = [170.0, 170.0, 180.0, 180.0, 175.0, 175.0, 175.0, 175.0, 175.0, 175.0, 175.0, 175.0 + r, 175.0 - r];
    let yields = [76.5, 78.0, 77.0, 79.5, 79.9, 80.3, 80.0, 79.7, 79.8, 78.4, 75.6, 78.5, 77.0];
    table(
        "Reaction yield (Montgomery, central composite)",
        vec![
            Column::numeric("Time (min)", time.iter().map(|&v| round(v)).collect()),
            Column::numeric("Temperature (°F)", temp.iter().map(|&v| round(v)).collect()),
            Column::numeric("Yield (%)", yields.to_vec()),
        ],
    )
}

fn reaction_yield_sample() -> Sample {
    let t = reaction_yield();
    let request = crate::SurfaceRequest {
        response: col(&t, "Yield (%)"),
        factors: vec![col(&t, "Time (min)"), col(&t, "Temperature (°F)")],
    };
    Sample {
        reports: vec![SavedReport::Surface {
            request,
            options: crate::SurfaceOptions {
                show_profiler: true,
                show_surface_profiler: true,
                ..crate::SurfaceOptions::default()
            },
        }],
        table: t,
    }
}

/// Freireich et al. (1963, *Blood* 21:699): weeks in remission for leukemia
/// patients given 6-mercaptopurine or placebo. `Censored` is 1 for patients
/// still in remission when observation ended.
#[must_use]
pub fn leukemia() -> DataTable {
    let mp = [6, 6, 6, 6, 7, 9, 10, 10, 11, 13, 16, 17, 19, 20, 22, 23, 25, 32, 32, 34, 35];
    let mp_censored = [0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1];
    let placebo = [1, 1, 2, 2, 3, 4, 4, 5, 5, 8, 8, 8, 8, 11, 11, 12, 12, 15, 17, 22, 23];
    let treatment = (0..42).map(|i| if i < 21 { "6-MP" } else { "Placebo" }.to_owned()).collect();
    table(
        "Leukemia remission (Freireich 1963)",
        vec![
            text("Treatment", treatment),
            Column::numeric("Weeks", mp.iter().chain(&placebo).map(|&w| f64::from(w)).collect()),
            Column::numeric("Censored", mp_censored.iter().chain(&[0; 21]).map(|&c| f64::from(c)).collect()),
        ],
    )
}

fn survival(t: &DataTable, time: &str, status: &str, group: &str, options: crate::SurvivalOptions) -> SavedReport {
    SavedReport::Survival {
        request: crate::SurvivalRequest {
            time: col(t, time),
            status: Some(col(t, status)),
            coding: crate::StatusCoding::NonzeroIsCensored,
            group: Some(col(t, group)),
        },
        options,
    }
}

fn leukemia_sample() -> Sample {
    let t = leukemia();
    let reports = vec![survival(&t, "Weeks", "Censored", "Treatment", crate::SurvivalOptions::default())];
    Sample { table: t, reports }
}

/// **Simulated** bearing life test: 30 units from each of two suppliers with
/// Weibull lifetimes (shape 2.2; characteristic life 2600 h for supplier A,
/// 3600 h for B). The test stops at 3000 h; about one unit in ten is removed
/// early at a uniformly random time. `Suspended` is 1 for units still running.
#[must_use]
pub fn bearings() -> DataTable {
    let mut noise = Noise(0xBEA2_1265_0BAD_F00D);
    let (mut unit, mut supplier, mut hours, mut suspended) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (s, scale) in [("A", 2600.0), ("B", 3600.0)] {
        for i in 1..=30 {
            let life: f64 = scale * (-(1.0 - noise.uniform()).ln()).powf(1.0 / 2.2);
            let removed = if noise.uniform() < 0.1 { 300.0 + 2700.0 * noise.uniform() } else { f64::INFINITY };
            let end = removed.min(3000.0);
            let (h, c) = if life < end { (life, 0.0) } else { (end, 1.0) };
            unit.push(format!("{s}-{i:02}"));
            supplier.push(s.to_owned());
            hours.push(h.round());
            suspended.push(c);
        }
    }
    table(
        "Bearing life test (simulated)",
        vec![
            text("Unit", unit),
            text("Supplier", supplier),
            Column::numeric("Hours", hours),
            Column::numeric("Suspended", suspended),
        ],
    )
}

fn bearings_sample() -> Sample {
    let t = bearings();
    let options =
        crate::SurvivalOptions { show_lognormal: true, risk_table: true, ..crate::SurvivalOptions::default() };
    let reports = vec![survival(&t, "Hours", "Suspended", "Supplier", options)];
    Sample { table: t, reports }
}

/// **Simulated** reliability growth test of a prototype on one cumulative
/// clock. Phase 1 (0–1500 h) follows a power-law process with `β = 0.7`
/// and `λ = 0.1495` (about 25 failures expected); after a redesign, phase 2
/// (1500–4000 h) follows `β = 0.6`, `λ = 0.186` from 1500 h (about 12
/// expected). Failure times are drawn by inverting the cumulative intensity
/// of unit-rate exponential gaps, recorded to 0.1 h; each phase ends with
/// a record (Failure = 0) at its end time.
#[must_use]
pub fn reliability_growth() -> DataTable {
    let mut noise = Noise(0x6E0_C2A3_5A11_0042);
    let (mut hours, mut failure, mut phase) = (Vec::new(), Vec::new(), Vec::new());
    for (name, start, end, beta, lambda) in
        [("1 Initial design", 0.0f64, 1500.0f64, 0.7f64, 0.1495f64), ("2 Redesign", 1500.0, 4000.0, 0.6, 0.186)]
    {
        let mut cumulative = 0.0;
        loop {
            cumulative += -(1.0 - noise.uniform()).ln();
            let t = (cumulative / lambda + start.powf(beta)).powf(1.0 / beta);
            if t >= end {
                break;
            }
            hours.push(round_to(t, 1).max(start + 0.1));
            failure.push(1.0);
            phase.push(name.to_owned());
        }
        hours.push(end);
        failure.push(0.0);
        phase.push(name.to_owned());
    }
    table(
        "Prototype failures (simulated)",
        vec![Column::numeric("Test hours", hours), Column::numeric("Failure", failure), text("Phase", phase)],
    )
}

fn reliability_growth_sample() -> Sample {
    let t = reliability_growth();
    let base =
        crate::GrowthRequest { event: Some(col(&t, "Failure")), ..crate::GrowthRequest::new(col(&t, "Test hours")) };
    let reports = vec![
        SavedReport::Growth { request: base.clone() },
        SavedReport::Growth { request: crate::GrowthRequest { phase: Some(col(&t, "Phase")), ..base } },
    ];
    Sample { table: t, reports }
}

/// **Simulated** repairs of 24 pumps, 12 of an old and 12 of a new design,
/// each observed for 300–720 days. Repairs follow power-law processes:
/// old pumps `β = 1.6`, `λ = 2.885·10⁻⁴` (wear-out, about 6 repairs by day
/// 500), new pumps a constant rate of 0.005 per day. Ages are whole days;
/// each pump ends with a record (Repair = 0) at its end of observation.
#[must_use]
pub fn repairable_systems() -> DataTable {
    let mut noise = Noise(0x9C_A1_75_00_24_16_05_01);
    let (mut unit, mut design, mut age, mut repair) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for k in 0..24 {
        let (name, beta, lambda) = if k < 12 { ("Old", 1.6f64, 2.885e-4f64) } else { ("New", 1.0, 0.005) };
        let end = (300.0 + 420.0 * noise.uniform()).round();
        let id = format!("P{:02}", k + 1);
        let mut cumulative = 0.0;
        loop {
            cumulative += -(1.0 - noise.uniform()).ln();
            let t = (cumulative / lambda).powf(1.0 / beta).round().max(1.0);
            if t >= end {
                break;
            }
            unit.push(id.clone());
            design.push(name.to_owned());
            age.push(t);
            repair.push(1.0);
        }
        unit.push(id);
        design.push(name.to_owned());
        age.push(end);
        repair.push(0.0);
    }
    table(
        "Pump repairs (simulated)",
        vec![
            text("Unit", unit),
            text("Design", design),
            Column::numeric("Age (days)", age),
            Column::numeric("Repair", repair),
        ],
    )
}

/// **Invented** inspection data: seals of suppliers A (60 units) and B (50)
/// checked every 250 hours up to 2000 hours. Each row counts the units
/// (`Units`) found failed at `Found failed (h)` that had passed the
/// inspection at `Last pass (h)`: an empty last pass means failed by the
/// first inspection (left-censored), an empty found-failed time means still
/// sound at 2000 hours (right-censored), and equal times are failures seen
/// exactly in service. The counts were chosen to follow Weibull
/// distributions with shape about 2 and scale about 1800 (A) and 1400 (B)
/// hours.
#[must_use]
pub fn inspection_life_data() -> DataTable {
    let nan = f64::NAN;
    let a = [1.0, 3.0, 5.0, 6.0, 7.0, 7.0, 7.0, 6.0, 18.0];
    let b = [2.0, 4.0, 6.0, 8.0, 7.0, 7.0, 5.0, 4.0, 7.0];
    let (mut supplier, mut start, mut end, mut units) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (name, counts, exact) in [("A", a, &[620.0, 1130.0][..]), ("B", b, &[480.0, 905.0, 1320.0][..])] {
        for (k, &n) in counts.iter().enumerate() {
            let lo = 250.0 * k as f64;
            supplier.push(name.to_owned());
            start.push(if k == 0 { nan } else { lo });
            end.push(if k == 8 { nan } else { lo + 250.0 });
            units.push(n);
        }
        for &t in exact {
            supplier.push(name.to_owned());
            start.push(t);
            end.push(t);
            units.push(1.0);
        }
    }
    table(
        "Seal inspections (invented)",
        vec![
            text("Supplier", supplier),
            Column::numeric("Last pass (h)", start),
            Column::numeric("Found failed (h)", end),
            Column::numeric("Units", units),
        ],
    )
}

fn seal_inspection_sample() -> Sample {
    let t = inspection_life_data();
    let request = crate::LifeDistributionRequest {
        frequency: Some(col(&t, "Units")),
        group: Some(col(&t, "Supplier")),
        detail: Some(stat_core::distribution_fit::FitFamily::Weibull),
        times: vec![500.0, 1000.0],
        ..crate::LifeDistributionRequest::new(crate::LifeDataLayout::Intervals {
            start: col(&t, "Last pass (h)"),
            end: col(&t, "Found failed (h)"),
        })
    };
    Sample { table: t, reports: vec![SavedReport::LifeDistribution { request }] }
}

/// **Invented** endurance test: twelve bearings run up to 1500 hours, with
/// failures at 610 and 1240 hours (`Censored` = 1 for the ten still running
/// at 1500 hours).
fn bearing_test_sample() -> Sample {
    let mut hours = vec![610.0, 1240.0];
    hours.extend([1500.0; 10]);
    let mut censored = vec![0.0, 0.0];
    censored.extend([1.0; 10]);
    let t = table(
        "Bearing endurance test (invented)",
        vec![Column::numeric("Hours", hours), Column::numeric("Censored", censored)],
    );
    let request = crate::LifeDistributionRequest {
        families: vec![
            stat_core::distribution_fit::FitFamily::Exponential,
            stat_core::distribution_fit::FitFamily::Weibull,
        ],
        probabilities: vec![0.01, 0.1],
        times: vec![1000.0],
        known_shape: Some(2.0),
        ..crate::LifeDistributionRequest::new(crate::LifeDataLayout::Times {
            time: col(&t, "Hours"),
            censor: Some(col(&t, "Censored")),
            coding: crate::StatusCoding::NonzeroIsCensored,
        })
    };
    Sample { table: t, reports: vec![SavedReport::LifeDistribution { request }] }
}

fn repairable_systems_sample() -> Sample {
    let t = repairable_systems();
    let request = crate::RecurrenceRequest {
        event: Some(col(&t, "Repair")),
        group: Some(col(&t, "Design")),
        ..crate::RecurrenceRequest::new(col(&t, "Unit"), col(&t, "Age (days)"))
    };
    Sample { table: t, reports: vec![SavedReport::Recurrence { request }] }
}

/// Student (1908, *Biometrika* 6:1, from Cushny & Peebles 1905): extra hours
/// of sleep for ten patients under two soporific drugs, stacked in one column
/// with the drug and patient identifying each value.
#[must_use]
pub fn sleep() -> DataTable {
    let a = [0.7, -1.6, -0.2, -1.2, -0.1, 3.4, 3.7, 0.8, 0.0, 2.0];
    let b = [1.9, 0.8, 1.1, 0.1, -0.1, 4.4, 5.5, 1.6, 4.6, 3.4];
    table(
        "Sleep drug trial (Student 1908)",
        vec![
            text("Patient", (0..20).map(|i| format!("P{:02}", i % 10 + 1)).collect()),
            text("Drug", (0..20).map(|i| if i < 10 { "A" } else { "B" }.to_owned()).collect()),
            Column::numeric("Extra sleep (h)", a.iter().chain(&b).copied().collect()),
        ],
    )
}

fn sleep_sample() -> Sample {
    let t = sleep();
    let (y, drug, patient) = (col(&t, "Extra sleep (h)"), col(&t, "Drug"), col(&t, "Patient"));
    let reports = vec![
        SavedReport::Paired {
            request: crate::PairedRequest::new(crate::PairedLayout::Stacked {
                response: y,
                condition: drug,
                subject: patient,
            }),
        },
        SavedReport::Groups {
            request: crate::GroupComparisonRequest::new(crate::GroupLayout::Stacked { response: y, group: drug }),
        },
        SavedReport::Power {
            request: crate::PowerRequest {
                design: crate::PowerDesign::OneSampleT { difference: 1.0, std_dev: 1.23 },
                ..crate::PowerRequest::default()
            },
        },
    ];
    Sample { table: t, reports }
}

/// A deterministic random source for the simulated datasets: xorshift64 with
/// Irwin–Hall normals (sum of 12 uniforms − 6).
struct Noise(u64);

impl Noise {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn normal(&mut self) -> f64 {
        (0..12).map(|_| self.uniform()).sum::<f64>() - 6.0
    }
}

fn round_to(x: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (x * f).round() / f
}

fn text(name: &str, values: Vec<String>) -> Column {
    Column::text(name, values)
}

fn table(name: &str, columns: Vec<Column>) -> DataTable {
    DataTable::new(name, columns).expect("sample columns have equal length and unique names")
}

fn col(t: &DataTable, name: &str) -> usize {
    t.find(name).expect("sample column exists")
}

fn linear(t: &DataTable, response: &str, predictors: &[&str], options: ReportOptions) -> SavedReport {
    SavedReport::Linear {
        spec: LinearModelSpec {
            response: col(t, response),
            predictors: predictors.iter().map(|p| col(t, p)).collect(),
            effects: Vec::new(),
            splines: Vec::new(),
        },
        options,
    }
}

fn distribution(t: &DataTable, column: &str, options: DistributionOptions) -> SavedReport {
    SavedReport::Distribution {
        request: DistributionRequest { column: col(t, column), quantile_method: QuantileMethod::default(), censoring: None },
        options,
    }
}

fn bivariate(t: &DataTable, x: &str, y: &str) -> SavedReport {
    SavedReport::Bivariate { request: BivariateRequest::new(col(t, x), col(t, y)) }
}

fn longley_sample() -> Sample {
    let t = longley();
    let predictors = ["GNP deflator", "GNP", "Unemployed", "Armed forces", "Population 14+", "Year"];
    let reports = vec![
        linear(
            &t,
            "Employed",
            &["GNP", "Unemployed", "Armed forces", "Year"],
            ReportOptions { show_diagnostics: true, ..ReportOptions::default() },
        ),
        SavedReport::Pca {
            request: crate::PcaRequest { columns: predictors.iter().map(|p| col(&t, p)).collect(), standardize: true },
        },
        SavedReport::Regularized {
            request: crate::RegularizedRequest {
                response: col(&t, "Employed"),
                predictors: predictors.iter().map(|p| col(&t, p)).collect(),
                alpha: 1.0,
                folds: 4,
                seed: 1,
            },
            profilers: crate::ProfilerOptions::default(),
        },
    ];
    Sample { table: t, reports }
}

/// **Simulated** flower measurements: 30 flowers of each of three varieties
/// (A, B, C) with petal and sepal length and width (cm). Variety means differ
/// mostly in petal size; measurements share a common size factor plus noise.
#[must_use]
pub fn flowers() -> DataTable {
    let mut noise = Noise(0xF10E_F10E_F10E_F10E);
    let means = [[5.0, 3.4, 1.5, 0.25], [5.9, 2.8, 4.3, 1.3], [6.6, 3.0, 5.6, 2.0]];
    let spread = [0.35, 0.3, 0.35, 0.15];
    let names = ["A", "B", "C"];
    let mut cols: [Vec<f64>; 4] = Default::default();
    let mut variety = Vec::new();
    for i in 0..90 {
        let v = i % 3;
        let size = noise.normal();
        for j in 0..4 {
            cols[j].push(round_to(means[v][j] + spread[j] * (0.6 * size + 0.8 * noise.normal()), 1));
        }
        variety.push(names[v].to_owned());
    }
    let [sl, sw, pl, pw] = cols;
    table(
        "Flower measurements (simulated)",
        vec![
            text("Variety", variety),
            Column::numeric("Sepal length", sl),
            Column::numeric("Sepal width", sw),
            Column::numeric("Petal length", pl),
            Column::numeric("Petal width", pw),
        ],
    )
}

fn flowers_sample() -> Sample {
    let t = flowers();
    let measures: Vec<usize> =
        ["Sepal length", "Sepal width", "Petal length", "Petal width"].iter().map(|c| col(&t, c)).collect();
    let reports = vec![
        SavedReport::Pca { request: crate::PcaRequest { columns: measures.clone(), standardize: true } },
        SavedReport::Cluster {
            request: crate::ClusterRequest {
                distance: Default::default(),
                columns: measures.clone(),
                method: crate::ClusterMethod::KMeans { k: 3, seed: 1 },
                standardize: true,
                choose: Some(crate::ChooseClusters::default()),
                centers: None,
            },
        },
        SavedReport::Cluster {
            request: crate::ClusterRequest {
                distance: Default::default(),
                columns: measures.clone(),
                method: crate::ClusterMethod::Hierarchical { linkage: crate::Linkage::Ward, k: 3 },
                standardize: true,
                choose: None,
                centers: None,
            },
        },
        SavedReport::Correlations { request: crate::CorrelationRequest::new(measures.clone()) },
        SavedReport::Graph {
            spec: Box::new(stat_viz::GraphSpec {
                encoding: stat_viz::EncodingPlan {
                    x: ["Sepal length", "Sepal width", "Petal length", "Petal width"].map(String::from).to_vec(),
                    color: Some("Variety".to_owned()),
                    ..Default::default()
                },
                ellipse: Some(stat_viz::EllipseElement::default()),
                form: stat_viz::GraphForm::ScatterMatrix,
                ..stat_viz::GraphSpec::default()
            }),
        },
        SavedReport::OutlierDistances { request: crate::OutlierRequest::new(measures.clone()) },
        SavedReport::Manova {
            request: crate::ManovaRequest {
                contrasts: vec![crate::ManovaContrast {
                    name: "A vs B".to_owned(),
                    factor: col(&t, "Variety"),
                    weights: vec![("A".to_owned(), 1.0), ("B".to_owned(), -1.0)],
                }],
                ..crate::ManovaRequest::new(measures.clone(), vec![col(&t, "Variety")])
            },
        },
        SavedReport::PartialLeastSquares {
            request: crate::PlsRequest {
                validation: crate::ValidationMethod::KFold { folds: 10, seed: 1 },
                ..crate::PlsRequest::new(col(&t, "Variety"), measures)
            },
            profilers: crate::ProfilerOptions::default(),
        },
    ];
    Sample { table: t, reports }
}

/// Wording of the course-evaluation items; `true` marks the negatively
/// worded ones, which are reverse-scored.
pub const COURSE_ITEMS: [(&str, bool); 6] = [
    ("Clear explanations", false),
    ("Well organized", false),
    ("Hard to follow", true),
    ("Useful examples", false),
    ("Pace too fast", true),
    ("Would recommend", false),
];

/// **Simulated** course evaluation: 120 students in two sections answer six
/// 1–5 agreement items. Each student has an overall opinion θ ~ N(0, 1);
/// item `j` is `round(3 + λⱼθ + 0.75ε)` clamped to 1–5 with loadings
/// 0.9, 0.8, 0.85, 0.7, 0.6, 1.0, and the two negatively worded items
/// ("Hard to follow", "Pace too fast") are answered on the reversed scale
/// (6 minus that). Four answers are left blank.
#[must_use]
pub fn course_evaluation() -> DataTable {
    let mut noise = Noise(0xC05E_E7A1_0000_0001);
    let loadings = [0.9, 0.8, 0.85, 0.7, 0.6, 1.0];
    let mut items: Vec<Vec<f64>> = vec![Vec::new(); COURSE_ITEMS.len()];
    let mut section = Vec::new();
    for i in 0..120 {
        let theta = noise.normal() + if i % 2 == 0 { 0.2 } else { -0.2 };
        for (j, (_, negative)) in COURSE_ITEMS.iter().enumerate() {
            let x = (3.0 + loadings[j] * theta + 0.75 * noise.normal()).round().clamp(1.0, 5.0);
            items[j].push(if *negative { 6.0 - x } else { x });
        }
        section.push(if i % 2 == 0 { "Morning" } else { "Evening" }.to_owned());
    }
    for (row, item) in [(7, 1), (33, 4), (58, 2), (91, 5)] {
        items[item][row] = f64::NAN;
    }
    let mut columns = vec![Column::numeric("Student", (1..=120).map(f64::from).collect())];
    columns[0].level = MeasurementLevel::Nominal;
    columns.push(text("Section", section));
    for ((name, _), values) in COURSE_ITEMS.iter().zip(items) {
        let mut c = Column::numeric(*name, values);
        c.level = MeasurementLevel::Ordinal;
        columns.push(c);
    }
    table("Course evaluation (simulated)", columns)
}

fn course_evaluation_sample() -> Sample {
    let t = course_evaluation();
    let items: Vec<usize> = COURSE_ITEMS.iter().map(|(n, _)| col(&t, n)).collect();
    let reversed: Vec<usize> = COURSE_ITEMS.iter().filter(|(_, neg)| *neg).map(|(n, _)| col(&t, n)).collect();
    let reports = vec![
        SavedReport::ItemAnalysis {
            request: crate::ItemAnalysisRequest {
                reversed,
                scale: Some((1.0, 5.0)),
                ..crate::ItemAnalysisRequest::new(items.clone())
            },
        },
        SavedReport::Correlations {
            request: crate::CorrelationRequest {
                spearman: true,
                partial: false,
                ..crate::CorrelationRequest::new(items)
            },
        },
    ];
    Sample { table: t, reports }
}

/// **Simulated** customer churn: 400 customers with tenure (months, 1–72),
/// monthly charge (20–120), contract (Monthly or Annual), support calls
/// (Poisson, mean rising with charge and falling with tenure) and churn (yes/no,
/// logit = 0.5 − 0.05·tenure + 0.02·(charge − 70) + 1.2·monthly contract +
/// 0.25·calls).
#[must_use]
pub fn churn() -> DataTable {
    let mut noise = Noise(0xC4C4_0B0B_1E1E_5A5A);
    let (mut tenure, mut charge, mut contract, mut calls, mut churned, mut spend) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..400 {
        let t = (1.0 + 71.0 * noise.uniform()).round();
        let c = round_to(20.0 + 100.0 * noise.uniform(), 2);
        let monthly = noise.uniform() < 0.55;
        // Poisson counts by inversion.
        let rate = (0.3 + 0.012 * c - 0.01 * t).max(0.1);
        let (mut k, mut p, mut cum, u) = (0u32, (-rate).exp(), 0.0, noise.uniform());
        loop {
            cum += p;
            if u <= cum || k > 30 {
                break;
            }
            k += 1;
            p *= rate / f64::from(k);
        }
        let logit = 0.5 - 0.05 * t + 0.02 * (c - 70.0) + if monthly { 1.2 } else { 0.0 } + 0.25 * f64::from(k);
        let yes = noise.uniform() < 1.0 / (1.0 + (-logit).exp());
        tenure.push(t);
        charge.push(c);
        contract.push(if monthly { "Monthly" } else { "Annual" }.to_owned());
        calls.push(f64::from(k));
        churned.push(if yes { "Yes" } else { "No" }.to_owned());
        spend.push(round_to(c * 12.0 * (0.9 + 0.2 * noise.uniform()) + 5.0 * t + 40.0 * noise.normal(), 0));
    }
    table(
        "Customer churn (simulated)",
        vec![
            Column::numeric("Tenure (months)", tenure),
            Column::numeric("Monthly charge", charge),
            text("Contract", contract),
            Column::numeric("Support calls", calls),
            Column::numeric("Annual spend", spend),
            text("Churned", churned),
        ],
    )
}

fn churn_sample() -> Sample {
    let t = churn();
    let c = |n: &str| col(&t, n);
    let reports = vec![
        SavedReport::Glm {
            request: crate::GlmRequest {
                response: c("Churned"),
                predictors: vec![c("Tenure (months)"), c("Monthly charge"), c("Contract"), c("Support calls")],
                family: crate::Family::Binomial,
                link: None,
                event: None,
                effects: Vec::new(),
                splines: Vec::new(),
                slices: None,
                slopes: None,
            },
            profilers: crate::ProfilerOptions {
                show_profiler: true,
                show_surface_profiler: true,
                show_observed: false,
            },
        },
        SavedReport::Glm {
            request: crate::GlmRequest {
                response: c("Support calls"),
                predictors: vec![c("Tenure (months)"), c("Monthly charge")],
                family: crate::Family::Poisson,
                link: None,
                event: None,
                effects: Vec::new(),
                splines: Vec::new(),
                slices: None,
                slopes: None,
            },
            profilers: crate::ProfilerOptions::default(),
        },
        SavedReport::Regularized {
            request: crate::RegularizedRequest {
                response: c("Annual spend"),
                predictors: vec![c("Tenure (months)"), c("Monthly charge"), c("Contract"), c("Support calls")],
                alpha: 1.0,
                folds: 10,
                seed: 1,
            },
            profilers: crate::ProfilerOptions::default(),
        },
        SavedReport::Proportions {
            request: crate::ProportionRequest::new(
                c("Churned"),
                crate::ProportionComparison::Groups { group: c("Contract") },
            ),
        },
        SavedReport::Rates {
            request: crate::RateRequest {
                exposure: Some(c("Tenure (months)")),
                group: Some(c("Contract")),
                ..crate::RateRequest::new(c("Support calls"))
            },
        },
    ];
    Sample { table: t, reports }
}

/// NIST StRD Norris: 36 readings (y) against reference values (x).
#[must_use]
pub fn norris() -> DataTable {
    let d = linear_regression_datasets().into_iter().find(|d| d.name == "Norris").expect("Norris is bundled");
    table(
        "Norris calibration (NIST)",
        vec![Column::numeric("Reference", d.predictors.col(0).to_vec()), Column::numeric("Reading", d.response)],
    )
}

fn norris_sample() -> Sample {
    let t = norris();
    let options = ReportOptions { show_residual_plot: true, show_diagnostics: true, ..ReportOptions::default() };
    let reports = vec![linear(&t, "Reading", &["Reference"], options)];
    Sample { table: t, reports }
}

/// **Simulated** plant growth: 60 plants, height (cm) = 12 + fertilizer effect
/// (A 0, B +3.5, C +1.5) + 0.9 × light hours + greenhouse effect (0, −1, +0.5,
/// +1) + N(0, 1.2²) noise; plants 17 and 44 are outliers (+9 cm, −8 cm).
/// Greenhouse is stored as numbers 1–4 marked nominal.
#[must_use]
pub fn plant_growth() -> DataTable {
    let mut noise = Noise(0x2545_F491_4F6C_DD1D);
    let fert = ["A", "B", "C"];
    let fert_effect = [0.0, 3.5, 1.5];
    let house_effect = [0.0, -1.0, 0.5, 1.0];
    let (mut f, mut light, mut house, mut height) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..60 {
        let fi = i % 3;
        let gi = (i / 3) % 4;
        let hours = round_to(8.0 + 8.0 * noise.uniform(), 1);
        let mut h = 12.0 + fert_effect[fi] + 0.9 * hours + house_effect[gi] + 1.2 * noise.normal();
        if i == 16 {
            h += 9.0;
        }
        if i == 43 {
            h -= 8.0;
        }
        f.push(fert[fi].to_owned());
        light.push(hours);
        house.push((gi + 1) as f64);
        height.push(round_to(h, 1));
    }
    let mut greenhouse = Column::numeric("Greenhouse", house);
    greenhouse.level = MeasurementLevel::Nominal;
    table(
        "Plant growth (simulated)",
        vec![
            text("Fertilizer", f),
            Column::numeric("Light hours", light),
            greenhouse,
            Column::numeric("Height", height),
        ],
    )
}

fn plant_growth_sample() -> Sample {
    let t = plant_growth();
    let options = ReportOptions { show_diagnostics: true, ..ReportOptions::default() };
    let reports = vec![
        linear(&t, "Height", &["Fertilizer", "Light hours", "Greenhouse"], options),
        bivariate(&t, "Fertilizer", "Height"),
        // The same fixed effects with greenhouses as a random sample of greenhouses.
        SavedReport::Mixed {
            request: crate::MixedRequest {
                response: col(&t, "Height"),
                predictors: vec![col(&t, "Fertilizer"), col(&t, "Light hours")],
                effects: Vec::new(),
                random: vec![crate::RandomEffectSpec::intercept(col(&t, "Greenhouse"))],
                family: crate::MixedFamily::Normal,
                trials: None,
                offset: None,
                reml: true,
                quadrature_points: 1,
                slices: None,
                slopes: None,
                frequency: None,
                weight: None,
                robust: false,
                residual_groups: None,
                df_method: stat_mixed::DfMethod::Satterthwaite,
                order: Vec::new(),
            },
            profilers: crate::surface_profiler::ProfilerOptions::default(),
        },
        // A residual variance for each fertilizer, with robust standard
        // errors clustered by greenhouse.
        SavedReport::Mixed {
            request: crate::MixedRequest {
                response: col(&t, "Height"),
                predictors: vec![col(&t, "Fertilizer")],
                effects: Vec::new(),
                random: vec![crate::RandomEffectSpec::intercept(col(&t, "Greenhouse"))],
                family: crate::MixedFamily::Normal,
                trials: None,
                offset: None,
                reml: true,
                quadrature_points: 1,
                slices: None,
                slopes: None,
                frequency: None,
                weight: None,
                robust: true,
                residual_groups: Some(col(&t, "Fertilizer")),
                df_method: stat_mixed::DfMethod::Satterthwaite,
                order: Vec::new(),
            },
            profilers: crate::surface_profiler::ProfilerOptions::default(),
        },
    ];
    Sample { table: t, reports }
}

/// **Simulated** catalyst experiment: a 2³ factorial in temperature (150, 190
/// °C), pressure (2, 4 bar) and catalyst (A, B), run twice, plus two centre runs
/// (170 °C, 3 bar) per catalyst. Yield (%) = 70 + 5·T + 2.5·P + 1.5·T·P −
/// 2·T² + 3 (catalyst B) + N(0, 1) in coded units.
#[must_use]
pub fn catalyst() -> DataTable {
    let mut noise = Noise(0x1234_5678_9ABC_DEF1);
    let (mut temp, mut pres, mut cat, mut yld) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut add = |t: f64, p: f64, c: usize, noise: &mut Noise| {
        let y = 70.0 + 5.0 * t + 2.5 * p + 1.5 * t * p - 2.0 * t * t + 3.0 * c as f64 + noise.normal();
        temp.push(170.0 + 20.0 * t);
        pres.push(3.0 + p);
        cat.push(["A", "B"][c].to_owned());
        yld.push(round_to(y, 1));
    };
    for _replicate in 0..2 {
        for c in 0..2 {
            for p in [-1.0, 1.0] {
                for t in [-1.0, 1.0] {
                    add(t, p, c, &mut noise);
                }
            }
        }
    }
    for c in 0..2 {
        for _ in 0..2 {
            add(0.0, 0.0, c, &mut noise);
        }
    }
    table(
        "Catalyst experiment (simulated)",
        vec![
            Column::numeric("Temperature", temp),
            Column::numeric("Pressure", pres),
            text("Catalyst", cat),
            Column::numeric("Yield", yld),
        ],
    )
}

fn catalyst_sample() -> Sample {
    let t = catalyst();
    let options = ReportOptions { show_residual_plot: true, show_surface_profiler: true, ..ReportOptions::default() };
    let SavedReport::Linear { mut spec, options } =
        linear(&t, "Yield", &["Temperature", "Pressure", "Catalyst"], options)
    else {
        unreachable!()
    };
    // Main effects, Temperature*Pressure and Temperature^2 (positions in the predictor list).
    spec.effects = vec![vec![(0, 1)], vec![(1, 1)], vec![(2, 1)], vec![(0, 1), (1, 1)], vec![(0, 2)]];
    Sample { reports: vec![SavedReport::Linear { spec, options }], table: t }
}

/// NIST StRD Michelso: 100 measurements of the speed of light (thousands of km/s).
#[must_use]
pub fn michelson() -> DataTable {
    let d = univariate_datasets().into_iter().find(|d| d.name == "Michelso").expect("Michelso is bundled");
    let n = d.values.len();
    table(
        "Michelson speed of light (NIST)",
        vec![
            Column::numeric("Measurement", (1..=n).map(|i| i as f64).collect()),
            Column::numeric("Speed (1000 km/s)", d.values),
        ],
    )
}

fn michelson_sample() -> Sample {
    let t = michelson();
    let options = DistributionOptions {
        show_quantile_plot: true,
        tolerance: Some(crate::ToleranceSettings::default()),
        ..DistributionOptions::default()
    };
    let speed = col(&t, "Speed (1000 km/s)");
    let one_sample = crate::OneSampleRequest { null_mean: 299.792_458, ..crate::OneSampleRequest::new(speed) };
    let reports = vec![distribution(&t, "Speed (1000 km/s)", options), SavedReport::OneSample { request: one_sample }];
    Sample { reports, table: t }
}

/// **Simulated** clinic waiting times: 150 visits at three clinics on two
/// shifts. Wait (minutes) is log-normal with median 12 / 18 / 25 minutes and
/// log-scale spread 0.35 / 0.55 / 0.8 by clinic, 20% longer at night; 6 waits
/// are missing.
#[must_use]
pub fn wait_times() -> DataTable {
    let mut noise = Noise(0x0DDB_A11C_AFE5_EED5);
    let clinics = ["North", "South", "East"];
    let medians = [12.0f64, 18.0, 25.0];
    let spreads = [0.35, 0.55, 0.8];
    let (mut clinic, mut shift, mut wait) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..150 {
        let c = i % 3;
        let night = (i / 3) % 2 == 1;
        let mut w = (medians[c].ln() + spreads[c] * noise.normal()).exp();
        if night {
            w *= 1.2;
        }
        clinic.push(clinics[c].to_owned());
        shift.push(if night { "Night" } else { "Day" }.to_owned());
        wait.push(if i % 25 == 7 { f64::NAN } else { round_to(w, 1) });
    }
    table(
        "Clinic wait times (simulated)",
        vec![text("Clinic", clinic), text("Shift", shift), Column::numeric("Wait (min)", wait)],
    )
}

fn wait_times_sample() -> Sample {
    let mut t = wait_times();
    // Formula columns: a log transform, and each wait relative to its
    // clinic's median.
    let n = t.n_rows();
    for (name, f) in [
        ("Log wait", "ln(col(\"Wait (min)\"))"),
        ("Wait / clinic median", "col(\"Wait (min)\") / col_quantile(\"Wait (min)\", 0.5, \"Clinic\")"),
    ] {
        let column = Column { formula: Some(f.to_owned()), ..Column::numeric(name, vec![f64::NAN; n]) };
        t.insert_column(t.n_cols(), column).expect("a new column name");
    }
    crate::formula::refresh_all(&mut t);
    let options =
        DistributionOptions { show_quantile_plot: true, show_box_cox: true, ..DistributionOptions::default() };
    let reports = vec![
        distribution(&t, "Wait (min)", options),
        bivariate(&t, "Clinic", "Wait / clinic median"),
        bivariate(&t, "Clinic", "Wait (min)"),
        bivariate(&t, "Shift", "Wait (min)"),
    ];
    Sample { table: t, reports }
}

/// NIST StRD SiRstv: silicon wafer resistivity, five readings by each of five
/// instruments.
#[must_use]
pub fn resistivity() -> DataTable {
    let d = anova_datasets().into_iter().find(|d| d.name == "SiRstv").expect("SiRstv is bundled");
    let (mut instrument, mut value) = (Vec::new(), Vec::new());
    for (g, readings) in d.groups.iter().enumerate() {
        for &v in readings {
            instrument.push(format!("Instrument {}", g + 1));
            value.push(v);
        }
    }
    table("Silicon resistivity (NIST)", vec![text("Instrument", instrument), Column::numeric("Resistivity", value)])
}

fn resistivity_sample() -> Sample {
    let t = resistivity();
    let groups = SavedReport::Groups {
        request: crate::GroupComparisonRequest::new(crate::GroupLayout::Stacked {
            response: col(&t, "Resistivity"),
            group: col(&t, "Instrument"),
        }),
    };
    Sample { reports: vec![bivariate(&t, "Instrument", "Resistivity"), groups], table: t }
}

/// **Simulated** customer survey of 200 people: region (4), preferred channel
/// (Web, Phone, Store; preference depends on region), satisfaction 1–5 (stored
/// as numbers, ordinal), age, and annual spend (rising with age, with noise).
#[must_use]
pub fn survey() -> DataTable {
    let mut noise = Noise(0x5EED_0F5E_A5ED_C0DE);
    let regions = ["North", "South", "East", "West"];
    // Channel probabilities (Web, Phone) by region; Store gets the rest.
    let probs = [(0.55, 0.15), (0.30, 0.35), (0.45, 0.20), (0.25, 0.20)];
    let (mut region, mut channel, mut sat, mut age, mut spend) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..200 {
        let r = i % 4;
        let u = noise.uniform();
        let ch = if u < probs[r].0 {
            "Web"
        } else if u < probs[r].0 + probs[r].1 {
            "Phone"
        } else {
            "Store"
        };
        let a = round_to(22.0 + 50.0 * noise.uniform(), 0);
        let s = (3.4 + 0.9 * noise.normal() + if ch == "Store" { 0.4 } else { 0.0 }).round().clamp(1.0, 5.0);
        region.push(regions[r].to_owned());
        channel.push(ch.to_owned());
        sat.push(s);
        age.push(a);
        spend.push(round_to(200.0 + 9.0 * a + 120.0 * noise.normal(), 0));
    }
    let mut satisfaction = Column::numeric("Satisfaction", sat);
    satisfaction.level = MeasurementLevel::Ordinal;
    table(
        "Customer survey (simulated)",
        vec![
            text("Region", region),
            text("Channel", channel),
            satisfaction,
            Column::numeric("Age", age),
            Column::numeric("Annual spend", spend),
        ],
    )
}

fn survey_sample() -> Sample {
    let t = survey();
    let reports = vec![
        bivariate(&t, "Region", "Channel"),
        SavedReport::Correspondence {
            request: crate::CorrespondenceRequest { row: col(&t, "Region"), column: col(&t, "Channel") },
        },
        distribution(&t, "Satisfaction", DistributionOptions::default()),
        bivariate(&t, "Age", "Annual spend"),
    ];
    Sample { table: t, reports }
}

/// **Simulated** bottle fill weights: 30 subgroups of 5 (150 rows), target
/// 500 g with σ = 1.5 g; from subgroup 21 the mean is 501.8 g, and one bottle
/// in subgroup 8 is overfilled by 7 g. The "Filler setting" column names the
/// two periods (A before the shift, B after).
#[must_use]
pub fn fill_weights() -> DataTable {
    let mut noise = Noise(0x0F11_0F11_0F11_0F11);
    let (mut subgroup, mut weight, mut setting) = (Vec::new(), Vec::new(), Vec::new());
    for g in 1..=30 {
        for b in 0..5 {
            let mut w = 500.0 + 1.5 * noise.normal();
            if g >= 21 {
                w += 1.8;
            }
            if g == 8 && b == 2 {
                w += 7.0;
            }
            subgroup.push(f64::from(g));
            weight.push(round_to(w, 2));
            setting.push(if g >= 21 { "B" } else { "A" }.to_owned());
        }
    }
    let mut sg = Column::numeric("Subgroup", subgroup);
    sg.level = MeasurementLevel::Ordinal;
    table(
        "Fill weights (simulated)",
        vec![sg, Column::numeric("Weight (g)", weight), Column::text("Filler setting", setting)],
    )
}

fn fill_weights_sample() -> Sample {
    let t = fill_weights();
    let reports = vec![
        SavedReport::ControlChart {
            request: ControlChartRequest {
                subgroup: Some(col(&t, "Subgroup")),
                ..ControlChartRequest::new(col(&t, "Weight (g)"), ChartKind::MeanAndRange, 5)
            },
            options: ControlChartOptions::default(),
        },
        SavedReport::ControlChart {
            request: ControlChartRequest {
                subgroup: Some(col(&t, "Subgroup")),
                phase: Some(col(&t, "Filler setting")),
                ..ControlChartRequest::new(col(&t, "Weight (g)"), ChartKind::MeanAndRange, 5)
            },
            options: ControlChartOptions::default(),
        },
        distribution(&t, "Weight (g)", DistributionOptions::default()),
        SavedReport::TimeWeighted {
            request: crate::TimeWeightedRequest {
                column: col(&t, "Weight (g)"),
                kind: crate::TimeWeightedKind::EWMA_DEFAULT,
                target: Some((500.0, 1.5)),
            },
        },
        SavedReport::TimeWeighted {
            request: crate::TimeWeightedRequest {
                column: col(&t, "Weight (g)"),
                kind: crate::TimeWeightedKind::CUSUM_DEFAULT,
                target: Some((500.0, 1.5)),
            },
        },
        SavedReport::Capability {
            request: crate::CapabilityRequest {
                column: col(&t, "Weight (g)"),
                limits: crate::SpecLimits { lower: Some(495.0), upper: Some(505.0), target: Some(500.0) },
                within: crate::WithinSigma::PooledStdDev { subgroup_size: 5 },
                box_cox: None,
                model: crate::CapabilityModel::Normal,
                summary: false,
                bootstrap: None,
                by: Some(col(&t, "Filler setting")),
                group_order: crate::GroupOrder::Labels,
                probability_plot: true,
            },
        },
    ];
    Sample { table: t, reports }
}

/// **Simulated** daily final inspection over 30 days. Lot sizes vary from 80
/// to 160 units; each unit is defective with probability 0.06 (0.15 on day
/// 18); surface flaws occur at 0.12 per unit inspected; and a fixed audit
/// panel shows Poisson(6) flaws a day.
#[must_use]
pub fn inspection() -> DataTable {
    let mut noise = Noise(0x1D5E_C7ED_0C0F_FEE5);
    let poisson = |rate: f64, noise: &mut Noise| {
        // Inversion; rates here are small enough for the direct product form.
        let (limit, mut k, mut prod) = ((-rate).exp(), 0u32, noise.uniform());
        while prod > limit {
            k += 1;
            prod *= noise.uniform();
        }
        f64::from(k)
    };
    let (mut day, mut lot, mut defective, mut flaws, mut panel) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for d in 1..=30 {
        let n = (80.0 + 80.0 * noise.uniform()).round();
        let p = if d == 18 { 0.15 } else { 0.06 };
        let bad = (0..n as usize).filter(|_| noise.uniform() < p).count();
        day.push(f64::from(d));
        lot.push(n);
        defective.push(bad as f64);
        flaws.push(poisson(0.12 * n, &mut noise));
        panel.push(poisson(6.0, &mut noise));
    }
    let mut day = Column::numeric("Day", day);
    day.level = MeasurementLevel::Ordinal;
    table(
        "Daily inspection (simulated)",
        vec![
            day,
            Column::numeric("Units inspected", lot),
            Column::numeric("Defective units", defective),
            Column::numeric("Surface flaws", flaws),
            Column::numeric("Audit panel flaws", panel),
        ],
    )
}

fn inspection_sample() -> Sample {
    let t = inspection();
    let size = crate::SubgroupSize::Column(col(&t, "Units inspected"));
    let chart = |kind, count: &str, size| SavedReport::Attribute {
        request: crate::AttributeChartRequest::new(kind, col(&t, count), size),
    };
    let reports = vec![
        chart(crate::AttributeChartKind::P, "Defective units", size),
        chart(crate::AttributeChartKind::U, "Surface flaws", size),
        chart(crate::AttributeChartKind::C, "Audit panel flaws", crate::SubgroupSize::None),
    ];
    Sample { reports, table: t }
}

/// **Simulated** solder defects: for each of two lines, defects traced to
/// 13 cause paths (category, cause, sub-cause) over a month, each count its
/// path's rate times a factor drawn uniformly from 0.85 to 1.15. Nozzle wear
/// dominates line A and the reflow profile line B.
#[must_use]
pub fn solder_defects() -> DataTable {
    // (category, cause, sub-cause, rate on line A, rate on line B)
    let paths: [(&str, &str, &str, f64, f64); 13] = [
        ("Machine", "Nozzle wear", "Clogged apertures", 30.0, 8.0),
        ("Machine", "Nozzle wear", "Worn tips", 14.0, 5.0),
        ("Machine", "Conveyor speed", "", 6.0, 4.0),
        ("Material", "Solder paste", "Expired paste", 9.0, 10.0),
        ("Material", "Solder paste", "Low viscosity", 7.0, 6.0),
        ("Material", "Board finish", "Oxidised pads", 5.0, 7.0),
        ("Method", "Reflow profile", "Peak too low", 8.0, 26.0),
        ("Method", "Reflow profile", "Ramp too fast", 4.0, 15.0),
        ("Method", "Stencil cleaning", "", 10.0, 9.0),
        ("Operator", "Paste handling", "Out of the fridge too long", 3.0, 2.0),
        ("Operator", "Setup errors", "Wrong program loaded", 2.0, 1.0),
        ("Environment", "Humidity", "", 2.0, 2.0),
        ("Measurement", "Inspection misses", "", 1.0, 2.0),
    ];
    let mut noise = Noise(0x50_1DE2_DEFE_C7A5);
    let (mut line, mut category, mut cause, mut sub, mut count) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (name, k) in [("A", 0), ("B", 1)] {
        for &(c, cs, sc, a, b) in &paths {
            let rate = if k == 0 { a } else { b };
            line.push(name.to_owned());
            category.push(c.to_owned());
            cause.push(cs.to_owned());
            sub.push(sc.to_owned());
            count.push((rate * (0.85 + 0.3 * noise.uniform())).round().max(1.0));
        }
    }
    table(
        "Solder defects (simulated)",
        vec![
            text("Line", line),
            text("Category", category),
            text("Cause", cause),
            text("Sub-cause", sub),
            Column::numeric("Defects", count),
        ],
    )
}

fn solder_defects_sample() -> Sample {
    let t = solder_defects();
    let reports = vec![
        SavedReport::Pareto {
            request: crate::ParetoRequest {
                category: col(&t, "Cause"),
                frequency: Some(col(&t, "Defects")),
                group: Some(col(&t, "Line")),
            },
            options: crate::ParetoChartOptions { other_percent: 4.0, percent_scale: false },
        },
        SavedReport::Pareto {
            request: crate::ParetoRequest {
                category: col(&t, "Category"),
                frequency: Some(col(&t, "Defects")),
                group: None,
            },
            options: crate::ParetoChartOptions::default(),
        },
        SavedReport::CauseEffect {
            request: crate::CauseEffectRequest {
                causes: vec![col(&t, "Category"), col(&t, "Cause"), col(&t, "Sub-cause")],
                effect: "Solder defects".to_owned(),
            },
        },
    ];
    Sample { table: t, reports }
}

/// **Simulated** shaft diameters (mm): three batches of five shafts, each
/// measured at its left end, centre and right end. Batches differ by up to
/// 0.023 mm, shafts within a batch by about 0.0015 mm (sd), every shaft tapers
/// by 0.008 mm from left to right, and gauge noise has sd 0.0015 mm.
#[must_use]
pub fn shaft_diameters() -> DataTable {
    let mut noise = Noise(0x5AF7_D1A3_E7E2_0001);
    let (mut batch, mut shaft, mut position, mut diameter) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (b, offset) in [("Monday", 0.0), ("Wednesday", 0.015), ("Friday", -0.008)] {
        for s in 1..=5 {
            let piece = 0.0015 * noise.normal();
            for (p, taper) in [("1 Left", 0.004), ("2 Center", 0.0), ("3 Right", -0.004)] {
                batch.push(b.to_owned());
                shaft.push(f64::from(s));
                position.push(p.to_owned());
                diameter.push(round_to(25.0 + offset + piece + taper + 0.0015 * noise.normal(), 4));
            }
        }
    }
    let mut shaft = Column::numeric("Shaft", shaft);
    shaft.level = MeasurementLevel::Ordinal;
    table(
        "Shaft diameters (simulated)",
        vec![text("Batch", batch), shaft, text("Position", position), Column::numeric("Diameter (mm)", diameter)],
    )
}

fn shaft_diameters_sample() -> Sample {
    let t = shaft_diameters();
    let chart = |factors: &[&str], options| SavedReport::MultiVari {
        request: crate::MultiVariRequest {
            response: col(&t, "Diameter (mm)"),
            factors: factors.iter().map(|f| col(&t, f)).collect(),
        },
        options,
    };
    let reports = vec![
        chart(&["Batch", "Shaft"], crate::MultiVariOptions::default()),
        chart(
            &["Batch", "Shaft", "Position"],
            crate::MultiVariOptions { show_values: false, show_ranges: false, show_cells: false, ..Default::default() },
        ),
        SavedReport::Graph {
            spec: Box::new(stat_viz::GraphSpec {
                encoding: stat_viz::EncodingPlan {
                    x: vec!["Batch".to_owned(), "Shaft".to_owned()],
                    y: vec!["Diameter (mm)".to_owned()],
                    ..Default::default()
                },
                form: stat_viz::GraphForm::MultiVari,
                ..stat_viz::GraphSpec::default()
            }),
        },
    ];
    Sample { table: t, reports }
}

/// Woods, Steinour & Starke (1932), as analysed by Hald (1952) and Draper &
/// Smith: heat evolved (cal/g) while 13 batches of cement hardened, with the
/// percentages of four clinker compounds.
#[must_use]
pub fn hald() -> DataTable {
    let x1 = [7.0, 1.0, 11.0, 11.0, 7.0, 11.0, 3.0, 1.0, 2.0, 21.0, 1.0, 11.0, 10.0];
    let x2 = [26.0, 29.0, 56.0, 31.0, 52.0, 55.0, 71.0, 31.0, 54.0, 47.0, 40.0, 66.0, 68.0];
    let x3 = [6.0, 15.0, 8.0, 8.0, 6.0, 9.0, 17.0, 22.0, 18.0, 4.0, 23.0, 9.0, 8.0];
    let x4 = [60.0, 52.0, 20.0, 47.0, 33.0, 22.0, 6.0, 44.0, 22.0, 26.0, 34.0, 12.0, 12.0];
    let heat = [78.5, 74.3, 104.3, 87.6, 95.9, 109.2, 102.7, 72.5, 93.1, 115.9, 83.8, 113.3, 109.4];
    table(
        "Hald cement (1932)",
        vec![
            Column::numeric("Tricalcium aluminate (%)", x1.to_vec()),
            Column::numeric("Tricalcium silicate (%)", x2.to_vec()),
            Column::numeric("Tetracalcium aluminoferrite (%)", x3.to_vec()),
            Column::numeric("Dicalcium silicate (%)", x4.to_vec()),
            Column::numeric("Heat (cal/g)", heat.to_vec()),
        ],
    )
}

fn hald_sample() -> Sample {
    let t = hald();
    let predictors = [
        "Tricalcium aluminate (%)",
        "Tricalcium silicate (%)",
        "Tetracalcium aluminoferrite (%)",
        "Dicalcium silicate (%)",
    ];
    let SavedReport::Linear { spec, .. } = linear(&t, "Heat (cal/g)", &predictors, ReportOptions::default()) else {
        unreachable!()
    };
    let reports = vec![
        SavedReport::Stepwise {
            request: crate::StepwiseRequest {
                candidates: spec.clone(),
                direction: crate::StepDirection::Mixed,
                criterion: crate::StepCriterion::PValue { enter: 0.15, remove: 0.15 },
                heredity: crate::Heredity::Strong,
            },
            options: crate::ProfilerOptions { show_profiler: true, show_surface_profiler: true, show_observed: false },
        },
        SavedReport::BestSubsets { request: crate::BestSubsetsRequest::new(spec.clone()) },
        linear(&t, "Heat (cal/g)", &predictors, ReportOptions { show_diagnostics: true, ..ReportOptions::default() }),
    ];
    Sample { reports, table: t }
}

/// NIST StRD Lew: 200 beam deflection readings in time order.
#[must_use]
pub fn lew() -> DataTable {
    let d = univariate_datasets().into_iter().find(|d| d.name == "Lew").expect("Lew is bundled");
    let n = d.values.len();
    table(
        "Beam deflection (NIST)",
        vec![Column::numeric("Reading", (1..=n).map(|i| i as f64).collect()), Column::numeric("Deflection", d.values)],
    )
}

fn lew_sample() -> Sample {
    let t = lew();
    let reports = vec![
        SavedReport::ControlChart {
            request: ControlChartRequest::new(col(&t, "Deflection"), ChartKind::IndividualsAndMovingRange, 1),
            options: ControlChartOptions::default(),
        },
        distribution(
            &t,
            "Deflection",
            DistributionOptions { runs: Some(crate::RunsSettings::default()), ..DistributionOptions::default() },
        ),
    ];
    Sample { table: t, reports }
}

fn gauge_sample() -> Sample {
    let t = gauge_study();
    let mut reports = vec![SavedReport::Gauge {
        request: GaugeStudyRequest {
            part: col(&t, "Part"),
            operator: col(&t, "Operator"),
            measurement: col(&t, "Measurement"),
            tolerance: Some(0.6),
            pool_interaction: false,
            study_multiplier: 6.0,
            design: crate::gauge_design::GaugeStudyDesign::default(),
        },
        options: GaugeReportOptions { show_operator_plot: true, ..GaugeReportOptions::default() },
    }];
    reports.push(SavedReport::IntraclassCorrelation {
        request: crate::IccRequest::new(col(&t, "Part"), col(&t, "Operator"), col(&t, "Measurement")),
    });
    Sample { table: t, reports }
}

/// Longley's (1967) U.S. macroeconomic series, 1947–1962, from the NIST StRD
/// (public domain), with descriptive column names.
///
/// # Panics
/// Never in practice: the bundled dataset has a fixed, valid shape.
#[must_use]
pub fn longley() -> DataTable {
    let d = linear_regression_datasets()
        .into_iter()
        .find(|d| d.name == "Longley")
        .expect("Longley is bundled with stat-strd");
    let names = ["GNP deflator", "GNP", "Unemployed", "Armed forces", "Population 14+", "Year"];
    let mut columns = vec![Column::numeric("Employed", d.response.clone())];
    columns.extend(names.iter().enumerate().map(|(j, n)| Column::numeric(*n, d.predictors.col(j).to_vec())));
    DataTable::new("Longley (1947–1962)", columns).expect("columns have equal length")
}

/// A **simulated** crossed gauge study: 10 parts, 3 operators (A, B, C), each
/// part measured 3 times by each operator (90 rows), recorded to 0.001.
///
/// Generated deterministically from a known model — part values spread with
/// σ ≈ 0.19, operator biases of 0, +0.03 and −0.02, a small part × operator
/// interaction, and measurement error with σ ≈ 0.025 — so the analysis has a
/// known answer. It is not real measurement data.
///
/// # Panics
/// Never in practice: the generated columns have equal length.
#[must_use]
pub fn gauge_study() -> DataTable {
    const PART_VALUES: [f64; 10] = [2.10, 2.32, 1.95, 2.48, 2.21, 2.05, 2.37, 1.88, 2.27, 2.16];
    const OPERATOR_BIAS: [f64; 3] = [0.0, 0.03, -0.02];
    const OPERATORS: [&str; 3] = ["A", "B", "C"];
    // xorshift64 for reproducible noise; Irwin–Hall sum of 12 uniforms ≈ N(0, 1).
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut normal = || {
        let mut sum = 0.0;
        for _ in 0..12 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            sum += (state >> 11) as f64 / (1u64 << 53) as f64;
        }
        sum - 6.0
    };
    let (mut part, mut operator, mut replicate, mut value) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    // Recorded in the usual study order: each operator measures every part, then repeats.
    for (o, (&name, &bias)) in OPERATORS.iter().zip(&OPERATOR_BIAS).enumerate() {
        for rep in 1..=3 {
            for (p, &true_value) in PART_VALUES.iter().enumerate() {
                let interaction = 0.008 * (((p * 7 + o * 3) % 5) as f64 - 2.0);
                let y = true_value + bias + interaction + 0.025 * normal();
                part.push(format!("P{:02}", p + 1));
                operator.push(name.to_owned());
                replicate.push(f64::from(rep));
                value.push((y * 1000.0).round() / 1000.0);
            }
        }
    }
    DataTable::new(
        "Gauge study (simulated)",
        vec![
            Column::text("Part", part),
            Column::text("Operator", operator),
            Column::numeric("Replicate", replicate),
            Column::numeric("Measurement", value),
        ],
    )
    .expect("columns have equal length")
}

#[cfg(test)]
mod tests {
    use crate::{
        BivariateAnalysis, ControlChartAnalysis, DistributionAnalysis, GaugeAnalysis, LinearModelAnalysis, SavedReport,
    };
    use stat_data::RowStateManager;

    /// Every sample builds, is deterministic, and every report it opens with
    /// runs without error.
    #[test]
    fn every_sample_and_its_reports_work() {
        let catalog = super::catalog();
        assert_eq!(catalog.len(), 135);
        for info in catalog {
            let s = (info.build)();
            assert_eq!((info.build)().table, s.table, "{} is deterministic", info.name);
            assert!(s.table.n_rows() > 0 && !s.reports.is_empty(), "{}", info.name);
            let rows = RowStateManager::new(s.table.n_rows());
            for r in &s.reports {
                let ok = match r {
                    SavedReport::Pareto { .. }
                    | SavedReport::CauseEffect { .. }
                    | SavedReport::MultiVari { .. }
                    | SavedReport::Scoped { .. } => crate::run_report(r, &s.table, &rows).map(|_| ()),
                    SavedReport::Bootstrap { .. } | SavedReport::PermutationTest { .. } => {
                        crate::run_report(r, &s.table, &rows).map(|_| ())
                    }
                    SavedReport::GaussianMixture { .. }
                    | SavedReport::Embedding { .. }
                    | SavedReport::LatentClass { .. }
                    | SavedReport::TextExploration { .. }
                    | SavedReport::TopicModel { .. }
                    | SavedReport::Sentiment { .. } => crate::run_report(r, &s.table, &rows).map(|_| ()),
                    SavedReport::Linear { spec, .. } => LinearModelAnalysis::run(&s.table, &rows, spec).map(|_| ()),
                    SavedReport::ControlChart { request, .. } => {
                        ControlChartAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Gauge { request, .. } => GaugeAnalysis::run(&s.table, &rows, request).map(|_| ()),
                    SavedReport::Distribution { request, .. } => {
                        DistributionAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Bivariate { request } => BivariateAnalysis::run(&s.table, &rows, request).map(|_| ()),
                    SavedReport::Pca { request } => crate::PcaAnalysis::run(&s.table, &rows, request).map(|_| ()),
                    SavedReport::Correlations { request } => {
                        crate::CorrelationAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::OutlierDistances { request } => {
                        crate::OutlierAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::ItemAnalysis { request } => {
                        crate::ItemAnalysisReport::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Manova { request } => crate::ManovaAnalysis::run(&s.table, &rows, request).map(|_| ()),
                    SavedReport::FactorAnalysis { request } => {
                        crate::FactorAnalysisReport::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::DiscriminantAnalysis { request } => {
                        crate::DiscriminantAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Imputation { request } => {
                        crate::ImputationReport::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::SeriesExplore { request } => {
                        crate::SeriesExploreAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::SignalProcessing { request } => {
                        crate::SignalAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::ImageAnalysis { request } => {
                        crate::ImageAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Correspondence { request } => {
                        crate::CorrespondenceReport::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Cluster { request } => {
                        crate::ClusterAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Glm { request, .. } => crate::GlmAnalysis::run(&s.table, &rows, request).map(|_| ()),
                    SavedReport::Regularized { request, .. } => {
                        crate::RegularizedAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::DecisionTree { request, .. } => {
                        crate::TreeAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::RandomForest { request, .. } => {
                        crate::ForestAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::BoostedTrees { request, .. } => {
                        crate::BoostAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::PartialLeastSquares { request, .. } => {
                        crate::PlsAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::SupportVectorMachine { request, .. } => {
                        crate::SvmAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Mars { request, .. } => crate::MarsAnalysis::run(&s.table, &rows, request).map(|_| ()),
                    SavedReport::NeuralNetwork { request, .. } => {
                        crate::NeuralAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::GaussianProcess { request, .. } => {
                        crate::GaussianProcessAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::SequentialOptimization { request } => {
                        crate::SequentialAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::ModelSearch { request } => {
                        crate::ModelSearch::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::FunctionalData { request, .. } => {
                        crate::FunctionalAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::ModelComparison { request } => {
                        crate::ModelComparison::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::MultipleResponses { request } => {
                        crate::MultipleResponsesAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Mixed { request, .. } => {
                        crate::MixedAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Survival { request, .. } => {
                        crate::SurvivalAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Surface { request, .. } => {
                        crate::SurfaceAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::TimeWeighted { request } => {
                        crate::TimeWeightedAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Capability { request } => {
                        crate::CapabilityAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Attribute { request } => {
                        crate::AttributeChartAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::MultivariateChart { request } => {
                        crate::MultivariateChartAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Anom { request } => crate::AnomAnalysis::run(&s.table, &rows, request).map(|_| ()),
                    SavedReport::DoseResponse { request } => {
                        crate::DoseResponseAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::CategoricalLogistic { request } => {
                        crate::CategoricalLogisticAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Nonlinear { request, .. } => {
                        crate::NonlinearAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::MethodComparison { request } => {
                        crate::MethodComparisonAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::ShelfLife { request } => {
                        crate::ShelfLifeAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Growth { request } => crate::GrowthAnalysis::run(&s.table, &rows, request).map(|_| ()),
                    SavedReport::Recurrence { request } => {
                        crate::RecurrenceAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::LifeDistribution { request } => {
                        crate::LifeDistributionAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::LifeStress { request } => {
                        crate::LifeStressAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Degradation { request } => {
                        crate::DegradationAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::ProportionalHazards { request, .. } => {
                        crate::ProportionalHazardsAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::SurvivalRegression { request, .. } => {
                        crate::SurvivalRegressionAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::TestPlan { request } => crate::TestPlanAnalysis::run(request).map(|_| ()),
                    SavedReport::CompetingRisks { .. }
                    | SavedReport::Warranty { .. }
                    | SavedReport::ExponentialSmoothing { .. }
                    | SavedReport::Arima { .. }
                    | SavedReport::PrincipalCoordinates { .. }
                    | SavedReport::Pharmacokinetics { .. }
                    | SavedReport::Fatigue { .. } => crate::run_report(r, &s.table, &rows).map(|_| ()),
                    SavedReport::Stepwise { request, .. } => {
                        crate::StepwiseAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::BestSubsets { request } => {
                        crate::BestSubsetsAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::DistributionFit { request } => {
                        crate::DistributionFitAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Graph { spec } => spec.frame(&s.table, &rows).map(|_| ()),
                    SavedReport::Scatter3d { .. } | SavedReport::DataSurface { .. } => {
                        crate::run_report(r, &s.table, &rows).map(|_| ())
                    }
                    SavedReport::Mixture { request, .. } => {
                        crate::MixtureAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::OneSample { request } => {
                        crate::OneSampleAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Groups { request } => {
                        crate::GroupComparison::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Paired { request } => {
                        crate::PairedComparison::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Proportions { request } => {
                        crate::ProportionAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Rates { request } => crate::RateAnalysis::run(&s.table, &rows, request).map(|_| ()),
                    SavedReport::Power { request } => crate::PowerAnalysis::run(request).map(|_| ()),
                    SavedReport::AttributeCapability { request } => {
                        crate::AttributeCapabilityAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::BetweenWithinCapability { request } => {
                        crate::BetweenWithinAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::AcceptanceSampling { request } => crate::AcceptanceAnalysis::run(request).map(|_| ()),
                    SavedReport::GaugeLinearity { .. }
                    | SavedReport::GaugeRepeatability { .. }
                    | SavedReport::IntraclassCorrelation { .. }
                    | SavedReport::AttributeAgreement { .. }
                    | SavedReport::AttributeGauge { .. }
                    | SavedReport::PcaChart { .. }
                    | SavedReport::MonitoringLimits { .. }
                    | SavedReport::MultiwayTable { .. }
                    | SavedReport::StratifiedTables { .. }
                    | SavedReport::SignalToNoise { .. }
                    | SavedReport::DeclaredDesign { .. }
                    | SavedReport::DesignEvaluation { .. }
                    | SavedReport::FactorialAnalysis { .. }
                    | SavedReport::AdjustPValues { .. }
                    | SavedReport::AreaUnderCurve { .. }
                    | SavedReport::ManyTests { .. }
                    | SavedReport::TreatmentEffects { .. }
                    | SavedReport::Screening { .. }
                    | SavedReport::FactorialRepeated { .. }
                    | SavedReport::Nested { .. }
                    | SavedReport::Bayesian { .. }
                    | SavedReport::MultipleResponseItems { .. }
                    | SavedReport::Signal { .. }
                    | SavedReport::ImagePlot { .. } => crate::run_report(r, &s.table, &rows).map(|_| ()),
                    SavedReport::GoodnessOfFit { request } => {
                        crate::GoodnessAnalysis::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::Crossover { request } => {
                        crate::CrossoverReport::run(&s.table, &rows, request).map(|_| ())
                    }
                    SavedReport::DistributionCalculator { request } => {
                        crate::CalculatorReport::run(request).map(|_| ())
                    }
                };
                assert!(ok.is_ok(), "{}: {r:?}: {ok:?}", info.name);
            }
        }
    }

    /// The comparison samples show what their descriptions say.
    /// Two reports of one sample with the same title could not be told apart
    /// in the report list (two X̄ and R charts, two multi-vari charts, two
    /// support vector machines).
    #[test]
    fn sample_report_titles_are_distinct() {
        for info in super::catalog() {
            let s = (info.build)();
            let rows = RowStateManager::new(s.table.n_rows());
            let mut titles: Vec<String> =
                s.reports.iter().filter_map(|r| crate::run_report(r, &s.table, &rows).ok()).map(|c| c.title).collect();
            titles.sort();
            let n = titles.len();
            titles.dedup();
            assert_eq!(titles.len(), n, "{}: {titles:?}", info.name);
        }
    }

    #[test]
    fn crossover_counts_and_best_samples() {
        let s = super::bioequivalence_sample();
        let SavedReport::Crossover { request } = &s.reports[0] else { panic!() };
        let rows = RowStateManager::new(s.table.n_rows());
        let a = crate::CrossoverReport::run(&s.table, &rows, request).unwrap();
        assert_eq!(a.analysis.sizes, (12, 12));
        assert!(a.analysis.equivalence.equivalent(), "the simulated ratio (e^−0.05) is inside 0.80–1.25");
        let s = super::horse_kicks_sample();
        let SavedReport::GoodnessOfFit { request } = &s.reports[0] else { panic!() };
        let g = crate::GoodnessAnalysis::run(&s.table, &RowStateManager::new(5), request).unwrap();
        assert!((g.fit.mean - 0.61).abs() < 1e-12 && g.fit.pearson_p > 0.5);
        let s = super::drying_times_sample();
        let SavedReport::Groups { request } = &s.reports[0] else { panic!() };
        let c = crate::GroupComparison::run(&s.table, &RowStateManager::new(30), request).unwrap();
        let best = c.sections().into_iter().find(|x| x.kind == crate::SectionKind::BestComparisons).unwrap();
        let crate::SectionContent::Table(t) = best.content else { panic!() };
        let conclusion = |paint: &str| t.rows.iter().find(|r| r[0].ends_with(paint)).unwrap()[7].clone();
        assert_eq!(conclusion(" D"), "not the best");
        assert_ne!(conclusion(" A"), "not the best");
    }

    /// The files in `examples/data` load and match the samples they were written from.
    #[test]
    fn example_files_match_the_samples() {
        use crate::file_formats::{FileKind, Opened, read_text_file};
        let read = |kind, text| match read_text_file(kind, "x", text).unwrap() {
            Opened::Table(t, _) => t,
            Opened::Project(p) => p.table,
        };
        // Column levels survive in the data-only projects: no type changes needed.
        let plant = read(FileKind::Project, include_str!("../../../examples/data/plant_growth.acraft"));
        assert_eq!(plant, super::plant_growth());
        assert_eq!(plant.column(2).unwrap().level, stat_data::MeasurementLevel::Nominal);
        let survey = read(FileKind::Project, include_str!("../../../examples/data/customer_survey.acraft"));
        assert_eq!(survey, super::survey());
        assert_eq!(survey.column(2).unwrap().level, stat_data::MeasurementLevel::Ordinal);
        // Plain data files need no level changes either: their columns infer correctly.
        let wait = read(FileKind::Tsv, include_str!("../../../examples/data/clinic_wait_times.tsv"));
        assert_eq!(wait.columns(), super::wait_times().columns());
        let catalyst = read(FileKind::Json, include_str!("../../../examples/data/catalyst_experiment.json"));
        assert_eq!(catalyst.columns(), super::catalyst().columns());
        let michelson = read(FileKind::Csv, include_str!("../../../examples/data/michelson_speed_of_light.csv"));
        assert_eq!(michelson.columns(), super::michelson().columns());
        let fill = read(FileKind::DelimitedText, include_str!("../../../examples/data/fill_weights_semicolon.txt"));
        assert_eq!(fill.column(1).unwrap().data, super::fill_weights().column(1).unwrap().data);
        let project =
            crate::project::load_project(include_str!("../../../examples/data/plant_growth_outliers_excluded.acraft"))
                .unwrap();
        assert_eq!(project.reports.len(), 2);
        assert!(project.rows.is_excluded(16) && project.rows.is_excluded(43));
        assert_eq!(project.table, super::plant_growth(), "levels kept, including the nominal greenhouse");
        let sleep = read(FileKind::Csv, include_str!("../../../examples/data/sleep_drug_trial.csv"));
        assert_eq!(sleep.columns(), super::sleep().columns());
        let bearings = read(FileKind::Csv, include_str!("../../../examples/data/bearing_life_test.csv"));
        assert_eq!(bearings.columns(), super::bearings().columns());
        let leukemia =
            crate::project::load_project(include_str!("../../../examples/data/leukemia_remission.acraft")).unwrap();
        assert_eq!(leukemia.table, super::leukemia());
        assert!(matches!(leukemia.reports[..], [SavedReport::Survival { .. }]));
    }

    /// The Phase 2 samples recover what they were simulated from.
    #[test]
    fn phase2_samples_recover_their_structure() {
        // k-means on the flowers finds the three varieties (allowing a few strays).
        let f = super::flowers();
        let s = (super::catalog()[11].build)();
        let SavedReport::Cluster { request } = &s.reports[1] else { panic!() };
        let a = crate::ClusterAnalysis::run(&f, &RowStateManager::new(90), request).unwrap();
        let clusters = a.cluster_column();
        let mut agree = 0;
        for c in 1..=3 {
            // Majority variety of each cluster.
            let mut counts = std::collections::HashMap::new();
            for r in 0..90 {
                if clusters[r] == c as f64 {
                    *counts.entry(f.column(0).unwrap().cell_text(r)).or_insert(0) += 1;
                }
            }
            agree += counts.values().max().copied().unwrap_or(0);
        }
        assert!(agree >= 80, "{agree} of 90 flowers in their variety's cluster");
        // The churn logistic model finds the simulated effects' signs.
        let s = (super::catalog()[12].build)();
        let SavedReport::Glm { request, .. } = &s.reports[0] else { panic!() };
        let g = crate::GlmAnalysis::run(&s.table, &RowStateManager::new(400), request).unwrap();
        let c = &g.fit().coefficients;
        assert!(c[1].estimate < 0.0 && c[2].estimate > 0.0 && c[3].estimate > 0.0 && c[4].estimate > 0.0, "{c:?}");
    }

    /// The simulated data show the features they were built for.
    #[test]
    fn simulated_data_have_their_intended_features() {
        use crate::{ReportOptions, SectionContent, SectionKind};
        // Plant growth: the two planted outliers are flagged.
        let s = (super::catalog()[2].build)();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::Linear { spec, .. } = &s.reports[0] else { panic!() };
        let a = LinearModelAnalysis::run(&s.table, &rows, spec).unwrap();
        assert_eq!(a.fit().parameters().len(), 1 + 2 + 1 + 3, "intercept, 2 fertilizer, light, 3 greenhouse");
        let sections = a.sections(&ReportOptions { show_diagnostics: true, ..ReportOptions::default() });
        let infl = sections.iter().find(|x| x.kind == SectionKind::InfluenceDiagnostics).unwrap();
        let SectionContent::Table(t) = &infl.content else { panic!("outliers should be flagged") };
        let flagged: Vec<&str> = t.rows.iter().map(|r| r[0].as_str()).collect();
        assert!(flagged.contains(&"17") && flagged.contains(&"44"), "{flagged:?}");
        // Wait times: clearly non-normal, with missing values.
        let w = super::wait_times();
        let col = w.column(2).unwrap().as_numeric().unwrap();
        assert_eq!(col.iter().filter(|v| v.is_nan()).count(), 6);
        let values: Vec<f64> = col.iter().copied().filter(|v| v.is_finite()).collect();
        assert!(stat_core::normality::shapiro_wilk(&values).unwrap().p_value < 0.001);
        // Fill weights: the shift is detected by the run rules.
        let f = (super::catalog()[8].build)();
        let SavedReport::ControlChart { request, .. } = &f.reports[0] else { panic!() };
        let c = ControlChartAnalysis::run(&f.table, &RowStateManager::new(150), request).unwrap();
        let options = crate::ControlChartOptions::default();
        assert!(!c.location_violations(&options).is_empty());
        // With a phase per filler setting the shift no longer flags subgroups 21 on.
        let SavedReport::ControlChart { request, .. } = &f.reports[1] else { panic!() };
        let c = ControlChartAnalysis::run(&f.table, &RowStateManager::new(150), request).unwrap();
        assert_eq!(c.phase_names(), ["A", "B"]);
        assert!(c.location_violations(&options).iter().all(|v| v.index < 20));
    }

    /// The survival samples: the published leukemia medians, and the bearing
    /// simulation's Weibull parameters recovered within sampling error.
    /// The samples added to demonstrate quality, selection and model-term
    /// features show the behaviour their menu names promise.
    #[test]
    fn demonstration_samples_show_their_features() {
        let find = |prefix: &str| {
            let info = super::catalog().into_iter().find(|i| i.name.starts_with(prefix)).expect("sample listed");
            assert!(info.name.contains(" — "), "{} names what it demonstrates", info.name);
            (info.build)()
        };
        // Multivariate charts: T² flags subgroups 23 (volume contributes most)
        // and 25,
        // |S| flags subgroup 25, and the MEWMA first signals among the
        // monitored bottles in subgroup 23.
        let s = find("Bottle filling");
        let rows = RowStateManager::new(s.table.n_rows());
        let run = |k: usize| {
            let SavedReport::MultivariateChart { request } = &s.reports[k] else { panic!("multivariate charts") };
            crate::MultivariateChartAnalysis::run(&s.table, &rows, request).unwrap()
        };
        let t2 = run(0);
        assert_eq!(t2.signals(), [22, 24]);
        let sections = t2.sections();
        let contributions = sections.iter().find(|x| x.kind == crate::SectionKind::MultivariateContributions).unwrap();
        let crate::SectionContent::Table(c) = &contributions.content else { panic!() };
        assert!(c.rows[0][2].ends_with('▲'), "volume drives the signal: {:?}", c.rows[0]);
        assert_eq!(run(1).signals(), [24]);
        let mewma = run(2).signals();
        let first_monitored = mewma.iter().find(|&&i| i >= 80);
        assert!(first_monitored.is_some_and(|i| (88..92).contains(i)), "{mewma:?}");
        // Analysis of means: furnaces 3 (hard) and 4 (soft), an interaction
        // cell, furnaces 1 (few cracks) and 2 (many), and night-shift pits.
        let s = find("Heat treatment");
        let rows = RowStateManager::new(s.table.n_rows());
        let anom: Vec<_> = s
            .reports
            .iter()
            .map(|r| {
                let SavedReport::Anom { request } = r else { panic!("analysis of means only") };
                crate::AnomAnalysis::run(&s.table, &rows, request).unwrap()
            })
            .collect();
        assert_eq!(anom[0].outside()[0], ["Furnace 3", "Furnace 4"]);
        assert_eq!(anom[1].outside()[2], ["Furnace 2 × Night"]);
        assert_eq!(anom[2].outside()[0], ["Furnace 1", "Furnace 2"]);
        assert_eq!(anom[3].outside()[0], ["Night"]);
        // Attribute charts: day 18 (row 17) is out of control on the p chart only.
        let s = find("Daily inspection");
        let rows = RowStateManager::new(s.table.n_rows());
        let charts: Vec<_> = s
            .reports
            .iter()
            .map(|r| {
                let SavedReport::Attribute { request } = r else { panic!("attribute charts only") };
                crate::AttributeChartAnalysis::run(&s.table, &rows, request).unwrap()
            })
            .collect();
        assert_eq!(charts.len(), 3);
        assert_eq!(charts[0].signal_rows(), [17]);
        // Coating batches: batch-to-batch variation lowers the index below the within-batch one.
        let s = find("Coating batches");
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::BetweenWithinCapability { request } = &s.reports[0] else { panic!() };
        let a = crate::BetweenWithinAnalysis::run(&s.table, &rows, request).unwrap();
        let r = a.result();
        assert_eq!(r.subgroups, 25);
        assert!(r.sigma_between > 0.4 && r.sigma_between < 1.2, "σ_between {}", r.sigma_between);
        assert!((r.sigma_within - 0.5).abs() < 0.1, "σ_within {}", r.sigma_within);
        assert!(r.between_within.cpk < 0.8 * r.within.cpk);
        // Supplier lots: the exact interval covers the simulated 2% and 0.05 per unit;
        // the designed plan is the smallest single plan meeting 1%/5% and 6%/10%.
        let s = find("Supplier lots");
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::AttributeCapability { request } = &s.reports[0] else { panic!() };
        let b = crate::AttributeCapabilityAnalysis::run(&s.table, &rows, request).unwrap();
        let (lo, hi) = b.binomial().unwrap().interval;
        assert!(lo < 0.02 && 0.02 < hi, "{lo}–{hi}");
        assert!(b.signal_rows().is_empty(), "a stable process");
        let SavedReport::AttributeCapability { request } = &s.reports[1] else { panic!() };
        let p = crate::AttributeCapabilityAnalysis::run(&s.table, &rows, request).unwrap();
        let (lo, hi) = p.poisson().unwrap().interval;
        assert!(lo < 0.05 && 0.05 < hi, "{lo}–{hi}");
        let SavedReport::AcceptanceSampling { request } = &s.reports[2] else { panic!() };
        let a = crate::AcceptanceAnalysis::run(request).unwrap();
        assert_eq!(a.designed(), Some(&crate::SamplingPlan::Single { n: 110, c: 3 }));
        assert_eq!(a.plans.len(), 3);
        // Stepwise on Hald: two ingredients are selected from four.
        let s = find("Hald cement");
        let rows = RowStateManager::new(13);
        let SavedReport::Stepwise { request, .. } = &s.reports[0] else { panic!() };
        let a = crate::StepwiseAnalysis::run(&s.table, &rows, request).unwrap();
        assert_eq!(a.selected().unwrap().design().effects().len(), 2);
        // Best subsets on Hald: by BIC the best model is x1 + x2.
        let SavedReport::BestSubsets { request } = &s.reports[1] else { panic!() };
        let a = crate::BestSubsetsAnalysis::run(&s.table, &rows, request).unwrap();
        assert_eq!(a.model_label(a.best_index().unwrap()), "Tricalcium aluminate (%) + Tricalcium silicate (%)");
        // Rosner: the generalized ESD finds the three largest values.
        let s = find("Rosner");
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::Distribution { request, options } = &s.reports[0] else { panic!() };
        let a = crate::DistributionAnalysis::run(&s.table, &rows, request).unwrap();
        let mut flagged = a.flagged_rows(options);
        flagged.sort_unstable();
        assert_eq!(flagged, [51, 52, 53]);
        assert!(a.sections(options).iter().any(|x| x.kind == crate::SectionKind::OutlierSteps));
        // Blood counts: drug B differs from the control (Dunnett p ≈ 0.0059), drug A does not.
        let s = find("Blood counts");
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::Groups { request } = &s.reports[0] else { panic!() };
        let sections = crate::GroupComparison::run(&s.table, &rows, request).unwrap().sections();
        let control = sections.iter().find(|x| x.kind == crate::SectionKind::ControlComparisons).unwrap();
        let crate::SectionContent::Table(t) = &control.content else { panic!() };
        assert_eq!(t.rows.len(), 2);
        assert_eq!((t.rows[0][0].as_str(), t.rows[0][7].as_str()), ("Drug A", "no"));
        assert!(t.rows[1][7].starts_with("yes"));
        // Catalyst: the interaction and squared term are in the model.
        let s = find("Catalyst factorial");
        let SavedReport::Linear { spec, .. } = &s.reports[0] else { panic!() };
        let rows = RowStateManager::new(s.table.n_rows());
        let a = LinearModelAnalysis::run(&s.table, &rows, spec).unwrap();
        let names: Vec<_> = a.design().effects().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["Temperature", "Pressure", "Catalyst", "Temperature × Pressure", "Temperature²"]);
        // Fill weights: EWMA and CUSUM both detect the shift after subgroup 20 (row 100 on).
        let s = find("Fill weights");
        let rows = RowStateManager::new(s.table.n_rows());
        let mut time_weighted = 0;
        for r in &s.reports {
            if let SavedReport::TimeWeighted { request } = r {
                let signals = crate::TimeWeightedAnalysis::run(&s.table, &rows, request).unwrap().signal_rows();
                assert!(signals.iter().any(|&i| i >= 100), "{request:?}");
                time_weighted += 1;
            }
        }
        assert_eq!(time_weighted, 2);
        assert!(s.reports.iter().any(|r| matches!(r, SavedReport::Capability { .. })));
    }

    #[test]
    fn survival_samples() {
        let s = (super::catalog()[13].build)();
        let SavedReport::Survival { request, .. } = &s.reports[0] else { panic!() };
        let a = crate::SurvivalAnalysis::run(&s.table, &RowStateManager::new(42), request).unwrap();
        let medians: Vec<_> = a.groups().iter().map(|g| g.km.median()).collect();
        assert_eq!(medians, [Some(23.0), Some(8.0)]);
        let s = (super::catalog()[14].build)();
        let SavedReport::Survival { request, .. } = &s.reports[0] else { panic!() };
        let a = crate::SurvivalAnalysis::run(&s.table, &RowStateManager::new(60), request).unwrap();
        let g = a.groups();
        let censored: usize = g.iter().map(|x| x.km.n_censored()).sum();
        assert!((8..=40).contains(&censored), "{censored} suspended units");
        let (wa, wb) = (g[0].weibull.as_ref().unwrap(), g[1].weibull.as_ref().unwrap());
        for w in [wa, wb] {
            assert!((1.3..3.6).contains(&w.weibull_shape()), "shape {}", w.weibull_shape());
        }
        assert!(wb.weibull_scale() > wa.weibull_scale());
        assert!((1900.0..3400.0).contains(&wa.weibull_scale()), "{}", wa.weibull_scale());
    }

    #[test]
    fn longley_shape() {
        let t = super::longley();
        assert_eq!((t.n_rows(), t.n_cols()), (16, 7));
        assert_eq!(t.column(6).unwrap().as_numeric().unwrap()[0], 1947.0);
        assert_eq!(t.column(0).unwrap().as_numeric().unwrap()[0], 60323.0);
    }

    #[test]
    fn gauge_study_shape_and_determinism() {
        let t = super::gauge_study();
        assert_eq!((t.n_rows(), t.n_cols()), (90, 4));
        assert_eq!(t.column(0).unwrap().cell_text(0), "P01");
        assert_eq!(t.column(1).unwrap().cell_text(89), "C");
        assert_eq!(super::gauge_study(), t, "generated identically every time");
    }

    /// The loan trees find the rules the data were simulated from.
    #[test]
    fn loan_trees_find_the_simulated_rules() {
        let s = super::loans_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::DecisionTree { request, .. } = &s.reports[0] else { panic!() };
        let a = crate::TreeAnalysis::run(&s.table, &rows, request).unwrap();
        let used: Vec<usize> = a.tree().nodes.iter().filter_map(|n| n.split.as_ref().map(|s| s.feature)).collect();
        assert!(used.contains(&0) && used.contains(&4), "history and debt ratio set the outcome: {used:?}");
        assert!(a.leaves().len() >= 4, "{} leaves", a.leaves().len());
        let SavedReport::DecisionTree { request, .. } = &s.reports[4] else { panic!() };
        let a = crate::TreeAnalysis::run(&s.table, &rows, request).unwrap();
        let used: Vec<usize> = a.tree().nodes.iter().filter_map(|n| n.split.as_ref().map(|s| s.feature)).collect();
        assert!(used.contains(&0) && used.contains(&4), "history and debt ratio set the rate: {used:?}");
    }

    /// The loan sample's forest, boosted trees and model comparison run on the
    /// fixed roles.
    #[test]
    fn loan_forest_and_comparison() {
        let s = super::loans_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        assert!(s.table.find("Prob Default (forest)").is_some());
        let SavedReport::RandomForest { request, .. } = &s.reports[1] else { panic!() };
        let a = crate::ForestAnalysis::run(&s.table, &rows, request).unwrap();
        let importance = a.permutation_importance();
        let best = (0..importance.len()).max_by(|&i, &j| importance[i].0.total_cmp(&importance[j].0)).unwrap();
        assert_eq!(best, 0, "credit history matters most: {importance:?}");
        let SavedReport::BoostedTrees { request, .. } = &s.reports[2] else { panic!() };
        let b = crate::BoostAnalysis::run(&s.table, &rows, request).unwrap();
        assert!(b.model().stopped_early, "validation rows stop boosting");
        let SavedReport::ModelComparison { request } = &s.reports[3] else { panic!() };
        assert_eq!(request.models.len(), 3);
        let c = crate::ModelComparison::run(&s.table, &rows, request).unwrap();
        assert_eq!(c.event(), Some("Default"));
        let kinds: Vec<_> = c.sections().iter().map(|k| k.kind).collect();
        assert!(kinds.contains(&crate::SectionKind::RocCurves) && kinds.contains(&crate::SectionKind::CalibrationPlot));
    }

    /// Partial least squares on the fuel spectra recovers the three
    /// constituents and the baseline in a few factors and predicts octane
    /// closely.
    #[test]
    fn fuel_spectra_pls() {
        let s = super::fuel_spectra_sample();
        assert_eq!((s.table.n_rows(), s.table.n_cols()), (60, 42));
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::PartialLeastSquares { request, .. } = &s.reports[0] else { panic!() };
        let a = crate::PlsAnalysis::run(&s.table, &rows, request).unwrap();
        assert!((3..=6).contains(&a.factors()), "{} factors", a.factors());
        let octane = s.table.column(1).unwrap().as_numeric().unwrap();
        let mse = (0..60)
            .map(|r| {
                let x: Vec<f64> = (2..42).map(|c| s.table.column(c).unwrap().as_numeric().unwrap()[r]).collect();
                (a.fit().predict(a.factors(), &x)[0] - octane[r]).powi(2)
            })
            .sum::<f64>()
            / 60.0;
        assert!(mse.sqrt() < 0.3, "rmse {}", mse.sqrt());
    }

    /// PLS-DA separates the simulated flower varieties.
    #[test]
    fn flowers_pls_da() {
        let s = super::flowers_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let Some(SavedReport::PartialLeastSquares { request, .. }) = s.reports.last() else { panic!() };
        let a = crate::PlsAnalysis::run(&s.table, &rows, request).unwrap();
        let variety = s.table.column(super::col(&s.table, "Variety")).unwrap();
        let classes: Vec<String> = {
            let mut v: Vec<String> = (0..90).map(|r| variety.cell_text(r)).collect();
            v.sort();
            v.dedup();
            v
        };
        let wrong = (0..90)
            .filter(|&r| {
                let x: Vec<f64> = ["Sepal length", "Sepal width", "Petal length", "Petal width"]
                    .iter()
                    .map(|c| s.table.column(super::col(&s.table, c)).unwrap().as_numeric().unwrap()[r])
                    .collect();
                let p = a.fit().predict(a.factors(), &x);
                let best = (0..p.len()).max_by(|&i, &j| p[i].total_cmp(&p[j])).unwrap();
                classes[best] != variety.cell_text(r)
            })
            .count();
        assert!(wrong <= 18, "{wrong} of 90 misclassified");
    }

    /// The radial kernel finds the curved fault boundary that a line cannot,
    /// and the regression follows the curved efficiency.
    #[test]
    fn pump_svms() {
        let s = super::pumps_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        assert_eq!(s.reports.len(), 4);
        let test_error = |k: usize| {
            let SavedReport::SupportVectorMachine { request, .. } = &s.reports[k] else { panic!() };
            let a = crate::SvmAnalysis::run(&s.table, &rows, request).unwrap();
            let sections = a.sections();
            let fit = sections.iter().find(|x| x.kind == crate::SectionKind::FitByRole).unwrap();
            let crate::SectionContent::Table(fit) = &fit.content else { panic!() };
            fit.rows.iter().find(|r| r[0] == "Test").unwrap()[2].parse::<f64>().unwrap()
        };
        let (radial, linear) = (test_error(0), test_error(1));
        assert!(radial < 0.15 && linear > radial + 0.1, "radial {radial}, linear {linear}");
        let SavedReport::ModelComparison { request } = &s.reports[2] else { panic!() };
        assert_eq!(request.models.len(), 2);
        let SavedReport::SupportVectorMachine { request, .. } = &s.reports[3] else { panic!() };
        let a = crate::SvmAnalysis::run(&s.table, &rows, request).unwrap();
        let sections = a.sections();
        let fit = sections.iter().find(|x| x.kind == crate::SectionKind::FitByRole).unwrap();
        let crate::SectionContent::Table(fit) = &fit.content else { panic!() };
        let r2 = fit.rows[1][4].parse::<f64>().unwrap();
        assert!(r2 > 0.6, "cross-validated R² {r2}");
    }

    /// The Weibull with a threshold (the generating family) ranks near the
    /// top and fits; the normal is rejected.
    #[test]
    fn seal_strength_identification() {
        let s = super::seal_strength_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::DistributionFit { request } = &s.reports[0] else { panic!() };
        let a = crate::DistributionFitAnalysis::run(&s.table, &rows, request).unwrap();
        let ranked = a.ranked();
        let top: Vec<_> = ranked.iter().take(3).map(|f| f.fit.family).collect();
        assert!(top.contains(&crate::FitFamily::Weibull3), "{top:?}");
        let w3 = a.fit(crate::FitFamily::Weibull3).unwrap();
        assert!(w3.anderson_darling.p_value > 0.05, "{:?}", w3.anderson_darling);
        assert!((w3.fit.threshold - 20.0).abs() < 2.0, "{}", w3.fit.threshold);
        let normal = a.fit(crate::FitFamily::Normal).unwrap();
        assert!(normal.anderson_darling.p_value < 0.05, "{:?}", normal.anderson_darling);
        // The capability study: the normal's left tail runs past the threshold, so it expects far more
        // seals below 21 N than the Weibull with threshold does.
        let SavedReport::Capability { request } = &s.reports[1] else { panic!() };
        let c = crate::CapabilityAnalysis::run(&s.table, &rows, request).unwrap();
        let p = c.percentile().unwrap();
        let normal_ppm = c.result().overall.expected_ppm.0;
        assert!(p.expected_ppm.0 < 0.5 * normal_ppm, "{:?} vs {normal_ppm}", p.expected_ppm);
        assert!(p.ppk > c.result().overall.cpk, "{} vs {}", p.ppk, c.result().overall.cpk);
        // The fitted density (shape > 1) is drawn from zero at the threshold,
        // below the smallest reading.
        let hist = c.sections().into_iter().find(|x| x.kind == crate::SectionKind::CapabilityHistogram).unwrap();
        let crate::SectionContent::Bars(b) = &hist.content else { panic!() };
        let first = b.curve.as_ref().unwrap().1[0];
        let stat_quality::PercentileSource::Fitted(f) = &p.source else { panic!() };
        assert!((first[0] - f.threshold).abs() < 1e-9 && first[1].abs() < 1e-9, "{first:?}");
    }

    /// Beam deflection: the oscillation inflates the moving ranges, so no
    /// run rule fires on the individuals chart (checked independently with a
    /// direct Python implementation of the eight rules: σ = MR̄/1.128 = 355.2,
    /// no point beyond 1σ eight in a row, none within 1σ fifteen in a row),
    /// while the runs-up-and-down test finds too many runs (133 against 120
    /// expected).
    #[test]
    fn beam_deflection_rules_and_runs() {
        let s = super::lew_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::ControlChart { request, options } = &s.reports[0] else { panic!() };
        let a = crate::ControlChartAnalysis::run(&s.table, &rows, request).unwrap();
        assert!(a.location_violations(options).is_empty());
        let c = crate::run_report(&s.reports[1], &s.table, &rows).unwrap();
        let runs = c.sections.iter().find(|x| x.title.contains("Runs")).expect("runs section");
        let crate::SectionContent::Table(t) = &runs.content else { panic!("{}", runs.title) };
        let up_down = t.rows.iter().find(|r| r[0] == "Up and down").unwrap();
        let p_many: f64 = up_down[4].parse().unwrap();
        assert!(p_many < 0.05, "too many runs up and down: {up_down:?}");
    }

    /// Solder defects: nozzle wear leads line A and the reflow profile line B,
    /// the rarest causes pool into "Other", and the diagram has six branches.
    #[test]
    fn solder_defects_pareto_and_diagram() {
        let s = super::solder_defects_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::Pareto { request, options } = &s.reports[0] else { panic!() };
        let spec = crate::ParetoAnalysis::run(&s.table, &rows, request).unwrap().chart(options).unwrap();
        assert_eq!(spec.panels.len(), 2);
        assert_eq!(spec.panels[0].bars[0].label, "Nozzle wear");
        assert_eq!(spec.panels[1].bars[0].label, "Reflow profile");
        for p in &spec.panels {
            let other = p.bars.last().unwrap();
            assert_eq!(other.label, "Other");
            assert!(other.pooled.len() >= 2, "{:?}", other.pooled);
            let total: f64 = p.bars.iter().map(|b| b.value).sum();
            assert!((total - p.total).abs() < 1e-9);
        }
        let SavedReport::CauseEffect { request } = &s.reports[2] else { panic!() };
        let a = crate::CauseEffectAnalysis::run(&s.table, &rows, request).unwrap();
        assert_eq!(a.diagram().branches.len(), 6);
        assert_eq!(a.diagram().node(&[0, 0]).unwrap().label, "Nozzle wear");
        assert_eq!(a.diagram().node(&[0, 0]).unwrap().children.len(), 2);
    }

    /// Shaft diameters: batch-to-batch variation dominates the multi-vari
    /// breakdown, ahead of shaft-to-shaft and within-shaft variation.
    #[test]
    fn shaft_diameters_multi_vari() {
        let s = super::shaft_diameters_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::MultiVari { request, .. } = &s.reports[0] else { panic!() };
        let a = crate::MultiVariAnalysis::run(&s.table, &rows, request).unwrap();
        let v = a.result().factor_variation();
        assert!(v[0].mean_range > 2.0 * v[1].mean_range, "{v:?}");
        assert!(v[0].mean_range > 2.0 * a.result().mean_within_range(), "{v:?}");
        assert_eq!(a.result().cells().len(), 15);
        let SavedReport::MultiVari { request, .. } = &s.reports[1] else { panic!() };
        let a = crate::MultiVariAnalysis::run(&s.table, &rows, request).unwrap();
        // The taper: left ends are wider than right ends on average.
        let mean_at = |p: usize| {
            let cells: Vec<f64> = a.result().cells().iter().filter(|c| c.path[2] == p).map(|c| c.mean).collect();
            cells.iter().sum::<f64>() / cells.len() as f64
        };
        assert!(mean_at(0) - mean_at(2) > 0.005, "{} {}", mean_at(0), mean_at(2));
    }
}
