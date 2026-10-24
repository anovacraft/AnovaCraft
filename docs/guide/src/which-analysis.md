# Which analysis do I need?

Start from your question. Each answer names the menu item to use; the
[Menus](menus/file.md) pages say more about each one.

## "What do my numbers look like?"

- One column of measurements: **Describe ▸ Distribution…** (average,
  spread, a histogram, and whether it looks bell-shaped).
- Two columns against each other: **Describe ▸ Bivariate…** (a scatter
  plot and the correlation).
- Counts of categories: **Describe ▸ Multi-way table…**.
- Any picture you like: **Visualize ▸ Graph canvas…**.

## "Is there a difference?"

- One group against a target value: **Compare ▸ One sample vs a value…**.
- Two or more groups (for example, three suppliers): **Compare ▸ Two or
  more groups…**.
- The same things measured twice (before and after): **Compare ▸ Paired or
  repeated measures…**.
- Percentages (pass rates, yes/no answers): **Compare ▸ Proportions…**.
- How many people or samples you need: **Compare ▸ Power and sample
  size…**.
- How likely a difference is, as a probability rather than a p-value, or
  evidence that there is *no* difference: the **Bayesian** submenu under
  **Compare**.

## "What drives the result?"

- A measurement explained by one or more other columns: **Model ▸ Linear
  model…**.
- A yes/no outcome, or a count: **Model ▸ Generalized linear model…**.
- Many possible predictors and you want the useful ones: **Model ▸
  Stepwise selection…** or **Model ▸ Penalized regression…**.
- Prediction is what matters, not explanation: the decision tree, forest
  and boosting items under **Model**.
- A curve that bends or levels off (growth, decay, a dose–response curve):
  **Model ▸ Nonlinear regression…**, or **Model ▸ Curve templates…** for a
  laboratory assay such as an immunoassay standard curve or enzyme
  kinetics.

## "Some of my values are missing"

- Fill the gaps with estimates, or make several filled-in copies so that
  later results allow for the uncertainty: **Data ▸ Impute missing
  values…**.

## "How do many columns relate?"

- Which columns move together: **Multivariate ▸ Correlations…**.
- Boil many columns down to a few: **Multivariate ▸ Principal
  components…**.
- Find natural groups of rows: the clustering items under **Multivariate**.

## "What are people saying?"

- The words and themes in a column of comments: **Multivariate ▸ Text
  exploration…** or **Multivariate ▸ Topic model…**.
- Whether the comments are positive or negative: **Multivariate ▸
  Sentiment scores…**.

## "How should I run my experiment?"

Use the **Design** menu. It makes a new table listing the runs to do. You
do not need any data to start. After the experiment, fill in the results
and analyse them, usually with **Model ▸ Linear model…** or **Model ▸
Response surface (second-order model)…**.

## "Is my process stable and capable?"

- Watch a process over time: **Quality ▸ Control chart…**.
- Does it meet its specification limits: **Quality ▸ Process
  capability…**.
- Can we trust our measuring device: the gauge study items under
  **Quality**.

## "How long until it fails?"

- Time until an event, with some units still running: **Reliability ▸
  Survival curves…**.
- Fit a life distribution (such as Weibull): **Reliability ▸ Life
  distributions…**.

## "What happens next month?"

Use the **Series and Signals** menu: describe the series, then forecast it.
