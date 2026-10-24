# AnovaCraft — Roadmap

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

## Corrections to the source gap analysis

The analysis this plan was drawn from got some facts about the code wrong.
The plan below follows the code.

| Claim | Actually |
|---|---|
| Data is backed by Polars; reshaping can use Polars' melt/pivot. | Columns are `Vec<f64>` / `Vec<String>` (`stat_data::ColumnData`); Polars is not a dependency. Table operations are written directly on `DataTable`. |
| A Rhai/TOML headless engine (`stat-script`) exists. | It did not; Rhai was chosen and the engine and headless runner now exist, with the Script window and action recording (`FEATURES.md`, Scripting). F1 is complete. |
| `stat-predictive` has decision trees ready to hook up. | It contained only the elastic net. Trees were built as new work (`FEATURES.md`, Model). |
| `stat-functional` and `stat-text` exist. | Neither existed; both are new crates, and both have shipped (§8 `stat-functional`, §9 `stat-text`). |

---

## 1. Data management

Every §1 item has shipped (see `FEATURES.md`, Data and documents).

---

## 3. Predictive modeling

Every §3 item has shipped (see `FEATURES.md`, Model): decision trees, model
comparison, random forests, boosted trees, neural networks, partial least
squares, support vector machines, adaptive regression splines and the
automated model search.

---

## 4. Multivariate

All §4 items have shipped (see the Multivariate menu in FEATURES.md).

---

## 5. Reliability and survival

All §5 items have shipped (see the Reliability menu in FEATURES.md).

---

## 6. Quality

All §6 items have shipped (see the Quality menu in FEATURES.md).
Measurement systems analysis is §13.

---

## 7. Time series

Shipped: series description (plot, ACF/PACF, Ljung–Box, unit-root tests,
spectrum), decomposition, trend models, cross-correlation, exponential
smoothing and seasonal ARIMA with the automatic order search and
transfer-function inputs (see FEATURES.md, Time Series). All of §7 has
shipped.

---

## 8. Functional data

Every §8 item has shipped (see `FEATURES.md`, Multivariate ▸ Functional data).

---

## 9. Text analysis

All §9 items have shipped in the new crate `stat-text` (see Multivariate ▸
Text in FEATURES.md): tokenization, stop words, Porter stemming, phrases,
term tables, word-frequency chart and word cloud; TF-IDF, latent semantic
analysis and document clusters; LDA topic models; lexicon sentiment with
negation (the VADER lexicon, MIT licence).

---

## 10. Dashboards and sharing (L)

Dashboards (reports and notes from a project's tables on a grid, with
shared local filters and shared column pickers, saved in the project),
HTML export of dashboards and export of reports as PDF, Word, PowerPoint,
HTML, Markdown and Excel have shipped (see FEATURES.md, Data and documents).
The read-only web dashboard viewer (Publish as web app…; the manual's "Web
build" page) has shipped too, so all of §10 is done.

---

## 11. Automation (needs F1)

All of §11 has shipped (script library and batch runner; see FEATURES.md,
Scripting).

---

## 12. Design of experiments

All of §12 has shipped (see Design in FEATURES.md).

---

## 13. Measurement systems analysis

All §13 items have shipped (see the Quality menu in FEATURES.md).

---

## 14. Regression and ANOVA

All of §14 has shipped (see the Model and Compare menus in FEATURES.md).

---

## 15. Tests, intervals and tables

All §15 items have shipped (see the Describe and Compare menus in FEATURES.md).

---

## 16. Graphs

All §16 items have shipped (see Visualize in FEATURES.md).

---

## 17. Tests, comparisons and effect sizes

Third gap check (2026-10-18) against two curve-fitting and publication-graphing
packages' public feature lists; standard names only. Most items are small and
sit on existing engines in `stat-inference`, `stat-linear` and
`stat-reliability`.

All of §17 has shipped (see the Compare and Describe menus in FEATURES.md).

---

## 18. Curve fitting

All of §18 has shipped (see Nonlinear regression under Model in FEATURES.md).

---

## 19. Graphs

All of §19 has shipped (see Visualize and Plots in FEATURES.md).

---

## 20. Bayesian analysis

All of §20 has shipped (see Bayesian under Compare and Model in FEATURES.md).

---

## 21. Multiple imputation

All of §21 has shipped (see Multiple imputation under Data in FEATURES.md).

---

## 22. Assay curve templates

The 37 templates (immunoassay standard curves, receptor binding, enzyme
kinetics, pharmacological dose–response, growth/decay/survival) have
shipped (see FEATURES.md, Model ▸ Curve templates). What is left:

| ID | Item | Size | Depends | Reference |
|---|---|---|---|---|
| 22.1 | User-saved templates: save a dialog's model, bounds, sharing and weighting under a name and list it beside the built-in templates | S | — | — |


---

## Fifth gap check (2026-10-19)

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

## 23. Quality and process monitoring

All of §23 has shipped (see Quality in FEATURES.md).

---

## 24. Models and comparisons

All of §24 has shipped (see Model and Compare in FEATURES.md).

---

## 26. Sequential optimization of experiments

Both §26 items have shipped (see `FEATURES.md`): Gaussian-process
regression (Model ▸ Gaussian process, 26.1) and next-run proposals by
expected improvement or an upper confidence bound (Design ▸ This table ▸
Propose next runs, 26.2), with the Reaction yield (simulated) sample.

---

## 28. Spectra: baseline correction and peak fitting

All of §28 has shipped (see Series and Signals ▸ Signals and spectra in FEATURES.md).

---

## 29. Smaller additions

All of §29 has shipped (see FEATURES.md).

---

## 30. Signal processing

All of §30 has shipped, in the new `stat-signal` crate shared with §28 (see
Series and Signals ▸ Signals and spectra in FEATURES.md).

---

## Suggested order

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

## Not planned

- Bridges to external Python or R interpreters (decided 2026-10-14).
- Bindings to external C/C++ machine-learning libraries; equivalent methods
  are implemented from their published algorithms (§3).
- Integration with proprietary analytics servers or file formats beyond
  documented open ones.
- Any compatibility with another product's scripting language; automation is
  Rhai only.
- Structural equation models (left out of the fifth gap check, 2026-10-19).
