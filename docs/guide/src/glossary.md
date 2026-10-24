# Glossary

Words you will meet in reports, in plain language. The
[Reference Manual](../../manual/src/SUMMARY.md)
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
