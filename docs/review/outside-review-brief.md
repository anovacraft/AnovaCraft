# Independent code review brief: AnovaCraft core engine

You are being asked to do an independent correctness review of part of an
open-source statistics application. Read this whole brief before starting.
It is self-contained; you need no other context.

## 1. What the software is

AnovaCraft is an open-source desktop statistics application written in Rust
(edition 2024, latest stable toolchain). It covers descriptive statistics,
hypothesis tests, linear and generalized linear models, design of
experiments, quality control, reliability, time series and predictive modelling. It is a
Cargo workspace of engine crates (pure numerical code, no GUI) plus an
application-logic crate and an `egui` desktop front end.

- Repository: https://github.com/anovacraft/AnovaCraft, branch `main`
- Project conventions: `CLAUDE.md` at the repository root (architecture,
  accuracy requirements, coding rules). Read it first.
- What each routine is validated against: `docs/validation.md`
- What the application does, by menu: `FEATURES.md`

Every statistical routine is implemented from published formulas and is
meant to agree with an independent reference (NIST StRD certified values,
published worked examples, or SciPy / statsmodels / R / scikit-learn).

## 2. Why you are reviewing it

Most of the code has had three earlier bug-review passes, all done by one AI model. The time-series crate (`stat-timeseries`) is new and has had none.
Any single reviewer has blind spots, so this pass is deliberately done by a
different agent. **Please form your own view.** Do not read pull requests
#26, #31 or #43, or the git log messages of commits titled "bug review" or
"fixes", until you have finished and written up your own findings. We will
compare your list against theirs afterwards; overlap is fine, but what we
most want is what they missed.

## 3. Scope

Review these crates (source files under `crates/<name>/src`, and their tests). Record the commit hash of `main` you reviewed at the top of your report, because the code changes daily.

| Crate | Approx. lines | What it holds |
|---|---|---|
| `stat-core` | 9,500 | Matrices, compensated summation, pivoted Householder QR, SVD, eigen, special functions (ln Γ, incomplete beta/gamma), probability distributions and quantiles, studentized range, noncentral distributions, distribution fitting (14 families, Anderson–Darling), Box–Cox, normality tests, quadrature, random numbers, Nelder–Mead / least-squares optimizers, descriptive statistics and quantile types |
| `stat-linear` | 6,200 | OLS, ANOVA tables (sequential/partial sums of squares), ANCOVA, Welch ANOVA, Kruskal–Wallis, equal-variance tests (Levene, Brown–Forsythe, Bartlett), model hierarchy, stepwise and best-subsets selection, MANOVA, nonlinear regression, Deming / Passing–Bablok method comparison, ICH Q1E stability |
| `stat-discrete` | 4,600 | Contingency tables, χ² and exact tests (including r×c exact by Monte Carlo), stratified tables (Mantel–Haenszel, Breslow–Day), logistic regression and GLMs (several links and families), ordinal/nominal logistic, probit dose–response with fiducial limits |
| `stat-inference` | 8,200 | One-, two-sample and paired tests, rank and sign tests, variance tests, multiple comparisons (Tukey–Kramer, Games–Howell, Holm, Dunn, Dunnett, Hsu MCB), proportions, rates, equivalence, power and sample size, repeated measures, crossover, tolerance intervals, outlier tests, bootstrap and permutation, analysis of means |
| `stat-timeseries` | 7,200 | New, not yet reviewed by anyone. ACF/PACF, Ljung–Box, Yule–Walker AR, cross-correlation with prewhitening, differencing and Box–Cox (Guerrero λ), classical and STL decomposition, ADF and KPSS tests, trend models and accuracy measures, periodogram and smoothed spectrum, exponential smoothing in state-space form (all ETS forms, AICc selection), seasonal ARIMA by exact Kalman-filter likelihood, forecasts, stepwise automatic order search. References are R (stats, forecast, tseries, urca) and statsmodels |
| `app-model` *(selected files only)* | ~18,000 of 103,000 | The layer that turns a data table plus row states into analysis inputs and reports. Review: `model_data.rs`, `report_run.rs`, `linear_analysis.rs`, `glm_analysis.rs`, `compare.rs`, `distribution.rs`, `formula.rs`, `editing.rs`, `project.rs`, `project/tables.rs` (several tables per project, derived tables), `prediction.rs`, `prediction_formula.rs`, `table_ops.rs`, `recode.rs`, `brushing.rs` |

Out of scope: `app-gui` (the desktop front end), the other engine crates,
look and feel, wording, and performance unless it makes a realistic input
unusable (e.g. minutes for a few thousand rows).

If you run short of time, cover the engine crates first, in the order listed.

## 4. What to look for

In priority order:

1. **Wrong numbers.** A routine returns a value that disagrees with the
   published formula or with established software on valid input. Typical
   places: degrees of freedom, one- vs two-sided p-values, which tail a
   quantile comes from, continuity corrections, pooled vs unpooled
   variances, biased vs unbiased estimators, quantile definitions, tie
   handling in rank tests, confidence-level conventions (α vs α/2),
   weights and frequency columns treated the same when they should not be.
2. **Numerical accuracy.** Catastrophic cancellation, overflow/underflow in
   tails (p-values near 0 or 1, very large df or parameters), loss of
   precision from naive summation or forming XᵀX, poor convergence or a
   wrong convergence test in iterative fits (Newton/IRLS, quadrature,
   root finding), silent non-convergence reported as success.
3. **Edge cases.** n = 0, 1 or 2; a group of size one; all values equal;
   zero variance; perfect separation in logistic models; empty cells or
   zero margins in tables; rank-deficient or collinear designs; missing
   values (NaN) and excluded rows; extreme parameters. The intended
   behaviour is either a correct limiting value or a documented error, never
   a panic, NaN or infinity presented as a result.
4. **Row-state handling (app-model).** Excluded or missing rows leaking into
   a fit, into factor levels, or into counts; results that depend on row
   order; a column referenced by name going stale after a rename, insert or
   delete; a project file that does not reproduce the same report after
   save and reopen.
5. **Error handling.** Panics reachable from user input (`unwrap`, indexing,
   division by zero, integer overflow), errors swallowed, misleading error
   messages, public functions whose doc comment promises an error the code
   never returns.
6. **Doc/code mismatch.** Every public item has a `///` comment giving its
   formula and failure modes. Code that does something other than its doc
   says is a finding, whichever of the two is wrong.

Not wanted: style preferences, renames, refactors, clippy-level lints,
missing features. A finding must describe behaviour that is wrong.

## 5. Things already known (do not report these)

These are known limitations, accepted or deferred by the owner:

- Accuracy floors for NIST datasets whose decimal data binary64 cannot
  represent exactly (Filip, NumAcc3/4 standard deviations, SmLs04–09,
  Lanczos1). The required digits are listed in `CLAUDE.md`, section
  "Deterministic Verification".
- Floating-point results may differ by one ulp between Linux and macOS;
  tests use tolerances on purpose.
- Text such as "nan" or "inf" in a delimited file makes a text column,
  except recognised missing-value markers; this is a design decision.
- Bootstrap reads values at their displayed precision; BCa's jackknife is
  limited to 500 rows; attribute agreement needs balanced data.

Note that "Known gaps" in `docs/validation.md` lists routines checked only
by properties, with no outside reference yet. Those are not exempt: a real
disagreement with outside software there is a welcome finding.

## 6. How to build and test

```
git clone https://github.com/anovacraft/AnovaCraft
cd AnovaCraft
cargo test -p stat-core -p stat-linear -p stat-discrete -p stat-inference -p stat-timeseries -p stat-strd
cargo test -p app-model            # larger; takes longer to build
cargo clippy --workspace --all-targets -- -D warnings
```

`stat-strd` holds the NIST StRD suite (`crates/stat-strd/tests/*_certified.rs`).
Python scripts that generated reference values sit next to their data in
`crates/*/tests/data/*.py`. To build `app-gui` on Linux you need the usual
`egui`/`wgpu` system libraries, but you do not need it for this review.

You are encouraged to check suspected bugs against independent software
(SciPy, statsmodels, R, mpmath for high precision) and to write throwaway
Rust tests to reproduce them. **Do not push to the repository and do not
open pull requests.** The deliverable is the findings report below; fixes
will be made separately.

## 7. Deliverable

One Markdown file, `findings.md`, with three parts.

### 7.1 Findings

One entry per finding, most severe first, in exactly this shape:

```
### F-01: <one-line statement of the defect>

- Severity: critical | major | minor
- Category: wrong-result | accuracy | edge-case | row-state | panic | error-handling | doc-mismatch
- Location: crates/<crate>/src/<file>.rs:<line> (function `<name>`), at commit <hash you reviewed>
- Confidence: confirmed (reproduced) | likely | suspected
- Input that shows it: <concrete data or call>
- Observed: <what the code returns>
- Expected: <correct value and where it comes from: formula with citation, or tool + version + exact call>
- Reproduction: <Rust test or script, or the commands you ran>
- Suggested fix: <one or two sentences>
```

Severity guide: **critical** = a plausible user gets a wrong p-value,
estimate or interval without any warning; **major** = wrong in a narrower
but realistic case, or a panic/crash on realistic input; **minor** =
far-fetched input, doc mismatch, or an unhelpful error.

Report only what you have actually checked. If you suspect something but
could not confirm it, keep it with Confidence: suspected and say what would
confirm it. Please do not pad the list.

### 7.2 Coverage

A table of every file you read, marked **full** (read line by line) or
**skimmed**, with any checks you ran against outside software. This lets us
see which code had a second pair of eyes and which did not.

| File | Depth | External checks run |
|---|---|---|

### 7.3 General observations (optional, at most ten bullets)

Patterns you noticed that are not individual bugs, for example a numerical
technique used in several places that is fragile, or a class of input the
tests never exercise.
