# AnovaCraft

AnovaCraft is an open-source desktop application for statistical analysis,
design of experiments and predictive modelling, written in Rust. Every
analysis opens as a report of collapsible sections, and every plot takes part
in linked brushing: select rows in one plot or in the data grid and they light
up everywhere.

It covers, by menu:

- **Describe:** summary statistics, distributions and distribution fitting,
  correlations, multivariate outliers.
- **Compare:** one-sample, two-group, paired and many-group tests, rank tests,
  proportions and rates, equivalence, analysis of means, power and sample size,
  Bayesian tests.
- **Model:** linear models with effect tests and diagnostics, stepwise and
  best-subsets selection, generalized linear and mixed models, nonlinear
  curves and assay templates, regularized regression, decision trees, forests,
  boosted trees, partial least squares, support vector machines, neural
  networks, Gaussian processes and an automated model search.
- **Multivariate:** principal components, factor and discriminant analysis,
  clustering, multidimensional scaling, embeddings, correspondence analysis,
  multiple imputation.
- **Design:** full and fractional factorials, screening and definitive
  screening designs, response-surface, optimal and mixture designs, Taguchi
  arrays, desirability and next-run proposals.
- **Quality:** control charts with run rules, capability (normal and
  non-normal), measurement systems analysis, acceptance sampling, Pareto and
  multi-vari charts.
- **Reliability:** survival curves, life-distribution fits, reliability growth
  and repairable systems.
- **Series and Signals:** time-series description, decomposition, exponential
  smoothing and ARIMA forecasting, spectra, filters, peaks, wavelets and
  image spectra.
- **Visualize:** a graph canvas (drag columns onto shelves, combine layers,
  small multiples), 3-D, contour and surface plots, and a figure page.

[FEATURES.md](FEATURES.md) lists everything in detail. Each analysis has
sample data under **File ▸ Open sample data**, and the app carries two
manuals under **Help**: a Quick Guide for newcomers and a full Reference
Manual (sources in `docs/guide` and `docs/manual`).

## Accuracy

Results are checked against independent references: the NIST Statistical
Reference Datasets for regression, ANOVA, nonlinear fits and univariate
statistics, published worked examples, and reference values computed with
established open-source software. [docs/validation.md](docs/validation.md)
lists every routine, its reference data, tolerance and test.

## Installing

Download the app from the [Releases](https://github.com/anovacraft/AnovaCraft/releases) page.

- **macOS** (11 or later, Apple silicon or Intel): unzip
  `AnovaCraft-<version>-macos-universal.zip` and drag **AnovaCraft.app** to
  Applications. The app is not yet signed with an Apple Developer ID, so
  macOS blocks it the first time: open it once, then choose **System
  Settings ▸ Privacy & Security ▸ Open Anyway** (on macOS 14 and earlier,
  right-click the app and choose **Open**).
- **Windows** (64-bit): unzip `AnovaCraft-<version>-windows-x64.zip` and run
  `AnovaCraft.exe`; no installer is needed. Windows SmartScreen may warn
  about an unrecognized app the first time: choose **More info ▸ Run
  anyway**.

## Building from source

You need the latest stable Rust toolchain (1.85 or newer).

```sh
cargo run --release -p app-gui            # run the desktop app
cargo test --workspace                    # run the test suite
./scripts/build-macos-app.sh universal    # AnovaCraft.app for Apple silicon and Intel, in dist/
./scripts/build-windows-exe.sh            # Windows build, cross-compiled with MinGW-w64
```

The headless script runner runs Rhai scripts and batch jobs without the GUI:

```sh
cargo run --release -p stat-script --bin anovacraft-run -- script.rhai
```

## Scripting

Analyses, tables and reports can be automated with
[Rhai](https://rhai.rs), an open-source embedded scripting language. The same
language drives formula columns and recorded actions. See
[docs/scripting.md](docs/scripting.md) and [docs/formulas.md](docs/formulas.md).

## Project layout

The workspace is split into GUI-independent engine crates (`stat-core`,
`stat-linear`, `stat-inference`, `stat-discrete`, `stat-mixed`,
`stat-multivariate`, `stat-doe`, `stat-quality`, `stat-reliability`,
`stat-predictive`, `stat-bayes`, `stat-functional`, `stat-text`,
`stat-timeseries`, `stat-signal`, `stat-viz`), the scripting engine
(`stat-script`), the NIST reference suite (`stat-strd`), application logic
(`app-model`) and the `egui` desktop front end (`app-gui`).
[CLAUDE.md](CLAUDE.md) describes the architecture and conventions.

## Independent implementation

AnovaCraft is an independent, clean-room implementation built from published
statistical literature. It contains no code, scripts, assets or designs from
any commercial statistics package and is not affiliated with or endorsed by
any of their makers. Product names mentioned in the documentation are
trademarks of their owners.

## Licence

AnovaCraft is licensed under either of the [Apache License 2.0](LICENSE-APACHE)
or the [MIT licence](LICENSE-MIT), at your option. Bundled third-party
software, fonts, icons and data keep their own licences, listed in
[THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt) and in the app under
**Help ▸ About AnovaCraft ▸ Open-source licences**.
