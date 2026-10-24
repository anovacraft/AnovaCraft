# Role & Project Overview
You are an expert Rust systems architect and statistical computing engineer.
Your objective is to design and implement an open-source, high-performance statistical analysis engine in Rust, capable of performing advanced data analysis, design of experiments (DOE), and predictive modeling.

---

### Clean-Room & Legal Provenance Constraints
You must operate strictly under clean-room engineering principles:

1. **Zero Proprietary Code or Asset Reuse:**
   - Never decompile, disassemble, inspect, or reference any proprietary statistical software's binaries, internal bytecode, scripts, or runtime libraries.
   - Do not extract, copy, or recreate any proprietary UI layouts, iconography, brand marks, palettes, or proprietary schema files.
   - Do not replicate any proprietary scripting language's syntax or script parsers. Automation uses Rhai (an open-source embedded language) and the documented JSON project/pipeline formats.

2. **Permissible Knowledge Base:**
   - Rely solely on public academic literature, standard statistical formulations (NIST SEMATECH e-Handbook, peer-reviewed journals), open documentation of standard methods, and general user-facing statistical workflow descriptions.
   - All statistical routines must be implemented from canonical mathematical definitions (e.g., standard formulas for OLS, GLM, ANOVA, PCA, partial least squares, Box-Cox transformations, and response surface methodology).

3. **Independent Taxonomy:**
   - Establish an independent naming taxonomy for modules, functions, outputs, and terminology (e.g., use standard statistical terms: `fit_linear_model`, `factorial_design`, `principal_component_analysis`).

4. **Independent Visual & Interaction Design:**
   - Specify and build the GUI from generic functional requirements (what the user needs to accomplish) and published visualization literature, never from screenshots, videos, or descriptions of a proprietary product's appearance.
   - Menu labels, panel names, report section titles, icons, glyph shapes, and color assignments must be chosen independently. Do not reuse a proprietary product's menu structure, platform names, or visual encodings, even in approximated form.
   - Generic interaction concepts with established public provenance (linked brushing, collapsible outlines, context menus, slice/trace plots of fitted models) are permitted; their presentation must be original.

5. **Ask before changing optimizer presentation:**
   - Optimizers that suggest factor settings toward a goal under constraints (Design ▸ Propose next runs, best settings in Several responses, and any future one) never draw, shade or hide disallowed combinations or excluded regions in a plot (response explorer, contour or design previews included), never list the constraints beside the suggested settings (reports give only their number), and choose their suggestions by the search, never by the user picking from a table of scored candidates. Do not add supersaturated designs or space-filling designs with categorical factors. Ask the project owner before changing any of this.

---

### Technical Architecture & Stack Guidelines

- **Language & Edition:** Rust (latest stable edition).
- **Core Dependencies:**
  - Data Frames / Columnar Memory: `polars` / Apache Arrow.
  - Linear Algebra & Numerics: `faer` or `ndarray` + OpenBLAS/LAPACK.
  - Statistical Distributions & Hypotheses: `statrs`.
  - Serialization / Data I/O: `serde`, `csv`, `parquet`.
  - Parallelism: `rayon`.
- **Code Quality:**
  - Write fully idiomatic Rust: enforce strict type safety, zero-cost abstractions, robust error handling with `thiserror`/`anyhow`, and clean memory ownership.
  - Zero `unsafe` code unless required for vectorized/SIMD math with exhaustive comments and safety invariants.

---

### Implementation Phases & Behavioral Expectations

1. **Phase 1 — Core Statistical Engine (`stat-core`, `stat-linear`, `stat-discrete`, `stat-strd`):**
   - Descriptive statistics (moments, quantiles, robust estimators).
   - Univariate and bivariate continuous/categorical distributions.
   - Linear regression (OLS) with complete ANOVA tables, parameter estimates, standard errors, $t$-tests, $p$-values, confidence intervals, and residual diagnostics ($R^2$, adjusted $R^2$, AIC/BIC).

2. **Phase 2 — Multivariate & Model Fitting:**
   - Multiple regression, Ridge/Lasso, and Generalized Linear Models (GLM).
   - Principal Component Analysis (PCA) via SVD.
   - K-means and hierarchical clustering routines.

3. **Phase 3 — Design of Experiments (DOE):**
   - Full and fractional factorial design generators.
   - Plackett-Burman and Response Surface (Central Composite, Box-Behnken) designs.
   - Response-surface evaluation: evaluate fitted response surface equations and locate stationary points/optima via canonical analysis of quadratic models (Box & Draper, *Response Surfaces, Mixtures, and Ridge Analyses*).

---

### Output & Verification Standards

- **Feature list:** `FEATURES.md` lists what the application does, by menu. Any change a user can see (a new analysis, report section, option, plot behaviour or file format) updates it in the same commit. Check it before building something new, in case it already exists elsewhere in the app. Planned work is in `ROADMAP.md`; when an item ships, move it from there to `FEATURES.md`.
- **Pending menus:** `docs/pending-ui.md` lists methods whose engine and script functions are built but which have no menu, dialog or report yet (currently none). When one gets its menu, dialog, report sections, manual page and sample data, mark it Done in `ROADMAP.md` and the tracker and remove it from that file. Check that file before planning UI work.
- **User manuals:** two mdBooks (preview with `mdbook serve docs/manual` or `docs/guide`). The **Reference Manual** (`docs/manual`) explains everything in detail for power users; the **Quick Guide** (`docs/guide`) describes every menu item in plain language for novices, one page per menu (the GUI test `quick_guide_names_every_menu_item` fails when a menu item is missing from it). Any change to user-visible behaviour updates the manual page that describes it in the same PR (and the Quick Guide entry when a menu item is added, renamed or moved), including changes to features already considered done (renamed menu items, new defaults, changed layouts, removed options), not only new features. A new feature gets a page or section; a screen whose look changed gets its screenshot re-rendered with `scripts/capture-manual-shots.sh` (pictures are rendered from the app at 1440 × 900, never stretched; listed in `crates/app-gui/src/app/gui_tests/manual.rs`) or, if the test cannot take it, a `Screenshot to add` marker listed in `docs/manual/src/maintaining/screenshots.md`. The formula and scripting references stay in `docs/formulas.md` and `docs/scripting.md`, which the manual includes rather than copies. `scripts/check-manual.sh` runs the manual checks CI runs. The app carries both books as PDFs (Help menu, opened in the system viewer): any PR that changes either book, `docs/formulas.md` or `docs/scripting.md` runs `scripts/build-manual-pdfs.sh` and commits `crates/app-gui/assets/manuals` (the test `embedded_manuals_match_their_sources` fails otherwise).
- **Validation record:** `docs/validation.md` lists every statistical routine, how its accuracy is validated, the reference data (with its source) and tolerance, and the test that checks it. Any PR that adds a statistical routine or changes how one computes its results updates its row in the same PR; routines in unmerged PRs sit under "Pending in open pull requests" until they merge.
- **Test-Driven Development:** Every statistical routine must have accompanying unit and integration tests.
- **GUI tests:** `crates/app-gui/src/app/gui_tests/` drives the running app headlessly with `egui_kittest` (clicks, typing, drags; checks what is on screen and the app's state). Any PR that adds or changes a dialog, report interaction or plot gesture adds a GUI test for it, and every GUI bug fix adds a test that fails without the fix. Tests find controls by their accessible text; give unlabelled controls a name with `widget_info`. Set `ANOVACRAFT_GUI_SHOTS=<folder>` to save PNGs at each `Gui::shot` (needs a Vulkan/Metal driver; Mesa lavapipe works headless). See `docs/manual/src/maintaining/gui-tests.md`.
- **Reference Benchmarks:** Verify numerical accuracy by benchmarking outputs against verified standard reference datasets (such as the NIST Statistical Reference Datasets / StRD for linear regression and ANOVA).
- **Documentation:** Include standard doc-comments (`///`) on every public struct and method explaining the statistical methodology, mathematical formula, input parameters, and failure modes.

---

### GUI Architecture & Interaction Model

#### 1. Tech Stack
- **GUI Framework:** `egui` with `eframe` (Native desktop backend via `wgpu`).
- **Plotting & Visualizations:** `egui_plot` for responsive, interactive 2D scatter plots, contour plots, and residual charts.
- **Iconography:** Use only open-source icon sets (e.g., Lucide or Feather icons, rendered as SVG) under their original licenses. Do not copy, trace, or approximate any proprietary product's icons, logos, or brand assets.
- **Visual identity:** Define the project's own color palette and theme tokens (light and dark). Color must never be the sole carrier of meaning; pair it with a shape, icon, or text label.
  - Plot colors come only from `app_gui::theme::PlotPalette` (light/dark tokens; faint colors premultiplied in gamma space, as CSS composites them); stat-viz's fixed colors map to dark counterparts via `stat_viz::palette::for_dark`. Do not reintroduce per-plot color constants.
  - Plots are drawn through `plot_axes::PlotChrome` (canvas, grid behind the data, axis spines). Default grid: horizontal major rules only, no minor grid, no vertical rules (both available per axis in Plot settings). Points are solid and borderless, with a soft halo in dark mode.
  - Default axis ranges use `stat_viz::auto_range`: 6% padding per side, never below zero when all data are non-negative (except that scatter points at or just above zero get padding below, so the frame does not cut them in half), a single value framed from zero; categorical axes extend half a category beyond the first and last; nice tick steps 1, 2, 2.5, 5 × 10ⁿ.
- **Scrolling lists:** any list taller than its box shows a ▾ (▴) cue where entries are cut off. Drop-down lists are built with `dialog_kit::ComboExt::show_list`, which adds the cues, never with bare `ComboBox::show_ui`.
- **Typography:** bundled open-licensed fonts (`crates/app-gui/assets/fonts`, licences alongside): Inter for text, JetBrains Mono for numbers (tick labels and numeric table cells, so digits align), DejaVu Sans as a glyph fallback, and the generated `AnovaCraftMarks` font (`build_marks.py`) that positions combining marks such as the bar in X̄, which egui does not place itself.

#### 2. Layout Structure
Implement a three-pane desktop workspace:

- **Left Sidebar (Dataset & Variables):**
  - Variables list: each variable shows its measurement level (Stevens' levels of measurement) through an open-source icon plus a text label or tooltip, for example:
    - Continuous: Lucide `ruler`
    - Nominal (unordered categories): Lucide `tag`
    - Ordinal (ordered categories): Lucide `list-ordered`
  - Row status summary: total rows, selected rows, and excluded rows.
- **Center Workspace (tabbed):**
  - *Data view:* high-performance virtualized grid of the active dataset's rows and columns.
  - *Results view:* vertically stacked, collapsible result sections for each analysis run.
- **Top Menu Bar:** categories named with standard statistical terminology, for example:
  - **Data** (import/export, filter, transform, recode)
  - **Describe** (summary statistics, distributions)
  - **Compare** (one-sample, independent-group, paired, proportion and rate tests; power and sample size)
  - **Model** (linear model, GLM, regularized regression)
  - **Multivariate** (PCA, clustering)
  - **Design** (factorial, screening, and response-surface designs)
  - **Quality** (control charts with run rules, measurement systems analysis)
  - **Reliability** (survival curves with censoring, life-distribution fits)
  - **Visualize** (graph canvas: drag columns onto shelves, combine layers, linked small multiples; graphs saved as JSON pipeline files reopen on any dataset with the same column names)

#### 3. Core Behavioral Mechanics

- **Collapsible Result Hierarchy:**
  - Structure every analysis result as a nested tree of collapsible sections (`egui::CollapsingHeader`).
  - Each section provides an options menu (a standard overflow icon such as Lucide `ellipsis`, plus right-click) for adding or removing diagnostics, additional tests, or plots in place.
  - **Model reports:** every model report includes an Effect Pareto (−log₁₀ p per effect). Models keep the hierarchy principle (an interaction or power implies its lower-order effects; dialogs and stepwise selection enforce heredity, and reports note violations). One-way models (a single categorical factor) add tests of equal variances and a choice of pairwise comparisons (Tukey–Kramer by default, Games–Howell, Holm-adjusted t, or none).
  - **Response Explorer:** for a fitted model, render a row of 1-D conditional-effect (slice) plots, one per factor, each showing the predicted response with its confidence band while the other factors are held at current settings. Each plot has a draggable reference line setting that factor's value; all plots and the predicted value update in real time.

- **Plot Navigation:**
  - Plots never move in response to scrolling or pinching. Each axis is a draggable ruler: dragging near either end moves that limit only (the grabbed value follows the pointer), dragging elsewhere shifts the whole axis. Dragging inside the plot area is reserved for box selection.
  - Every plot offers a "Reset view" control (and double-click) that re-fits it to its data.
  - Grouped points use a distinct color *and* marker shape per group (color-blind-safe palette); selection is shown with a ring so it stays visible on any group color.

- **Linked Row State (Brushing):**
  - Implement a centralized `RowStateManager` holding per-row selection, exclusion, and display-color state, following the linked-brushing model of Becker & Cleveland (1987), "Brushing Scatterplots", *Technometrics* 29(2).
  - Selecting rows anywhere (box or click selection in any plot, or selecting rows in the data view) updates the shared state, and every open plot and data view re-renders to highlight the same subset immediately.

#### 4. Documents & Files
- Several datasets can be open at once. Each open dataset (with its row states and reports) is a document, listed as tabs along the bottom edge of the window and in the left sidebar, where right-clicking an entry offers save, export and close. Opening a file or sample while one is open adds a new document.
- A document's data grid, results pane or a single report can open in a native window of its own (egui immediate viewports, `app::windows`); a view lives in one place at a time, the main window showing a placeholder. The main window's document (`shown`) is separate from the command target (`active`, the document of the window used last), which menus, dialogs and the sidebar act on; shortcuts run in each window on its document.
- Import: CSV, TSV, other delimited text (delimiter detected), JSON arrays of row objects, and spreadsheet workbooks (`.xlsx`, `.xlsm`, `.xlsb`, `.xls`, `.ods`; one document per non-empty sheet). Export: CSV, TSV, JSON and `.xlsx`.
- The data grid is editable like a spreadsheet (cell cursor, double-click/Enter/typing to edit, Enter/Tab to commit and move, Escape to cancel), with type-checked entry, row and column insert/delete, rename, data-type and measurement-level changes, and undo/redo (last 500 edits, within 512 MB) (`app_model::editing`). Edits re-run every report that depends on the data; a column used by a report cannot be deleted until that report is closed.
- Project files (`.acraft`; documented JSON — see `app_model::project`) hold one or more tables (derived tables listed under their source, with Update when the source changed) and the window layout; each table stores its data, measurement levels, row states and each report's request and display options. Reopening a project re-runs every report, reproducing it exactly. Closing a document with unsaved changes asks first.

---

# Product Functional Specification: Statistical Analysis Engine

The engine is divided into a Cargo workspace with distinct mathematical engines and an `egui`-based GUI shell.

## Workspace Layout
Crate directories and package names use hyphens (Cargo convention). No crate may be named `core`, `std` or `alloc`, which would shadow Rust's built-in libraries.

- `crates/stat-core`: Shared numerical kernels — dense matrices, compensated summation, pivoted Householder QR, special functions, probability distributions, distribution fitting (`distribution_fit`: fourteen families with threshold forms, Anderson–Darling with simulated p-values, Johnson transformations; shared by distribution identification, non-normal capability and life distributions), random numbers, box-constrained Nelder–Mead and Latin hypercube starts (`optimize`), and the workspace error type.
- `crates/stat-data`: Columnar tabular memory (plain `Vec<f64>` / `Vec<String>` columns; no Polars dependency), unified `RowStateManager` (selection, exclusion, color flags, named row subsets whose active one acts as an exclusion).
- `crates/stat-linear`: OLS, ANOVA, ANCOVA, group tests (Welch ANOVA, Kruskal–Wallis, equal-variance tests: Levene, Brown–Forsythe, Bartlett), model diagnostics, model hierarchy (effect heredity), heredity-aware stepwise selection and best subsets; MANOVA; nonlinear least squares (`nonlinear`: built-in and formula curves, robust and global fits, inverse prediction, curve comparison, and the assay curve templates in `nonlinear::assay`); method comparison (Deming, Passing–Bablok) and stability shelf life (ICH Q1E).
- `crates/stat-inference`: Hypothesis tests and intervals — one-sample, two-sample and paired tests, one-way repeated-measures ANOVA, rank and sign tests, variance tests, multiple comparisons (Tukey–Kramer, Games–Howell, Holm, Dunn), proportions, rates, equivalence and power; analysis of means (`anom`: means, proportions, rates and two-way layouts, exact critical values from the equicorrelated multivariate t).
- `crates/stat-discrete`: Contingency tables, Chi-square, Logistic regression, GLM (logit, probit, cloglog, log, identity and inverse links; binomial, Poisson, Gamma), probit dose–response with fiducial limits, multiple-response (check-all-that-apply) frequencies and per-item tests with Bonferroni adjustment.
- `crates/stat-mixed`: Linear and generalized linear mixed models — random intercepts and correlated (or independent) slopes for crossed or nested grouping columns; normal (REML/ML with Satterthwaite t/F tests), binomial, Poisson, Gamma and negative binomial families (Laplace approximation, or adaptive Gauss–Hermite quadrature for a single random intercept); variance components with Wald intervals, conditional modes, conditional and marginal predictions. Verified against an independent dense reference implementation (`tests/data/reference_cases.py`).
- `crates/stat-multivariate`: PCA (SVD), Correspondence Analysis, factor analysis, discriminant analysis, K-means, Hierarchical clustering, Gaussian mixture and latent class clustering, multidimensional scaling, t-SNE and UMAP embeddings, correlation matrices (Pearson, Spearman, partial), outlier distances (Mahalanobis, jackknifed, T²), item analysis (Cronbach's alpha), missing-value imputation and multiple imputation (`multiple_imputation`: chained equations and multivariate normal data augmentation; Rubin's-rules pooling of reports is `app_model::pooling`).
- `crates/stat-doe`: Full/fractional factorials, Response Surface (CCD, Box-Behnken), D/I-optimal exchange, Mixture/Scheffé designs, Definitive Screening (DSD), Derringer–Suich desirability functions; mixed-level full factorials, extreme-vertices mixture designs, orthogonal arrays with signal-to-noise ratios.
- `crates/stat-quality`: SPC control charts (Shewhart, EWMA, CUSUM with Nelson rules; phases and subgroups of varying size with per-point limits in `phased`; moving-average, three-way between/within, zone and short-run charts in `extended`; p, np, c, u, Laney p′/u′ and rare-event g/t charts in `attribute`), process capability (normal theory, and percentile-based indices for non-normal processes with bootstrap intervals in `nonnormal`; between/within capability of batches or streams in `between_within`; binomial and Poisson capability with exact intervals in `attribute_capability`), acceptance sampling (single, double and k-method variables plans from risk points with OC, AOQ, ATI and ASN in `acceptance`), Gauge R&R (ANOVA-based), Pareto ranking with an "Other" pool (`pareto`), multi-vari breakdown by nested or crossed factors (`multivari`), multivariate charts (`multivariate`: Hotelling T², generalized variance |S|, MEWMA, with T² contributions); gauge study worksheets, bias/linearity, Cg/Cgk and intraclass correlation (`gauge_worksheet`, `gauge_bias`, `icc`).
- `crates/stat-reliability`: Kaplan-Meier, parametric Weibull/Lognormal survival fitting; reliability growth (Crow–AMSAA), repairable systems (mean cumulative function, trend tests) and reliability test plans.
- `crates/stat-predictive`: Coordinate-descent Elastic Net (Lasso/Ridge); CART classification and regression trees (surrogate splits, cost-complexity pruning, cross-validated size); random forests (bootstrap samples, random predictor subsets, out-of-bag error, permutation and impurity importance; grown in parallel with rayon, seeded per tree); gradient-boosted trees (squared, absolute, logistic and multinomial losses; histogram splits, Newton leaves with L2, subsampling, early stopping); partial least squares (NIPALS, SIMPLS, PLS-DA, VIP, factors by cross-validation); support vector machines (SMO with second-order working sets, linear/radial/polynomial kernels, one-against-one classes, Platt probabilities with pairwise coupling, grid tuning); multivariate adaptive regression splines (`mars`); neural networks with one or two hidden layers (`neural`); Gaussian-process regression (`gaussian_process`: Matérn 5/2 and squared-exponential kernels with one length scale per input, nugget, maximum likelihood from multi-start BFGS, exact leave-one-out) and next-run proposals (`acquisition`: expected improvement, target and desirability forms, upper confidence bound, constant-liar batches within constraints); validation roles (random, stratified, grouped, k-fold) and fit measures (RMSE, MAE, R², misclassification, log loss, AUC/ROC). The shared prediction framework (roles per row, fit by role, saved prediction columns) is `app_model::prediction`. Saved prediction columns are compared by `app_model::model_comparison`; the automated model search (`app_model::model_search`) fits every suitable model on the same validation roles. (Stepwise selection lives in `stat-linear`.)
- `crates/stat-bayes`: Bayesian analysis — conjugate posteriors with equal-tailed and HPD intervals, default Bayes factors (JZS t tests, correlation, contingency tables), g-prior linear models and ANOVA with model averaging, the No-U-Turn sampler with rank-normalized R̂/ESS/MCSE, PSIS-LOO and WAIC, and Bayesian GLMs with group effects. Verified against R BayesFactor, posterior and loo, and PyMC (`tests/data`). The reports are `app_model::bayes_analysis` and `app_model::bayes_models`.
- `crates/stat-functional`: Curves from long data — B-spline and Fourier bases with Gram and roughness-penalty matrices, penalized smoothing with GCV, landmark (PCHIP warps) and continuous (Ramsay–Li minimum-eigenvalue) registration, functional PCA. Verified against scikit-fda (`tests/data/reference_cases.py`). The report with score models and the Curve Explorer is `app_model::functional_analysis`.
- `crates/stat-text`: Text analysis — Unicode word segmentation, stop words, Porter stemming, phrases, document–term matrices with TF-IDF weightings, latent semantic analysis by truncated SVD (exact or randomized), LDA topic models by collapsed Gibbs sampling, and lexicon sentiment scores with negation (bundled VADER lexicon, MIT licence). Verified against Porter's reference output, NumPy, SciPy and vaderSentiment.
- `crates/stat-timeseries`: Equally spaced series — ACF/PACF, Ljung–Box, Yule–Walker AR, cross-correlation with prewhitening, differencing and Box–Cox (Guerrero λ), classical and STL decomposition, ADF and KPSS tests, trend models with accuracy measures, periodogram and smoothed spectrum, exponential smoothing in state-space form (all ETS forms, selection by AICc), seasonal ARIMA by exact Kalman-filter likelihood with forecasts and the stepwise automatic order search. Verified against R (stats, forecast, tseries, urca) and statsmodels.
- `crates/stat-signal`: Signals and spectra — Savitzky–Golay and other smoothing filters, x–y derivatives, cumulative trapezoid/Simpson integrals and linear, spline and PCHIP interpolation, baselines (asymmetric least squares, arPLS, SNIP), peak finding (height, prominence, separation), and multi-peak fitting (Gaussian, Lorentzian, pseudo-Voigt, Voigt via the Faddeeva function, split shapes) with standard errors and AICc/F comparison of peak counts; FFT of any length (mixed radix, Bluestein), windows, periodogram, Welch and spectrogram; IIR design in second-order sections (Butterworth, Chebyshev I/II, elliptic), windowed-sinc FIR, FFT and notch filters, forward or zero-phase; linear and circular convolution, cross- and autocorrelation, Wiener and Richardson–Lucy deconvolution, Hilbert envelope and phase; discrete wavelet transforms (haar, db, sym, coif) with SURE/universal denoising and the continuous wavelet transform. Verified against SciPy, NumPy, pandas, pybaselines, lmfit, PyWavelets and scikit-image (`tests/data/curves_reference_cases.py`, `tests/data/reference_cases.py`). The reports are `app_model::signal_curves` and `app_model::signal_analysis`.
- `crates/stat-viz`: Grammar-of-graphics graph engine — column-to-channel encodings (axes, panel splits, color, size, shape, subgroups), small-multiple panel layout and pages, axis scales (linear, log₁₀, ln, logit, square root, reciprocal), a per-graph row filter, frequency and weight columns, column switchers, and composable layers (jittered points; weighted polynomial fits with confidence and prediction bands, equation, R² and p-value; smoothing splines and LOESS with confidence bands; notched box plots and violins; summary statistics with error bars; density ellipses, kernel-density contours and hexagonal/square binning; formula curves; bars (side by side, stacked, 100%) with error bars, line and run charts, category heat maps; histograms, Wilkinson dot plots and probability plots against fitted distributions with order-statistic bands), plus treemap and parallel-coordinates chart forms; point labels and key placement; SVG/PDF/EPS export and a summary table. GUI-independent; an optional `egui` feature draws layers with `egui_plot`.
- `crates/stat-script`: Rhai scripting engine (tables, row states, table operations, analyses via `app_model::run_report`, report sections as data, file output; operation/size limits and cancellation) used by automation scripts, script notebooks (`NotebookSession`: cells sharing variables and functions) and replaying recorded actions; the `anovacraft-run` headless runner. (Formula columns use the same language through `app_model::formula`, which edits call, so it lives below this crate.) Script API reference: `docs/scripting.md`.
- `crates/stat-strd`: NIST StRD reference datasets with certified values, the StRD file parser, and the accuracy (log relative error) verification suite.
- `crates/app-model`: GUI-independent application logic — data edits with undo, formula columns (`formula`: per-row Rhai expressions recomputed after edits; reference `docs/formulas.md`), analysis runs bound to row state, report section content (formatted tables, plot data tagged with row indices), brushing gestures → `RowStateManager` updates. Fully unit tested; `app-gui` stays a thin rendering layer over it.
- `crates/app-gui`: `egui` desktop frontend featuring cross-chart brushing, collapsible hierarchical report trees, and interactive response explorers (see GUI Architecture & Interaction Model).

## Implementation Guidelines per Module

1. **Deterministic Verification:**
   - Linear, ANOVA and univariate summary statistics must match NIST Statistical Reference Datasets (StRD) within $10^{-10}$ relative error (a log relative error of at least 10 on every certified value).
   - **Exception — data-representation limit:** where the certified values are for decimal data that binary64 cannot represent exactly, and the problem's conditioning amplifies that unavoidable half-ulp rounding beyond $10^{-10}$, the required accuracy is the demonstrated representation floor instead. The test suite must prove the floor (e.g., by showing that a half-ulp perturbation of the stored data alone moves the results by as much). Currently this applies to **Filip** (linear regression; condition number ≈ 7·10⁸; floor ≈ 7.5 digits; required ≥ 7) to the univariate **NumAcc3** / **NumAcc4** standard deviations (values such as 10000000.1 carry representation error comparable to the certified sd of 0.1; floors ≈ 9.5 / 8.4 digits; required ≥ 9 / ≥ 8), and to the one-way ANOVA datasets **SmLs04–SmLs06** / **SmLs07–SmLs09** (values offset by 10⁶ / 10¹² around deviations of ±0.1; floors ≈ 9.3 / 3.3 digits; required ≥ 8 / ≥ 2), and to the nonlinear dataset **Lanczos1** (residual sum of squares 1.4·10⁻²⁵, so half-ulp rounding of the responses moves it in its third digit; floor ≈ 2.7 digits; its residual SS, residual SD and standard deviations required ≥ 2.5, estimates ≥ 4).
   - Reliability and survival estimates must cross-validate against published Kaplan-Meier and maximum likelihood standard formulas.

2. **Clean-Room Boundary:**
   - Scripting uses Rhai. Do NOT implement a parser for, or imitate the syntax of, any proprietary scripting language.
   - Do not duplicate proprietary column properties or menu names; use standard scientific nomenclature.
