# Development guide



> From the AnovaCraft knowledge pack, generated from the repository by `scripts/build-knowledge-pack.py`. AnovaCraft is an open-source statistics application written in Rust. Answer from this text: it does not depend on any other file. Paths such as `crates/...` or `docs/...` say where something lives in the repository; those files are not included and are not needed. Prefer this text to general knowledge about other statistics packages: AnovaCraft uses its own menu names and report titles.

## Building and running

- Install Rust (stable) from rustup.rs, clone the repository, then `cargo run --release -p app-gui` starts the desktop app.
- `cargo test --workspace --no-fail-fast` runs every test; GUI tests are in `crates/app-gui/src/app/gui_tests/` (egui_kittest, headless).
- `cargo clippy --workspace --all-targets --keep-going -- -D warnings` is the lint gate CI uses.
- `anovacraft-run script.rhai` (crate `stat-script`) runs Rhai scripts headless.
- `scripts/check-manual.sh` checks both manuals as CI does (needs mdBook for the build step).
- Pull requests use `.github/pull_request_template.md` and are merged with a merge commit.

## Project rules (CLAUDE.md)

### Role & Project Overview
You are an expert Rust systems architect and statistical computing engineer.
Your objective is to design and implement an open-source, high-performance statistical analysis engine in Rust, capable of performing advanced data analysis, design of experiments (DOE), and predictive modeling.

---

##### Clean-Room & Legal Provenance Constraints
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

##### Technical Architecture & Stack Guidelines

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

##### Implementation Phases & Behavioral Expectations

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

##### Output & Verification Standards

- **Feature list:** `FEATURES.md` lists what the application does, by menu. Any change a user can see (a new analysis, report section, option, plot behaviour or file format) updates it in the same commit. Check it before building something new, in case it already exists elsewhere in the app. Planned work is in `ROADMAP.md`; when an item ships, move it from there to `FEATURES.md`.
- **Pending menus:** `docs/pending-ui.md` lists methods whose engine and script functions are built but which have no menu, dialog or report yet (currently none). When one gets its menu, dialog, report sections, manual page and sample data, mark it Done in `ROADMAP.md` and the tracker and remove it from that file. Check that file before planning UI work.
- **User manuals:** two mdBooks (preview with `mdbook serve docs/manual` or `docs/guide`). The **Reference Manual** (`docs/manual`) explains everything in detail for power users; the **Quick Guide** (`docs/guide`) describes every menu item in plain language for novices, one page per menu (the GUI test `quick_guide_names_every_menu_item` fails when a menu item is missing from it). Any change to user-visible behaviour updates the manual page that describes it in the same PR (and the Quick Guide entry when a menu item is added, renamed or moved), including changes to features already considered done (renamed menu items, new defaults, changed layouts, removed options), not only new features. A new feature gets a page or section; a screen whose look changed gets its screenshot re-rendered with `scripts/capture-manual-shots.sh` (pictures are rendered from the app at 1440 × 900, never stretched; listed in `crates/app-gui/src/app/gui_tests/manual.rs`) or, if the test cannot take it, a `Screenshot to add` marker listed in `docs/manual/src/maintaining/screenshots.md`. The formula and scripting references stay in `docs/formulas.md` and `docs/scripting.md`, which the manual includes rather than copies. `scripts/check-manual.sh` runs the manual checks CI runs. The app carries both books as PDFs (Help menu, opened in the system viewer): any PR that changes either book, `docs/formulas.md` or `docs/scripting.md` runs `scripts/build-manual-pdfs.sh` and commits `crates/app-gui/assets/manuals` (the test `embedded_manuals_match_their_sources` fails otherwise).
- **Validation record:** `docs/validation.md` lists every statistical routine, how its accuracy is validated, the reference data (with its source) and tolerance, and the test that checks it. Any PR that adds a statistical routine or changes how one computes its results updates its row in the same PR; routines in unmerged PRs sit under "Pending in open pull requests" until they merge.
- **Test-Driven Development:** Every statistical routine must have accompanying unit and integration tests.
- **GUI tests:** `crates/app-gui/src/app/gui_tests/` drives the running app headlessly with `egui_kittest` (clicks, typing, drags; checks what is on screen and the app's state). Any PR that adds or changes a dialog, report interaction or plot gesture adds a GUI test for it, and every GUI bug fix adds a test that fails without the fix. Tests find controls by their accessible text; give unlabelled controls a name with `widget_info`. Set `ANOVACRAFT_GUI_SHOTS=<folder>` to save PNGs at each `Gui::shot` (needs a Vulkan/Metal driver; Mesa lavapipe works headless). See `docs/manual/src/maintaining/gui-tests.md`.
- **Reference Benchmarks:** Verify numerical accuracy by benchmarking outputs against verified standard reference datasets (such as the NIST Statistical Reference Datasets / StRD for linear regression and ANOVA).
- **Documentation:** Include standard doc-comments (`///`) on every public struct and method explaining the statistical methodology, mathematical formula, input parameters, and failure modes.

---

##### GUI Architecture & Interaction Model

###### 1. Tech Stack
- **GUI Framework:** `egui` with `eframe` (Native desktop backend via `wgpu`).
- **Plotting & Visualizations:** `egui_plot` for responsive, interactive 2D scatter plots, contour plots, and residual charts.
- **Iconography:** Use only open-source icon sets (e.g., Lucide or Feather icons, rendered as SVG) under their original licenses. Do not copy, trace, or approximate any proprietary product's icons, logos, or brand assets.
- **Visual identity:** Define the project's own color palette and theme tokens (light and dark). Color must never be the sole carrier of meaning; pair it with a shape, icon, or text label.
  - Plot colors come only from `app_gui::theme::PlotPalette` (light/dark tokens; faint colors premultiplied in gamma space, as CSS composites them); stat-viz's fixed colors map to dark counterparts via `stat_viz::palette::for_dark`. Do not reintroduce per-plot color constants.
  - Plots are drawn through `plot_axes::PlotChrome` (canvas, grid behind the data, axis spines). Default grid: horizontal major rules only, no minor grid, no vertical rules (both available per axis in Plot settings). Points are solid and borderless, with a soft halo in dark mode.
  - Default axis ranges use `stat_viz::auto_range`: 6% padding per side, never below zero when all data are non-negative (except that scatter points at or just above zero get padding below, so the frame does not cut them in half), a single value framed from zero; categorical axes extend half a category beyond the first and last; nice tick steps 1, 2, 2.5, 5 × 10ⁿ.
- **Scrolling lists:** any list taller than its box shows a ▾ (▴) cue where entries are cut off. Drop-down lists are built with `dialog_kit::ComboExt::show_list`, which adds the cues, never with bare `ComboBox::show_ui`.
- **Typography:** bundled open-licensed fonts (`crates/app-gui/assets/fonts`, licences alongside): Inter for text, JetBrains Mono for numbers (tick labels and numeric table cells, so digits align), DejaVu Sans as a glyph fallback, and the generated `AnovaCraftMarks` font (`build_marks.py`) that positions combining marks such as the bar in X̄, which egui does not place itself.

###### 2. Layout Structure
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

###### 3. Core Behavioral Mechanics

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

###### 4. Documents & Files
- Several datasets can be open at once. Each open dataset (with its row states and reports) is a document, listed as tabs along the bottom edge of the window and in the left sidebar, where right-clicking an entry offers save, export and close. Opening a file or sample while one is open adds a new document.
- A document's data grid, results pane or a single report can open in a native window of its own (egui immediate viewports, `app::windows`); a view lives in one place at a time, the main window showing a placeholder. The main window's document (`shown`) is separate from the command target (`active`, the document of the window used last), which menus, dialogs and the sidebar act on; shortcuts run in each window on its document.
- Import: CSV, TSV, other delimited text (delimiter detected), JSON arrays of row objects, and spreadsheet workbooks (`.xlsx`, `.xlsm`, `.xlsb`, `.xls`, `.ods`; one document per non-empty sheet). Export: CSV, TSV, JSON and `.xlsx`.
- The data grid is editable like a spreadsheet (cell cursor, double-click/Enter/typing to edit, Enter/Tab to commit and move, Escape to cancel), with type-checked entry, row and column insert/delete, rename, data-type and measurement-level changes, and undo/redo (last 500 edits, within 512 MB) (`app_model::editing`). Edits re-run every report that depends on the data; a column used by a report cannot be deleted until that report is closed.
- Project files (`.acraft`; documented JSON — see `app_model::project`) hold one or more tables (derived tables listed under their source, with Update when the source changed) and the window layout; each table stores its data, measurement levels, row states and each report's request and display options. Reopening a project re-runs every report, reproducing it exactly. Closing a document with unsaved changes asks first.

---

### Product Functional Specification: Statistical Analysis Engine

The engine is divided into a Cargo workspace with distinct mathematical engines and an `egui`-based GUI shell.

#### Workspace Layout
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

#### Implementation Guidelines per Module

1. **Deterministic Verification:**
   - Linear, ANOVA and univariate summary statistics must match NIST Statistical Reference Datasets (StRD) within $10^{-10}$ relative error (a log relative error of at least 10 on every certified value).
   - **Exception — data-representation limit:** where the certified values are for decimal data that binary64 cannot represent exactly, and the problem's conditioning amplifies that unavoidable half-ulp rounding beyond $10^{-10}$, the required accuracy is the demonstrated representation floor instead. The test suite must prove the floor (e.g., by showing that a half-ulp perturbation of the stored data alone moves the results by as much). Currently this applies to **Filip** (linear regression; condition number ≈ 7·10⁸; floor ≈ 7.5 digits; required ≥ 7) to the univariate **NumAcc3** / **NumAcc4** standard deviations (values such as 10000000.1 carry representation error comparable to the certified sd of 0.1; floors ≈ 9.5 / 8.4 digits; required ≥ 9 / ≥ 8), and to the one-way ANOVA datasets **SmLs04–SmLs06** / **SmLs07–SmLs09** (values offset by 10⁶ / 10¹² around deviations of ±0.1; floors ≈ 9.3 / 3.3 digits; required ≥ 8 / ≥ 2), and to the nonlinear dataset **Lanczos1** (residual sum of squares 1.4·10⁻²⁵, so half-ulp rounding of the responses moves it in its third digit; floor ≈ 2.7 digits; its residual SS, residual SD and standard deviations required ≥ 2.5, estimates ≥ 4).
   - Reliability and survival estimates must cross-validate against published Kaplan-Meier and maximum likelihood standard formulas.

2. **Clean-Room Boundary:**
   - Scripting uses Rhai. Do NOT implement a parser for, or imitate the syntax of, any proprietary scripting language.
   - Do not duplicate proprietary column properties or menu names; use standard scientific nomenclature.

## Keeping the manuals current

AnovaCraft has two manuals, both mdBooks in the repository that change in
the same pull request as the code they describe, so they never fall behind
a release:

- this **Reference Manual** (`docs/manual`), the detailed one for power
  users: every option, report section, method and script function;
- the **Quick Guide** (`docs/guide`), for people new to the app or to
  statistics: one page per menu describing every item in plain language,
  plus a first walk-through, a "which analysis do I need?" chooser and a
  glossary. Each entry links to the Reference Manual page with the
  details. Its pictures (named `guide-*`) are in `docs/guide/src/images`.

### The rule

Any change a user can see updates the manual page that describes it, in the
same pull request. That includes:

- a new analysis, report section, option, dialog setting, menu item or file
  format: add or extend its page;
- a change to something already finished, such as a renamed menu item, a
  new default, a different layout or a removed option: correct every page
  that mentions it, and replace or re-mark any screenshot it makes wrong;
- a feature leaving the Coming soon page: move
  it to its own page;
- a menu item added, renamed or moved: its entry on the Quick Guide's page
  for that menu (`docs/guide/src/menus/<menu>.md`), in **bold**, in a sentence
  or two of plain language.

`FEATURES.md` (the terse list of what the app does) changes in the same pull
request too; the manual is the explanation, `FEATURES.md` the inventory.

### How it is enforced

- **CLAUDE.md** states the rule, so every coding session follows it.
- The **pull request template** has a manual checkbox.
- **CI** builds both manuals on every pull request and checks them: a
  missing page or include, a link to a page that does not exist, an image
  that is not in `images/`, a picture whose proportions are not a window's
  (5:4 to 2:1) or whose size is forced, or a screenshot marker missing from
  the checklist fails the build. It also warns when a pull request
  changes user-facing code (`crates/app-gui`, `crates/app-model`,
  `crates/stat-script`) or `FEATURES.md` without touching either manual,
  `docs/formulas.md` or `docs/scripting.md`. A warning is a prompt to check,
  not a failure: internal refactors need no manual change.
- The test `embedded_manuals_match_their_sources` fails when either book
  changed without its PDF being rebuilt (see below).
- The GUI test `quick_guide_names_every_menu_item` (run with the other
  tests) opens every menu of the running app and fails if the Quick Guide
  does not name one of its items.

### Where things go

| What | Where |
|---|---|
| Pages | `docs/manual/src/<part>/<page>.md` |
| Table of contents | `docs/manual/src/SUMMARY.md` |
| Quick Guide pages | `docs/guide/src/menus/<menu>.md` and the pages beside it |
| Screenshots | `docs/manual/src/images/<id>.png`, rendered by `scripts/capture-manual-shots.sh` (see Screenshots) |
| Formula function reference | `docs/formulas.md` (included here, not copied) |
| Script API reference | `docs/scripting.md` (included here, not copied) |

### The PDFs in the app

Help ▸ Quick guide and Help ▸ Reference manual open PDFs of the two books
that are built into the app (`crates/app-gui/assets/manuals`, embedded with
`include_bytes!`, so every build carries them). After changing either book,
`docs/formulas.md` or `docs/scripting.md`, rebuild them:

```sh
./scripts/build-manual-pdfs.sh
```

It builds both books with mdBook, prints each to a PDF with a title page,
linked contents, in-document chapter links and a bookmark outline
(`scripts/book-pdf.mjs`, which needs Node with Playwright's Chromium), and
re-encodes the screenshots as JPEG at most 1100 pixels wide (quality 0.65;
`MANUAL_IMAGE_WIDTH` and `MANUAL_JPEG_QUALITY` change that). It also writes
`docs-stamp.txt`, a hash of every source file of the books
(`scripts/manual-stamp.mjs`). Commit the three files it writes.

The test `embedded_manuals_match_their_sources` (run with the other tests)
computes the same hash and fails when the Markdown changed but the PDFs were
not rebuilt. When two branches both rebuilt the PDFs, resolve the merge by
running the script again on the merged sources.

### Previewing

Install mdBook once, then serve the book with live reload:

```sh
cargo install mdbook
mdbook serve docs/manual --open   # the Reference Manual
mdbook serve docs/guide --open    # the Quick Guide
```

`scripts/check-manual.sh` runs the same checks as CI.

### Style

- Write for someone using the app, not building it: what to click, what they
  get, what it means.
- Use the app's own labels in **bold**, and menu paths as Menu ▸ Item.
- In the Quick Guide, write for someone who has never taken a statistics
  course: what the item is for and when to reach for it, in everyday words;
  leave options and formulas to the Reference Manual.
- Name the published method and, where useful, the sample dataset to try.
- Follow the project's clean-room naming: standard statistical terms only.

### Knowledge pack for chat assistants

`knowledge-pack/` holds Markdown files that a user can give any web chat
assistant so it can answer questions about AnovaCraft: an `llms.txt` index, a
short `00-essentials.md`, and seven files covering the overview and glossary,
menus and features, this manual, the validation record, design conventions, a
code map built from doc comments, and the development guide, plus
`anovacraft-knowledge-pack.md`, which holds all seven in one self-contained
file for assistants that take a single upload. The files are
generated from FEATURES.md, both manuals, `docs/validation.md`, CLAUDE.md,
ROADMAP.md and the crates' doc comments; never edit them by hand.

```sh
python3 scripts/build-knowledge-pack.py          # regenerate
python3 scripts/build-knowledge-pack.py --check  # is it current?
python3 scripts/build-knowledge-pack.py --zip knowledge-pack.zip  # also zip it
```

Regenerate the pack when a release is prepared, or whenever the
documentation has changed a good deal.

### Licence notices

`THIRD-PARTY-NOTICES.txt` at the repository root holds the copyright notices
and licence texts of everything the shipped programs contain that
AnovaCraft's authors did not write: the bundled fonts, icons and lexicon, and
every Rust crate compiled into the app, `anovacraft-run` or the web viewer
(on macOS, Windows and WebAssembly). The app and `anovacraft-run` embed it,
the macOS and Windows build scripts copy it beside the program, and Publish
as web app… writes it beside the viewer in the published folder. It is
generated from Cargo.lock and the crates' own licence files (crates that ship
none get the standard text from `scripts/licences/`); never edit it by hand.

```sh
python3 scripts/build-notices.py          # regenerate after changing dependencies
python3 scripts/build-notices.py --check  # is it current?
```

The test `third_party_notices_match_cargo_lock` fails when Cargo.lock changes
without the file being rebuilt. A new bundled font, icon set or data file gets
an entry in the script's `ASSETS` list, with its licence file in the
repository.

## Screenshots

The pictures in both manuals (this Reference Manual and the Quick Guide)
are rendered from the running app by one command, so they can be
refreshed whenever a screen changes:

```sh
./scripts/capture-manual-shots.sh                     # every picture
./scripts/capture-manual-shots.sh pareto-chart arima  # only these
```

It runs the ignored test `manual_screenshots`
(`crates/app-gui/src/app/gui_tests/manual.rs`), which opens each sample,
report, dialog or menu in a window of 1440 × 900 points with the light
colour scheme, and saves `docs/manual/src/images/<id>.png`. Reports are
shown whole-window; menus and dialogs are cropped to a 16:10 close-up, so
every picture keeps a normal window's proportions. If Pillow is installed,
the script then shrinks each picture to a 256-colour palette. It needs a
graphics driver; on a Linux server without one, install Mesa's software
driver first (`apt-get install mesa-vulkan-drivers`).

`scripts/check-manual.sh` fails on a picture whose width-to-height ratio
is outside 5:4 to 2:1, or whose size is forced in the page, so nothing is
shown stretched or squashed. The Quick Guide's pictures (`guide-*`) are
moved to `docs/guide/src/images`.

### Adding one

1. Add a line for it to `REPORT_SHOTS`, `DIALOG_SHOTS` or `custom_shots`
   in `manual.rs`, and run the script with its id. When an ordinary GUI
   test already reaches the view and calls `gui.shot(…)` there, add it to
   `TEST_SHOTS` instead (picture, the test's shot name, the test): the
   script runs that test with `MANUAL_TEST_SHOTS` set, and the shot is
   taken at 1440 × 900 in light colours under the picture's name.
2. Look at the picture.
3. In the page, put an image line where the picture belongs, for example
   `(Picture: The main window with the Longley sample open)`.

The test draws a view's own window (a data grid or report moved out of the
main window) inside the main window, as an ordinary inner window, through
the same code as a native window. Pictures with several pieces (four chart
forms, say) are drawn one by one and tiled into one 1440 × 900 picture.

A picture the test cannot take is marked in the text like this until
someone captures it by hand at about 1440 × 900 (on macOS, ⌘⇧4 then Space,
and click the window):

> **Screenshot to add** (`example-id`): what the picture should show.

`scripts/check-manual.sh` checks that every marker is listed below and that
every image the manual uses exists.

### To capture

Nothing at present.

### Done

Rendered by `scripts/capture-manual-shots.sh`:

| ID | Page | What to show |
|---|---|---|
| `formula-typeset` | Data: formula columns | Longley, New formula column with `sqrt(GNP) / Unemployed ** 2`: the Reads as line under the text with the radical sign and fraction bar, and the preview of the first rows |
| `distribution-censored-report` | Describe | Well water arsenic, Distribution of Arsenic with Non-detect as left censoring: censored-values table, Kaplan–Meier percentiles, fits by AIC and the distribution function plot |
| `pk-report` | Describe | PK profiles, Pharmacokinetic parameters by Subject and Formulation with the dose column: parameter table, summary by formulation and semi-log profiles |
| `pcoa-report` | Multivariate | Shrub traits, Principal coordinates of Gower distances coloured by Growth form: eigenvalues with broken stick and the ordination plot |
| `report-filter-panel` | Reading a report | Plant growth, Distribution of Height with the filter panel open on Fertilizer, stepping at "Level 2 of 3: B" |
| `graph-drop-zones` | Graph canvas | Longley graph canvas of Employed by GNP while a column is dragged over it, the five zones drawn and the centre one highlighted |
| `save-plot-dialog` | Saving plots and results | Longley, the Residual Normal Quantile Plot's Save plot dialog with a size on paper of 89 mm wide, Serif font at 125% and the preview |
| `duplicate-on-another-table` | Documents, files and projects | Tensile tests sample, Duplicate on another table from batch A's graph onto batch C, with Test piece and Elongation (%) chosen |
| `plot-axis-breaks` | Working with plots | Longley graph canvas of Employed by GNP with two vertical breaks, Plot settings open at Segments and marks with the lowest piece on Log (base 10) and Zigzag marks, and the note naming the scales |
| `plot-axis-look` | Working with plots | Longley linear model, Plot settings with the Ticks and Spine groups open, beside the plot with outside ticks, an offset vertical axis and a π tick label |
| `graph-annotations` | Graph canvas | Longley, Employed by GNP with a fit line: a text annotation, an arrow with text at its tail and a rectangle, one selected (dashed outline), and the Edit key window open |
| `fatigue-report` | Reliability | Steel fatigue, Basquin lognormal fit: S–N plot with B10 and median curves and the lower bound, run-outs marked, and the residual probability plot |
| `reference-lines-window` | Working with plots | A control chart with a line at the mean and a shaded specification band, its Reference lines window open beside it |
| `row-colours-in-plot` | Row states | Clinical visits, rows coloured and marked by Arm, the bivariate plot with its "Arm = Active" and "Arm = Placebo" key |
| `dialog-value-labels` | Editing the data grid | Clinical visits, the Value labels dialog for Arm with 0 = Placebo and 1 = Active |
| `heading-charts` | Editing the data grid | Clinical visits grid with Column charts on: level bars, a histogram and the date line |
| `project-folders` | Documents, files and projects | Plant growth with an empty table added to its project: the sidebar tree with a folder "Week 1" holding the empty table and a report, and Plant growth's other reports listed under it |
| `window-overview` | A tour of the window | Main window, Longley sample, Side by side with one report |
| `welcome-page` | A tour of the window | Window with nothing open: start page with recent projects, the task cards and a tip |
| `analysis-dialog` | Using analysis dialogs | Linear model dialog, Longley: response and two predictors chosen, the summary line and Run, Cancel and Reset at its foot |
| `first-analysis-brushing` | Your first analysis | Norris linear model, one residual selected and ringed in both plots and the grid |
| `data-grid` | Editing the data grid | Cell being edited, one column selected, a column's right-click menu open |
| `import-database` | Data from databases and the web | File ▸ Import from database…, an SQLite file with its tables listed and one picked |
| `import-web` | Data from databases and the web | File ▸ Import from the web…, an address, an Authorization header marked secret, the summary line |
| `imputation-report` | Missing values and imputation | Air quality sample: EM imputation report with the pattern map and the scatter plot of imputed values |
| `report-options-menu` | Reading a report | Linear model report with ⋯ Options open |
| `response-explorer` | Response Explorer and 3-D profiler | Catalyst factorial explorer, one reference line mid-drag |
| `distribution-report` | Describe | Michelson: histogram, box plot and normal quantile plot |
| `distribution-identification` | Describe | Seal strength: goodness-of-fit table and the Weibull-with-threshold quantile plot |
| `capability-non-normal` | Quality | Seal strength capability with a Weibull-with-threshold fit: indices, histogram with fitted density, quantile plot |
| `pareto-chart` | Quality | Solder defects: Pareto chart of causes by line, cumulative line and Other bars |
| `cause-effect-diagram` | Quality | Solder defects fishbone diagram, one cause clicked and its rows selected |
| `space-filling-dialog` | Design | Design ▸ Latin hypercube (maximin)…: three factors, 25 runs, the criteria in the preview |
| `multi-vari-chart` | Quality | Shaft diameters multi-vari chart by batch and shaft, ranges view below |
| `linear-model-report` | Model: linear models | Longley report, Effect Pareto and estimates expanded |
| `best-subsets` | Model: linear models | Hald cement best subsets: models table and the BIC-by-size plot |
| `screening-volcano` | Model: linear models | Transcript counts screen: volcano plot of Treatment with the discoveries named |
| `dunnett-comparisons` | Compare | Blood counts: comparisons with the control and the interval plot |
| `best-comparisons` | Compare | Paint drying times: comparisons with the best and the interval plot |
| `crossover-equivalence` | Compare | Bioequivalence crossover: effects, equivalence table and subject profiles |
| `distribution-calculator` | Describe | Distribution calculator: Weibull density with the region between a and b hatched |
| `decision-tree-diagram` | Model: decision trees | Loan applications tree, one node clicked and its rows selected |
| `forest-oob-error` | Model: random forests and boosted trees | Loan applications forest: out-of-bag error by trees, importance below |
| `pls-report` | Model: partial least squares | Fuel spectra: factors table, PRESS plot and score plot |
| `svm-tuning` | Model: support vector machines | Pump sensors radial SVM: tuning table and error-by-cost plot |
| `mars-report` | Model: adaptive regression splines | Fuel consumption degree-2 adaptive splines: basis functions, GCV by number of terms, Response Explorer |
| `neural-network-report` | Model: neural networks | Polymer batches strength network: network diagram, penalty tuning plot, Response Explorer |
| `gaussian-process-report` | Model: Gaussian processes | Reaction yield Gaussian process: length scales, actual by predicted, contour maps |
| `sequential-optimization` | Model: Gaussian processes | Reaction yield proposals: best run, proposed runs, progress by round |
| `model-search` | Model: model search | Polymer batches search of tensile strength: ranking table and held-out RMSE by model |
| `model-comparison` | Model: comparing models | Loan applications comparison: ROC curves and calibration plot |
| `several-responses` | Model: several responses | Coating formulation: explorer, goals and simulation |
| `several-responses-stepwise` | Model: several responses | Coating formulation, terms selected stepwise: the Term Selection table |
| `dialog-several-responses-stepwise` | Model: several responses | Several responses dialog with Select terms stepwise for each response |
| `dialog-stepwise-several` | Linear models | Stepwise dialog with Several responses ticked |
| `correlations-heat-map` | Multivariate | Flower measurements correlations: Pearson matrix and heat map with its key |
| `scatterplot-matrix` | Multivariate | Flower measurements scatterplot matrix by variety: ellipses, diagonal histograms, a box selection ringed everywhere |
| `outlier-distances` | Multivariate | Flower measurements jackknifed distances by row: limit line, rows above it marked |
| `manova` | Multivariate | Flower measurements MANOVA: multivariate tests and the canonical plot coloured by variety |
| `item-analysis` | Multivariate | Course evaluation item analysis: reliability, item statistics with reversed items marked, inter-item heat map |
| `factor-analysis` | Multivariate | Ability tests promax solution: rotated pattern loadings, factor correlations and the loading plot |
| `discriminant-canonical-plot` | Multivariate | Honey origins linear discriminant analysis: canonical plot with the origins and their means, confusion matrices by role |
| `gaussian-mixture` | Multivariate | Cell populations Gaussian mixture: BIC plot by structure and the cluster plot with component ellipses |
| `latent-class` | Multivariate | Symptom checklist latent class analysis: model-selection table and the response-probability profile plot |
| `dimension-reduction` | Multivariate | Expression profiles t-SNE map coloured by cell type, with a box selection ringed |
| `shepard-diagram` | Multivariate | Town distances metric scaling: map and Shepard diagram |
| `functional-data` | Multivariate | Exotherm profiles functional data: summary table and smoothed curves |
| `design-dialog` | Design | Central composite dialog with its live preview |
| `extreme-vertices-dialog` | Design | Mixture dialog with Extreme vertices chosen: four components with bounds and the preview |
| `robust-design-dialog` | Design | Robust design dialog: L8 inner array of four control factors, L4 outer array of two noise factors |
| `signal-to-noise` | Design | Moulding shrinkage signal-to-noise report: summary, effects plot and ranking |
| `split-plot-dialog` | Design | D-optimal design dialog with one factor marked Hard in the Changes column, whole plots and the variance ratio set, and the split-plot notes |
| `design-constraints` | Design | D-optimal design dialog with two constraints in the Constraints box and the notes listing them |
| `design-augment` | Design | Augment design dialog folding over a screening design, with its notes |
| `design-evaluation` | Design | Design comparison sample: fraction-of-design-space curves of both designs with the key, and the comparison table |
| `factorial-analysis` | Design | Filtration rate factorial analysis: half-normal plot of effects with ME and SME, and the cube plot |
| `bayes-estimation` | Bayesian analysis | Response rates by site: posterior of each arm's response proportion and their difference, probabilities with the region of practical equivalence |
| `bayes-factor-test` | Bayesian analysis | Stroop reaction times two-sample Bayes factor: data, Bayes factor table, prior and posterior of δ |
| `bayes-anova` | Bayesian analysis | Stroop reaction times Bayesian ANOVA: models table and inclusion probabilities |
| `bayes-model` | Bayesian analysis | Eight schools hierarchical model: model, priors and posterior summary with R̂ and ESS |
| `bayes-model-trace` | Bayesian analysis | Eight schools: trace plot of the between-school SD, one pane per chain, and the rank histograms |
| `bayes-model-dialog` | Bayesian analysis | Model ▸ Bayesian ▸ Bayesian regression or hierarchical model…, opened on the Eight schools sample |
| `gauge-nested` | Quality | Weld pull test nested gauge R&R: variance components, the nested ANOVA and the gauge run chart |
| `gauge-expanded` | Quality | Bore gauges expanded study: variance components, intervals and the gauge run chart |
| `gauge-linearity` | Quality | Gauge linearity report: bias plot with the fitted line, band and zero line, and the summary |
| `gauge-repeatability` | Quality | Gauge repeatability report: Cg and Cgk table and the run chart |
| `intraclass-correlation` | Quality | Gauge study intraclass correlation report: evaluation table and Shrout & Fleiss forms |
| `attribute-agreement` | Quality | Visual inspection grade report: within-appraiser and against-standard tables and the agreement plot |
| `attribute-gauge` | Quality | Go/no-go gauge report: bias and repeatability table and the gauge performance curve |
| `control-chart-phases` | Quality | Fill weights X̄ chart with Filler setting as the phase: two sets of limits, boundary and phase names |
| `between-within-capability` | Quality | Coating batches between/within report: indices, variance components, values by batch |
| `attribute-capability` | Quality | Supplier lots binomial capability: summary, p chart, cumulative percent defective |
| `acceptance-sampling` | Quality | Supplier lots acceptance sampling: plan table and OC curves with the risk points |
| `control-chart-three-way` | Quality | Coil thickness three-way chart: subgroup means as individuals, their moving range, within-subgroup range chart |
| `control-chart-zone` | Quality | Coil thickness zone chart of Line speed: cumulative scores above the points, signals as diamonds |
| `attribute-laney` | Quality | Ward safety p chart and p′ chart of readmissions: narrow limits flagging most weeks, widened p′ limits |
| `capability-summary` | Quality | Tablet hardness capability summary page: indices, control charts, last subgroups, histogram, probability plot |
| `multivariate-chart` | Quality | Bottle filling T² chart of subgroups: reference and monitoring limits, subgroup 23 flagged, contribution chart |
| `analysis-of-means` | Quality | Heat treatment hardness by furnace and shift: furnace chart with furnace 3 flagged, interaction chart |
| `categorical-logistic-report` | Model: generalized, mixed and penalized models | Service ratings ordinal report: odds ratios, parallel-lines test and the predicted-probability curves against Wait |
| `dose-response` | Model: dose–response, method comparison and shelf life | Rotenone probit analysis: curve with the ED50 drop line and fiducial interval, effective-dose table |
| `method-comparison` | Model: dose–response, method comparison and shelf life | Glucose methods scatter plot with identity, Deming and Passing–Bablok lines, and the difference plot |
| `shelf-life` | Model: dose–response, method comparison and shelf life | Tablet stability plot: three batches with parallel lines, the lower confidence bound of batch C meeting 95% |
| `nonlinear-regression` | Model: nonlinear regression | Seedling growth logistic fit: estimates with Wald and profile intervals, fitted curve with confidence band and prediction limits |
| `nonlinear-global-fit` | Model: nonlinear regression | Compound potency global fit: shared parameters, LogEC50 per compound, model comparison |
| `survival-curves` | Reliability | Leukemia remission Kaplan–Meier curves with bands |
| `life-distributions` | Reliability | Seal inspections: model comparison by supplier and the Weibull probability plot with percentage axis |
| `life-distributions-dialog` | Reliability | Life distributions dialog with the start/end layout, frequency column and families chosen |
| `life-stress` | Reliability | Device-A: the life–stress plot with percentile lines down to 10 °C, and the probability plot by temperature |
| `degradation` | Reliability | Laser degradation: the path plot with fitted lines and the 10% threshold, and the Weibull probability plot of the pseudo failure times |
| `proportional-hazards` | Reliability | Leukemia with covariates: hazard ratios, the proportional-hazards check and survival curves by treatment |
| `survival-regression` | Reliability | Capacitor life test: distribution comparison, time ratios and fitted survival by temperature |
| `reliability-growth` | Reliability | Prototype failures growth report: estimates by phase, MTBF plot with a band per phase |
| `repairable-systems` | Reliability | Pump repairs: mean cumulative functions of the two designs with limits, difference plot |
| `competing-risks` | Reliability | Pump failure modes: overlaid cumulative incidence curves by site with limits, and the series-system plot with each mode's fit |
| `warranty-returns` | Reliability | Monthly warranty returns: observed and forecast returns per month with the prediction band |
| `text-exploration` | Multivariate: text analysis | Espresso machine reviews text exploration: the word cloud with one word clicked and its documents selected, and the document map coloured by cluster |
| `topic-model` | Multivariate: text analysis | Espresso machine reviews topic model: the top terms by topic table and the documents-by-topic bars |
| `sentiment-scores` | Multivariate: text analysis | Espresso machine reviews sentiment: the score distribution with the neutral band and the sentiment words table |
| `time-series-description` | Series and Signals | Monthly electricity demand series description: time series plot, ACF with spikes at 12 and 24, and the unit-root tests table |
| `time-series-decomposition` | Series and Signals | Monthly electricity demand multiplicative decomposition: the four panels and the seasonal indices table |
| `time-series-trend` | Series and Signals | Product adoption logistic trend: data, fitted S-curve and forecasts, and the coefficients table |
| `time-series-cross-correlation` | Series and Signals | Advertising and sales prewhitened cross-correlation: needle plot with the peak at lag 2, and the note on reading the lags |
| `exponential-smoothing` | Series and Signals | Hotel occupancy: the forecast plot with 80% and 95% limits under the model selection table |
| `arima` | Series and Signals | Reactor temperature: model search, coefficients and the forecast plot for the next 24 hours |
| `arima-inputs` | Series and Signals | Kiln fuel feed: Transfer Functions table, impulse response plot and the start of the forecast |
| `signal-spectrum` | Series and Signals: signal processing | Test bench signals Fourier spectrum: Welch summary and the spectrograms of runs A and B with the 50 and 120 Hz bands and the colour bar |
| `signal-filter` | Series and Signals: signal processing | Test bench signals 80 Hz low-pass filter: frequency response and the spectrum before and after |
| `digital-filter-dialog` | Series and Signals: signal processing | Series and Signals ▸ Digital filter…: signal, x and group columns, filter design, band, cut-off and order |
| `signal-correlation` | Series and Signals: signal processing | Accelerometer against microphone cross-correlation: peaks at a lag of 5 samples in each run |
| `signal-wavelets` | Series and Signals: signal processing | Strain wavelet analysis: multilevel decomposition of run A and the energy-by-level table |
| `image-spectrum` | Series and Signals: image processing | Machined surface scan image spectrum: the log-magnitude spectrum with the two tool-mark pairs ringed and numbered |
| `image-radial-profiles` | Series and Signals: image processing | Machined surface scan radial profiles at 0°, 90° and through each dominant frequency |
| `image-filter` | Series and Signals: image processing | Machined surface scan notch filter: the filtered image with the pit visible, and the part removed |
| `image-filter-dialog` | Series and Signals: image processing | Series and Signals ▸ Image frequency filter…: value, x, y and group columns, the image size, band filter and notches |
| `image-registration` | Series and Signals: image processing | Blurred and shifted scene registration: the shift found and the reference image |
| `reliability-test-plan` | Reliability | Test plans dialog (Demonstration tab) and the report with plans table and passing-probability curve |
| `multiway-table` | Describe | Multi-centre trial multi-way table: centre and treatment by outcome with subtotals, counts, row % and mean score change |
| `stratified-tables` | Compare | Multi-centre trial stratified analysis: tests by stratum, CMH tests, common odds ratio |
| `bootstrap-report` | Resampling | Repair times bootstrap report: estimates table and the bootstrap distribution of the mean with its interval |
| `permutation-test-report` | Resampling | Repair times permutation test of median repair time by crew, with the permutation distribution |
| `scatter-3d` | 3-D scatter, contour and surface plots | Hillside survey 3-D scatter of elevation by easting and northing: soil type colours and shapes, size by soil depth, drop lines |
| `data-surface` | 3-D scatter, contour and surface plots | Hillside survey filled contour plot of elevation with its colour bar and observations, and the filled 3-D surface with the floor contour map |
| `contour-filled` | Working with plots | Reaction yield response surface: filled contour plot with its colour bar, level lines and values, and the stationary point |
| `mixture-ternary` | Model: linear models | Yarn elongation ternary contour plot of the fitted mixture model, filled, with its colour bar and the blends as points |
| `graph-canvas` | Graph canvas | Columns on X, Y, colour and panel shelves; points plus a smoother |
| `categorical-logistic-dialog` | Model: generalized, mixed and penalized models | Ordinal or nominal logistic regression dialog on Service ratings: Rating as the response, the category order list, Wait and Channel chosen |
| `factorial-repeated-measures` | Compare | The Reaction times by task and cue report: univariate tests with GG/HF columns, the sphericity table and the simple effects of Task within each age group |
| `nested-comparison` | Compare | The Dendrite lengths per mouse report: group means, tests, variance components and nested ANOVA |
| `export-reports-dialog` | Saving plots and results | The Export reports dialog opened from a Plant growth report's Export… button, with Word document chosen and Letter paper |
| `view-windows` | Documents, files and projects | Longley in the main window, its data grid and a bivariate report each in a window of its own (drawn inside the main window, as the tests draw them), a box selection highlighted in both |
| `project-tree` | Documents, files and projects | Plant growth project with "Summary by Fertilizer" and "Selected rows" under the source in the sidebar, its tabs grouped under the project's name, and the Source changed banner above the summary's grid |
| `keep-in-project` | Documents, files and projects | Summarize by group on Plant growth, Fertilizer as the group, Height summarized, Keep in this project ticked and the summary line naming the new table and project |
| `view-window-placeholder` | Documents, files and projects | A dataset whose data grid is in its own window: the placeholder panel with Show window and Bring back here, and the window symbol in the sidebar |
| `dashboard-overview` | Dashboards | Greenhouse trial sample on its dashboard: toolbar, Greenhouse and Fertilizer filters, heading, note and four report tiles, the climate tiles saying "not filtered by Fertilizer" |
| `dashboard-move-tile` | Dashboards | A report tile dragged by its strip: column guides and the outline where it will land |
| `dashboard-filter` | Dashboards | The Greenhouse filter's menu open with its level check boxes, tile strips showing the rows kept |
| `dashboard-switcher` | Dashboards | Greenhouse trial dashboard with the Humidity picker on Temperature, the distribution tile following it and the scatter-plot tile saying "not switched: Humidity" |
| `dashboard-web-viewer` | Dashboards | The Greenhouse dashboard in the viewer the published web app runs: top bar (title, "changes are not saved"), filters and tiles, no editing controls |
| `publish-build-viewer` | Dashboards | Publish as web app… before the viewer is built: the Build the web viewer window with the command, Copy and Build it now |
| `join-dialog` | Table operations | Data ▸ Merge ▸ Join… with keys chosen and the preview |
| `area-under-curve-report` | Describe | Plasma concentration sample: the Area under the Curve table by formulation, the peaks table and formulation A's curve with the baseline line |
| `many-tests-report` | Compare | Gene screen sample: the Two groups, many variables report with the Tests by Variable table (smallest p-value first) and the volcano plot, significant genes named |
| `adjust-p-values-report` | Compare | Trial endpoints sample: the Multiple Testing Summary, Adjusted p-values table and ordered p-values against the Benjamini–Hochberg line |
| `guide-menu-compare` | Quick Guide: Compare | The Compare menu open |
| `report-filter-switcher` | Reading a report | Distribution report with a row filter chip and the column picker line |
| `graph-distribution-layers` | Graph canvas | Seal strength sample: peel strength histogram, dot plot, normal probability plot, and a Weibull probability plot on a log horizontal scale |
| `graph-brackets` | Graph canvas | Reading trial sample: scores by programme as a swarm with Tukey–Kramer significance brackets |
| `graph-estimation` | Graph canvas | Reading trial sample: Cumming estimation plot of the four programmes against Control |
| `graph-flows` | Graph canvas | Parcel flows sample: Sankey diagram of origin, hub and destination sized by parcels, Hub A clicked |
| `graph-cluster-map` | Graph canvas | Regional climate sample: clustered heat map with row and column dendrograms |
| `graph-round-forms` | Graph canvas | Harbour weather wind rose and donut chart (one slice selected), Soil texture ternary plot, Household budget waterfall chart |
| `script-library` | Scripting and recording | Sidebar Scripts section: folder scripts, a project script, right-click menu with Keep in project |
| `batch-run` | Scripting and recording | Run script over a folder window after a run: folder, pattern, output with one failed file |

## Automated GUI tests

The application's screens are tested automatically, without a window or a
graphics card, on every pull request. Each test starts the whole app,
clicks menus, buttons and drop-down lists, types into fields and drags
axes the way a person would, then checks what is on screen and what the app
did. A change that breaks a dialog or a plot interaction fails CI before it
reaches anyone.

### Running them

The GUI tests run with the rest of the test suite:

```sh
cargo test --workspace --no-fail-fast
```

To run only the GUI tests:

```sh
cargo test -p app-gui gui_tests
```

### Pictures of the screens

Set `ANOVACRAFT_GUI_SHOTS` to a folder to also save a picture of the window
at each point a test marks:

```sh
ANOVACRAFT_GUI_SHOTS=gui-shots cargo test -p app-gui gui_tests
```

Rendering the pictures needs a graphics driver: a Mac uses its own (Metal),
and on a Linux machine without a graphics card, Mesa's software Vulkan
driver (`mesa-vulkan-drivers` on Debian and Ubuntu) works. The pictures
are a record for reviewers, not a comparison: a test fails only on what it checks, never on
a pixel difference, so fonts or graphics drivers that draw slightly
differently on another computer do not fail it.

For design reviews, `feedback_tour` (in `gui_tests/tour.rs`, run on demand)
saves 60 numbered pictures of the most used screens: the welcome page and
data grid, every top menu open, the common dialogs, a report from each main
sample, the graph canvas, scripts and settings, and a few dark-theme views:

```sh
ANOVACRAFT_GUI_SHOTS=feedback-shots cargo test -p app-gui feedback_tour -- --ignored
```

### Where they live

The tests are in `crates/app-gui/src/app/gui_tests/`, grouped by area
(dialogs, documents, plots, scripts). They start the app with the default
settings, so the settings saved on your computer never change what they
see, and they never write to it.

Each test finds controls by the text a screen reader would read: a button's
label, a window's title, a check box's text. Controls drawn without text,
such as the axis rulers, are given an accessible name so tests (and screen
readers) can find them.

### Adding tests

A pull request that adds or changes a dialog, a report interaction or a
plot gesture adds a GUI test for it, and a fix for a GUI bug adds a test
that fails without the fix.

## Web build

AnovaCraft runs in a web browser through WebAssembly as the **read-only
dashboard viewer** that Publish as web app…
puts in its folder. This page explains how that build is made and checked,
and what differs from the desktop app at run time.

### Building the viewer

```sh
./scripts/build-web-viewer.sh           # writes target/web-viewer
./scripts/build-web-viewer.sh --serve   # also publishes the Greenhouse sample
                                        # and serves it on http://localhost:8000
```

The script needs the `wasm32-unknown-unknown` target (it adds it with rustup
if missing) and `wasm-bindgen-cli` at exactly the version of the
`wasm-bindgen` crate in `Cargo.lock` (it installs that with `cargo install`
when it is missing). It builds `app-gui` in release mode for that target,
runs `wasm-bindgen --target web`, and shrinks the result with `wasm-opt`
when binaryen is installed. The two files it writes,
`anovacraft_viewer.js` and `anovacraft_viewer_bg.wasm`, are what Publish as
web app… copies. The app looks for them in, in order:

1. the folder named by the `ANOVACRAFT_WEB_VIEWER` environment variable;
2. a `web-viewer` folder beside the program, one level up (so a Cargo build
   in `target/release` finds `target/web-viewer`), or in a macOS app
   bundle's `Resources`.

Without them it writes nothing and opens the "Build the web viewer" window
(`app/viewer_build.rs`), which shows this command and, in a source
checkout, runs it in the background and then carries on with the publish.

`app-gui --viewer project.acraft` shows a project the way the viewer does,
in a desktop window; `app-gui --publish-sample folder` writes the published
Greenhouse sample to a folder (what `--serve` serves).

### Checking it

```sh
./scripts/check-web-build.sh
```

runs `cargo check` for `wasm32-unknown-unknown` on the engine crates,
`app-model` and `stat-script` without their default features, `stat-viz`
with its `egui` feature, and `app-gui`. It is not part of CI; run it after
changing dependencies or adding platform-specific code. The viewer's logic
is tested natively (`gui_tests/viewer.rs` drives it with `egui_kittest`);
the only browser-only check is loading a published folder in a browser.

### How the app builds for the web

| Crate | For `wasm32-unknown-unknown` |
|---|---|
| All engine crates (`stat-core` … `stat-timeseries`, `stat-strd`) | Compile unchanged. |
| `stat-viz`, with and without its `egui` feature | Compiles unchanged. |
| `app-model` | Without its default features: no database drivers or web access (its `sqlite`, `postgres`, `mysql`, `web` and `odbc` features). |
| `stat-script` | Without its `external-data` feature (which passes app-model's default features on). |
| `app-gui` | Compiles; on this target it starts as the viewer. |

What `app-gui` does differently on this target:

- **Dependencies:** the database drivers and web import, the native file
  dialogs (`rfd`) and the clipboard (`arboard`) are native-only
  dependencies. `web_shims` stands in for the dialogs (always cancelled)
  and the clipboard (always empty); the viewer offers no command that uses
  them.
- **Entry point:** `main` calls `web_main::start`, which fetches the
  project file next to the page (`project.acraft`, or the one a
  `?project=` query parameter names), reads it with `load_project_file`
  and starts `eframe::WebRunner` on the page's `anovacraft_canvas`,
  drawing with WebGL. Errors and panics are shown in the page's
  `anovacraft_status` line.
- **Viewer mode:** `StatsApp::new_viewer` opens the project and shows its
  dashboards full window (`app/viewer.rs`): no menus, sidebar, tabs or
  windows of their own. Its dashboards are read-only (`OpenDashboard::read_only`):
  the toolbar keeps the name and Undo/Redo; tile strips have no move
  handle or buttons; there is no resize grip; and only filter, picker and
  tile display edits are applied (`DashboardOutcome::keep_view_changes`).
- **Settings:** the viewer uses the default settings and saves none.

#### At run time

- **Threads:** the engine's `rayon` loops (random forests, bootstrap and
  permutation loops) run on one thread, which rayon does by itself when
  threads are unavailable. Results are the same (each tree and replicate
  has its own seed); they take longer. The script window, batch runs and
  external data, which start threads, are not part of the viewer.
- **Clock:** `std::time::SystemTime::now` and `Instant::now` panic on this
  target. Nothing the viewer draws calls them (the engine uses them only in
  tests); `html_export::export_date` must not be called there.
- **Files:** `std::fs` compiles but every call fails; the project comes from
  the page.
- **Graphics:** the web build uses eframe's WebGL (glow) renderer, which
  every current browser has; the desktop app keeps wgpu.

#### Size

The build uses the `wasm-release` profile (optimized for size, with
link-time optimization, stripped): about 20 MB of WebAssembly before
`wasm-opt`, 7 MB gzipped, which web servers usually apply on their own. It links every
analysis, since `run_report` dispatches to all of them, plus egui and the
bundled fonts. Leaving out analyses a dashboard does not use would need
`run_report` split by feature; that is not worth doing for now.

## AnovaCraft — Roadmap

Planned work, in the order it should be built. `FEATURES.md` says what exists;
this file says what comes next and how. When an item ships, move it into
`FEATURES.md` and delete it here.

All names below are standard statistical terms or our own. The gap analysis
this plan came from used a commercial product's platform names; none of them
are carried over (CLAUDE.md, clean-room rules). Every method is built from the
published reference cited with it. A second gap check (2026-10-14) against
another commercial package's public feature list added §3.8–3.9, 4.8–4.9,
5.7–5.10, 6.4–6.10, the §7 additions, 12.7–12.8 and §13–16, again under
standard statistical names only. A third (2026-10-18) added §17–19, and a fourth the same day added §20–22
(Bayesian analysis, multiple imputation, assay curve templates). A fifth
(2026-10-19) added §23–30.

**Item IDs** match the section numbers here.

**Sizes:** S = one session · M = two or three sessions · L = a multi-session
phase, delivered in parts.

---

### Corrections to the source gap analysis

The analysis this plan was drawn from got some facts about the code wrong.
The plan below follows the code.

| Claim | Actually |
|---|---|
| Data is backed by Polars; reshaping can use Polars' melt/pivot. | Columns are `Vec<f64>` / `Vec<String>` (`stat_data::ColumnData`); Polars is not a dependency. Table operations are written directly on `DataTable`. |
| A Rhai/TOML headless engine (`stat-script`) exists. | It did not; Rhai was chosen and the engine and headless runner now exist, with the Script window and action recording (`FEATURES.md`, Scripting). F1 is complete. |
| `stat-predictive` has decision trees ready to hook up. | It contained only the elastic net. Trees were built as new work (`FEATURES.md`, Model). |
| `stat-functional` and `stat-text` exist. | Neither existed; both are new crates, and both have shipped (§8 `stat-functional`, §9 `stat-text`). |

---

### 1. Data management

Every §1 item has shipped (see `FEATURES.md`, Data and documents).

---

### 3. Predictive modeling

Every §3 item has shipped (see `FEATURES.md`, Model): decision trees, model
comparison, random forests, boosted trees, neural networks, partial least
squares, support vector machines, adaptive regression splines and the
automated model search.

---

### 4. Multivariate

All §4 items have shipped (see the Multivariate menu in FEATURES.md).

---

### 5. Reliability and survival

All §5 items have shipped (see the Reliability menu in FEATURES.md).

---

### 6. Quality

All §6 items have shipped (see the Quality menu in FEATURES.md).
Measurement systems analysis is §13.

---

### 7. Time series

Shipped: series description (plot, ACF/PACF, Ljung–Box, unit-root tests,
spectrum), decomposition, trend models, cross-correlation, exponential
smoothing and seasonal ARIMA with the automatic order search and
transfer-function inputs (see FEATURES.md, Time Series). All of §7 has
shipped.

---

### 8. Functional data

Every §8 item has shipped (see `FEATURES.md`, Multivariate ▸ Functional data).

---

### 9. Text analysis

All §9 items have shipped in the new crate `stat-text` (see Multivariate ▸
Text in FEATURES.md): tokenization, stop words, Porter stemming, phrases,
term tables, word-frequency chart and word cloud; TF-IDF, latent semantic
analysis and document clusters; LDA topic models; lexicon sentiment with
negation (the VADER lexicon, MIT licence).

---

### 10. Dashboards and sharing (L)

Dashboards (reports and notes from a project's tables on a grid, with
shared local filters and shared column pickers, saved in the project),
HTML export of dashboards and export of reports as PDF, Word, PowerPoint,
HTML, Markdown and Excel have shipped (see FEATURES.md, Data and documents).
The read-only web dashboard viewer (Publish as web app…; the manual's "Web
build" page) has shipped too, so all of §10 is done.

---

### 11. Automation (needs F1)

All of §11 has shipped (script library and batch runner; see FEATURES.md,
Scripting).

---

### 12. Design of experiments

All of §12 has shipped (see Design in FEATURES.md).

---

### 13. Measurement systems analysis

All §13 items have shipped (see the Quality menu in FEATURES.md).

---

### 14. Regression and ANOVA

All of §14 has shipped (see the Model and Compare menus in FEATURES.md).

---

### 15. Tests, intervals and tables

All §15 items have shipped (see the Describe and Compare menus in FEATURES.md).

---

### 16. Graphs

All §16 items have shipped (see Visualize in FEATURES.md).

---

### 17. Tests, comparisons and effect sizes

Third gap check (2026-10-18) against two curve-fitting and publication-graphing
packages' public feature lists; standard names only. Most items are small and
sit on existing engines in `stat-inference`, `stat-linear` and
`stat-reliability`.

All of §17 has shipped (see the Compare and Describe menus in FEATURES.md).

---

### 18. Curve fitting

All of §18 has shipped (see Nonlinear regression under Model in FEATURES.md).

---

### 19. Graphs

All of §19 has shipped (see Visualize and Plots in FEATURES.md).

---

### 20. Bayesian analysis

All of §20 has shipped (see Bayesian under Compare and Model in FEATURES.md).

---

### 21. Multiple imputation

All of §21 has shipped (see Multiple imputation under Data in FEATURES.md).

---

### 22. Assay curve templates

The 37 templates (immunoassay standard curves, receptor binding, enzyme
kinetics, pharmacological dose–response, growth/decay/survival) have
shipped (see FEATURES.md, Model ▸ Curve templates). What is left:

| ID | Item | Size | Depends | Reference |
|---|---|---|---|---|
| 22.1 | User-saved templates: save a dialog's model, bounds, sharing and weighting under a name and list it beside the built-in templates | S | — | — |


---

### Fifth gap check (2026-10-19)

Drawn from a commercial package's public release notes for its latest
version (assessment kept in the project's working files).
Standard statistical names only; none of the product's platform, report or
option names are carried over, and every method is built from the reference
cited with it. Structural equation models were left out (2026-10-19).
Verification follows the usual rule: an independent reference for every
routine (published examples, or R packages installable with apt as
`r-cran-*`, or SciPy/statsmodels/scikit-learn), recorded in
`docs/validation.md`.

---

### 23. Quality and process monitoring

All of §23 has shipped (see Quality in FEATURES.md).

---

### 24. Models and comparisons

All of §24 has shipped (see Model and Compare in FEATURES.md).

---

### 26. Sequential optimization of experiments

Both §26 items have shipped (see `FEATURES.md`): Gaussian-process
regression (Model ▸ Gaussian process, 26.1) and next-run proposals by
expected improvement or an upper confidence bound (Design ▸ This table ▸
Propose next runs, 26.2), with the Reaction yield (simulated) sample.

---

### 28. Spectra: baseline correction and peak fitting

All of §28 has shipped (see Series and Signals ▸ Signals and spectra in FEATURES.md).

---

### 29. Smaller additions

All of §29 has shipped (see FEATURES.md).

---

### 30. Signal processing

All of §30 has shipped, in the new `stat-signal` crate shared with §28 (see
Series and Signals ▸ Signals and spectra in FEATURES.md).

---

### Suggested order

Reassessed 2026-10-14 after the second gap check. Within each step, items
are listed in build order. §3.1, 3.3, 3.4, 3.6 and 3.7 shipped in PR #14;
§14.4 best subsets, Dunnett's half of §14.7, §15.1 tolerance intervals,
§15.2 outlier tests and §15.4 median and runs tests shipped after it, then
§6.6 distribution identification. On 2026-10-15 every remaining small (S)
item shipped together: §4.1–4.3, 4.8, 5.4, 5.8, 5.9, 11.1, 11.2, 12.5,
12.8, 13.1, 13.3, 13.5, 14.3, 14.5–14.7, 15.3, 15.5, 15.8 and 15.9.

1. **Predictive models:** all of §3 has shipped, ending with §3.5 neural
   networks and §3.9 automated model search.
2. **Regression, tests and tables:** the everyday methods a statistics
   package is expected to have, most of them small and built on engines
   that already exist. All of §14 and §15 has shipped.
3. **Multivariate:** MANOVA, factor analysis, discriminant analysis,
   Gaussian mixture clustering, latent class analysis and dimension
   reduction (§4.4–4.7, 4.9) have all shipped.
4. **Quality and measurement:** all of §6 has shipped, with §16.1
   histogram, dot and probability plot layers, and all of §13 measurement
   systems analysis.
5. **Reliability and survival:** all of §5 has shipped (proportional hazards,
   survival regression, competing risks, degradation, life–stress, life
   distributions and warranty analysis).
6. **Design of experiments:** all of §12 has shipped, including §12.2 split-plot designs.
7. **Time series and automation:** all of §7 has shipped.
8. **Dashboards:** all of §10 has shipped: dashboards, HTML export, shared
   column pickers and the read-only web dashboard viewer (§1.4 external data, §1.5 imputation and §16.2 3-D and
   contour plots have shipped).
9. **Functional data and text:** both have shipped (§8 functional data, §9 text analysis).
10. **Third gap check (2026-10-18):** §17, §18 and §19 have all shipped,
    built in three parallel threads (signal processing was left out at
    the time).
11. **Fourth gap check (2026-10-18):** §20 Bayesian analysis, §21 multiple
    imputation and §22 assay curve templates, built in three parallel
    threads. §20 and §21 have shipped, and §22 apart from user-saved
    templates (22.1).
12. **Fifth gap check (2026-10-19):** §23–30, in parallel threads, one per
    batch, smallest first: (a) quick quality and describe options 23.2,
    23.3, 23.4, 29.1, 29.3, 29.6; (b) models and comparisons 24.1–24.7;
    (c) DOE options 29.7–29.12; (d) PCA-model chart and monitoring limits
    23.1, 23.5; (e) mixed-model additions §27; (f) distances, censored
    distributions, PK and fatigue 29.2, 29.4, 29.5, 24.8; (g) spectra §28
    with smoothing and x–y calculus 30.1, 30.6 (creates `stat-signal`);
    (h) causal treatment effects §25; (i) sequential optimization §26;
    (j) script window, notebook and data-table items 29.13–29.15; (k)
    Fourier analysis, filters, convolution and wavelets 30.2–30.5 (shipped) in
    `stat-signal` (crate skeleton conflicts with (g) are plain unions).

Why this order: step 2 moves ahead of multivariate because tolerance
intervals, outlier tests, best subsets, nonlinear and ordinal regression
are used far more often than most of §4, and most are one session each.
Distribution identification leads step 4 because capability, life
distributions and probability plots all share it. Steps 3–9 otherwise
keep the order already agreed.

### Not planned

- Bridges to external Python or R interpreters (decided 2026-10-14).
- Bindings to external C/C++ machine-learning libraries; equivalent methods
  are implemented from their published algorithms (§3).
- Integration with proprietary analytics servers or file formats beyond
  documented open ones.
- Any compatibility with another product's scripting language; automation is
  Rhai only.
- Structural equation models (left out of the fifth gap check, 2026-10-19).

## Pending menu work

Methods whose calculations are built, tested and available to scripts, but
which have no menu entry, dialog or report yet. When a method is built ahead
of its menus (for example while other pull requests are under review), list
it here with the menu, dialog inputs, report sections and sample data it
should get; when those ship, mark it Done in ROADMAP.md and the tracker and
remove it from this file.

None at present. The last batch (tolerance intervals, outlier tests,
Dunnett's comparisons with a control, the median and runs tests, and best
subsets) got its menus after PRs #14, #16 and #18 merged.

## Pull request template

#### What changes for the user

Before:

After:

#### How

#### Checklist

- [ ] Tests cover each new or changed statistical routine, against an independent reference
- [ ] `docs/validation.md` updated with each new or changed statistical routine, its reference data and its test (or no statistical routine changed)
- [ ] `FEATURES.md` updated for anything a user can see (or nothing user-visible changed)
- [ ] Reference Manual (`docs/manual`) and, for menu items, the Quick Guide (`docs/guide`) updated for anything a user can see, including changes to features already considered done; changed screens re-rendered with `scripts/capture-manual-shots.sh` or marked and listed in `docs/manual/src/maintaining/screenshots.md` (or nothing user-visible changed)
- [ ] Manual PDFs rebuilt with `scripts/build-manual-pdfs.sh` if either manual changed
- [ ] `ROADMAP.md` item moved to `FEATURES.md` if one shipped

## Continuous integration

- `.github/workflows/ci.yml`: Clippy (`clippy`), Tests (${{ matrix.os }}) (`test`), User manual (`manual`)
- `.github/workflows/release.yml`: macOS app (universal) (`macos`), Windows app (x64) (`windows`), Draft release (`release`)

## Scripts

- `scripts/book-pdf.mjs`
- `scripts/build-knowledge-pack.py`: Build the AnovaCraft knowledge pack for web chat assistants.
- `scripts/build-macos-app.sh`: Build "AnovaCraft.app" for macOS from this workspace.  ./scripts/build-macos-app.sh            # app for this Mac's architecture ./scripts/build-macos-app.sh universal  # one app for Apple silicon and Intel  The app is built with the `dist` profile (thin LTO, one codegen unit: slower to build than --release, faster to run) and written to dist/. It is not code-signed: the first time, allow it under System Settings ▸ Privacy & Security ▸ Open Anyway (or right-click ▸ Open on macOS 14 and earlier). A zip of the app for downloads is written beside it.
- `scripts/build-manual-pdfs.sh`: Build the PDFs of the two manuals that the app carries (Help ▸ Quick guide and Help ▸ Reference manual) into crates/app-gui/assets/manuals/, with the docs stamp the app's tests compare against the Markdown. Run it in every change that touches docs/manual, docs/guide, docs/formulas.md or docs/scripting.md, and commit the three files it writes.  ./scripts/build-manual-pdfs.sh  Needs mdBook 0.4.40 (`mdbook`, or MDBOOK=/path/to/mdbook) and Node with Playwright and its Chromium (npm install -g playwright; npx playwright install chromium). MANUAL_IMAGE_WIDTH and MANUAL_JPEG_QUALITY tune how the
- `scripts/build-notices.py`: Write THIRD-PARTY-NOTICES.txt: the copyright notices and licence texts of everything the application contains that AnovaCraft's authors did not write.
- `scripts/build-web-viewer.sh`: Build the web dashboard viewer: app-gui for WebAssembly, run through wasm-bindgen, into target/web-viewer (anovacraft_viewer.js and anovacraft_viewer_bg.wasm). "Publish dashboard as web app…" copies these two files into the folder it writes; it finds target/web-viewer beside a Cargo build of the app, or the folder named by ANOVACRAFT_WEB_VIEWER.  ./scripts/build-web-viewer.sh            # build target/web-viewer ./scripts/build-web-viewer.sh --serve    # also publish the Greenhouse # sample there and serve it on # http://localhost:8000
- `scripts/build-windows-exe.sh`: Build AnovaCraft for 64-bit Windows from macOS (or Linux) by cross-compiling with the MinGW-w64 toolchain. The result is dist/AnovaCraft-windows-x64/ (AnovaCraft.exe plus any runtime DLLs it needs) and a .zip of that folder (AnovaCraft-<version>-windows-x64.zip), ready to copy to a Windows PC and run — no installer needed.
- `scripts/capture-manual-shots.sh`: Render the pictures of both user manuals (docs/manual and docs/guide) from the running app, at one fixed 1440 × 900 window, into docs/manual/src/images (the Quick Guide's guide-* pictures then move to docs/guide/src/images).  ./scripts/capture-manual-shots.sh                    # every picture ./scripts/capture-manual-shots.sh pareto-chart arima # only these  Needs a GPU driver or a software one (on Linux: apt-get install mesa-vulkan-drivers). The pictures are listed in crates/app-gui/src/app/gui_tests/manual.rs.
- `scripts/check-manual.sh`: Check the two user manuals the way CI does: the Reference Manual (docs/manual) and the Quick Guide (docs/guide).  ./scripts/check-manual.sh              # build and check the book ./scripts/check-manual.sh origin/main  # also warn if user-facing code # changed since that ref without a # manual change  Fails on: a book that does not build, a link to a page that does not exist, a missing image, a picture whose proportions are not a normal window's (or whose size is forced in the page), or a screenshot marker not
- `scripts/check-web-build.sh`: Check that the parts of AnovaCraft that can run in a web browser compile for WebAssembly (wasm32-unknown-unknown). See docs/manual/src/maintaining/web-build.md. app-gui is the web dashboard viewer there; scripts/build-web-viewer.sh builds it.  ./scripts/check-web-build.sh  Not run by CI. Needs the target: rustup target add wasm32-unknown-unknown
- `scripts/fetch-nist-anova.sh`: Download NIST's AtmWtAg one-way ANOVA dataset into crates/stat-strd/data/anova/, where `cargo test -p stat-strd --test anova_certified` checks the one-way ANOVA against its certified values. (The other ten NIST ANOVA problems are bundled: SiRstv is transcribed and SmLs01–SmLs09 are generated from their published construction.)  The file is a work of the U.S. Government (public domain): https://www.itl.nist.gov/div898/strd/anova/anova.html
- `scripts/fetch-nist-nls.sh`: Download the 27 NIST StRD nonlinear regression datasets into crates/stat-strd/data/nls/, where `cargo test -p stat-strd --test nls_certified` checks the nonlinear regression fits against their certified values (from both certified starting points).  The files are a work of the U.S. Government (public domain): https://www.itl.nist.gov/div898/strd/nls/nls_main.shtml
- `scripts/manual-stamp.mjs`
