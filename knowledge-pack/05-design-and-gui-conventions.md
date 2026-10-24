# Design and GUI conventions



> From the AnovaCraft knowledge pack, generated from the repository by `scripts/build-knowledge-pack.py`. AnovaCraft is an open-source statistics application written in Rust. Answer from this text: it does not depend on any other file. Paths such as `crates/...` or `docs/...` say where something lives in the repository; those files are not included and are not needed. Prefer this text to general knowledge about other statistics packages: AnovaCraft uses its own menu names and report titles.

### Clean-Room & Legal Provenance Constraints
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

### GUI Architecture & Interaction Model

#### 1. Tech Stack
- **GUI Framework:** `egui` with `eframe` (Native desktop backend via `wgpu`).
- **Plotting & Visualizations:** `egui_plot` for responsive, interactive 2D scatter plots, contour plots, and residual charts.
- **Iconography:** Use only open-source icon sets (e.g., Lucide or Feather icons, rendered as SVG) under their original licenses. Do not copy, trace, or approximate any proprietary product's icons, logos, or brand assets.
- **Visual identity:** Define the project's own color palette and theme tokens (light and dark). Color must never be the sole carrier of meaning; pair it with a shape, icon, or text label.
  - Plot colors come only from `app_gui::theme::PlotPalette` (light/dark tokens; faint colors premultiplied in gamma space, as CSS composites them); stat-viz's fixed colors map to dark counterparts via `stat_viz::palette::for_dark`. Do not reintroduce per-plot color constants.
  - Plots are drawn through `plot_axes::PlotChrome` (canvas, grid behind the data, axis spines). Default grid: horizontal major rules only, no minor grid, no vertical rules (both available per axis in Plot settings). Points are solid and borderless, with a soft halo in dark mode.
  - Default axis ranges use `stat_viz::auto_range`: 6% padding per side, never below zero when all data are non-negative (except that scatter points at or just above zero get padding below, so the frame does not cut them in half), a single value framed from zero; categorical axes extend half a category beyond the first and last; nice tick steps 1, 2, 2.5, 5 × 10ⁿ.
- **Scrolling lists:** any list taller than its box shows a ▾ (▴) cue where entries are cut off. Drop-down lists are built with `dialog_kit::ComboExt::show_list`, which adds the cues, never with bare `ComboBox::show_ui`.
- **Typography:** bundled open-licensed fonts (`crates/app-gui/assets/fonts`, licences alongside): Inter for text, JetBrains Mono for numbers (tick labels and numeric table cells, so digits align), DejaVu Sans as a glyph fallback, and the generated `AnovaCraftMarks` font (`build_marks.py`) that positions combining marks such as the bar in X̄, which egui does not place itself.

#### 2. Layout Structure
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

#### 3. Core Behavioral Mechanics

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

#### 4. Documents & Files
- Several datasets can be open at once. Each open dataset (with its row states and reports) is a document, listed as tabs along the bottom edge of the window and in the left sidebar, where right-clicking an entry offers save, export and close. Opening a file or sample while one is open adds a new document.
- A document's data grid, results pane or a single report can open in a native window of its own (egui immediate viewports, `app::windows`); a view lives in one place at a time, the main window showing a placeholder. The main window's document (`shown`) is separate from the command target (`active`, the document of the window used last), which menus, dialogs and the sidebar act on; shortcuts run in each window on its document.
- Import: CSV, TSV, other delimited text (delimiter detected), JSON arrays of row objects, and spreadsheet workbooks (`.xlsx`, `.xlsm`, `.xlsb`, `.xls`, `.ods`; one document per non-empty sheet). Export: CSV, TSV, JSON and `.xlsx`.
- The data grid is editable like a spreadsheet (cell cursor, double-click/Enter/typing to edit, Enter/Tab to commit and move, Escape to cancel), with type-checked entry, row and column insert/delete, rename, data-type and measurement-level changes, and undo/redo (last 500 edits, within 512 MB) (`app_model::editing`). Edits re-run every report that depends on the data; a column used by a report cannot be deleted until that report is closed.
- Project files (`.acraft`; documented JSON — see `app_model::project`) hold one or more tables (derived tables listed under their source, with Update when the source changed) and the window layout; each table stores its data, measurement levels, row states and each report's request and display options. Reopening a project re-runs every report, reproducing it exactly. Closing a document with unsaved changes asks first.

---

## Reading a report

Each analysis produces a **report**: a tree of collapsible sections, such as
fit statistics, parameter estimates, test tables and plots. Reports appear
on the document's **Results** tab (newest first) and in **Side by side**
view.

(Picture: A linear model report with its ⋯ Options menu open)

### Sections

Click a section's heading to collapse or expand it. Numeric table cells use a
monospaced font, so digits line up.

**Collapse all** and **Expand all**, above the reports, close every report
down to its title or open every report and all its sections. **Export
all…**, beside them, saves all of the table's reports (or the ones you
pick) as a document; see
Saving plots and results. The × beside a
report's ⋯ Options button removes that report; for 30 seconds the status bar
offers **Undo remove**, which puts it back where it was. When rows are
excluded, a faint "N of M rows excluded" note sits beside the buttons;
click it to select the excluded rows.

With two to four reports, a **Go to** line lists their titles (newest
first); click one to open that report and scroll to it. With more, a **Go to
▾** list beside the buttons holds the titles instead, so the line never wraps.

P-values below 0.05 in report tables are marked with a ★ in strong text, so
they stand out without relying on colour. Turn this off in File ▸ Settings…
(Mark p-values below 0.05 with ★). Settings can also separate thousands
with commas (184,150,146) and line up the decimal points in each column.
Copied tables carry the plain numbers. While the getting-started tips are on,
the line beside the buttons is a reminder of how to work the plots; the **?**
beside each plot shows the same reminder.

### The options menu

Each report has a **⋯ Options** menu, also opened by right-clicking the
report's heading or any section heading. It:

- **Edit analysis…** (at the top of the menu, and when right-clicking the
  report's heading) reopens the report's dialog with its settings filled in;
  **Update** changes the report in place (see
  Using analysis dialogs);
- **Open in new window** (just below it, also on the report heading's
  right-click menu) shows the report in a window of its own; in that window
  the item reads **Back to main window** (see
  Views in windows of their own);

- adds or removes optional sections in place (diagnostics, extra tests,
  plots). The chapters on each analysis list its options;
- saves columns the report offers back into the table, such as predicted
  values, residuals, component scores or cluster numbers;
- **Filter rows…** narrows this report alone to chosen rows, and **Column
  picker** steps it through other columns (see
  Filtering a report and switching its column);
- **Copy as script** copies the script call that recreates this report (see
  Scripting and recording);
- **Collapse all sections** and **Expand all sections** close or open every
  section of this report at once;
- **Reset all plot views** re-fits every plot in the report;
- **Remove report** closes it.

Right-clicking the heading of a plot section also offers to save the plot
(see Saving plots and results).

### Filtering a report and switching its column

**⋯ Options ▸ Filter rows…** gives a report a row filter of its own. A
**Row filter** line appears under the report's title: **+ Add condition**
lists the table's columns. A categorical column becomes a condition with a
check box per level; a continuous one becomes a **from** / **to** interval
(ends included). Each condition shows as a button such as
`Region ∈ {East, West}` or `20 ≤ Age ≤ 40`; click it to change the levels or
the interval, or to remove the condition. A row must pass every condition.
**Clear** (or ⋯ Options ▸ **Remove row filter**) takes the filter away.

**Filter panel** on the same line opens the conditions as a form under it:
each condition in a box with its column's name, a list of its levels with
**all** and **none** (a categorical column) or a **from** and a **to**
slider (a continuous one), and **×** to remove it; **+ Add condition** sits
at the bottom. **Step through** on a condition shows one slice at a time:
a categorical column one level at a time, a continuous column a window of
fixed width (the interval you set, or a tenth of the column's range when
the interval spans all of it) moved along by its own width. **◀ Previous**
and **Next ▶** step by hand and wrap round at the ends; **▶ Play** steps on
by itself at the number of seconds shown (0.2 to 10) until **⏸ Pause**. A
note such as "Level 2 of 3: B" or "Window 4 of 10: 1950 ≤ Year ≤ 1951.5"
says where it stands. Each step recomputes the report, so this shows how an
analysis changes across the levels of another column, in the manner of
Cleveland's conditioning plots (*Visualizing Data*, 1993). Adjacent windows
share their end points, so a value exactly on a boundary appears in both.
The steps are ordinary filter edits: the filter shown when you save is the
one saved.

(Picture: Plant growth: a report with its filter panel open on Fertilizer, stepping through the levels)

The filter changes this report only. The rows it leaves out are not excluded
or hidden in the table, so the data grid and every other report still use
them. The report recomputes from the rows that pass (an interval dragged with
the mouse applies when you let go), its plots leave the other rows out, and a
faint "N of M rows filtered out" note beside the title says how many it left
out. Selecting points in a filtered report still selects those rows
everywhere. The filter is saved with the report in a project and in
**Copy as script**, and a column used by a filter cannot be deleted while the
report is open (a renamed column is followed).

Reports built around one response or measured column (linear, generalized
linear, penalized and stepwise models; distributions; control charts and
capability studies; bivariate fits, whose Y column switches; multi-vari
charts and analysis of means) also offer **⋯ Options ▸ Column picker**. A
line under the title shows the column now in that role, with **Previous**,
**Next** and a list to pick another, and "2 of 6" for its place in the list.
Choosing a column recomputes the report on it, with every other setting
kept, so you can step one analysis through many columns. The list starts
with every column of the same kind (numeric or text) that the report does
not already use in another role; **choices…** ticks which ones it holds, and
**×** removes the picker. For a capability study, the line below holds
the **specification limits for** the column shown: each column keeps its own
limits as you step through, and a column seen for the first time starts with
the limits of the one before. A generalized linear model's event level goes
back to its default when the response changes.

The same reports take a column dropped on a plot axis. Drag a column from
the variables list over a plot: an axis whose title is the report's own
column (the response of a model's fit plots, the column of a distribution's
histogram or a control chart's values) is outlined, and a note beside the
pointer says "Analyse Weight instead of Height". Letting go there switches
the report to that column, just as choosing it in the column picker does
(the picker need not be on). A column of another kind, or one the report
already uses in another role, is refused with a note saying so; axes that
show other quantities (counts, residuals, quantiles) take no drops.

(Picture: A distribution report with a row filter on one categorical column and the column picker on)

### Reports stay current

A report is bound to its table's data and row states. Editing the data or
excluding rows re-runs it. Projects store each report's request and display
options, and reopening the project re-runs it exactly.

Reports are computed in the background, so typing in the grid, excluding
rows or scrolling never waits for a slow analysis. A report that takes more
than a moment shows a spinner and **Updating…** in its title bar, with its
previous result dimmed beneath until the new one arrives. Analyses with many
repeated steps (random forests, boosted trees, Bayesian sampling, bootstrap
intervals, the model search) count them: **Updating… 120 of 500**.

**Cancel** in the title bar stops the computation. The report keeps its
previous result and says **Computation cancelled**; **Compute again** starts
it once more. A further edit to the data also starts it again. When rows were
added, removed or reordered, the previous result no longer lines up with the
table, so the report shows a short note instead of it until the new result
arrives.

If an analysis stops with an unexpected error, the report keeps its previous
result and shows the error above it; the rest of the app carries on.
New reports, and the reports of a project being reopened, appear at once and
fill in as they finish. Exporting reports first finishes any that are still
updating, so exported files always hold current results.

### Model reports

Every model report includes an **Effect Pareto**, a bar chart of −log₁₀ p for
each effect, so the strongest effects stand out. Models keep the hierarchy
principle: an interaction or a power implies its lower-order effects. Model
dialogs enforce it, stepwise selection respects it, and a report notes when
a model breaks it.

Linear, generalized linear, penalized, nonlinear and decision tree reports end with a
**Prediction Formula** section showing the fitted equation, with a **Copy
formula** button. **Save prediction formula** writes it as a
formula column, so predictions follow edits to the
data and are computed for new rows.

## Working with plots

All plots in reports and on the graph canvas behave the same way.

### Moving and rescaling

Plots never move when you scroll or pinch, so scrolling a long report is
safe. Instead, each axis is a ruler you drag:

- Drag near **either end** of an axis to move that limit only; the value you
  grabbed follows the pointer.
- Drag the **middle** of an axis to shift the whole axis.
- **Reset view** (or double-click the plot) re-fits the plot to its data.

On a plot at equal scale (see Resizing below), dragging the end of one axis
still moves only that limit; the other axis widens or narrows about its
centre to keep one unit the same length on both. On the graph canvas,
dragging one axis fixes that axis's range only; the other keeps fitting the
data.

Dragging inside the plot area selects rows (see below). A short reminder of
these gestures sits above the reports, and each plot's **?** shows it too.

After selecting points, right-click the plot and choose **Zoom to selected
points** to frame them; Reset view goes back.

Resting the pointer on a point in a report plot shows a tooltip with its row
number and its two values (the category's name on a categorical axis), and
notes when the row is excluded.

### Selecting points

Drag a box in the plot to select the rows inside it, or click a point. On the
graph canvas, and in report scatter plots through the plot's right-click
menu (**Lasso selection**), you can also draw a lasso. Shift adds, ⌘/Ctrl toggles and
⌥/Alt removes. Selected points are ringed, so the selection stays visible on
any group colour, and the same rows are highlighted in every other plot and
the grid.

### Plots with very many points

A plot draws one marker for each row, which stays quick up to some tens of
thousands of points. Above **20,000** points (in a report's scatter or
quantile plot, among a box plot's values beyond the whiskers and its
selection rings, or in one layer of the graph canvas) the points are drawn
instead as one picture at the screen's resolution, and a note under the plot
says so. Each pixel takes the colour of the points in it, with opacity
growing with how many there are (on a logarithmic scale, so crowded areas
read darker without hiding single points); excluded rows are drawn first in
their own colour and selected rows last, in the selection colour, slightly
larger. Group colours are kept; marker shapes and the dark theme's halo are
not drawn.

Only the drawing changes. Box, lasso and click selection, hovering and
zooming to the selection work on the rows as before; dragging an axis draws
the picture again for the new view. Saved and copied figures (SVG, PDF, EPS
and PNG) still contain every point.

### Groups

Grouped data use a colour-blind-safe palette, and each group also gets its
own marker shape, so groups can be told apart without colour.

### Plot settings

Each plot's **Plot settings ▾** menu sets titles, axis ranges, major and minor
ticks, the tick origin, grid lines, number format, axis position, equal
scaling of the two axes, and the key. On scatter plots and on the graph
canvas it also sets axis breaks on each continuous axis (see
Axis breaks below), and on the graph canvas the right axis
when the graph has one (see Graph canvas).

These settings are kept with the plot: saving the project, adding the report
to a dashboard or copying its script call keeps them, and reopening draws the
plot as you left it. A plot whose settings are all back at their defaults
stores nothing. Ranges typed into the **min** and **max** fields are kept
(the row then shows **fixed** and an **Automatic** button that lets the
axis fit the data again); ranges you only drag to are not kept, and reopened
plots fit their data again. **Reset view**, a double-click on the plot and
**Fit view to data** clear typed ranges too.

#### Ticks

Each axis section has a **Ticks** group:

- **Tick direction** — **Inside** (the default) points the marks into the
  plot, **Outside** away from it towards the labels, **Across** half each
  way. Labels move out to make room for outside marks.
- **Major marks** and **Minor marks** — length and thickness in points
  (5 and 1 for majors, 3 and 1 for minors). Minor marks are drawn whenever
  the axis has minor ticks (set under **Minor ticks**).
- **Tick labels** — your own labels, each a value and a text read as math
  markup (`\pi` draws π, `10^3` a superscript). A label at a tick's value
  replaces its number; anywhere else it adds a labelled tick of its own.
  **Add a tick at…** adds a row to fill in; ✕ removes one. An empty label
  leaves the tick unlabelled.

On the plot itself, double-click a tick label (or right-click the axis ▸
**Edit tick label…**) to write that tick's label in place: **Apply** (or
Enter) keeps it, **Use the number** goes back to the number, **Cancel** (or
Escape) leaves it as it was. Where no labelled tick is near, the editor adds
a tick at the value under the pointer.

#### Spine

The **Spine** group of each axis has:

- **Offset from data** — moves the axis line, its marks and labels away from
  the data area by up to 40 points, a style common in scientific figures.
  The draggable axis ruler moves with the line.
- **Extend to nearest ticks** — widens an automatic range to the major ticks
  just outside it, so the axis starts and ends on a labelled value (off by
  default, when ranges keep their usual 6% padding). Typed or dragged ranges
  are left as they are.

#### Plot area

Besides **Equal scale**, the **Plot area** section sets a **Scale ratio**
(one x unit drawn as long as *r* y units; 1 is equal scale) and a **Print
scale**: saved figures draw one horizontal unit *N* millimetres long, so a
printed figure can be measured with a ruler (report plots; the page grows
or shrinks to fit the range shown).

Saved SVG, PDF, EPS and PNG figures draw the axes with these settings: tick
direction, length and thickness, offsets, your tick labels (in markup), and
on report plots also the tick intervals, minor ticks, number format, grid
lines and the side each axis is on, so a saved figure matches the screen.

(Picture: Longley: a plot whose horizontal axis line is offset below the data area, its ruler and labels moved with it)

### Copying a plot's style

Right-click a plot (or its section heading) and choose **Copy plot style**
to keep its settings for the session; a graph canvas has it in its ⋯
Options menu and in its plot's right-click menu. Then right-click any other
plot ▸ **Paste plot style**. The submenu names the plot the style came from
and has a tick for each part that goes across:

| Part | What it carries |
|---|---|
| Axis ranges | The typed minimum and maximum of each axis (off at first: ranges belong to one plot's data) |
| Ticks, grid and spines | Tick spacing and marks, minor ticks, your tick labels, number format, grid lines, axis sides and offsets, category label direction, equal scale and its ratio, print scale |
| Font and size | The plot font, text size, saved figure size and background |
| Colours and markers | Marker and line colours, shapes and sizes (between graph canvases) |
| Reference lines | Reference lines and shaded bands |
| Key | Whether the key shows, where it sits (graph canvases), and renamed, hidden or reordered entries |

Titles, axis breaks and annotations never go across: they describe one
plot's data. Paste **Onto this plot**, **Onto every plot in this report**
(collapsed sections too) or **Onto every plot in this project** (every
report and graph canvas of every table in the project). Report plots and
graph canvases share axes, fonts, reference lines and the key; colours,
markers and the key's placement exist only on canvases. The pasted settings
are the plot's own from then on and are saved with it.

On a plot with categories along the horizontal axis (a bar chart, Pareto
chart, control chart, or a plot against a categorical column), **Category labels** sets how their names
are drawn: **Automatic** (the default) keeps them level when they fit,
slants them at 45° when level names would overlap, and stands them upright
when even slanted ones would; **Horizontal**, **Slanted (45°)** and
**Vertical** fix the direction. The axis grows to make room for turned
names. Only when even upright names would overlap is every second or third
one shown. Exported figures follow the same setting.

#### Axis breaks

On a scatter plot in a report (and on the graph canvas), each continuous
axis has an **Axis breaks** row: tick **Leave out** to leave a stretch of
the axis out, **Add a break** for more, and open **Segments and marks** to
put a piece on a scale of its own (a log piece below a linear one, say),
give a piece a fixed share of the axis, or choose slashes or a zigzag for
the marks. Points inside a break are not drawn and a note under the plot
counts them; a note also names the scales when the pieces use different
ones. Tick labels come from each piece's own values, hovering a point shows
its data values, and box and click selection pick what is drawn. Axes
labelled by categories, probability scales or dates, plots divided into
panes, contour maps and ternary plots take no breaks. The full description
is under Graph canvas ▸ Axis breaks. Breaks
are kept with the plot, and saved figures draw them as on screen.

(Picture: Longley graph canvas of Employed by GNP with two vertical breaks, a log-scale lowest piece and zigzag marks)


### Reference lines and bands

**Reference lines…**, in the plot's **Plot settings ▾** menu and in its
right-click menu, adds horizontal or vertical lines and shaded bands to a
report plot (scatter and fit plots, control charts, box plots, survival
curves, bar and Pareto charts) or a graph canvas.

- **+ Add line** adds a dashed line. Choose *Horizontal* or *Vertical*, then
  where it sits: a **Typed value**, or a statistic of the plotted values on
  that axis: **Mean**, **Median**, **Quantile** (with its percentage),
  **Mean ± k standard deviations** (with k; a negative k is below the mean),
  or the **Individuals chart** centre line, upper or lower limit
  (x̄ ± 3·MR̄/d₂, from the values in row order). Each line has its own label (empty: its value or
  statistic), colour, width and pattern (solid, dashed, dotted or
  dash-dot). *Show the value* adds the number after the label.
- **+ Add band** adds a shaded stretch between two values, each typed or a
  statistic, drawn behind the data with its fill colour and opacity. *Edge
  lines* draws its two edges; *Edge values* writes their values beside them.
  A band has an entry in the key, a filled square with its label.

Statistics leave out rows excluded from analyses and follow every change to
the data or the row states, so a line at the mean moves when rows are
excluded. On the graph canvas a statistic is computed for **Whole
graph**, **Each panel** (the default) or **Each subgroup**, the last drawing
one line per colour group in the group's colour and naming it in the label.
Lines are drawn on top of the data, bands underneath; the default view
widens to show a line that falls outside the data. A statistic needs a
continuous axis; on a categorical or date axis, type the value. A value
that cannot be drawn on the axis (such as a negative value on a log axis)
is left out with a note under the plot.

Lines and bands are saved with the plot (as `reference_lines` in its plot
style), and appear in saved plot images and on figure pages.

(Picture: Longley scatter plot with a dashed line at the mean of Employed, its Reference lines window open beside it)

### Contour maps

The contour plots of the response surface, mixture (ternary), nonlinear
surface and contour-and-surface reports colour the bands between contour
levels and show a **colour bar** beside the plot with the level values at
the boundaries between its boxes. The level lines and their values stay
on top, so the map still reads without colour.

(Picture: Reaction yield response surface: the contour plot filled with the sequential colours, with its colour bar, level lines and values and the stationary point)

The end of the plot's **Plot settings ▾** menu holds the contour options.
They apply to every contour plot of the report (and to the colours of the
3-D surface in a contour-and-surface report), and they are saved with the
project, in dashboards and in exports:

- **Fill between levels**, **Level lines** and **Level values** switch the
  three layers on or off. **Lines only** gives the plain look of level lines
  and values with no fill; **Default** goes back to the filled map.
- **Colours** chooses the scale:
  - *Sequential (blue to yellow)*: lightness rises steadily, so high and
    low read in grayscale too. The default.
  - *Heat (yellow to red)*: pale yellow through orange to dark red
    (after Brewer's yellow–orange–red scheme).
  - *Spectrum (blue to red)*: blue, cyan, green, yellow, orange and red, the
    usual choice for elevation and response maps. It is not colour-blind
    safe, and its lightness does not rise steadily, so keep the level lines
    and values on with it.
  - *Diverging (blue, neutral, red)*: blue below a centre value, a light
    neutral at it and vermillion above, for a response with a target.
    **Centre at** sets the value in the middle (by default the middle of the
    range); the farther end of the range takes the strongest colour.
  - *Grey (for print)*: white to near black.
- **Reverse colours** runs the scale from high to low.
- **Opacity** fades the fill (0.2 to 1), for example to see the grid.
- **Number of levels** sets about how many levels to draw, at round values
  (steps of 1, 2, 2.5 or 5 × 10ⁿ); unticked, each plot keeps its own number.
  A title that says *contours every …* follows the new step.
- **Fixed range** spans the levels and colours over a range you type
  instead of the surface's own, so maps of different responses or data can
  be compared colour for colour. Parts of the surface outside the range take
  the end colours.

Each band is coloured at its middle value. In dark mode the colours lean a
little towards the dark canvas. Points over a filled map get a rim in the
canvas colour, and selected and excluded rows keep their ring and cross.

The bands are true shapes, not pictures: **Save plot as** SVG, PDF or EPS
writes each band as one filled outline (with holes where the band surrounds
a higher or lower region), and PNG renders them, so a saved map matches the
screen. The method: the surface is taken as linear over the triangles of
its grid (each grid cell is split into four triangles around its centre,
whose value is the mean of the corners), so each band is cut from each
triangle exactly and the pieces join without gaps; the level lines are cut
from the same triangles and lie on the bands' edges.

### Math in labels

A title typed in **Plot settings** (the plot title or an axis title) is read
as a small markup, so it can carry subscripts, Greek letters and fractions.
The same markup is set in saved SVG, PDF, EPS and PNG figures, and in the
vertical title of the y axis.

| Type | To get |
|---|---|
| `x_1`, `x_{ij}` | a subscript: one character, or a group in braces |
| `x^2`, `e^{-t}` | a superscript |
| `x_i^2` | a subscript and a superscript at the same place |
| `\alpha`, `\beta`, `\mu`, `\sigma`, … | Greek letters by name; a capital first letter gives the capital (`\Delta`, `\Sigma`) |
| `\pm`, `\times`, `\cdot`, `\le`, `\ge`, `\ne`, `\approx`, `\infty`, `\partial`, `\deg` | ±, ×, ·, ≤, ≥, ≠, ≈, ∞, ∂, ° |
| `\frac{a}{b}` | a fraction, a over b with a rule between |
| `\sqrt{x}` | a square root, the radical sign as tall as what is under it |
| `\sum`, `\prod` | Σ, ∏ |
| `$a/b$` | inside dollar signs a slash is a fraction too; outside them `/` is an ordinary slash |
| `\_`, `\^`, `\{`, `\}`, `\$`, `\/`, `\\` | the character itself |

For example `Yield (kg m^{-2})`, `\sigma_{within}` or
`$\Delta y / \Delta x$`. Scripts are set at 70% of the text size, fraction
parts at 80%. A title with none of `_ ^ \ $ { }` is drawn exactly as plain
text. Something the markup cannot read — an unknown name such as `\foo`, a
`_` or `^` with nothing after it, an unmatched `$` — is shown as typed.

Default titles (column names) are shown as written. On the graph canvas,
**⋯ Options ▸ Read math markup in column names, levels and labels** reads
them as markup too, along with key entries and text labels, so a column
named `x_1` gets a subscript.

### Resizing

Every plot can be resized by dragging the faint handles beside it:

- the strip under the plot changes its height;
- the strip along its right edge changes its width;
- the corner where the two meet (marked with diagonal lines) changes both.

Double-click either strip to go back to the default size. A plot is never
made wider than the visible part of the pane, so the whole of it stays in
view. The same handles resize the 3-D scatter and surface plots, the plots
of the graph canvas, and the panels of a Response Explorer, where one strip
under the panels resizes them all together (see
Response Explorer).

Plots of one variable against another, and the 3-D plots, open at most
about 760 points wide (wider with larger text), so a wide window does not
flatten them into a strip; drag a plot's right edge to widen it. Plots whose shape depends on
equal units on both axes open at equal scale: observed against predicted or
fitted values, probability plots against the line y = x, and score plots.
Turn equal scaling off in Plot settings. Control
charts, Pareto charts, bar charts and box plots use the full width.

Drawings laid out to fit their text (correlation heat maps, decision trees,
cause-and-effect diagrams and the Effect Pareto) have no size handles: they
grow with the text size instead, and scroll sideways when wider than the
pane.

The key below each plot names what each line, band and marker shows: a
fitted model line is "Fit", but reference lines and curves carry their own
names ("Reference line y = x", "4/n threshold", "OC curve", "Mean
difference", "Limits of agreement"). Category names that would overlap
are turned rather than dropped (see **Category labels** under Plot
settings).

By default, plots draw horizontal major grid rules only, and axis ranges
frame the data with a little padding (6% per side), keeping a zero baseline
when all the data are non-negative. Points at or just above zero (a 0/1
response, the leaves of a dendrogram, Cook's distances) get padding below as
well, so the frame
does not cut them in half, and categorical axes leave half a category beyond
the first and last. Date axes tick on calendar units instead, and axes that
count samples or subgroups tick on whole numbers only. A tick label that would
be cut off at the top of the vertical axis, or run into the corner at the left end of the horizontal axis, is left out.

Labels drawn inside a plot keep clear of each other: group names at the ends
of survival curves and run-rule numbers on control charts move up or down
where they would collide, and on a Pareto chart a cumulative percentage that
would run into the one before it is left out (hover over its bar to read it).
Specification limits and other reference lines on a histogram are named
beside their lines, near the top, rather than across them.

### Themes

Plots follow the light or dark colour scheme (File ▸ Settings…). In dark
mode, points get a soft halo so they stand out, the darkest group colours (the
first, blue, and the grey) are lightened so they stay visible on the dark
canvas, and the zero line of residual plots is a neutral grey in both modes.

## From `crates/app-gui/src/theme.rs`

The application's own color tokens. Chosen independently; colors are never
the only cue (markers, labels and text carry the same information).

## From `crates/app-gui/src/dialog_kit.rs`

Parts every dialog shares, so they look and behave alike:

- a window that opens centred, can be moved and resized, and reopens where
  it was left ([`Dialog`]);
- the action row at its foot: **Run** for analyses or **Create** for
  designs and tables (**Update** and **Run as new** when editing an
  existing report), **Cancel**, and **Reset**, with a one-line summary of
  what the action will do and the rows it will use, or a note saying what
  is missing while the action is unavailable;
- keys: Enter runs (⌘/Ctrl+Enter while typing in a field), Escape cancels;
- a collapsed "More options" section for settings most people leave alone
  ([`more_options`]);
- column pickers with a filter box, measurement-level badges, Select all /
  none, Shift-click ranges and drops from the sidebar's variable list
  ([`column_list`], [`column_choice`]);
- for analyses, a **Script** toggle that shows the Rhai call the dialog
  would run, built live from its settings, with a button to copy it.

## From `crates/app-gui/src/plot_axes.rs`

Axis settings for plots: the grid and tick-label functions handed to
egui_plot, tick marks along the axes, and the settings menu (opened from a
plot's "Plot settings" button or by right-clicking an axis). Titles typed
there are read as math markup (`stat_viz::math_text`); axis breaks and
the right-hand axis appear where the plot supports them.
