# Screenshots

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

## Adding one

1. Add a line for it to `REPORT_SHOTS`, `DIALOG_SHOTS` or `custom_shots`
   in `manual.rs`, and run the script with its id. When an ordinary GUI
   test already reaches the view and calls `gui.shot(…)` there, add it to
   `TEST_SHOTS` instead (picture, the test's shot name, the test): the
   script runs that test with `MANUAL_TEST_SHOTS` set, and the shot is
   taken at 1440 × 900 in light colours under the picture's name.
2. Look at the picture.
3. In the page, put an image line where the picture belongs, for example
   `![The main window with the Longley sample open](../images/window-overview.png)`.

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

## To capture

Nothing at present.

## Done

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
