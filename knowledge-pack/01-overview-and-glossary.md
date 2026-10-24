# AnovaCraft: overview and glossary

> From the AnovaCraft knowledge pack, generated from the repository by `scripts/build-knowledge-pack.py`. AnovaCraft is an open-source statistics application written in Rust. Answer from this text: it does not depend on any other file. Paths such as `crates/...` or `docs/...` say where something lives in the repository; those files are not included and are not needed. Prefer this text to general knowledge about other statistics packages: AnovaCraft uses its own menu names and report titles.

## What AnovaCraft is

AnovaCraft is an open-source desktop application for statistical analysis,
design of experiments and predictive modelling. You open a data table, choose
an analysis from the menus, and get a report of collapsible sections with
tables and interactive plots. Selecting rows in any plot or in the data grid
highlights the same rows everywhere, so you can follow an observation from a
residual plot back to the data and into every other report.

Every method in AnovaCraft follows a published statistical definition, and
each analysis is tested against an independent reference (for example the
NIST Statistical Reference Datasets). Chapters name the method and its
reference so you can read more about it.

## For new users

AnovaCraft is a free program for looking at data and answering questions
with it: what is typical, whether groups differ, what drives a result, how
to plan an experiment, and whether a process is behaving.

This Quick Guide is the short version. It explains, in plain words, what
every menu item does and when you would use it. When you want the details
(every option, the method behind a result, how to script it), each entry
links to the **Reference Manual**.

### How the app works, in one minute

1. **Open some data.** A table of rows and columns, like a spreadsheet. Each
   row is one thing you measured (a person, a part, a day) and each column
   is one measurement. File ▸ Open sample data has dozens of ready-made
   tables to practise on.
2. **Pick an analysis from a menu.** The menus are named for the question
   you are asking: Describe, Compare, Model and so on. A dialog asks which
   columns to use.
3. **Read the report.** Results appear as a stack of sections you can fold
   and unfold. Tables give the numbers; plots show the picture.
4. **Click on points.** Selecting points in one plot highlights the same
   rows in every other plot and in the table, so you can see where an odd
   value came from.

(Picture: The main window with a table open)

### Where to go next

- Your first five minutes walks you through a whole
  analysis on a sample table.
- Finding your way around names the parts of the window.
- Which analysis do I need? starts from your question
  and points you to the right menu item.
- The **Menus** chapter has a page per menu, describing every item.
- The Glossary explains the statistical words you will meet
  in reports.

## Project facts

- Source code: https://github.com/anovacraft/AnovaCraft (open source; see the LICENSE file there).
- Written in Rust: statistical engine crates plus an egui/eframe desktop app for macOS, Windows and Linux, with a WebAssembly viewer build.
- Automation uses the Rhai scripting language (scripts, formula columns and recorded actions).
- Built clean-room from published statistical literature; its menu names, report titles and visual design are its own.
- Every statistical routine is tested against an independent reference (NIST StRD, R, SciPy, published examples); the validation record lists each one.

## Menus

- **Data** (import/export, filter, transform, recode)
- **Describe** (summary statistics, distributions)
- **Compare** (one-sample, independent-group, paired, proportion and rate tests; power and sample size)
- **Model** (linear model, GLM, regularized regression)
- **Multivariate** (PCA, clustering)
- **Design** (factorial, screening, and response-surface designs)
- **Quality** (control charts with run rules, measurement systems analysis)
- **Reliability** (survival curves with censoring, life-distribution fits)
- **Visualize** (graph canvas: drag columns onto shelves, combine layers, linked small multiples; graphs saved as JSON pipeline files reopen on any dataset with the same column names)

## Glossary

Words you will meet in reports, in plain language. The
Reference Manual
gives the formulas.

**Column, row.** A row is one thing measured; a column is one measurement
taken on each of them.

**Continuous, nominal, ordinal.** The kinds of column. *Continuous*
columns hold measurements you can average (weight, time). *Nominal* columns
hold categories with no order (supplier, colour). *Ordinal* columns hold
categories with an order (small, medium, large). The icon beside each
column in the sidebar shows which it is, and you can change it by
right-clicking the column.

**Mean, median.** Two kinds of average. The *mean* adds everything up and
divides by how many there are. The *median* is the middle value when they
are sorted, so a few extreme values do not pull it around.

**Standard deviation.** How spread out the values are: roughly, the typical
distance of a value from the mean.

**Distribution.** The overall shape of a column's values: where they
cluster, how widely they spread, whether they are lopsided.

**Normal distribution.** The symmetric bell-shaped curve. Many methods
assume data look roughly like this.

**Outlier.** A value far from the others. Check it before you trust or
discard it.

**Hypothesis test.** A way of asking whether a pattern in the data could
easily be due to chance alone.

**p-value.** How surprising the data would be if there were really no
effect. Small p-values (by convention, below 0.05) suggest a real effect.
The app marks them with ★. A p-value is not the size or importance of an
effect.

**Confidence interval.** A range of plausible values for something
estimated from the data, such as a mean. A 95% interval is built by a
method that captures the true value 95 times out of 100.

**Response, factor, predictor.** The *response* is what you are trying to
explain or predict. *Factors* and *predictors* are the columns you think
affect it.

**Model.** An equation that describes how the response depends on the
predictors.

**R² (R-squared).** The share of the response's variation that a model
explains, from 0 (nothing) to 1 (everything).

**Residual.** The gap between an actual value and the model's prediction.
Patterns in residuals hint that the model is missing something.

**Interaction.** When the effect of one factor depends on the level of
another (for example, a fertilizer that only helps in sunny beds).

**ANOVA.** Analysis of variance: a family of tests that compare means by
splitting the variation in the data into parts.

**Design of experiments (DOE).** Planning which combinations of settings to
try so that a few runs tell you as much as possible.

**Control limits.** Lines on a control chart showing how far a stable
process normally wanders. Points beyond them call for a look.

**Specification limits.** The limits a product must meet, set by the
customer or engineer. Not the same as control limits.

**Capability (Cpk).** How comfortably a process fits inside its
specification limits. Higher is better; 1.33 is a common target.

**Censored.** A unit that had not failed when observation stopped. We only
know it lasted *at least* that long. Reliability methods use these rows
too.

**Selected, excluded, hidden.** Row states. *Selected* rows are highlighted
everywhere. *Excluded* rows are left out of analyses but stay in the table.
*Hidden* rows are left out of plots.

**Missing value, imputation.** A *missing value* is an empty cell.
*Imputation* fills it with an estimate; *multiple imputation* makes several
filled-in copies, so results show how much the unknown values matter.

**Prior, posterior.** Words from Bayesian analysis. The *prior* is what
you believed before seeing the data; the *posterior* is the updated belief
after seeing them, given as probabilities.

**Bayes factor.** How many times more likely the data are under one
explanation than another (for example "a difference" against "no
difference"). The report labels its size: from 3 upwards (or 1/3
downwards) it calls the evidence at least moderate.
