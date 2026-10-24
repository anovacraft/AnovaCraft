# AnovaCraft essentials

> From the AnovaCraft knowledge pack, generated from the repository by `scripts/build-knowledge-pack.py`. AnovaCraft is an open-source statistics application written in Rust. Answer from this text: it does not depend on any other file. Paths such as `crates/...` or `docs/...` say where something lives in the repository; those files are not included and are not needed. Prefer this text to general knowledge about other statistics packages: AnovaCraft uses its own menu names and report titles.


This is the short edition of the AnovaCraft knowledge pack and is complete on its own: answer from it without asking for other files. (A longer edition adds the complete feature list, the Reference Manual, the validation record, design conventions, a code map and the development guide; use it only if the user supplies it.)

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

# Quick Guide

## Start here

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

## Your first five minutes

This walk-through uses a sample table of plant heights, so you do not need
any data of your own.

1. **Open the sample.** Choose **File ▸ Open sample data ▸ Models ▸ Plant
   growth**. The table opens, with its columns listed on the left. Like
   most samples it comes with a few reports already made; you will make
   your own.

   (Picture: The Plant growth sample open in the data grid)

2. **Ask a question.** Do the fertilizers grow plants of different
   heights? That is a comparison of groups, so choose **Compare ▸ Two or
   more groups…**.
3. **Fill in the dialog.** Choose `Height` as the data column and
   `Fertilizer` as the group column. The grey line near the bottom of the
   dialog says what will run, or what is still missing. Click **Run**.

   (Picture: The Compare groups dialog, filled in)

4. **Read the report.** The report appears at the top of the results,
   beside the table (the **Results** tab above shows the reports alone). The
   *Summary by Group* table gives each fertilizer's average height. The
   *Tests of Equal Centers* table asks whether those averages differ by
   more than chance: a small *p-value* (below 0.05, marked ★) says they
   do. Further down, *All Pairs* says which fertilizers differ from which,
   and a plot shows every plant's height by group. See the
   Glossary for the words in the report.

   (Picture: A comparison report)

5. **Explore.** Scroll down to the plot and drag a box around the tallest
   plants. The same rows are highlighted in the table beside the report.
6. **Keep it.** Choose **File ▸ Save project as…**. A project file keeps
   the data, your reports and what you selected, and reopens exactly as you
   left it.

That is the pattern for every analysis: open data, choose a menu item,
fill in the dialog, read the report.

## Finding your way around

When nothing is open, the window shows the **start page**: buttons to make
a new table, open a file, import or paste data; your recent projects with
their notes; six cards for common tasks ("What do you want to do?"; with
no table open, a card first asks you to open a file, start an empty table
or pick a sample, then opens its dialog); and
links to the sample data and the manuals, with a tip of the day.

(Picture: The parts of the main window)

- **Menu bar** (top). Every command lives here, grouped by what you want to
  do. The Menus chapter goes through them one by one.
- **Sidebar** (left). Your open tables, each with its reports and graphs
  listed under it (click one to go to it), then the columns of the current
  table. To tidy a project, right-click its name ▸ **New folder** and drag
  tables, reports or dashboards onto the folder; click a folder to bring
  its windows to the front. The small icon beside each column says what kind of values it
  holds: a ruler for measurements, a tag for categories, a numbered list for
  ordered categories. Under the columns, the row counts say how many rows
  there are and how many are selected or left out of analyses. Below them,
  **Notes** is a free-text box saved with the project, for reminders about
  where the data came from or what you found.
- **Workspace** (middle). Three tabs along the top: **Data** shows the
  table, **Results** shows the reports, and **Side by side** shows both.
- **Table tabs** (very bottom). One tab per open table. Click to switch, × to
  close.
- **Status bar** (just above the table tabs). What just happened, and a reminder of how
  to select rows in plots.

### A few habits that help

- **Hover over things.** Most buttons and menu items explain themselves in
  a tooltip.
- **Right-click things.** Columns, report sections and plots all have menus
  of their own: rename a column, save a plot as a picture, copy a table.
- **Put a plot in a document or slide.** Right-click the plot and choose
  **Copy plot**, then paste: it arrives as a sharp picture (on a Mac, as a
  vector PDF). **Save plot as…** lets you set its size on paper (for
  example 89 mm, one journal column), a serif font, bigger text or a clear
  background; the plot remembers those choices.
- **Fold what you are not reading.** Click any report heading to fold it
  away; **Collapse all** folds everything.
- **Undo.** ⌘Z (Ctrl+Z on Windows and Linux) undoes edits to the table.
- **Resize plots.** Drag the strip under a plot to make it taller, the
  strip on its right edge to make it wider, or the corner for both.
  Double-click the strip to go back to the usual size.
- **Change how a plot's axes look.** **Plot settings ▾** above each plot
  sets ranges, ticks, grid lines and titles. Its **Ticks** group turns tick
  marks inward or outward and lets you write your own tick labels (type
  `\pi` for π); **Spine** moves an axis line away from the data. You can
  also double-click a number on an axis to rename it, or right-click the
  axis ▸ **Edit tick label…**. On scatter plots, **Axis breaks** ▸ **Leave
  out** cuts a stretch out of an axis, so a few far-off values don't squash
  the rest; **Add a break** cuts another, and **Segments and marks** puts a
  piece on a log scale or gives it a fixed share of the axis.
- **Give plots the same look.** Right-click a plot you have formatted and
  choose **Copy plot style**, then right-click another plot ▸ **Paste plot
  style**: tick what goes across (ranges, ticks and grid, font and size,
  colours and markers, reference lines, key) and paste onto that plot,
  every plot in the report, or every plot in the project.
- **The ⋯ Options button** on each report adds or removes extra tables
  and plots without running the analysis again. Its **Duplicate on another
  table…** puts the same report, with its look, on another open table.
- **Drop a column on an axis.** Drag a column from the variables list onto
  the axis of a report's plot that shows the analysed column, and the
  report re-runs on the new column.
- **Filter one report.** ⋯ Options ▸ Filter rows… narrows a report to
  chosen rows; its **Filter panel** can **Step through** a column one level
  at a time, or play the levels like a slide show.
- **Look for ▾ at the bottom of a list.** It means the list goes on:
  scroll to see more choices. A scroll bar beside a list, report or table
  also means there is more to see.

On a Mac, shortcuts use ⌘; on Windows and Linux they use Ctrl.
**Help ▸ Keyboard shortcuts** lists them all.

## Which analysis do I need?

Start from your question. Each answer names the menu item to use; the
Menus pages say more about each one.

### "What do my numbers look like?"

- One column of measurements: **Describe ▸ Distribution…** (average,
  spread, a histogram, and whether it looks bell-shaped).
- Two columns against each other: **Describe ▸ Bivariate…** (a scatter
  plot and the correlation).
- Counts of categories: **Describe ▸ Multi-way table…**.
- Any picture you like: **Visualize ▸ Graph canvas…**.

### "Is there a difference?"

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

### "What drives the result?"

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

### "Some of my values are missing"

- Fill the gaps with estimates, or make several filled-in copies so that
  later results allow for the uncertainty: **Data ▸ Impute missing
  values…**.

### "How do many columns relate?"

- Which columns move together: **Multivariate ▸ Correlations…**.
- Boil many columns down to a few: **Multivariate ▸ Principal
  components…**.
- Find natural groups of rows: the clustering items under **Multivariate**.

### "What are people saying?"

- The words and themes in a column of comments: **Multivariate ▸ Text
  exploration…** or **Multivariate ▸ Topic model…**.
- Whether the comments are positive or negative: **Multivariate ▸
  Sentiment scores…**.

### "How should I run my experiment?"

Use the **Design** menu. It makes a new table listing the runs to do. You
do not need any data to start. After the experiment, fill in the results
and analyse them, usually with **Model ▸ Linear model…** or **Model ▸
Response surface (second-order model)…**.

### "Is my process stable and capable?"

- Watch a process over time: **Quality ▸ Control chart…**.
- Does it meet its specification limits: **Quality ▸ Process
  capability…**.
- Can we trust our measuring device: the gauge study items under
  **Quality**.

### "How long until it fails?"

- Time until an event, with some units still running: **Reliability ▸
  Survival curves…**.
- Fit a life distribution (such as Weibull): **Reliability ▸ Life
  distributions…**.

### "What happens next month?"

Use the **Series and Signals** menu: describe the series, then forecast it.

## File

Opening, saving and sharing your work.

(Picture: The File menu)

### Opening data

**Open…** opens a data file or a saved project. It reads comma- and
tab-separated text, JSON, and spreadsheet workbooks (Excel and
OpenDocument; each sheet becomes its own table). You can also drag files
onto the window. Text files and workbooks first show a preview of their
cells, where you click the cell where the table starts (its first column
name) if it does not start in the top-left corner, choose the sheets to
open, and press **Open**. Pictures (PNG, JPEG, TIFF, BMP) open as a table
with one row per pixel: its position `x` and `y` and its brightness, or
red, green and blue. A large picture asks whether to shrink it, cut out a
part, or keep every pixel. New data opens showing just the table; no
analysis runs until you pick one from a menu. Saved projects and sample
data open with their reports, as they were saved.

**Open recent** lists the files you opened lately.

**New data table** starts an empty table to type into, like a fresh
spreadsheet.

**New table from clipboard** makes a table from cells you copied in a
spreadsheet. The same preview shows first; if the first copied line holds
names, they become the column names.

**Add to this project** puts a new table into the project you are working
in instead of starting a project of its own: an **Empty table**, a
**Table from clipboard**, or a **Data file…**. The data file preview also has
a box, *Add to the open project*, that does the same for any file you open.

**New project from template…** builds a template's reports, graphs and
dashboards on new data: an open table, a data file, or every data file in a
folder (each becomes a table of one new project). Columns with the same
names match by themselves; for the others you choose which column stands in.

**Import from database…** reads the result of a database query (SQLite
files, PostgreSQL, MySQL or MariaDB servers). Passwords are never saved.

**Import from the web…** downloads JSON or CSV data, or a table on a web
page, from a web address.

**Open sample data** has dozens of small example tables, grouped by topic.
Each one is chosen to show off a kind of analysis and most open with a
report already made, so they are the quickest way to learn.

More detail: Documents, files and projects,
The Open Data dialog,
Data from databases and the web,
Sample data.

### Saving and sharing

**Save project** and **Save project as…** save everything in one
`.acraft` project file: the tables, your reports and their settings, and
which rows are selected or excluded. Reopening the project rebuilds every
report exactly.

**Save project as template…** saves the project's reports, graphs,
dashboards, scripts and plot styles without the data, as a `.acrafttemplate`
file, so you can build the same reports on next month's data with **New
project from template…**.

**Export data as** saves just the table, without reports, for other
programs: **Comma-separated (.csv)** or **Tab-separated (.tsv)** text,
**JSON rows (.json)**, or an **Excel workbook (.xlsx)**. Tick **Write
value labels** to write names (Female, Male) instead of the codes behind
them. **Image (PNG or 16-bit TIFF)…** turns a column back into a grey
picture, placing each row by its x and y columns.

**Export reports…** saves reports as a document to read or send: PDF, Word,
PowerPoint, a web page, Markdown or an Excel workbook. **Export all…** at
the top of the results does the same.

**Close** closes the current table (you are asked first if it has unsaved
changes). In a project of several tables it reads *Remove table from
project*, beside *Close project*.

More detail: Saving plots and results.

### Dashboards

**New dashboard** makes a page of report tiles and notes from the
project's tables, with filters that every tile follows. Good for a summary
you look at again and again.

**Export dashboard as HTML…** saves a dashboard as a web page anyone can
open in a browser.

Removing a dashboard asks first, and **Edit ▸ Undo** brings it back.

More detail: Dashboards.

### The app itself

**Settings…** changes the colour scheme (light, dark, or follow the system), the text size,
how long the pointer rests on something before its explanation pops up, how numbers and p-values are shown in reports, whether the
getting-started tips appear, and whether simple files open without the
preview.

**Quit AnovaCraft** closes the app, asking first about unsaved work.

More detail: Settings.

## Edit

Changing the values in the table, much as you would in a spreadsheet.
Click a cell and start typing, or double-click it, to change it. A value
too long for its cell opens in a larger box so you can see all of it.

(Picture: The Edit menu)

### Undo and the clipboard

**Undo** and **Redo** step back and forward through your edits (the last
500 are kept).

**Cut**, **Copy** and **Paste** move cells to and from other programs,
such as a spreadsheet. **Copy selected rows with column names** copies
whole selected rows with a header line, ready to paste elsewhere.

**Clear cells** empties the selected cells, leaving them missing.

**Fill down** copies the top cell of a selection into the cells below it.

**Select all cells** selects the whole table.

**Find…** searches the table for a value.

More detail: Editing the data grid,
Copying and pasting.

### Rows and columns

**Add rows** adds empty rows at the end: set **How many**, then click
**Add row at the end** (it says "Add 5 rows at the end" when you ask for
five). The grid's **+ Row** button opens the same choice, and can also
insert the rows above or below the rows you selected (or the cell you
clicked). In the grid, pressing Enter on the last row also adds a row.

**Delete selected rows** removes the selected rows (the number in brackets
says how many). **Delete selected columns** does the same for the columns
whose headings you clicked. You can undo either.

**Add column** adds a new column holding **Numbers**, **Text** or
**Dates**. Change **How many** at the top to add several at once, and type
a **Name** if you like (several columns are numbered: Rep 1, Rep 2, …).
One undo removes everything you added. The grid's **+ Column** button
offers the same, and can also insert the columns to the left or right of
the columns you selected (or the cell you clicked), including a formula
column. A **Formula…** column is worked out from other columns (for
example, weight divided by height squared) and updates itself when the
data change. Under the formula, **Reads as** shows it written as maths,
with fraction bars, powers and square-root signs, so you can check it. **Random values…** opens the same dialog as **Data ▸ New
random column…**.

More detail: Formula columns,
Dates and times.

## Data

Reshaping, tidying and choosing which rows to use.

(Picture: The Data menu)

**Refresh from source** reloads a table that came from a database or the
web, so it shows the latest data.

### Row states

A row can be *selected* (highlighted everywhere), *excluded* (left out of
analyses but kept in the table) or *hidden* (left out of plots). Use these
to set aside a suspect value without deleting it.

**Select all rows**, **Clear selection** and **Invert selection** change
which rows are selected.

**Exclude selected rows from analyses** leaves the selected rows out of
every report, which updates at once. **Include selected rows in analyses**
and **Include all rows in analyses** bring them back. **Select excluded
rows** shows you which rows are excluded.

**Hide selected rows in plots**, **Show selected rows in plots** and
**Show all rows in plots** do the same for plots only.

**Colour and marker by column…** gives every row a colour, and a marker
shape, from its value in a column you pick, such as one colour and shape
per treatment group. Plots that are not already grouped then draw the rows
that way and name the groups in their key. **Clear row colours and
markers** takes them off again.

More detail: Row states: select, exclude, hide.

### Row subsets

A *row subset* is a named set of rows you can switch to, such as "Men",
"Batch 3" or "After the change". While one is active, every report and
plot uses only its rows; switch back to **All rows** to use them all.

**Save selection as subset…** names the selected rows. The menu then lists
**All rows** and each saved subset with its row count; pick one to make it
active. With a subset active you can also **Select the subset's rows**,
**Replace with the selection**, **Rename subset…** or **Delete subset**.
The same menu opens from the *Subset* line under the row counts in the
sidebar.

More detail: Row subsets.

### Rows

**Sort…** puts the rows in order of one or more columns.

**Remove duplicates…** keeps the first of each set of repeated rows (you
can undo it, or make a new table instead).

**Subset and sample…** makes a new table of some of the rows: the selected
ones, the ones matching a condition, or a random sample.

**Use row as column names** turns the row you clicked into the column
names, for a table whose names ended up as its first row of values. Undo
puts the row back.

### Reshape

**Stack columns…** turns several side-by-side columns (say, a column per
month) into one long column with a label saying where each value came
from. **Unstack by label…** does the reverse. **Transpose…** swaps rows
and columns.

**Distance matrix…** makes a new table of how different every pair of rows
is, by a measure you choose (Gower handles a mix of numbers and
categories). Use it for **Principal coordinates…**, **Dimension
reduction…** or **Clustering…**.

### Merge

**Combine rows…** puts tables with the same columns one under the other.
**Join…** matches rows of two tables on a shared key column (such as a
customer number) and puts their columns side by side.

### Other tools

**Summarize by group…** makes a new table of statistics (counts, means
and so on) for each group, such as the average height per fertilizer.

**Recode values…** changes values or merges categories, for example
turning "NY", "N.Y." and "New York" into one spelling.

**Value labels…** gives names to codes, such as 1 = Female and 2 = Male.
The table keeps the numbers, but the grid, reports and plots show the
names. The **Stored values** button above the grid shows the numbers again.

**Impute missing values…** fills gaps in the data with sensible estimates,
so methods that need complete rows can use more of them. Its *multiple
imputation* methods make several filled-in copies instead, stacked in a new
table; analyses of that table combine the copies, so their results allow
for not knowing the missing values.

**New random column…** adds a column of random numbers from a chosen
distribution, for simulations or random assignment.

**New rank column…** adds a column numbering the values of a column from
smallest (1) to largest, optionally within each group, with a choice of
how equal values are numbered (shared average, lowest, highest, no gaps,
or in row order).

More detail: Table operations,
Rank columns,
Missing values and imputation,
Multiple imputation.

## Describe

Getting to know your data before testing anything: what is typical, how
spread out it is, what shape it has.

(Picture: The Describe menu)

**Distribution…** describes one column at a time. For measurements you
get the average, spread and percentiles, a histogram, a box plot and a
check of whether the values follow a bell curve. For categories you get
counts and percentages. This is usually the first thing to run on new
data. *Try it:* sample *Michelson light speed*.
More detail

If some values are only known to be below a limit (lab results reported as
"< 2", called non-detects) or above one, choose how they are marked under
*Censored values* in the same dialog. The report then estimates the
distribution and its percentiles without pretending those values are exact.
*Try it:* sample *Well water arsenic*.
More detail

**Identify distribution…** tries fourteen common distribution shapes
(normal, lognormal, Weibull and others) on one column and ranks how well
each fits. Useful when you need to know the shape, for example before
working out how often a part will fall out of specification. *Try it:*
sample *Seal strength*.
More detail

**Bivariate…** looks at two columns together: a scatter plot and the
correlation for two measurements (for a fitted line, use **Model ▸ Linear
model…**), a
side-by-side comparison for a measurement and a category, or a table of
counts for two categories. *Try it:* sample *Norris calibration*.
More detail

**Multi-way table…** counts rows by several categories at once (for
example, outcome by treatment within each clinic), with percentages,
subtotals and averages. *Try it:* sample *Multi-centre trial*.
More detail

**Multiple response…** counts the answers to a "tick all that apply"
question, such as "How did you hear about us?". The answers can be in one
column as lists (Email; Phone) or in one yes/no column per choice. You see
how many people ticked each choice, and, with a group column such as age
group, whether the groups tick choices differently. *Try it:* sample
*Visitor survey*.
More detail

**Summarize by group…** makes a table of counts, means and other
statistics for each group; it is the same command as **Data ▸ Summarize
by group…**.

**Area under a curve…** measures the area under measurements taken over
time (or dose, or wavelength), such as a drug's concentration in the blood
after a dose. It joins the average at each time with straight lines, gives
the area above a baseline with its margin of error, and finds the peaks.
It can also save the measurements rescaled to 0–100% or as a share of the
total. *Try it:* sample *Plasma concentration*.
More detail

**Pharmacokinetic parameters…** summarises how a drug's concentration in
the blood rises and falls after a dose, for each subject: the peak and when
it happens, the area under the curve, the half-life, and (given the dose)
clearance and volume, then averages them by treatment. *Try it:* sample
*PK profiles*.
More detail

**Distribution calculator…** works out probabilities from a named
distribution, using no data from the table, such as "what share of a normal
distribution lies above 2?". It draws the curve and shades the area.
More detail

## Compare

Testing whether there is a real difference: between a group and a target,
between groups, or before and after. Each report gives the size of the
difference with a confidence interval and a p-value (see the
Glossary).

(Picture: The Compare menu)

**One sample vs a value…** asks whether a column's average (or median, or
spread) differs from a target, such as "do our bags weigh 500 g on
average?". It can also show that a value is *close enough* to a target
(equivalence). *Try it:* sample *Michelson light speed*.
More detail

**Two or more groups…** compares a measurement across groups, such as
three suppliers or two teaching methods. It runs the usual tests (t test,
analysis of variance) and versions that do not assume a bell curve, checks
whether the groups are equally spread, and says which pairs of groups
differ. *Try it:* sample *Silicon resistivity* or *Plant growth*.
More detail

**Two groups, many variables…** runs the same two-group test on many
columns at once, such as thirty genes measured in treated and untreated
samples. With many tests some p-values are small by chance, so it also
gives adjusted p-values and a volcano plot (size of difference against
significance) that picks out the columns worth a closer look. *Try it:*
sample *Gene screen*.
More detail

**Paired or repeated measures…** is for the same people or items measured
more than once, such as before and after a treatment. Pairing removes the
differences between individuals, so it finds smaller effects. *Try it:*
sample *Sleep drug trial*.
More detail

**Factorial repeated measures…** is for people or items measured under
every combination of two or three conditions (task and cue, time and
dose), possibly in separate groups such as age groups. It tests each
condition, each group and their combinations, checks the assumption of
equal spread with corrections when it fails, and can test one factor
within each level of another. *Try it:* sample *Reaction times by task and
cue*.
More detail

**Nested comparison…** compares groups when each subject gives several
measurements, such as several cells from each mouse. It counts subjects,
not measurements, so the test is not fooled by many measurements of a few
subjects, and shows how much the subjects differ. *Try it:* sample
*Dendrite lengths per mouse*.
More detail

**Proportions…** compares percentages, such as pass rates or survey "yes"
answers, against a target or between groups. *Try it:* sample *Customer
churn*. More detail

**Event rates…** compares how often events happen per unit of time or
exposure, such as accidents per thousand hours.
More detail

**Goodness of fit for counts…** checks whether counts in categories match
expected proportions (are the dice fair?), or whether counts follow a
Poisson pattern. *Try it:* sample *Horse-kick deaths*.
More detail

**Crossover trial and equivalence…** analyses a study in which each
subject gets both treatments in turn, and checks whether two products are
equivalent (for example, a generic medicine). *Try it:* sample
*Bioequivalence crossover*.
More detail

**Stratified tables and exact tests…** compares yes/no outcomes between
two groups across several strata (such as clinics), and gives exact tests
for small tables of counts. *Try it:* sample *Multi-centre trial*.
More detail

**Permutation test…** compares two groups, or tests a correlation, by
shuffling the data many times instead of assuming a bell curve. A good
choice for small or oddly shaped samples. *Try it:* sample *Repair times*.
More detail

**Adjust and combine p-values…** takes a column of p-values from tests
you have already run (one per row) and adjusts them for the number of
tests, by the usual methods (Bonferroni, Holm, false discovery rate and
others). It also combines them into one overall test: is there any effect
at all? *Try it:* sample *Trial endpoints*.
More detail

The **Bayesian** submenu (also under Model) holds four analyses that
describe what the data say as probabilities, starting from a stated prior
belief, instead of p-values.

**Bayesian estimation…** gives the likely range of a mean, a proportion
or an event rate (or of the difference between two groups) as a
probability distribution, and answers questions such as "how likely is the
new treatment to be better by at least 5 points?". When a group had no
events, the average of a ratio does not exist and shows as —; read the
median and the intervals instead. *Try it:* sample
*Response rates by site*.
More detail

**Bayes factor tests…** weigh the evidence for a difference or a
correlation against the evidence for none. Unlike a p-value, a Bayes factor
can also show that there is probably *no* effect. *Try it:* sample *Stroop
reaction times*.
More detail

**Bayesian linear model or ANOVA…** compares every combination of the
chosen effects and says how likely each effect is to matter.
More detail

**Bayesian regression or hierarchical model…** fits regression models
(including yes/no outcomes and counts) for grouped data, such as patients
within hospitals, by simulation, and checks that the simulation worked.
*Try it:* sample *Eight schools*.
More detail

**Power and sample size…** helps you plan before collecting data: how many
measurements you need to detect a difference of a given size, or how
likely a planned study is to find it. It also tells you how many values a
tolerance interval needs, including one that assumes nothing about the
shape of the data. It uses no data (any open table will do, even a new
empty one).
More detail

## Model

Explaining or predicting one column (the *response*) from others (the
*predictors* or *factors*). A model is an equation; the report says how
well it fits, which predictors matter, and lets you try "what if"
settings.

(Picture: The Model menu)

### Linear models

**Linear model…** is the workhorse. It explains a measurement from one or
more other columns, which can be measurements or categories, and from
their combinations. The report gives the equation, how much of the
variation it explains (R²), which predictors matter, a plot of the fit, and a
Response Explorer where you drag each factor's setting and watch the
prediction change. Plots of what is left over (residuals) are in the
report's ⋯ Options menu.
Analysis of variance and covariance are linear models too. Under *More
options* a measured predictor can bend (a spline term), and the report's
⋯ Options can compare one factor's levels at each level of another
(comparisons within slices) or each group's slope. *Try it:*
sample *Longley economy*. More detail

**Response surface (second-order model)…** fits a curved model with
squared terms and finds the settings that give the highest or lowest
response, with coloured contour maps of the surface. Used after a designed
experiment. *Try it:* sample *Reaction yield*.
More detail

**Mixture model…** is for blends whose ingredients add up to 100%, such as
a recipe or an alloy, and draws the triangle plot of the blend.
*Try it:* sample *Yarn elongation*.
More detail

**Stepwise selection…** chooses which of many candidate predictors to keep,
adding or dropping one at a time. Tick *Several responses* to run a
separate search for each of several responses and explore them together.
*Try it:* sample *Hald cement*.
More detail

**Best subsets…** fits every combination of the candidate predictors and
lists the best model of each size, so you can weigh simplicity against
fit. *Try it:* sample *Hald cement*.
More detail

**Screen many responses…** fits the same model to many response columns
at once, such as the counts of thousands of genes against a treatment, and
lists which responses the factors affect, with p-values adjusted so that
few of those listed are false alarms. A volcano plot shows each response's
effect against its strength of evidence; any response's full report opens
from ⋯ Options. *Try it:* sample *Transcript counts*.
More detail

### Generalized, nonlinear and mixed

**Generalized linear model…** handles responses a linear model cannot:
yes/no outcomes (logistic regression), counts (Poisson regression), and
positive skewed measurements. *Try it:* sample *Customer churn*.
More detail

**Ordinal or nominal logistic regression…** predicts a category with more
than two levels: ordered ones, such as a rating from poor to excellent, or
unordered ones, such as the way people travel to work. *Try it:* samples
*Service ratings* and *Commute modes*.
More detail

**Nonlinear regression…** fits a curve whose shape you choose, such as
growth that levels off, a dose–response curve or a peak. It can also fit
several groups at once and test whether they differ, read unknown samples
off a standard curve, set aside outliers, and fit a rate equation
(differential equation). *Try it:* samples *Seedling growth*, *Compound
potency*, *Assay standard curve* and *Substrate depletion*.
More detail

**Curve templates…** fits a ready-made curve for a laboratory assay: an
immunoassay standard curve, receptor binding, enzyme kinetics and
inhibition, a dose–response curve, or growth, decay and survival. Choose
the kind of experiment and the template, then your columns; the starting
values come from your data. Besides the fit, the report gives what the
assay needs, such as Ki, kcat, EC50 or a half-life, a check of the
standards against acceptance limits with unknown samples read off the
curve, or the best-supported inhibition mechanism. *Try it:* samples
*ELISA plate*, *Enzyme inhibition*, *Antagonist shift*, *Potency assay*
and the other curve template samples.
More detail

**Dose–response (probit)…** finds the dose that affects a given share of
subjects, such as the dose that kills half the insects. *Try it:* sample
*Rotenone*. More detail

**Treatment effects (observational data)…** estimates what a treatment
did when it was not given at random, by comparing treated and untreated
rows that were alike before treatment (matching or weighting on each row's
chance of being treated). It shows whether the groups were made alike
(balance), the effect with its interval, and the effect within subgroups.
*Try it:* sample *Cardiac rehabilitation*.
More detail

**Mixed model…** is a linear model for grouped data, such as several
measurements per patient or per batch, where the groups are a sample from
a larger population. It also handles ordered ratings (choose the
*Ordinal* family), rows that stand for several observations (*Frequency*) or
count more than others (*Weight*), groups whose scatter differs (*Residual
variance differs by*) and robust standard errors. *Try it:* sample
*Furnace coatings*, or *Wine bitterness* for ratings.
More detail

The **Bayesian** submenu is also under Compare: **Bayesian estimation…**,
**Bayes factor tests…**, **Bayesian linear model or ANOVA…** (every
combination of effects compared, with how likely each effect is to matter)
and **Bayesian regression or hierarchical model…** (regression for grouped
data, such as patients within hospitals, fitted by simulation; *try it:*
sample *Eight schools*). See Compare and the
Reference Manual.

### Method comparison and stability

**Method comparison…** checks whether two measuring methods agree, allowing
for error in both. If one method rises while the other falls, the report
notes that the robust (Passing–Bablok) line cannot give a full range for
its slope. *Try it:* sample *Glucose methods*.
More detail

**Shelf life…** estimates how long a product stays within specification
from stability-test data. With only two test times per batch it cannot
check whether batches degrade at different rates, so it assumes they do not
and says so. If your batches are a sample of future production, choose
*Batches are: A random sample of batches* for a shelf life a new batch can be
expected to meet. *Try it:* sample *Tablet stability*, or *Capsule stability*
for random lots.
More detail

### Predictive models

These aim to predict well rather than to explain. Most hold back part of
the data to check predictions honestly (validation).

**Penalized regression (lasso, ridge)…** is a linear model that shrinks
weak predictors toward zero, which helps when there are many predictors.
The lasso can drop predictors entirely. *Try it:* sample *Longley
economy*. More detail

**Decision tree…** splits the data into groups with simple rules ("income
above 40,000 and fewer than two loans"), drawn as a tree that is easy to
explain. *Try it:* sample *Loan applications*.
More detail

**Random forest…** averages hundreds of trees for better predictions and
ranks the predictors by importance. *Try it:* sample *Loan applications*.
More detail

**Boosted trees…** builds trees one after another, each fixing the
mistakes of the ones before. Often the most accurate of the tree methods.
*Try it:* sample *Loan applications*.
More detail

**Partial least squares…** predicts from very many related columns, such
as the readings of a spectrum, by first combining them into a few
summaries. *Try it:* sample *Fuel spectra*. More detail

**Support vector machine…** separates classes (or predicts a measurement)
with a flexible boundary. *Try it:* sample *Pump sensors*.
More detail

**Adaptive regression splines…** builds a model from straight-line pieces
with bends, finding curves and interactions on its own. *Try it:* sample
*Fuel consumption*. More detail

**Neural network…** learns a flexible prediction from layers of simple
units, which can capture curves and interactions no one specified. It
checks itself on held-back rows to avoid memorizing the data. *Try it:*
sample *Polymer batches (simulated)*.
More detail

**Gaussian process…** fits a smooth surface through a measured response and
says how sure it is everywhere: sure near your runs, less sure between and
beyond them. It suits small experiments and computer simulations. *Try it:*
sample *Reaction yield (simulated)*.
More detail

**Discriminant analysis…** assigns rows to known groups from their
measurements, such as telling honey's origin from its chemistry. (Also on
the Multivariate menu.) *Try it:* sample *Honey origins*.
More detail

### Compare and optimize

**Compare models…** puts the predictions of several saved models side by
side, with measures of accuracy, to pick the best. *Try it:* sample *Loan
applications*. More detail

**Model search…** fits every suitable predictive model to one response,
ranks them by how well they predict the held-back rows, and opens the
winner's full report. A quick way to find a good starting model.
*Try it:* sample *Polymer batches (simulated)*.
More detail

**Several responses (desirability, simulation)…** handles several
responses at once (with the same terms for all, or each response's own
terms chosen stepwise), finds settings that balance all your goals, and
simulates how variation in the settings spreads to the results. *Try it:*
sample *Coating formulation*. More detail

## Multivariate

Looking at many columns at once: how they move together, how to sum them
up in a few numbers, and how rows fall into groups.

(Picture: The Multivariate menu)

### Relationships

**Correlations…** measures how strongly each pair of columns moves
together, from −1 to +1, as a table and a colour-coded map. *Try it:*
sample *Flower measurements*.
More detail

**Scatterplot matrix…** draws a scatter plot for every pair of columns in
one grid, so patterns and odd rows stand out.
More detail

**Outlier distances…** finds rows that are unusual in their combination
of values, even when no single value looks odd.
More detail

**Item analysis…** checks whether the questions of a questionnaire or test
measure the same thing consistently (Cronbach's alpha). *Try it:* sample
*Course evaluation*. More detail

### Reduce dimensions

**Principal components…** combines many related columns into a few new
ones that keep most of the information, and plots rows and columns on
them. *Try it:* sample *Flower measurements*.
More detail

**Factor analysis…** looks for a few hidden traits behind many observed
scores, such as verbal and numerical ability behind a set of tests.
*Try it:* sample *Ability tests*.
More detail

**Correspondence analysis…** draws a map of a table of counts, placing
categories that go together close to each other. *Try it:* sample
*Customer survey*. More detail

**Dimension reduction (MDS, t-SNE, UMAP)…** draws a two-dimensional map in
which similar rows sit close together, from many columns or from a table
of distances. *Try it:* samples *Expression profiles* and *Town
distances*. More detail

**Principal coordinates…** draws a map of the rows from any measure of how
different they are, including one for a mix of numbers and categories
(Gower), or from a table of distances, and shows how much of the
differences each axis captures. *Try it:* samples *Shrub traits* and *Town
distances*.
More detail

### Clusters

**Clustering…** sorts rows into groups of similar rows, either a chosen
number of groups (k-means) or a family tree of groups (hierarchical).
Not sure how many groups? k-means can compare several numbers of groups
and suggest one. If you already have a guess at the group centers, type
them in (or read them from a table) and k-means starts from there. The
family tree can also use other distances, such as Gower for columns that
mix numbers and categories, or a table of distances.
More detail

**Gaussian mixture clustering…** finds groups that may overlap and have
different shapes, and gives each row a probability of belonging to each.
*Try it:* sample *Cell populations*.
More detail

**Latent class analysis…** finds hidden groups from yes/no or category
answers, such as types of patients from a symptom checklist. *Try it:*
sample *Symptom checklist*.
More detail

### Groups

**Multivariate analysis of variance…** compares groups on several
measurements at once. *Try it:* sample *Plastic film*.
More detail

**Discriminant analysis…** learns how known groups differ and assigns new
rows to a group. *Try it:* sample *Honey origins*.
More detail

### Curves

**Functional data…** treats each row group's readings over time (or
another scale) as one smooth curve, such as a temperature profile per
batch. It smooths the curves, can line up their peaks, finds the main ways
they differ from each other, and can relate those to factors such as
settings in an experiment. *Try it:* sample *Exotherm profiles*.
More detail

### Text

**Text exploration…** breaks a column of free text (comments, reviews,
notes) into words and short phrases, counts how often each appears, and
shows the most common terms in a table and a word cloud. It can also find
the main themes behind the wording and save them as columns for other
analyses. *Try it:* sample *Espresso machine reviews*.
More detail

**Topic model…** finds a chosen number of topics, each a set of words that
tend to appear together, and says how much of each document belongs to
each topic. *Try it:* sample *Espresso machine reviews*.
More detail

**Sentiment scores…** scores each document from negative to positive using
a word list that also handles "not" and "very", and compares the scores
across groups. *Try it:* sample *Espresso machine reviews*.
More detail

## Design

Planning an experiment: which combinations of settings to run so that a
small number of runs tells you as much as possible. Each design item makes
a new table listing the runs in a random order, with an empty column for
each result. You need no data to start. Run the experiment, type in the
results, then analyse them with **Model ▸ Linear model…**, **Response
surface**, **Mixture model** or **Several responses** (a split-plot design
fills in **Model ▸ Mixed model…** for you). The items under *This table*
work on a table you already have.

(Picture: The Design menu)

More detail on everything here: Design in the Reference
Manual.

### Factorial and screening designs

**Full factorial (2 levels)…** tries every combination of a low and a high
setting of each factor. Clear and simple for up to about five factors.

**Full factorial (3 levels)…** does the same with low, middle and high
settings, which shows curvature.

**Fractional factorial…** runs a well-chosen fraction of the full set (say
16 runs instead of 64), giving up some information on interactions.

**Full factorial (mixed levels)…** allows a different number of settings
per factor (for example, 3 machines × 2 temperatures × 4 materials).

**Plackett–Burman…** and **Definitive screening…** check many factors in
very few runs, to find the handful that matter. Definitive screening also
spots curvature; you can add a few extra runs or center runs, and compare
the choices, for a more reliable answer.

**Screening with categorical factors…** does the same when some factors
are categories, such as three suppliers or two machines, mixed with
numeric ones.

### Response surface designs

**Central composite…** and **Box–Behnken…** are designs for fitting a
curved model and finding the best settings, once you know which factors
matter. Analyse them with **Model ▸ Response surface (second-order
model)…**.

### Custom designs

**D-optimal (coordinate exchange)…** and **I-optimal (coordinate
exchange)…** build a design for exactly the model, number of runs and
restrictions you have, when no standard design fits. D-optimal is best for
estimating effects, I-optimal for predicting.

**Split-plot (hard-to-change factors)…** is for experiments where one
factor, such as an oven temperature, is slow to change, so runs are
grouped by it.

### Space-filling designs

For computer simulations and other experiments with no random error, these
spread the runs evenly through the region: **Latin hypercube
(maximin)…**, **Maximin distance…**, **Minimax distance…** and **Uniform
(centered L2 discrepancy)…**. *Try it:* sample *Borehole water flow*.

### Mixture

For recipes whose ingredients add up to 100%: **Simplex lattice…** and
**Simplex centroid…** cover the whole range of blends, and **Extreme
vertices…** handles lower and upper limits on each ingredient. Analyse
them with **Model ▸ Mixture model…**.

### Robust design

**Robust design (orthogonal arrays)…** plans an experiment to find
settings that work well despite noise you cannot control, such as
humidity. **Signal-to-noise analysis…** analyses it. *Try it:* sample
*Moulding shrinkage*.

### Measurement

**Gauge study worksheet…** makes the run sheet for a measurement-system
study: which operator measures which part, and when. (Also on the Quality
menu.)

### This table

**Declare table as design…** tells the app that a table you already have
is an experiment, so it can check it and analyse it like one.

**Augment design…** adds runs to an existing design, for example to
untangle effects that were mixed up or to add a curved term. For a
split-plot design it can add new whole plots.

**Simulate responses…** makes up results for a design from random
effects, a few of them large, so you can practise the analysis and see
whether the design finds them before you run the experiment.

**Evaluate design…** shows how good a design is before you run it: how
precisely it estimates each effect and which effects are mixed up.
*Try it:* sample *Design comparison*.

**Factorial analysis…** analyses a two-level experiment, including one
with no repeated runs, and shows which effects stand out from the noise.
*Try it:* sample *Filtration rate*.

**Propose next runs…** looks at the runs you have made so far and suggests
the next few to try, where the best result is most likely to be found. Add
them to the table, run them, type in the results and ask again, round after
round. *Try it:* sample *Reaction yield (simulated)*.
More detail

## Quality

Keeping a process stable, checking it meets its specifications, and making
sure your measurements can be trusted.

(Picture: The Quality menu)

### Control charts

A control chart plots a measurement in time order with limits showing how
far a stable process normally wanders. Points outside the limits, or
unusual runs of points, are flagged for a look.

**Control chart…** is the standard chart for measurements: individual
values, or averages and ranges of small samples. It can show different
limits for each phase of a process (before and after a change), draw the
limits at another number of σ, show only the latest points, or use limits
saved from an earlier period. *Try it:* sample *Fill weights*. More detail

**Count and rare-event chart (p, np, c, u, p′, u′, g, t)…** charts counts
instead of measurements: the share of defective items, the number of
defects, or the time between rare events. For defects per area or length
(u charts) the size can be a fraction, such as 2.5 m² of panel. *Try it:*
sample *Daily inspection*. More detail

**EWMA chart…** and **CUSUM chart…** catch small, lasting shifts sooner
than a standard chart, by building up evidence over several points.
More detail

**Multivariate control chart…** watches several related measurements in
one chart. *Try it:* sample *Bottle filling*.
More detail

**Principal-components control chart…** watches many related
measurements, such as a machine's sensors, through a few summary patterns
learnt from a period of normal running. One chart shows rows that go
unusually far along those patterns; a second shows rows that break them.
*Try it:* sample *Reactor sensors*.
More detail

**Monitoring alert and action limits…** sets alert and action levels for
environmental monitoring (colony counts, positive plates or particle
counts) from your own data, and charts each location against them with
early warnings. *Try it:* sample *Clean-room monitoring*.
More detail

### Process capability

**Process capability…** compares the process spread with its
specification limits and reports indices such as Cpk and the expected
share out of specification. It can also compare groups such as machines
or shifts side by side, and add a column saying whether each value is in
specification. *Try it:* sample *Fill weights*.
More detail

**Capability summary page…** puts capability, control charts and plots for
one measurement on a single page. *Try it:* sample *Tablet hardness*.
More detail

**Between/within capability…** is capability for processes made in
batches, separating the variation between batches from the variation
within them. *Try it:* sample *Coating batches*.
More detail

**Attribute capability…** is capability for pass/fail or defect-count
data. *Try it:* sample *Supplier lots*.
More detail

### Measurement systems

Before trusting a measurement, check the gauge that takes it.

**Gauge R&R…** splits measurement variation into repeatability (the same
person measuring again) and reproducibility (different people), and
compares it with the part-to-part variation. *Try it:* sample *Gauge
study*. More detail

**Gauge study worksheet…** makes the run sheet for such a study. (Also on
the Design menu.)

**Bias and linearity…** checks whether a gauge reads too high or too low,
and whether that changes across its range. *Try it:* sample *Gauge
linearity*. More detail

**Gauge repeatability (Cg, Cgk)…** checks a gauge by measuring one master
part many times. It can also check whether readings depend on the side
the gauge approaches from (hysteresis) and whether they creep over time
(drift). *Try it:* sample *Gauge repeatability*.
More detail

**Intraclass correlation…** measures how consistently different raters or
instruments score the same items.
More detail

**Attribute agreement…** checks how well inspectors agree with each other,
with themselves and with the right answer when they grade items by eye.
*Try it:* sample *Visual inspection*.
More detail

**Attribute gauge bias and repeatability…** checks a go/no-go gauge.
*Try it:* sample *Go/no-go gauge*.
More detail

### Acceptance sampling

**Acceptance sampling plan…** works out how many items to inspect from a
lot, and how many failures to allow, to accept good lots and reject bad
ones. *Try it:* sample *Supplier lots*.
More detail

### Problem-solving tools

**Analysis of means…** compares each group's average with the overall
average on a chart, flagging groups that stand out. *Try it:* sample
*Heat treatment*. More detail

**Pareto chart…** ranks problem types from most to least common, so you
can tackle the big few first. *Try it:* sample *Solder defects*.
More detail

**Cause-and-effect diagram…** draws a fishbone diagram of possible causes.
More detail

**Multi-vari chart…** shows at a glance where variation comes from: within
a part, between parts or over time. *Try it:* sample *Shaft diameters*.
More detail

## Reliability

How long things last, and when they fail. Life data are special because
some units are often still working when the study ends (*censored*): we
know they lasted at least that long. These methods use those rows too.

(Picture: The Reliability menu)

### Life data

**Survival curves…** draws the share of units (or patients) still
surviving over time, and tests whether groups survive differently (or
get steadily better or worse across ordered groups), with the number
still at risk listed under the plot.
*Try it:* sample *Leukemia remission*.
More detail

**Life distributions…** fits a life distribution, such as Weibull or
lognormal, to failure times, and estimates things like the time by which
10% will have failed. *Try it:* sample *Bearing life test*.
More detail

**Competing risks…** handles units that can fail in more than one way,
estimating the chance of each kind of failure. *Try it:* sample *Pump
failure modes*. More detail

### Life data with predictors

**Proportional hazards…** and **Parametric survival regression…** relate
survival to other columns (treatment, age, temperature): which ones
shorten or lengthen life, and by how much. *Try it:* samples *Leukemia
with covariates* and *Capacitor life test*.
More detail

**Life–stress models…** uses tests run at high stress (heat, voltage) to
predict life at normal use. *Try it:* sample *Device-A accelerated life
test*. More detail

**Fatigue (stress–life) curves…** fits the curve of cycles to failure
against stress from fatigue tests, counting specimens that never broke
(run-outs), and gives the stress a part can take for a given number of
cycles. *Try it:* sample *Steel fatigue*.
More detail

**Degradation…** predicts failure times from measurements that wear down
over time, before anything has actually failed. *Try it:* sample *Laser
degradation*. More detail

### Field and repair data

**Warranty returns…** forecasts future returns from the sales and returns
recorded so far. *Try it:* sample *Monthly warranty returns*.
More detail

**Repairable systems…** analyses machines that are repaired and put back
into service: are repairs becoming more or less frequent? *Try it:* sample
*Pump repairs*. More detail

**Reliability growth…** tracks whether a product under development is
failing less often as fixes go in. *Try it:* sample *Prototype failures*.
More detail

### Planning

**Test plans…** works out how many units to test, and for how long, to
demonstrate a reliability target. It uses no data (any open table will do,
even a new empty one).
More detail

## Series and Signals

Data recorded in order: series over time (daily sales, monthly demand),
signals sampled along an axis (spectra, vibration traces) and images.
Describe the pattern, forecast what comes next, filter out noise and find
the frequencies hidden in the data. The rows must be in time (or x)
order.

(Picture: The Series and Signals menu)

### Explore

**Series description…** plots the series and shows how each value relates
to earlier ones (autocorrelation), which reveals trends and seasonal
cycles. *Try it:* sample *Monthly electricity demand*.
More detail

**Decomposition…** splits a series into trend, seasonal pattern and what
is left over. *Try it:* sample *Monthly electricity demand*.
More detail

**Trend models…** fits a straight line or curve through time, such as an
S-shaped adoption curve, and extends it. *Try it:* sample *Product
adoption*. More detail

**Cross-correlation…** finds whether one series leads another, such as
advertising spend leading sales by two weeks. *Try it:* sample
*Advertising and sales*.
More detail

### Forecasting models

**Exponential smoothing…** forecasts by weighting recent values most, with
trend and seasonal parts if needed. It can pick the best form for you.
*Try it:* sample *Hotel occupancy*.
More detail

**ARIMA…** is a flexible family of forecasting models built from a
series' own past values and past errors; it can also search for the best
model automatically. It can also use another column that drives the
series, such as fuel feed driving a kiln's temperature, and forecast from
planned values of it. *Try it:* samples *Reactor temperature* and *Kiln
fuel feed*.
More detail

### Signals and spectra

**Smoothing…** irons out the noise in a signal measured point by point,
such as a sensor trace, and draws the raw and smoothed signals together.
*Try it:* sample *Machine vibration*.
More detail

**Derivatives and integrals…** gives the rate of change and the running
total (area under the curve) of y against x, and can resample the curve at
evenly spaced x. *Try it:* sample *Machine vibration*, where integrating
acceleration gives velocity.
More detail

**Baseline correction…** removes the slowly varying background under a
spectrum or chromatogram so the peaks sit on zero. *Try it:* sample
*Raman bands*.
More detail

**Peak finding…** lists the peaks that stand out from the noise, with
their positions, heights, widths and areas. *Try it:* sample *ECG-like
signal*, where the peaks are the heartbeats.
More detail

**Peak fitting…** separates peaks that overlap by fitting a peak shape to
each, gives their centres, heights, widths and areas with standard errors,
and says how many peaks the data support. With many curves it fits them all
and makes one table. *Try it:* sample *Raman bands*.
More detail

For signals recorded many times a second (sound, vibration, sensors). Give
an x (time) column or the number of samples per second, and a group column
if several runs are stacked in one column.

**Fourier spectrum…** shows which frequencies a signal is made of, such as
a 50 Hz hum, lists the strongest ones, and can draw a spectrogram of how
they change over time. *Try it:* sample *Test bench signals*.
More detail

**Digital filter…** keeps or removes a range of frequencies: smooth away
fast noise, take out slow drift, or cut one interfering frequency, and
save the result as a column. *Try it:* sample *Test bench signals*.
More detail

**Convolution and correlation…** finds the delay at which two signals
match best, finds repeating patterns in one signal, smooths or un-blurs a
signal with a kernel column, or traces its envelope. *Try it:* sample *Test
bench signals*.
More detail

**Wavelets…** splits a signal into detail at each scale, removes noise
while keeping sharp steps, and draws a scalogram of which frequencies are
present at each moment. *Try it:* sample *Test bench signals*.
More detail

### Images

For pictures opened as data (one row per pixel, with x and y columns):
microscope and camera images, surface scans, scanned prints.

**Image spectrum…** shows the repeating patterns in an image as bright
spots in its 2-D spectrum, lists the strongest with their spacing and
direction, and shows how detail falls off from coarse to fine. Radial
profiles cut the spectrum along the directions you type (across and up
the image by default), so a pattern that runs one way shows as peaks in one
profile and not the other. *Try it:*
sample *Machined surface scan*.
More detail

**Image frequency filter…** smooths or sharpens an image, removes slow
shading, or notches out a regular pattern such as tool marks, and saves the
filtered image as a column. *Try it:* sample *Machined surface scan*, where
removing the tool marks reveals a pit.
More detail

**Image registration and deblurring…** sharpens an image blurred by a
known amount, finds how far a second image is shifted and lines it up, or
shows the image's 2-D autocorrelation. *Try it:* sample *Blurred and
shifted scene*.
More detail

## Visualize

Building your own graphs. (Every analysis draws its own plots too; this
menu is for pictures you design yourself.)

(Picture: The Visualize menu)

**Graph canvas…** opens a blank graph. Drag columns onto the X and Y
shelves, and onto colour, size or panel shelves to split the picture by
group. Then add layers such as points, lines, box plots, histograms or a
fitted curve. Points you select are highlighted everywhere, as in reports.
You can also drop a column straight onto the graph: while you drag, the
edges and centre light up (bottom for the horizontal axis, left for the
vertical axis, top and right to split into panels, centre for colour).
The **Row filter** can step through a column one level at a time, with a
**▶ Play** button, to watch the graph change level by level.
The canvas's **Chart** list also offers pie and donut charts, radar,
polar and ternary charts, wind roses, waterfall charts, ridgeline plots and
vector fields; a lollipop chart is a choice in the bars layer's settings.
*Try it:* samples *Harbour weather*, *Household budget* and *Soil texture*.

Put a column on the **Right axis** shelf to draw it against a second scale
on the right. **Plot settings** can leave gaps in an axis (axis breaks,
each piece with its own scale and share of the axis if you like) and takes
titles with simple math, such as `x_1`, `x^2`, `\alpha` or
`\frac{a}{b}`. **Reference lines…** (in Plot settings, the plot's
right-click menu, or at the end of the canvas's Chart row) draws lines
across a plot at a value you type or at the mean, median, a percentile,
mean ± some SDs or control-chart limits, which move when the data change.
It also shades bands, such as a specification range, behind the data.
More detail

On the canvas, the **Chart** list also offers ready-made pictures: a
*clustered heat map* (a grid of colored cells with similar rows and columns
put together), an *estimation plot* (how far each group's average is from a
reference group, with a range of plausible values), and *Sankey*, *chord*
and *network* diagrams of flows between categories. The *Brackets* layer
marks which groups differ, with p-values or stars (its **…** menu picks
which pairs to show, the star cut-offs and an "ns" label); the *Hulls* layer draws
an outline around each group of points. *Try them:* samples *Reading
trial*, *Parcel flows* and *Regional climate*.

Right-click a graph to **add text, an arrow, a line or a rectangle**; drag
it to move it, double-click to edit it, or click it and nudge it with the
arrow keys. **Drawing order** (at the end of the layers) puts one layer in
front of another, and **Edit key…** renames, hides or reorders the entries
of the key.

**Graph from pipeline file…** rebuilds a graph you saved earlier, on the
current table, as long as it has columns of the same names. Handy for
making the same chart every month.

**Scatterplot matrix…** draws a scatter plot for every pair of the
columns you choose; it is the same command as in the Multivariate menu.

**3-D scatter plot…** places rows in a box by three columns; drag to turn
it. *Try it:* sample *Hillside survey*.
More detail

**Contour and surface plot…** draws a map of one column over two others,
as a coloured contour map or a 3-D surface. The plot's **Plot settings**
choose the colours (or plain lines). *Try it:* sample *Hillside survey*.
More detail

**Image plot…** shows a column as a picture, one coloured square per row
placed by its x and y columns: the way to look at a table opened from an
image file. Drag a box over part of the picture to select those rows.
More detail

**Figure page…** puts several plots on one page for a paper or poster,
lettered A, B, C. Right-click a plot and choose **Add to figure page** to
add it, then set the page size (in millimetres or inches) and the number of
columns, and save the page as one picture or PDF. *Try it:* sample
*Longley*, with its quantile plot and scree plot.
More detail

## Scripts

Repeating work automatically. Scripts are written in Rhai, a small
programming language. You do not need to write one to use the app;
recording is the easy way in.

(Picture: The Scripts menu)

**Script window** opens an editor where you can write and run scripts
that work on your open tables.

The editor helps as you type: it suggests function and column names
(press Tab to accept), highlights the matching bracket, folds long blocks
away (the ⊟ marks in the margin) and finds and replaces text (**Find…**).

**Open script…** loads a script file into the Script window.

**New notebook** starts a notebook in the current table's project: a page
of notes and small pieces of script, each run on its own with its results
shown right under it. It is a good way to keep a worked analysis with your
notes. Notebooks are saved with the project.

**Notebooks** lists the project's notebooks so you can open one again (it
appears once a project has a notebook).

**Run script over a folder…** runs one script on every data file in a
folder and collects the results in a table, for example to analyse each
month's file the same way.

**Record actions** writes down what you do from now on (opening tables,
edits, analyses and their options) as a script that repeats it. To stop,
choose **Stop recording**, which takes its place in the menu; the script
appears in the Script window.

More detail: Scripting and recording.

## Help

(Picture: The Help menu)

**Quick guide** opens this guide as a PDF in your computer's PDF viewer
(Preview on a Mac, your default PDF app on Windows or Linux). The guide is
built into AnovaCraft, so it needs no internet connection and always matches
the version you have.

**Reference manual** opens the
Reference Manual the same way: a PDF that
describes every option, report section and method in detail. Its contents
page and bookmarks link to every chapter.

**Keyboard shortcuts** lists every shortcut key.

**Show crash reports** opens the folder where AnovaCraft writes a short
report each time it stops on an internal error. If you report a problem,
attach the newest file there.

If AnovaCraft ever closes unexpectedly, your work is not lost: every couple
of minutes it keeps a copy of each project with unsaved changes, and the next
time it starts it offers **Restore unsaved work**. Click **Restore** to open
the copies (save them to keep the changes), **Discard** to delete them, or
**Decide later** to be asked again next time.

**About AnovaCraft** shows the version number and AnovaCraft's licence
(Apache 2.0 or MIT, whichever you prefer), with links to both manuals and to
their pages online, and a note that it is an independent project. Its **Open-source licences** button lists the fonts,
icons, data and software libraries AnovaCraft is built with, with each one's
copyright notice and licence.

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

# How the code is organised

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
