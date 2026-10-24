# Formula columns

A formula column computes its values from the other columns, one row at a
time, and recomputes them whenever the data changes: after an edit, a paste,
a sort, rows added or deleted, an undo or redo, or a script's changes.

Only what a change affects is recomputed. A formula that reads just its own
row's values recomputes only the rows edited or added (and nothing at all
after a sort or a deletion, since its values move with their rows), so
editing a cell of a large table stays quick. A formula that reads other rows
or the row's position — `lag`, `lead`, `col("name", row)`, the column
summaries such as `col_mean`, ranks, cumulative sums, smoothing and filters,
`row()`, `n_rows()` and the random numbers — recomputes every row.

## Making one

- **Edit ▸ Add column ▸ Formula…** (or the grid toolbar's **+ Column ▸ Formula…**) adds a new column at the end.
- Right-click a column name for **Insert formula column to the right…**,
  **Compute with a formula…** (replaces that column's values; Undo puts them
  back), **Edit formula…** and **Convert formula to values** (keeps the
  current values as entered data).
- In a script: `t.set_formula("BMI", "Weight / Height ** 2")` (see
  [scripting](scripting.md#changing-data)).
- From a model report (linear, generalized linear, lasso/ridge/elastic net,
  nonlinear regression): **Save prediction formula** in the report's options menu (see below).

### Prediction formulas

Each of these reports ends with a **Prediction Formula** section that shows
the equation (with a **Copy formula** button). **Save prediction formula**
writes it as a formula column named "Predicted *y* formula". It computes the same values as **Save
predicted values**, but keeps working as the data changes: edit a predictor
or add a row and its prediction updates. The coefficients are written with
every digit, so the values agree with the report's predictions. For example,
a model with a continuous `Dose`, a categorical `Site` and their interaction
gives:

```
-1.25
+ 0.75 * Dose
+ (switch text(Site) { "East" => 0, "North" => 0.4, "West" => -0.2, _ => 0.0 / 0.0 })
+ (switch text(Site) { "East" => 0, "North" => 0.1, "West" => 0.3, _ => 0.0 / 0.0 }) * (Dose - 2.5)
```

- Each categorical effect is a `switch` giving every level its effect (for an
  interaction of two categorical factors, every pair of levels, joined by a
  tab). A level the model was not fitted to falls to `0.0 / 0.0` (not a
  number), which gives a missing prediction without counting as a failed
  row.
- In interactions and powers a continuous factor is centred at its mean over
  the rows the model was fitted to (here `Dose - 2.5`), as in the report.
- Logistic models give `logistic(…)` (the probability of the modelled level)
  and Poisson models `exp(…)` (the mean count). Penalized models use the
  λ_1SE fit and leave out predictors with a zero coefficient.
- Nonlinear regression gives its curve with the estimates written in, such
  as `53.52 / (1 + exp(-0.2777 * (Day - 17.68)))` (with every digit), or a
  model formula preceded by one `let` per parameter:
  `let Vmax = 212.68…; let Km = 0.0641…; Vmax * Conc / (Km + Conc)`. The
  model formula itself (Model ▸ Nonlinear regression…) is written in the same
  language, with the parameters as variables.
- A spline term (Model ▸ Linear model… or Generalized linear model…, More
  options) is written as one basis column per coefficient:
  `natural_spline(x, [knots], k)` for a natural cubic spline and
  `bspline(x, [knots], degree, k)` for a B-spline, where `[knots]` lists the
  lower boundary knot, the interior knots and the upper boundary knot, and
  `k` (from 1) picks the column. Beyond the boundary knots a natural spline
  continues as a straight line and a B-spline as its end polynomial.
- A date-time column used as a category cannot be written into a formula, so
  that option is not offered for such a model.
- The formula is fixed when saved: refitting the model (for example after
  excluding rows) does not change it. Save it again for the new fit.

The formula editor lists the table's columns and the functions below; a
click inserts one at the text cursor. The preview shows the first rows as
you type, and a formula that cannot be read is reported with its line and
column and a caret under the place.

Under the text, **Reads as** draws the formula as mathematics, so a long
expression can be checked at a glance: `/` becomes a fraction bar, `**` a
raised power, `sqrt(x)` a radical sign, `abs(x)` |x|, `exp(x)` *e* to a
power, `floor` and `ceiling` ⌊x⌋ and ⌈x⌉, `normal_cdf(z)` Φ(z),
`col_sum("x")` Σ x, `cum_sum("x")` a running sum Σ over rows *j* ≤ *i*,
and `lag("x")` and `lead("x", 2)` the row subscripts x<sub>i − 1</sub> and
x<sub>i + 2</sub> (*i* is this row, as `row()`, and *n* the number of rows,
as `n_rows()`). Parentheses appear where the order of computing needs them:
`-x ** 2` is drawn (−x)², because a leading minus applies before the power;
write `-(x ** 2)` for −x². Comparisons read ≤, ≥, ≠ and `&&`, `||`, `!` read
*and*, *or*, *not*. Only a single expression is drawn: a formula with steps
(`let`, `;`), `if` or `switch` has no typeset view, and nor does one that
cannot be read.

![Longley: New formula column with sqrt(GNP) / Unemployed ** 2, its Reads as line with the radical sign and fraction bar, and the preview](../images/formula-typeset.png)

Before the formula is applied every row
is computed; if some rows fail (for example `ln` of text) the editor says how
many and why, and **Apply anyway** stores them as missing values. **Apply**
is unavailable, with a note saying what is missing, until the column has a
name and a formula; inside the editor Enter starts a new line, so press
⌘/Ctrl+Enter to apply from the keyboard.

**Store as** chooses the column's type: *As computed* keeps numbers as
numbers and makes the column text when any value is text; a date format shows
the numbers (seconds) as dates.

A formula column's cells cannot be typed into, and a column that a formula
uses cannot be deleted until that formula is changed or converted to values.
Renaming a column rewrites the formulas that use it. Formulas may use other
formula columns, but not in a circle. Formulas are saved in project files
with the values they computed, and recorded scripts include them. If a
project file holds formulas that cannot be computed (they use each other in
a circle, or no longer compile, as can happen in a hand-edited file), those
columns keep their saved values and the others are computed as usual; each
such column is named in a note when the project is read.

Tables made by table operations (sort into a new table, subset, join, …)
hold values, not formulas.

## Writing formulas

Formulas are written in [Rhai](https://rhai.rs), the language scripts use,
and are evaluated once per row:

- A column whose name is a plain identifier (letters, digits and `_`, not
  starting with a digit) is a variable holding this row's value:
  `Weight / (Height / 100) ** 2`. Any column can be read by name with
  `col("Wait (min)")`.
- Arithmetic: `+ - * / % **`. `/` always divides exactly (`1 / 2` is `0.5`).
- Comparison and logic: `== != < <= > >=`, `&&`, `||`, `!`.
- Choices: `if x > 5 { "high" } else { "low" }`, or `switch`.
- Several steps: `let bmi = Weight / Height ** 2; round(bmi, 1)` (the last
  expression is the value).
- Loops: `let s = 0; for k in 1..4 { s += k * x } s`, or
  `for (v, i) in [a, b, c] { … }` for each value with its position. A
  variable named in `let` or `for` is the formula's own, even when a column
  has the same name: inside the loop body `k` is the loop variable, not a
  column `k` (write `col("k")` for the column), and renaming column `k`
  leaves the loop alone.
- Text is written in quotes; `+` joins text (`concat` writes numbers the way
  the data view does).
- Dates and times are numbers of seconds since 1970-01-01 00:00 UTC.
- A missing number is `NaN`, so arithmetic on it gives a missing value and
  comparisons with it are false; a missing text value is `""`. Use
  `is_missing(x)` to test.
- The result may be a number, `true`/`false` (stored as 1 and 0), text, or
  `missing()`. A row whose formula fails gets a missing value.

Each row is limited to 200 000 operations, so a runaway loop cannot hang the
program. Simple formulas compute a million rows in about a second.

## Functions

### Conditions

| Function | What it gives |
|---|---|
| `if x > 0 { "positive" } else { "not positive" }` | Choose a value by a condition. |
| `switch x { 1 => "one", 2 \| 3 => "two or three", _ => "other" }` | Choose a value by matching cases. |
| `x > 0 && y > 0` | Both conditions hold (`\|\|` for either, ! for not). |

### Missing values

| Function | What it gives |
|---|---|
| `is_missing(x)` | true when x is missing. |
| `if_missing(x, 0)` | x, or the second value when x is missing. |
| `missing()` | A missing value. |

### Mathematics

| Function | What it gives |
|---|---|
| `sqrt(x)` | Square root. |
| `x ** 2` | Power (also pow(x, y)). |
| `exp(x)` | e to the power x. |
| `ln(x)` | Natural logarithm. |
| `log10(x)` | Base-10 logarithm (also log2). |
| `abs(x)` | Absolute value. |
| `round(x, 2)` | Round to a number of decimals (round(x) to a whole number). |
| `floor(x)` | Largest whole number not above x (ceiling for the smallest not below). |
| `min(x, y)` | Smaller of two values (max for the larger). |
| `modulo(x, y)` | Remainder after division, never negative. |
| `sign(x)` | −1, 0 or 1. |
| `logit(p)` | Log odds ln(p / (1 − p)); logistic(x) is its inverse. |
| `lambert_w(x)` | Lambert W: the w ≥ −1 with w·eʷ = x (x ≥ −1/e). |
| `wright_omega(z)` | Wright omega: the w with w + ln w = z, which is lambert_w(exp(z)) without overflow (used by the enzyme progress-curve template). |
| `natural_spline(x, [a, k1, …, b], k)` | Column k (from 1) of the natural cubic spline basis with boundary knots a and b and the interior knots between, as spline terms of linear models and GLMs use it. |
| `bspline(x, [a, k1, …, b], degree, k)` | Column k of the B-spline basis of that degree (1 to 3) without the intercept column. |
| `sin(x)` | Sine of an angle in radians (also cos, tan, asin, acos, atan, atan2, and the hyperbolic tangent tanh). |

### Distributions

| Function | What it gives |
|---|---|
| `normal_cdf(x)` | Standard normal probability P(Z ≤ x); normal_cdf(x, mean, sd). |
| `normal_pdf(x)` | Standard normal density; normal_pdf(x, mean, sd). |
| `normal_quantile(p)` | Standard normal quantile; normal_quantile(p, mean, sd). |
| `t_cdf(x, df)` | Student's t probability P(T ≤ x). |
| `t_quantile(p, df)` | Student's t quantile. |
| `chisq_cdf(x, df)` | Chi-square probability P(X ≤ x). |
| `chisq_quantile(p, df)` | Chi-square quantile. |
| `f_cdf(x, df1, df2)` | F probability P(F ≤ x). |
| `f_quantile(p, df1, df2)` | F quantile. |
| `binomial_pmf(k, n, p)` | Binomial probability of k successes. |
| `binomial_cdf(k, n, p)` | Binomial probability of at most k successes. |
| `poisson_pmf(k, mean)` | Poisson probability of k events. |
| `poisson_cdf(k, mean)` | Poisson probability of at most k events. |

### Text

| Function | What it gives |
|---|---|
| `concat(a, " ", b)` | Join values as text (up to six). |
| `text(x)` | A number as text, written as in the data view. |
| `number(s)` | Text read as a number (missing if it is not one). |
| `upper(s)` | Capital letters (lower for small letters). |
| `trim(s)` | Without spaces at either end. |
| `length(s)` | Number of characters. |
| `substr(s, 0, 3)` | Characters from a position (from 0), of a length. |
| `find(s, "x")` | Position of the first match (from 0), or −1. |
| `replace(s, "old", "new")` | Replace every match. |
| `word(s, 0, ",")` | The nth piece (from 0) between separators; word(s, n) splits at spaces. |
| `has_item(s, "Email", ";")` | true if a check-all-that-apply list holds the item (spaces around items ignored; separator `;` when left out). Use it as a row filter, or to make one yes/no column per item. |
| `item_count(s, ";")` | How many different items a check-all-that-apply list holds. |
| `s.contains("x")` | true if the text contains x (also starts_with, ends_with). |
| `regex_match(s, "^[A-Z]\\d+$")` | true if a regular expression matches. |
| `regex_find(s, "\\d+")` | The first match of a regular expression. |
| `regex_replace(s, "\\s+", " ")` | Replace every match of a regular expression ($1 for groups). |

### Dates

| Function | What it gives |
|---|---|
| `year(d)` | Year of a date (also month, day, hour, minute, second). |
| `weekday(d)` | Day of the week, 1 (Monday) to 7 (Sunday). |
| `day_of_year(d)` | Day of the year, from 1. |
| `date_part(d)` | The date at midnight, without the time of day. |
| `make_date(2026, 1, 31)` | A date from year, month and day (make_datetime adds h, m, s). |
| `date_diff(start, end, "days")` | Time from start to end in a unit (whole months or years). |
| `date_add(d, 1, "months")` | A date moved by a number of units. |
| `format_date(d, "%d %b %Y")` | A date as text (%Y %m %d %H %M %S %j %B %b %A %a). |

### Rows

| Function | What it gives |
|---|---|
| `row()` | This row's number (from 1). |
| `n_rows()` | Number of rows. |
| `lag("column", 1)` | The value a number of rows earlier (missing before the first row). |
| `lead("column", 1)` | The value a number of rows later. |
| `col("column")` | This row's value of any column, by name. |
| `col("column", 1)` | The value in a given row (from 1). |

### Column summaries

| Function | What it gives |
|---|---|
| `col_mean("column")` | Mean of a column; col_mean("column", "group") within each group. |
| `col_sd("column")` | Standard deviation (n − 1), optionally by a group column. |
| `col_min("column")` | Smallest value (col_max for the largest), optionally by group. |
| `col_sum("column")` | Sum, optionally by group. |
| `col_count("column")` | Number of non-missing values, optionally by group. |
| `col_quantile("column", 0.5)` | Quantile (Hyndman–Fan type 8), optionally by group as a third argument. |
| `cum_sum("column")` | Running total down to this row, optionally by group. |
| `rank("column")` | Rank from 1 (ties averaged), optionally by group. |
| `rank_ties("column", "min")` | Rank from 1 with a tie rule: "average", "min" (1, 2, 2, 4), "max" (1, 3, 3, 4), "dense" (1, 2, 2, 3) or "first" (row order); optionally by group as a third argument. |
| `percent_of_range("column")` | The value as a percentage of the column's range (0 at the smallest value, 100 at the largest), optionally by group. |
| `fraction_of_total("column")` | The value divided by the column's total, optionally by group. |
| `subtract_baseline("column", 3)` | The value minus the mean of the column's first 3 values in row order (a baseline), optionally by group as a third argument. |
| `p_adjust("column", "bh")` | The column's p-values adjusted for multiple testing: `"bonferroni"`, `"sidak"`, `"holm"`, `"hochberg"`, `"bh"` (Benjamini–Hochberg), `"by"` (Benjamini–Yekutieli) or `"q"` (Storey's q-values, λ = 0.5); optionally within each group as a third argument. Missing p-values stay missing and are not counted. |

### Signals

These treat a column as a signal in row order (or, for the x–y functions,
sorted by an x column). Each takes an optional group column as its last
argument, and then works on each group separately. Missing values are skipped
and stay missing.

| Function | What it gives |
|---|---|
| `savgol("column", 11, 3)` | Savitzky–Golay smoothing: a polynomial of degree 3 fitted by least squares over 11 rows centred on this one (window odd, larger than the degree); the ends use the fit over the first or last window. |
| `moving_average("column", 5)` | Centred moving average of 5 rows (odd window; the series is mirrored beyond its ends). |
| `running_median("column", 5)` | Running median of 5 rows (odd window, mirrored ends), resistant to spikes. |
| `exp_smooth("column", 0.3)` | Exponential smoothing: s₁ = y₁, sₜ = 0.3·yₜ + 0.7·sₜ₋₁. |
| `gaussian_smooth("column", 2)` | Gaussian kernel smoothing with σ = 2 rows (kernel cut at 4σ, mirrored ends). |
| `derivative("y", "x")` | dy/dx: central differences on the rows sorted by x (unequal spacing allowed; duplicate x averaged), one-sided at the ends. |
| `cum_integral("y", "x")` | Running integral of y over x by the trapezoid rule, from 0 at the smallest x. |
| `interpolate("y", "x", 2.5)` | y read off at x = 2.5 by straight lines between the points; missing outside the range of x. |
| `interpolate_spline("y", "x", 2.5)` | The same by a not-a-knot cubic spline. |
| `interpolate_pchip("y", "x", 2.5)` | The same by a shape-preserving piecewise cubic (PCHIP, Fritsch–Carlson), which never overshoots the data. |

### Signal processing

These work down a column in row order, optionally within each group (a
last argument naming a column), on the rows with a value; missing values
stay missing.

| Function | What it gives |
|---|---|
| `lowpass_filter("column", 0.1)` | The column through a fourth-order Butterworth low-pass filter run forwards and backwards (zero phase). The cut-off is in cycles per row, between 0 and 0.5. `highpass_filter` keeps the frequencies above the cut-off instead. |
| `bandpass_filter("column", 0.05, 0.2)` | Keeps the frequencies between two edges (cycles per row); `bandstop_filter` removes them. |
| `envelope("column")` | The envelope: the magnitude of the analytic signal from the Hilbert transform. `instant_phase` gives its unwrapped phase in radians. |
| `wavelet_denoise("column", "db4")` | The column denoised by soft-thresholding its wavelet detail coefficients (universal threshold, noise level from the finest level, all levels the length allows). The wavelet (`haar`, `db1`–`db10`, `sym2`–`sym10`, `coif1`–`coif5`) defaults to `db4`; a group column is the third argument. |

### Random numbers

| Function | What it gives |
|---|---|
| `random_uniform()` | Uniform on [0, 1); random_uniform(low, high). |
| `random_normal()` | Standard normal; random_normal(mean, sd). |
| `random_integer(1, 6)` | A whole number between two limits. |
| `random_seed(42); random_normal()` | Choose the seed (the default is 0). |
Column names in `lag`, `lead`, `col` and the column summaries are written in
quotes. Summaries skip missing values; `col_sd` divides by n − 1 and
`col_quantile` uses Hyndman and Fan's type 8 definition, as the Distribution
report does. Random numbers are drawn from a generator seeded by the formula's
seed (`random_seed`, 0 by default) and the row number, so recomputing gives the
same values; sorting the rows does not carry them along.
