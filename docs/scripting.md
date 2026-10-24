# Scripting reference

AnovaCraft scripts are written in [Rhai](https://rhai.rs/book/), a small
embedded language with JavaScript-like syntax (`let`, `fn`, `if`, `for … in`,
closures `|x| x + 1`, arrays `[1, 2]`, object maps `#{ a: 1 }`, template
strings `` `R² = ${r2}` ``). Script files end in `.rhai`.

Scripts call the application's own operations: an analysis run from a script
is the same request a dialog makes and gives the same report, section for
section.

```rhai
let t = open_table("data/plants.csv");
t.set_level("Greenhouse", "nominal");
let r = t.analyze("linear_model", #{
    response: "Height",
    predictors: ["Fertilizer", "Light hours", "Greenhouse"],
});
print(`R² = ${r.value("Fit Statistics", "R²")}`);
r.save("plants-model.md");
```

Worked examples are in [`examples/scripts`](https://github.com/anovacraft/AnovaCraft/tree/main/examples/scripts).

## Running scripts

```text
anovacraft-run [--max-operations N] script.rhai [ARGUMENTS…]
anovacraft-run --licences
```

`print` output goes to standard output, `debug` output to standard error.
Arguments after the script are in the `args` array. Relative paths are
resolved against the script's own directory. The exit status is 0 on
success, 1 if the script fails (the message names the line and column) and 2
for a usage error. Build it with `cargo build --release -p stat-script`.
`anovacraft-run --licences` prints the copyright notices and licence texts of
the open-source software built into it (`THIRD-PARTY-NOTICES.txt`).

To run a script over a folder of data files, see [Batch runs](#batch-runs).

## The Script window

**Scripts ▸ Script window** opens an editor for one script at a time (**New**,
**Open…**, **Save**, **Save As…**; **Scripts ▸ Open script…**, or opening or
dropping a `.rhai` file, loads one). Rhai is highlighted as you type.
An unsaved script is asked about before the app quits.

- **Run** (Ctrl/⌘ + Enter) runs the whole script; **Run selection**
  (Ctrl/⌘ + Shift + Enter) runs only the selected text, keeping its line
  numbers. **Stop** ends a running script between operations (an analysis
  already under way finishes first).
- The **Output** pane shows `print` and `debug` output, then what the run
  changed. An error names its line: the line is marked in the editor (a `▸`
  in the margin and a tinted background), and clicking the message moves the
  cursor there.
- Scripts work on the **open tables**: `current_table()` is the table in the
  active tab and `open_tables()` lists every open table, in tab order.
  Changes to them appear when the script ends: all of a table's data edits
  become **one step that Edit ▸ Undo reverses**, reports follow their
  columns as they do for edits made by hand, the selection and exclusions are
  applied, and new reports join the table's report list.
- **Other tables** (opened from files, samples, `new_table`, or made by table
  operations) stay inside the script unless it calls `t.show()`, which opens
  the table as a new, unsaved tab when the script ends; the tab you were on
  stays in front, and the Output pane names each new tab.
- The script runs on copies, so the application stays usable while it runs.
  If you edit or close a table while a script that uses it is running, your
  edits are kept and the script's version opens as a new tab instead.
- If the script fails, the changes it made before the error are still
  applied (and can be undone).
- Relative paths are resolved against the script file's folder (the current
  folder for an unsaved script; the project file's folder for a script kept
  in a project).
- **Keep in project** stores the script in the current table's project,
  either to run on request or to run when the project opens. Save the project
  to keep it; the table's right-click menu (and **File**) then lists it under
  **Scripts** (Run, Open in Script window, Run when the project opens, Remove
  from project). Opening a project whose scripts are set to run on open
  always asks first (**Run**, **Don't run**, or **Show in Script window** to
  read it before deciding), since scripts can change data and write files.

`examples/scripts/flag_outliers.rhai` is written for the window: open a table
(for example the Michelson sample), open the script and press Run.

### Editing aids

- **Brackets:** with the cursor beside a `(`, `[` or `{`, it and its partner
  are tinted and underlined.
- **Folding:** a line that opens a `{ … }` block spanning several lines has
  ⊟ in the margin; click it to show the block as `{⋯}`, and ⊞ to open it
  again. **Fold ▸ Fold all blocks** folds every outermost block, **Unfold
  all** opens them. Folding changes only what is shown: Run, Save and Find
  use the whole script, typing in a folded script keeps the hidden lines, and
  an error opens every fold so its line can be marked.
- **Completion:** after two letters of a name, a list offers matching script
  functions (with their arguments) and names the script declares (`let`,
  `const`, `fn`); inside quotes it offers the active table's column names.
  **Ctrl + Space** opens the list at any point. ↑ ↓ choose, **Tab** or
  **Enter** inserts (a function gets its opening bracket), **Esc** closes;
  clicking an entry inserts it too.
- **Find and replace** (**Find…**, ⌘/Ctrl + F in the editor): type the text,
  then **Next** or **Previous** (Enter steps on) to select each match;
  **Replace** changes the selected match and moves on, **Replace all**
  changes every one and says how many. **Match case** and **Whole names**
  (only where the text is a complete name, not part of a longer one) narrow
  the search. A recorded action list opens in this window, so find and
  replace works on it too, for example to point a recording at another
  column or file.

## Notebooks

**Scripts ▸ New notebook** adds a notebook to the current table's project
and opens it; **Scripts ▸ Notebooks** lists the project's notebooks to open
again. A notebook is a list of **text cells** (notes, shown as written) and
**code cells** (Rhai) with their **outputs** under them.

- **▶ Run** on a code cell (or ⌘/Ctrl + Enter in it) runs that cell; Shift +
  Enter runs it and moves to the next cell. **▶ Run all** runs every code
  cell from the top, stopping at an error. **■ Stop** stops the running cell.
- Cells share one session: variables (`let`) and functions (`fn`) defined in
  one cell are there in the cells run after it, until **Restart**. Tables
  keep up with the application: a variable holding an open table sees edits
  made in the grid between cells.
- Each run cell shows its run number (`[3]`) and its outputs: lines printed
  with `print`; an error with its line (click it to go there); the value of
  its last line — a table as its first 50 rows, an array of maps as a table,
  a map as name and value pairs, a report as its tables, anything else as
  text; and every report the cell opened, as a link that shows it. Changes to
  the open tables merge as a Script window run's do (one undoable step for
  the data edits).
- Cells can be added (**+ Text**, **+ Code**, or a cell's **+** menu for one
  below it), deleted (✕) and moved (↑ ↓). Text cells switch between **Edit**
  and **Done**.
- **Notebook ▸ Clear outputs**, **Copy as one script** (the code cells in
  order, text cells as `//` comments, opened in the Script window),
  **Rename…** and **Delete notebook**.
- Notebooks, with their outputs, are saved in the project file (key
  `"notebooks"`), so a reopened project shows the results of the last run
  without running anything.

![A notebook: a text cell, a code cell with a table as output](../images/notebook.png)

## Script library

The **Scripts** section of the left sidebar (click its heading to open it)
lists the scripts you keep at hand:

- the `.rhai` files in your **scripts folder** — choose it once with
  **Choose…** (the application remembers it), and **Refresh** after adding
  files outside the application (the list also refreshes every few seconds
  while open);
- the scripts kept in the **active table's project**, marked when they run
  on opening.

Click a script to open it in the Script window. Right-click a folder script
to **Run** it, open it, or **Keep in project** (copy it into the active
table's project; save the project to store it); right-click a project script
to run, open or **Remove from project** it.

![Sidebar with the Scripts section open: three folder scripts and one project script, a right-click menu showing Keep in project](../images/script-library.png)

## Batch runs

A batch run applies one script to every data file in a folder — the same
summary for each day's export, say — and collects what each run returns in
a table.

- In the application: **Scripts ▸ Run script over a folder…**. Choose the
  script (the Script window's, or a file), the folder, and optionally which
  files (`*.csv`; `*` stands for any characters, `?` for one; every file
  the application opens when empty — `.csv`, `.tsv`, `.tab`, `.txt`,
  `.dat`, `.json`, workbooks and `.acraft` projects — the window shows how
  many match).
  **Run** works in the background, showing each run's output with its file
  name; **Stop** ends the batch after the current file. The results open as
  a new table.

  ![The Run script over a folder window after a run: the script, folder and pattern, and the output with one failed file](../images/batch-run.png)
- From the command line:

  ```text
  anovacraft-run --batch FOLDER [--pattern GLOB] [--results FILE] script.rhai [ARGUMENTS…]
  ```

  `--results` writes the table as CSV, TSV, JSON or `.xlsx` (by its
  extension: `.csv`, `.tsv`, `.json` or `.xlsx`, in any case; any other
  extension is a usage error, exit status 2, before anything runs); without it the table is printed to standard output as CSV and
  the runs' own output goes to standard error. The exit status is 1 when any
  run failed.

Each run starts afresh (nothing carries over between files) with these
constants besides `args`:

| Constant | Value |
|---|---|
| `batch_file` | the file's full path, for `open_table` or `open_project` |
| `batch_name` | the file's name |
| `batch_index` | its position in the batch, from 0 |

The value of the script's last statement is its result: a map gives one
results column per key (numbers stay numeric; a blank key is headed
`Result`), any other value a `Result` column, and `()` nothing. The results table has `File`, `Status` (`ok` or
`error`), `Error` (when a run failed: its message — the batch carries on
with the next file), then the returned columns (a map's keys in alphabetical
order, as Rhai keeps them). A syntax error stops the batch before any file is run.

```rhai
// Rows and mean of the "Weight" column of each file.
let t = open_table(batch_file);
let total = 0.0;
for w in t.column("Weight") { total += w; }
#{ rows: t.rows(), mean: total / t.rows() }
```

## Recording

**Scripts ▸ Record actions** starts writing a script of what you do; **Scripts ▸
Stop recording** ends it and opens the script in the Script window (copied to
the clipboard instead if the window holds unsaved work). The status bar shows
⏺ Recording meanwhile.

The tables open when recording starts are found as `current_table()`, by
name with `table(name)`, or by tab position. Tables opened or made while
recording are opened the same way in the script (`open_table`,
`open_sample`, `open_project`, `t.derive(…)` for table operations, and
`new_table` with each column written out for typed-in, pasted or designed
data) and shown as new tabs. Then each step is written as the call that
does it: data edits (undo and redo are written as the edit they made),
selection, exclusion, hiding and colours, new reports, changed display
options (`t.update_report`), closed reports, and saving. While a mouse
button is held (dragging a selection box, say) only the result is
recorded.

The application and scripts carry out these steps through the same code (the
action model in `app_model::action`), so running the recording with the same
tables open reaches the same data, row states and reports.

## Tables

A `Table` is one dataset with its row states and reports, like a document tab
in the application. Copies of a table value refer to the same table.

| Function | Result |
|---|---|
| `open_table(path)` | Open a `.csv`, `.txt` or `.dat` (delimiter detected), `.tsv` or `.tab` (tab-separated), `.json`, workbook (`.xlsx`, `.xlsm`, `.xlsb`, `.xls`, `.ods`: the first sheet with data), image (`.png`, `.jpg`, `.jpeg`, `.tif`, `.tiff`, `.bmp`: a long table of pixels `x`, `y`, `intensity`, colour images as their luminance, reduced by block averages when larger than 1024 pixels on a side) or `.acraft` file. Text and sheets are read with the settings the Open Data dialog would detect (title rows above the table skipped, names from the table's first row). A project brings its row states and reports. |
| `open_table(path, sheet)` | One sheet of a workbook, by name; the table is named `"file – sheet"`. |
| `open_table(path, #{ … })` | A text file or workbook read with the Open Data dialog's choices; members left out are detected. `header_row` (0 for no names; giving only this puts the values right below the names), `header_rows` (rows of names, joined with a space), `data_start`, `data_end` (0: to the end), `first_column`, `last_column` (0: to the end); for text `delimiter` (one character, `"tab"`, or `""` to detect), `decimal` (`"."` or `","`) and `missing` (a list of extra texts read as missing); for a workbook `sheet` (a name), or `sheets` (a list of names) with `stack: true` to stack them into one table with a column of sheet names called `sheet_column` (`"Sheet"`). Example: `open_table("report.xlsx", #{ sheet: "Line 1", header_row: 4, first_column: 3 })`. Recorded scripts write this map when the dialog's choices differ from the detected ones. |
| `open_workbook(path)` | Every sheet with data, as an array of tables (named `"file – sheet"` when there are several). |
| `open_project(path)` | Open a `.acraft` project. |
| `new_table(name)` | An empty table; the first column added sets the number of rows. |
| `open_sample(name)` | A built-in sample dataset (with its starting reports), found by the start of its menu name, e.g. `"Norris"`. |
| `split_plot_design(#{ … })` | A new D-optimal split-plot design table, as Design ▸ Split-plot creates it (with the design declared and the project script that fits its mixed model). Settings: `factors` (`[#{ name, low, high, changes }]`, `changes` `"easy"` (default), `"hard"` or `"very-hard"`), `runs`, `whole_plots`, `subplots` (when some factors are very hard and others hard), `model` (formula over the factor names, or `"main effects"`, `"interactions"` (default), `"quadratic"`), `ratio` (η, whole-plot ÷ run variance; 1), `subplot_ratio` (1), `starts` (20), `seed` (1), `randomize` (a seed, default 1, or `false`), `units` (`"actual"` or `"coded"`), `response` (`"Y"`). Analyse it with `t.analyze("mixed-model", …)`: the table's script `"Fit split-plot model"` holds that call. |
| `definitive_screening_design(#{ … })` | A new definitive screening design table. Settings: `factors` (a count, or `[#{ name, low, high }]`), `extra_runs` (at least this many runs beyond the smallest design, added in pairs from a larger conference matrix; 0), `center_runs` (center runs in all; 1), `randomize` (a seed, default 1, or `false`), `units` (`"actual"` or `"coded"`), `response` (`"Y"`). |
| `compare_definitive_screening(#{ … })` | A table comparing definitive screening designs for the same factors across nearby extra runs and center runs (settings as for `definitive_screening_design`): runs, error df after the main effects and after the main and quadratic effects, main-effect D-efficiency, largest \|r\| among quadratic effects and between two-factor interactions, mean standard error of a quadratic effect (σ units). |
| `screening_design(#{ … })` | A new D-optimal screening design with continuous and categorical factors. Settings: `factors` (`[#{ name, low, high }]` for a two-level continuous factor, `#{ name, levels }` for a categorical one with 2 to 12 levels, which becomes a text column `L1`, `L2`, …), `runs` (default: terms + 4, rounded up to balance the levels), `model` (`"main effects"` (default) or `"interactions"`), `starts` (20), `seed` (1), `randomize`, `units`, `response`. |
| `read_database(#{ kind, … })` | Run an SQL query into a new table. `kind`: `"sqlite"` (with `path`, the database file), `"postgresql"` or `"mysql"` (with `host`, `port`, `database`, `user`, `password`, and `tls`: `"disable"`, `"prefer"` (default) or `"require"`), or `"odbc"` (with `path`, the connection string, and `password`; only in builds with the `odbc` feature). Give `query` (SQL) or `table` (read the whole table). The table remembers its source (without the password), so a saved project can refresh it in the application. |
| `database_tables(#{ kind, … })` | The database's table names, with the same connection settings. |
| `read_url(url)`, `read_url(url, #{ headers, secret_headers, format, json_path, table })` | Fetch JSON, delimited text or a table on a web page from a web address into a new table. `headers` and `secret_headers` map names to values (secret ones are never saved; headers such as `Authorization`, `Cookie` or names containing `key`, `token` or `secret` always count as secret); `format`: `"auto"` (default; from the content type), `"json"`, `"delimited"` or `"html"`; `json_path`: dotted path to the array of rows inside the document (e.g. `"data.items"`); `table`: which table of a web page, by number from 1 or by id (default: the one the address's `#section` points to, else the largest). Nested objects become dotted column names. Query parameters of the address whose names mark a credential (the same rule) are saved in projects with their values blanked. |
| `current_table()` | In the Script window: the table in the active tab (an error in `anovacraft-run`, which has no open tables). |
| `open_tables()` | In the Script window: every open table, in tab order (empty in `anovacraft-run`). |
| `table(name)` | In the Script window: the open table with that name (an error if none or several have it). |
| `t.show()` | Open `t` as a new tab when the script ends (in the Script window; does nothing in `anovacraft-run`). Open tables are already shown. |
| `t.name`, `t.rename_table(new)` | Table name; give the table another name (undoable like any edit). |
| `t.rows()`, `t.columns()` | Number of rows; column names. |
| `t.has_column(name)`, `t.level(name)` | Whether a column exists; its measurement level (`"continuous"`, `"nominal"`, `"ordinal"`). |
| `t.column(name)` | The column's values as an array. Missing values are `()`. Dates and times are seconds since 1970-01-01 UTC. |
| `t.row(i)`, `t.cell(i, name)` | Row `i` as a map of column name → value; one value. Rows are numbered from 0. |

### Changing data

Each change is the same undoable edit the data grid makes; reports stored with
the table keep following their columns.

| Function | Effect |
|---|---|
| `t.add_column(name, values)`, `t.add_column(name, values, level)` | Append a column. All numbers (or `()`, `true`/`false`) make a numeric column; anything else makes a text column. |
| `t.set_column(name, values)` | Replace a column's values. |
| `t.simulate_responses(#{ … })` | Add simulated responses for the design in `t` (one edit) and return a new table of their true coefficients (one row per response: its name, the number of active terms, the active terms, and every term's coefficient). Settings: `factors` (column names, or `#{ column, low, high }`), `model` (formula over the factor names, or `"main effects"`, `"interactions"` (default), `"quadratic"`), `count` (1, at most 1000), `seed` (1), `name` (`"Simulated Y"`, numbered when several), and the prior: `main` (probability a main effect is active; 0.25), `all_parents`, `some_parents`, `no_parents` (probability a higher-order term is active when all, some or none of its factors are; 0.25, 0.1, 0.01), `effect_sd` (SD of an active coefficient in coded units; 2), `inactive_sd` (0), `noise_sd` (1), `intercept` (0). |
| `t.set_cell(i, name, value)` | Set one value. |
| `t.rename_column(old, new)`, `t.remove_column(name)` | Rename; delete (refused while a report uses the column). |
| `t.set_level(name, level)` | Change a measurement level. |
| `t.set_value_labels(name, #{ "1": "Female", "2": "Male" })`, `t.clear_value_labels(name)`, `t.value_labels(name)` | Name a column's stored values (analyses and plots then use the names); remove the names; read them as a map. |
| `t.set_hover(name, true)`, `t.hover_columns()` | Show a column's value in point tooltips; list such columns. |
| `t.save_labelled(path)` | Save the table like `t.save`, writing value labels in place of the stored values. |
| `t.add_rows(n)` | Append `n` empty rows. |
| `t.insert_rows(at, n)` | Insert `n` empty rows before row `at` (`at = t.rows()` appends). |
| `t.insert_rows([[at, [values…]], …])` | Insert rows with values (one per column), each ending up at its position; positions increase. |
| `t.remove_rows([i, …])` | Delete rows. |
| `t.insert_column(i, column)` | Insert a column at position `i`. `column` is a map in the project-file form: `#{ name, type: "numeric" \| "text" \| "datetime", level, values }` (a date-time column also gives `format`, `fraction_digits` and `offset_minutes`). A formula column gives `formula` and may leave out `values`. |
| `t.set_formula(name, formula)` | Give a column a [formula](formulas.md) (its values are computed and kept up to date), or add a formula column at the end when there is no such column. |
| `t.formula(name)` | A column's formula, or `()` for a column of entered values. |
| `t.clear_formula(name)` | Keep a formula column's current values as entered data. |
| `t.replace_column(name, column)` | Replace a column wholesale (new type, name or values), `column` as for `insert_column`. |
| `t.sort(keys)` | Sort in place. A key is a column name (ascending) or `#{ column: "x", descending: true }`. |
| `t.reorder_rows(order)` | Put rows in a new order: row `i` becomes the row that was at `order[i]`. |

### Row states

| Function | Effect |
|---|---|
| `t.select(\|row\| test)` | Select the rows for which the function returns `true` (replacing the selection); returns how many. `row` maps column names to values, e.g. `\|row\| row.Weight > 3 && row["Plant type"] == "B"`. |
| `t.select_rows([i, …])`, `t.clear_selection()`, `t.selected_rows()` | Select listed rows; clear; list. |
| `t.exclude(\|row\| test)`, `t.exclude_rows([i, …])` | Exclude rows from analyses (adding to those already excluded); `exclude` returns how many matched. |

The row test given to `select` and `exclude` may read the table but must not
add or remove rows: the script stops with an error if the number of rows
changes while rows are being tested.

| `t.set_excluded([i, …])` | Exclude exactly these rows. |
| `t.include_all()`, `t.excluded_rows()` | Include every row again; list excluded rows. |
| `t.hide_rows([i, …])`, `t.set_hidden([i, …])`, `t.unhide_all()`, `t.hidden_rows()` | Hide rows in plots (adding to those hidden); hide exactly these; show all; list. |
| `t.color_rows([i, …], "#rrggbb")`, `t.uncolor_rows([i, …])`, `t.row_color(i)` | Give rows a display colour; remove it; read it (`()` if none). |
| `t.mark_rows([i, …], "shape")`, `t.unmark_rows([i, …])`, `t.row_marker(i)` | Give rows a marker shape (`"circle"`, `"square"`, `"triangle"`, `"diamond"`, `"down triangle"`, `"plus"`, `"cross"`, `"asterisk"`); remove it; read it (`()` if none). |
| `t.color_by_column("name")`, `t.clear_row_colors()` | Colour and mark every row by a column's value, as Data ▸ Row states ▸ Colour and marker by column… does; remove every row colour and marker. |
| `t.set_style_column("name")`, `t.set_style_column(())`, `t.style_column()` | Name (or forget) the column the row colours came from, which plot keys use. |

| `t.save_subset("name", [i, …])`, `t.save_subset("name", \|row\| test)` | Save rows as a named row subset (replacing one of that name). |
| `t.use_subset("name")`, `t.use_all_rows()`, `t.active_subset()` | Make a subset active (analyses use only its rows); use every row again; the active subset's name (`()` if none). |
| `t.remove_subset("name")`, `t.subset_names()`, `t.subset_rows("name")` | Delete a subset; list the names; list a subset's rows. |

Excluded rows leave every analysis run afterwards, as in the application.
So do rows outside the active subset: `t.excluded_rows()` lists them too,
while `t.exclude_rows`, `t.include_all` and `t.set_excluded` change only the
rows you excluded yourself.

### Table operations

These make a new table and leave the original unchanged, like the Data menu's
Rows, Reshape, Merge and Summarize commands. Settings not given take the
dialog's defaults. A new table is named like the application names it
(`"Wide, stacked"`) unless the settings include `name`.

| Function | Settings |
|---|---|
| `t.sorted(keys)` | Sort keys as for `sort`. |
| `t.remove_duplicates(#{ columns })` | Columns that define a duplicate (all when empty). |
| `t.subset(#{ rows, columns, sample })` | `rows`: `"all"`, `"selected"`, `"included"` or `#{ matching: [#{ column, levels: […] }, #{ column, min, max }] }`; `sample`: `#{ size: #{ count: n } or #{ fraction: f }, strata: […], seed: n }`. |
| `t.stack(#{ columns, label, value })` | Wide to long. |
| `t.unstack(#{ label, values, ids })` | Long to wide. |
| `t.transpose(#{ columns, label })` | Rows to columns. |
| `t.summarize(#{ groups, columns, statistics, quantile_type, include_excluded })` | Statistics: `"n"`, `"missing"`, `"mean"`, `"sd"`, `"min"`, `"max"`, `"median"`, `"sum"`, `#{ quantile: p }` (default `["n", "mean", "sd"]`). |
| `t.multiple_imputation(#{ columns, engine, imputations, iterations, donors, column_methods, predictors, visit_order, seed })` | The stacked table of a multiple imputation: an `Imputation` column (0 = the original rows), `Row`, then the table's columns with the imputed ones filled in each copy. `engine` `"chained-equations"` (default) or `"normal-model"`; the other settings as for the `imputation` analysis below. Analyses on it can pool across the imputations (`pool_imputations`). |
| `t.augment_whole_plots(#{ factors, whole_plot, responses, model, whole_plots, runs, ratio, starts, seed, randomize })` | The split-plot design in `t` with new whole plots, as Design ▸ Augment design ▸ New whole plots makes it: `factors` (column names, or `#{ column, low, high }`), `whole_plot` (the column naming each run's whole plot), `model` (formula, or `"main effects"`, `"interactions"` (default), `"quadratic"`), `whole_plots` (how many to add), `runs` (in each), `ratio` (η; 1). Factors constant within every existing whole plot stay fixed within each new one. |
| `t.join(other, #{ keys, kind, relationship })` | Keys: column names shared by both tables, or `[["left name", "right name"], …]`; kind `"inner"` (default), `"left"`, `"right"`, `"full"`; relationship `"any"`, `"many-to-one"`, `"one-to-one"`. |
| `t.combine_rows([others…])`, `t.combine_rows([others…], #{ source_column })` | This table's rows followed by the others'. |
| `t.derive(#{ operation, … })`, `t.derive(#{ … }, [others…])` | Any operation by its project-file name (`"sort"`, `"remove-duplicates"`, `"subset"`, `"combine-rows"`, `"join"`, `"stack"`, `"unstack"`, `"transpose"`, `"summarize"`, `"multiple-imputation"`, `"distance-matrix"`). |
| `t.derive(#{ operation: "distance-matrix", columns, metric, standardize, label })` | The distances between the included rows: an `Object` column, then one column per row. `metric` (required): `"euclidean"`, `"manhattan"`, `"gower"`, `"jaccard"`, `"bray-curtis"`, `"hamming"`, `"simple-matching"`; `standardize` (Euclidean and Manhattan; `false`); `label` (a column naming the rows, or `()` for row numbers). |

### Saving

| Function | Effect |
|---|---|
| `t.save(path)` | Write `.csv`, `.tsv` (or `.tab`), `.json`, `.xlsx` (one sheet, dates as spreadsheet dates), or `.acraft` (the whole project). |
| `t.save_project(path)` | Write a `.acraft` project: data, measurement levels, row states and every report run on the table. Opening it in the application re-runs the reports. |
| `t.save_reports(path)`, `t.save_reports(path, #{ paper: "a4" })` | Write every report of the table, in display order, to one document whose format the extension names: `.pdf`, `.docx` (Word), `.pptx` (PowerPoint), `.html` (or `.htm`), `.md` (plots as PNG files in a `<name>_files` folder beside it) or `.xlsx` (a sheet per report); see Saving plots and results in the manual. `paper` (`"letter"`, the default, or `"a4"`) sizes PDF and Word pages. A report that cannot be computed appears with its error. |

## Analyses

```rhai
let r = t.analyze(kind, #{ settings });
let r = t.analyze(#{ analysis: kind, settings… });
```

`kind` is the analysis identifier used in project files; underscores and
hyphens are interchangeable (`"linear_model"` = `"linear-model"`). The
settings are exactly a project file's report entry, with columns by name, so
the quickest way to find them for an analysis is to run it once from its
dialog and look at `r.request()` (or the `"reports"` array of a saved
project). Display options go in `options` and default to the dialog's.

Any analysis also takes a **row filter** of its own: `row_filter`, a list of
conditions, each `#{ column: "Region", levels: ["East", "West"] }` (the
row's level is one of these) or `#{ column: "Age", min: 20, max: 40 }` (the
value lies in the interval, ends included). Only rows passing every
condition enter the report; the table's row states are not changed, so its
other reports still use every row. `column_switcher` (a list of column
names) gives a report with one response or process column the column
picker its ⋯ Options menu adds; `switcher_limits`
(`[#{ column, lower, upper, target }]`) holds a capability study's limits
for the other columns in that list. On a stacked imputation table,
`pool_imputations: "Imputation"` (the column numbering the imputations)
pools a linear model, generalized linear or mixed model, one-sample,
two-group or paired comparison, or correlation report across the
imputations by Rubin's rules: its pooled sections come first, and its usual
sections use the original rows (imputation 0) only. All of these come back
in `r.request()`.

A report with contour plots (`response-surface`, `mixture-model`,
`nonlinear-regression` with a surface, `data-surface`) also takes
`contour_style`, how its maps are drawn, as Plot settings sets it:
`#{ fill: true, lines: true, labels: true, scale: "sequential", reverse:
false, opacity: 1.0, levels: 8, min: 0.0, max: 100.0, centre: 50.0 }`.
`scale` is `"sequential"`, `"heat"`, `"spectrum"`, `"diverging"` or
`"grey"`; `levels` (1–50), `min` with `max` (a fixed range) and `centre`
(the diverging scale's middle) are optional, and members left out keep
their defaults. `fill: false` draws level lines and values only. In the
report's data a contour plot's `contours` holds `levels` and `lines` (one
`#{ level, lines }` per level, each line a list of `[x, y]` points).

Any report also takes `plot_styles`, the Plot settings of plots changed from
their defaults: a list of `#{ plot: "Residuals#0", x: #{…}, y: #{…}, y2:
#{…}, equal_scale, scale_ratio, print_scale, category_labels,
hover_coordinates, show_key, title, show_title }` (`scale_ratio`: one x unit
as long as that many y units at equal scale; `print_scale`: millimetres per
x unit in saved figures, or `()`), where `plot` names the section kind and which plot of that
kind (from 0), and each axis holds `major` (`"auto"` or an interval),
`minor` (`"none"`, `"auto"`, `#{ subdivisions: n }` or `#{ every: d }`),
`origin`, `grid` (`"major"`, `"major-and-minor"`, `"none"`), `tick_marks`,
`tick_labels`, `format` (`"auto"`, `#{ decimals: n }`, `#{ scientific: n }`
or `#{ percent: n }`), `title`, `show_title`, `side` (`"standard"`,
`"opposite"`), `break` (`[from, to]` or `()`), `tick_direction`
(`"inside"`, `"outside"`, `"both"`), `major_mark` and `minor_mark`
(`#{ length, width }` in points), `offset` (points between the data area and
the axis line), `extend_to_ticks`, `custom_ticks` (a list of
`#{ value, label }`, labels in math markup) and `range` (`[min, max]` typed
in Plot settings, or `()`). Members left out keep
their defaults. The simplest way to get one is to set the plot up in the
app and copy the report's script call.

| Kind | Main settings |
|---|---|
| `linear-model` | `response`, `predictors`, `effects` (optional: `[[[predictor position, power], …], …]`; main effects when absent), `options`; `splines` (one entry per predictor: `()` for a straight line, or `#{ kind: "natural" | "b-spline", degree, knots }` with `knots` a count of interior knots at quantiles or a list of positions); `options` also take `compare_slices` and `compare_slopes` (`"tukey"`, `"holm"`, `"bonferroni"`, `"none"` for the tests only, or `()` for off: comparisons within slices of two-factor interactions and slopes by group) |
| `stepwise` | `response`, `predictors`, `effects`, `direction`, `criterion` (`"p-value"` with `enter`/`remove`, `"aic"`, `"bic"`), `heredity` |
| `distribution-identification` | `column`, `distributions` (keys such as `"normal"`, `"weibull"`, `"weibull-threshold"`, `"gamma"`, `"log-logistic-threshold"`, `"smallest-extreme-value"`; all fourteen when absent), `transformations` (`true`), `plots` (4) |
| `best-subsets` | `response`, `predictors`, `effects`, `forced` (effects in every model, written like `effects`), `heredity` (`"strong"`, `"weak"`, `"none"`), `models_per_size` (3), `max_size` (`()` for every candidate), `criterion` (`"r-squared"`, `"adjusted-r-squared"`, `"cp"`, `"aicc"`, `"bic"`; picks the best overall) |
| `generalized-linear-model` | `response`, `predictors`, `family` (`"binomial"`, `"poisson"`, `"normal"`, `"gamma"`), `link` (`"logit"`, `"probit"`, `"cloglog"`, `"log"`, `"identity"`, `"inverse"`; `()` for the family's default), `event` (binomial: the response value modelled; `()` for the second of its two values in natural order), `effects` (as for `linear-model`; main effects and two-factor interactions), `splines` (one entry per predictor: `()` for a straight line, or `#{ kind: "natural" | "b-spline", degree, knots }` with `knots` a count of interior knots at quantiles or a list of positions), `compare_slices` and `compare_slopes` (`"tukey"`, `"holm"`, `"bonferroni"`, `"none"` for the tests only, or `()` for off: comparisons within slices of two-factor interactions and slopes by group) |
| `ordinal-logistic` | `response`, `predictors`, `effects` (as for `linear-model`), `order` (the categories, lowest first; sorted order when absent) |
| `nominal-logistic` | `response`, `predictors`, `effects`, `reference` (the reference category; the first category when absent) |
| `dose-response` | `dose`, `response` (number responding, or 0/1), `trials` (number of subjects; `()` for one per row), `link` (`"probit"`, `"logit"`, `"cloglog"`), `scale` (`"log10"`, `"ln"`, `"linear"`), `probabilities` (`[0.1, 0.5, 0.9]`), `level` (0.95), `heterogeneity_alpha` (0.05) |
| `nonlinear-regression` | `response`; either `form` (`"exponential"`, `"exponential_asymptote"`, `"logistic3"`, `"logistic4"`, `"gompertz"`, `"michaelis_menten"`, `"power"`, `"rational"`, `"line"`, `"dose_response_log"`, `"dose_response"`, `"logistic5"`, `"weibull_growth"`, `"beta_growth"`, `"segmented"`, `"two_exponentials"`, `"gaussian_peak"`, `"lorentzian_peak"`) with `predictor`, or `formula` (columns and parameters as variables), or `rate` (dy/dx) with `predictor`, `initial` (the parameter holding y at the start) and optional `start_x`, or `equation` (zero on an implicit curve in the response and one other column); `parameters` (`[#{ name, start, lower, upper, shared }, …]`, one per parameter in model order; a library form's `start` may be `()` for the automatic value, a formula's may not; `shared` (`true`) applies with `group`), `group` (data-set column), `weighting` (`"equal"`, `"inverse_y2"`, `"inverse_y"` or `#{ column: "W" }`), `method` (`"least_squares"`, `"robust"`, `"outliers"`) with `q` (0.01), `compare` (another form), `level` (0.95), `profile` (`true`), `max_iterations` (200). An assay curve template is `template: #{ key, x, second, dilution, constants: #{ … }, cv_limit, tolerance, end_tolerance, min_fraction, min_levels, standard }` together with the `formula` and `parameters` it produced (Model ▸ Curve templates… records them; `key` is one of the template keys such as `"standard_4pl"`, `"mixed_inhibition"`, `"gaddum_schild"`, `"relative_potency"`; column members are column names). Saves `predicted`, `prediction_formula`, `residual` and `inverse_predicted`. |
| `treatment-effects` | `treatment` (a column with two values), `treated_level` (the treated value; `()` for the second in natural order), `outcome` (numeric), `outcome_kind` (`"continuous"`, `"binary"` (0/1), `"count"`), `covariates` (columns measured before treatment; categorical ones enter as level indicators), `method` (`"weighting"`, the default, `"doubly-robust"`, `"matching"`), `estimand` (`"ate"`, `"att"`), `ratio` (matches per row; 1), `replace` (`false`; the ATE needs `true`), `caliper` (SD of the logit of the score; 0.2, `()` for none), `stabilized` (`true`), `trim` (leave out scores outside [trim, 1 − trim]; 0), `intervals` (`"sandwich"`, `"bootstrap"` with `replicates` (500) and `seed` (1)), `level` (0.95), `subgroups` (columns whose levels get their own estimates). Saves `propensity`, `weight` and, for matching without replacement, `matched_set`. |
| `method-comparison` | `reference` (X), `test` (Y), `deming` and `passing_bablok` (both `true`), `variance_ratio` (Var(error of Y)/Var(error of X); 1), `level` (0.95) |
| `shelf-life` | `time`, `response`, `batch` (`()` for one batch), `lower` and/or `upper` (acceptance limits), `pooling_alpha` (0.25), `level` (0.95), `horizon` (longest time searched; twice the longest time observed when absent), `time_unit` (`"months"`, `"weeks"`, `"days"`, `"years"` or `"none"`; for ICH Q1E's limit of 12 months beyond the data. Absent means `"none"`, which applies only the twice-the-data limit), `batches` (`"random"` fits batch as a random effect instead of the fixed-batch pooling analysis; needs 3 or more batches), `bound` (with random batches: `"future-batch"` (default) or `"mean"`) |
| `mixed-model` | `response`, `predictors`, `effects`, `random`, `family` (`"normal"`, `"binomial"`, `"poisson"`, `"gamma"`, `"negative_binomial"`, `"ordinal"`), `reml`, `trials`, `offset` (column added to the linear predictor; GLMMs), `quadrature_points`, `frequency` (whole-number count column; each row stands for that many observations), `weight` (prior weight column; not binomial), `order` (ordinal: the categories, lowest first; natural order when absent), `robust` (`false`; normal: CR2 cluster-robust standard errors on the outermost grouping), `residual_groups` (normal: column whose levels get their own residual variance, with a likelihood-ratio test), `df_method` (`"satterthwaite"` or `"between-within"`), `compare_slices`, `compare_slopes` (as for `generalized-linear-model`), … |
| `bayesian-estimation` | `parameter` (`"mean"`, `"proportion"`, `"rate"`), `response`, `group` (two groups) or `second` (paired means), `denominator` (trials or exposure per row), `frequency`, `event` (labelled proportion; `()` for the second value in natural order), `prior` (`"default"` (Jeffreys), `"flat"`, `"informative"` with `prior_mean` and `prior_spread`: the prior SD for a proportion or rate, the prior's weight in observations for a mean), `equal_variances` (`false`), `contrast` (two proportions: `"difference"`, `"ratio"`, `"odds-ratio"`), `reference` (one group; `()` for 0, ½ or 1), `rope` (`[low, high]` or `()`), `level` (0.95) |
| `bayes-factor` | `test` (`"one-sample"`, `"paired"`, `"two-sample"`, `"correlation"`, `"contingency"`), `first`, `second` (the group column for `"two-sample"`, the column variable for `"contingency"`), `null` (one-sample mean; 0), `sampling` (contingency: `"poisson"`, `"joint-multinomial"`, `"fixed-rows"`, `"fixed-columns"`), `frequency`, `scale` (prior r, κ or a; √2⁄2 for t tests, 1 otherwise), `direction` (`"two-sided"`, `"greater"`, `"less"`), `level` |
| `bayesian-linear-model` | `response`, `predictors`, `effects` (as for `linear-model`; heredity is added), `continuous_prior` (`"zellner-siow"` or `"hyper-g"`) with `continuous_scale` (r, √2⁄4; or a, 3), `categorical_scale` (½), `model_prior` (`"uniform"`, `"beta-binomial"`), `full_model_only` (`false`), `level`, `draws` (10000), `seed` |
| `bayesian-model` | `response`, `predictors`, `effects`, `random` (`[#{ group, within, intercept, slopes }]`), `family` (`"normal"`, `"binomial"`, `"poisson"`, `"negative-binomial"`), `trials`, `offset`, `known_sd`, `coefficient_prior` (`"normal"`, `"student-t"` with `coefficient_df`, `"cauchy"`), `coefficient_scale` (2.5), `intercept_location` (`()`: the response's mean), `intercept_scale` (2.5), `sd_prior` (`"exponential"`, `"half-normal"`, `"half-t3"`, `"half-cauchy"`), `sd_scale` (1), `autoscale` (`true`), `chains` (4), `warmup` (1000), `draws` (1000), `seed` (1), `target_accept` (0.8), `level`, `plot_parameter` (the parameter in the trace, rank and density plots) |
| `penalized-regression` | `response`, `predictors`, `alpha`, `folds`, `seed` |
| `decision-tree` | `response`, `predictors`, `validation` (`#{ method: "none" }`; `#{ method: "holdout", validation, test, seed, stratify, group }`; `#{ method: "k-fold", folds, seed }`; or `#{ method: "column", column }`), `criterion` (`"gini"` or `"entropy"`), `min_split`, `min_leaf`, `max_depth`, `pruning` (`"one-se"`, `"minimum"` or `"none"`). Its tree section's value is `#{ kind: "tree", classes, nodes: [#{ rule, summary, rows, children, shares, leaf }, …] }`. |
| `random-forest` | `response`, `predictors`, `validation` (as for `decision-tree`), `trees`, `features_per_split` (0: √p for a categorical response, p/3 for a continuous one), `sample_fraction` (bootstrap size as a share of the training rows, in (0, 1]), `min_split`, `min_leaf`, `max_depth`, `criterion`, `seed`. Saves the same columns as `decision-tree` (values, not formulas). |
| `boosted-trees` | `response`, `predictors`, `validation` (as for `decision-tree`; validation rows or k-fold choose the number of rounds), `rounds`, `learning_rate` (in (0, 1]), `max_depth`, `min_leaf`, `lambda` (L2 penalty on leaf values), `subsample` (share of training rows per round, in (0, 1]), `early_stopping` (rounds without improvement; 0 never stops), `loss` (`"squared"` or `"absolute"`, continuous responses), `seed`. Saves the same columns as `random-forest`. |
| `partial-least-squares` | `response`, `predictors` (numeric), `validation` (as for `decision-tree`; validation rows or k-fold choose the number of factors), `max_factors`, `factors` (a fixed number; omit it to choose the number), `algorithm` (`"simpls"` or `"nipals"`), `scale` (standardize predictors; `true` by default). Saves `predicted` with its prediction formula (continuous response), or the same columns as `random-forest`. |
| `model-search` | `response`, `predictors`, `validation` (as for `decision-tree`; with none, 5-fold cross-validation), `models` (list of `"decision-tree"`, `"random-forest"`, `"boosted-trees"`, `"neural-network"`, `"support-vector-machine"`, `"adaptive-splines"`, `"partial-least-squares"`, `"discriminant"`; all by default), `seed`, `repeats` (1 to 50: repeated k-fold cross-validation with seeds `seed`, `seed` + 1, …), `nested` (`true`: each model tuned within the outer training folds only). Saves no columns (open a model's own report to save its predictions). |
| `neural-network` | `response`, `predictors`, `validation` (as for `decision-tree`; validation rows or k-fold tune the penalty, 5 folds when every row trains), `hidden` (one or two node counts, e.g. `[5, 3]`; `[3]` by default), `activation` (`"tanh"`, `"logistic"`, `"relu"` or `"identity"`), `penalty` (a fixed α; omit to tune it), `solver` (`"lbfgs"`, `"adam"` or `"auto"`), `starts` (random starts; 3), `max_iter` (2000), `seed`. Saves `predicted` (or class probabilities and `most_likely`), `prediction_formula` (continuous response) and `validation`. |
| `gaussian-process` | `response`, `predictors`, `validation` (as for `decision-tree`; with none, each row's held-out prediction is its exact leave-one-out prediction), `kernel` (`"matern52"`, the default, or `"squared-exponential"`), `nugget` (a fixed noise variance as a fraction of the response variance; omit or `null` to estimate it), `starts` (likelihood starts; 10), `seed`; also `show_profiler`, `show_surface_profiler`, `show_observed`. Saves `predicted`, `std_error` and `validation`. |
| `support-vector-machine` | `response`, `predictors`, `validation` (as for `decision-tree`; validation rows or k-fold tune C and γ, 5 folds when every row trains), `kernel` (`"radial"`, `"polynomial"` or `"linear"`), `degree` (polynomial kernel; 3 by default), `cost` and `gamma` (fixed values; omit either to tune it), `epsilon` (regression, in response standard deviations; 0.1 by default), `seed`. Saves the same columns as `random-forest`. |
| `adaptive-splines` (also `mars`) | `response` (continuous), `predictors`, `linear` (continuous predictors that enter only linearly, no knots), `validation` (as for `decision-tree`; validation rows or k-fold choose the number of terms; with every row training GCV chooses it), `degree` (greatest interaction degree; 1 by default), `max_terms` (forward-pass terms including the intercept; 0 for min(200, max(20, 2p)) + 1), `penalty` (GCV cost per knot; 2 additive, 3 with interactions by default), `threshold` (smallest gain in R² for a forward step; 0.001), `min_span`, `end_span` (cases between knots and outside the end knots; 0 for Friedman's rules). Saves `predicted`, `prediction_formula` (a formula column of `max` hinges) and `validation`. |
| `model-comparison` | `response`, `models` (prediction columns, one per model: predicted values, or one class's probability), `event` (the event class of a categorical response; `()` for the last class), `validation` (a column of roles, or `()`). |
| `response-surface` | `response`, `factors`, `options` |
| `mixture-model` | `response`, `components`, `model` |
| `several-responses` | `responses`, `predictors`, `model` (`"main-effects"`, `"interactions"`, `"quadratic"`), `goals` (one per response: `()` for the default, or `#{ goal: "maximize" \| "minimize" \| "target" \| "none", low, target, high, shape, shape_above, importance }`), `settings` (one per predictor: a number or a level), `simulation` (`#{ runs, seed, factors: [#{ distribution: "fixed" \| "normal" \| "uniform" \| "triangular", spread }, …], noise, lower_spec, upper_spec, region }`, where `region` is one `[low, high]` per predictor: the box whose corners are checked for the probability of meeting every specification), `constraints` (linear constraints and `exclude` lines in the factors' units, one per line as for optimal designs, which the best settings keep to; `""` for none), `selection` (each response's terms chosen by its own stepwise search: `#{ direction: "forward" \| "backward" \| "mixed", criterion: "p-value" \| "aic" \| "bic", enter, remove, heredity: "strong" \| "weak" \| "none", candidates }`, where `candidates` lists effects as `[[predictor position, power], …]` and is left out for the `model` terms; `()` or absent fits every term). Its sections include the best settings found, the simulation at the given settings and, with a region, its corners and contour plot. |
| `distribution` | `column`, `quantile_type` (1–9; the application's default is 8), `options`. Besides the display switches, `options` takes `tolerance: #{ proportion, confidence, sides: "two-sided" \| "lower" \| "upper", method: "normal" \| "distribution-free" }`, `outliers: #{ method: "grubbs" \| "dixon" \| "generalized-esd", side: "two-sided" \| "greater" \| "less", alpha, dixon_ratio: "r10" … "r22", max_outliers }` and `runs: #{ reference, alternative }` (each `()` or absent when off). Censored values: `censor` (a flag column, non-zero = censored) with `censor_direction` (`"left"`, the default: below the value; or `"right"`), or `upper` (a column with each value's upper end; the analysed column is the lower end, an empty end is open). |
| `bivariate` | `x`, `y`, `pairwise` (for a continuous and a categorical column with three or more groups: `"tukey-kramer"`, the default, `"games-howell"`, `"holm-t"`, `"bonferroni"`, `"sidak"`, `"holm-sidak"`, `"fisher-lsd"`, `"newman-keuls"`, `"tamhane-t2"`, `"dunnett-t3"` or `"none"`) |
| `one-sample`, `compare-groups`, `paired` | columns, `alternative`, `level`, …; `compare-groups` also takes `method` (all-pairs comparisons: the `pairwise` keys of `bivariate` other than `"none"`, or `"dunn"`), `linear_trend` (`true`: a linear-trend contrast across the groups in order), `control` (a group label: comparisons with that control by Dunnett's method), `best` (`"largest"` or `"smallest"`: Hsu's comparisons with the best) `ratio_of_means` (`true`: two groups, the ratio of means with Fieller's interval) and `median_ties` (`"below"`, `"above"`, `"ignore"`: how the median test counts values at the median); `paired` takes `equivalence` (`[low, high]`) and `ratio_equivalence` (`true`: test the ratio of geometric means, with the bounds as ratios). A `linear-model`'s `options` also take `best`, `pairwise` (the keys above) and `linear_trend` for one-way models, and `show_effect_sizes` (η², partial η², ω², Cohen's f with intervals) for any model. |
| `many-tests` | `columns` (numeric), `group` (a column with two values), `test` (`"welch"`, the default, `"pooled"` or `"rank-sum"`), `adjust` (`"bh"`, the default, `"bonferroni"`, `"sidak"`, `"holm"`, `"hochberg"` or `"by"`), `alpha` (0.05), `order` (`"columns"`, `"p-value"` or `"difference"`: the order of the table's rows) |
| `screening` | `responses` (numeric columns), `factors`, `model` (`"linear"`, the default, `"poisson"`, `"negative-binomial"`, `"kruskal-wallis"` with one categorical factor), `subgroup` (a column: one screen per level; `()` for none), `fdr` (`"bh"`, the default, `"by"`, `"storey"`), `alpha` (0.05), `order` (`"p-value"`, the default, `"effect"`, `"columns"`). Saves no columns. |
| `area-under-curve` | `x`, `y` (numeric columns), `group` (one curve per level, or `()`), `baseline` (a value; 0) or `baseline_points` (the mean of the first k points is the baseline), `level` (0.95), `min_peak_share` (0: list every peak). Saves `baseline_subtracted`, `percent_of_range`, `fraction_of_total`. |
| `multiple-response` | Either `column` (a text column of lists) with `delimiter` (`";"`; `""` treats each cell as one item), or `indicators` (one column per item; a non-zero number, yes, true, x or ✓ is ticked); `group` (cross-tab and tests by a column, or `()`), `order` (`"frequency"`, most ticked first, or `"as-found"`). |
| `pharmacokinetics` | `subject`, `time`, `concentration`, `treatment` (a column or absent), `dose` (a number) or `dose_column` (absent: no dose, so no CL or Vz), `auc_method` (`"linear-up-log-down"`, the default, or `"linear"`), `administration` (`"extravascular"`, `"iv-bolus"`, `"iv-infusion"` with `infusion_duration`), `terminal` (`"best-fit"`, `"last-points"` with `terminal_points`, or `"time-range"` with `terminal_range: [start, end]`) |
| `adjust-p-values` | `p_values` (a column), `label` (a column naming the tests, or `()`), `weight` (Stouffer weights, or `()` for equal weights), `alpha` (0.05), `lambda` (Storey's λ, 0.5). Saves `bonferroni`, `sidak`, `holm`, `hochberg`, `bh`, `by` and `q`. |
| `proportions`, `rates`, `power` | see `r.request()` of a dialog-made report. `power` takes `design` and its `parameters`: `"one-sample-t"`, `"one-sample-z"`, `"paired-t"` (`difference`, `std_dev`, `correlation`), `"two-sample-t"`, `"one-way-anova"`, `"one-proportion"`, `"two-proportions"`, `"one-rate"` (`null`, `actual`, `exposure`), `"two-rates"` (`first`, `second`, `exposure`, `ratio`), `"one-std-dev"`, `"two-variances"`, `"equivalence-one-sample"`, `"equivalence-two-sample"`, `"margin-mean"` (`margin`, `std_dev`), `"margin-proportion"` (`margin`, `proportion`), `"margin-std-dev"`, `"margin-rate"` (`margin`, `rate`, `exposure`), `"tolerance-interval"` (`proportion`, `confidence`, `max_proportion`, `two_sided`: 1 or 0), `"tolerance-distribution-free"` (`proportion`, `removed`: order statistics outside the interval, `two_sided`: 1 or 0; solve for the confidence as the power), `"two-level-factorial"` (`factors`, `fraction`, `center_points`, `interactions`: 1 or 0, `effect`, `std_dev`), `"screening"` (`runs`, `factors`, `center_points`, `effect`, `std_dev`), `"general-factorial"` (`levels`, `other_cells`, `max_difference`, `std_dev`); and `solve` (`"power"` with `n`, `"sample-size"` with `target`, `"effect"` with both) |
| `goodness-of-fit` | `column`, `frequency` (optional column), `model` (`"proportions"` with `proportions: [#{ level, proportion }, …]`, empty for equal proportions; or `"poisson"` with `min_expected`, 5 by default) |
| `imputation` | `columns` (numeric; any but date-time columns for chained equations), `method` (`"normal-em"`, `"low-rank-svd"`, `"mean"`, `"median"`, `"multiple-chained-equations"`, `"multiple-normal-model"`); for the two multiple imputations `imputations` (20), `iterations` (10 for chained equations, 50 for the normal model), `donors` (5, predictive mean matching), `column_methods` (one per column: `"pmm"`, `"bayesian-regression"`, `"logistic"`, `"multinomial"`, `"none"` or `()` for the default), `predictors` (a matrix of `true`/`false`, row = column imputed) and `visit_order` (column positions), `seed` (of the first chain; chain *l* uses seed + *l* − 1); `rank` (low-rank SVD; `()` chooses it on held-out values), `shrinkage` (0; above 0 replaces the regularization), `regularize` (`true`: regularized iterative PCA), `standardize` (`true`), `max_iterations` (1000), `tolerance` (1e-8), `seed`. Saves `imputed1`, `imputed2`, … (the completed columns, in order) and `count` (values imputed per row). |
| `multiway-table` | `rows`, `columns` (classification columns, outermost first; at least one in all), `frequencies` (`"count"`, `"row-percent"`, `"column-percent"`, `"total-percent"`; `["count"]` when absent), `measures` (continuous columns), `statistics` (`"n"`, `"mean"`, `"sd"`, `"median"`, `"min"`, `"max"`, `"sum"`; `["mean"]`), `frequency` (optional count column), `margins` (`true`), `subtotals` (`false`) |
| `stratified-tables` | `row`, `column`, `strata` (stratum columns; `[]` for one table), `frequency` (optional count column), `confidence` (0.95, of the common odds ratio), `exact` (`true`: Fisher's exact test of each table), `monte_carlo_tables` (10000, used when a table is too large for the exact computation), `seed` (1), `fdr` (`"bh"`, `"by"`, `"storey"`, or `()` for off: each stratum's p-values adjusted across the strata) |
| `crossover` | `response`, `subject`, `period`, `treatment`, `reference` (a treatment value; the first in order when absent), `scale` (`"ratio"` or `"difference"`), `bounds` (`[low, high]`; 0.8 and 1.25 on the ratio scale by default), `alpha` (0.05) |
| `factorial-repeated-measures` | `response`, `subject` (long data: one row per subject and within-subject cell; replicates are averaged, incomplete subjects dropped), `within` (one to three within-subject factors), `between` (between-subject factors; four factors at most in all), `simple_effects` (optional `#{ factor, by }`: the test of `factor` at each level of `by`) |
| `nested-comparison` | `response`, `group`, `subject` (subjects nested in groups; each subject's label is read within its group), `level` (0.95) |
| `distribution-calculator` | `family` (a key such as `"normal"`, `"weibull"`, `"noncentral-t"`, `"binomial"`), `parameters` (`#{ key: value }`, defaults for any left out), `region` (`"below"`, `"above"`, `"between"`, `"outside"`), `a`, `b`, `probability`; uses no columns |
| `principal-components` | `columns`, `standardize` |
| `clustering` | `columns`, `standardize`, `method` (`"k-means"`, `"hierarchical"`), `k`, `seed`, `linkage`, `choose_k` (k-means: `()` or `#{ max_k, references, reference }` with `reference` `"data-range"` or `"principal-components"`: the gap statistic and average silhouettes for k = 1 … `max_k`), `distance` (hierarchical: `"euclidean"`, the default, a metric of the `distance-matrix` table operation such as `"gower"`, or `"matrix"` when the columns are a distance matrix) |
| `correspondence` | `rows`, `columns` |
| `correlations` | `columns` (two or more), `missing` (`"pairwise"`, `"listwise"`), `level` (0.95), `spearman`, `partial`, `covariance` (true/false) |
| `outlier-distances` | `columns`, `distance` (the one plotted: `"mahalanobis"`, `"jackknife"`, `"t2"`), `alpha` (upper-tail probability of the limit, 0.05) |
| `manova` | `responses` (two or more continuous columns), `predictors`, `effects` (as for `linear-model`; `[]` for main effects), `statistic` (ranking the Effect Pareto: `"pillai"`, the default, `"wilks"`, `"hotelling-lawley"`, `"roy"`), `contrasts` (`[#{ name, factor, weights: #{ level: weight, … } }, …]`, weights adding to zero), `plot_factor` (a categorical predictor for the canonical plot; `()` for the first) |
| `latent-class` | `items` (nominal or ordinal columns), `min_classes` (1), `max_classes` (5), `starts` (10), `smoothing` (0), `seed` (1). Saved columns: `class`, `probability_1`, `probability_2`, … |
| `text-exploration` | `column` (a text column), `case_fold` (true), `remove_stop_words` (true), `extra_stop_words` (`[]`), `stem` (false), `min_length` (2), `keep_numbers` (false), `min_document_frequency` (2), `max_terms` (0: all), `weighting` (`"tf-idf"`, `"log-tf-idf"`, `"count"`, `"log-count"`, `"binary"`), `table_terms` (100), `chart_terms` (20), `cloud_words` (80), `phrase_length` (3; below 2 for no phrases), `min_phrase_count` (2), `dimensions` (2; 0 for none), `clusters` (0), `seed` (1). The word cloud section's value is `#{ kind: "word-cloud", words: [#{ text, count, size, rows }] }`. Saved columns: `dimension_1`, `dimension_2`, …, `cluster` |
| `topic-model` | the term settings of `text-exploration` (`column` … `max_terms`), `topics` (5), `alpha` (`()` for 50 ÷ topics), `beta` (0.1), `iterations` (1000), `burn_in` (200), `seed` (1), `top_terms` (8). Saved columns: `topic` (most likely), `topic_1`, `topic_2`, … (proportions) |
| `sentiment` | `column`, `negation` (true), `negation_window` (3), `lexicon` (`#{ word: valence, … }` on −4…4, added to or replacing the bundled lexicon), `threshold` (0.05, the neutral band). Saved columns: `compound` (score), `sum`, `sentiment` (category), `positive_words`, `negative_words` |
| `gaussian-mixture` | `columns`, `structures` (any of `"EII"`, `"VII"`, `"EEI"`, `"VVI"`, `"EEE"`, `"VVV"`; all six by default), `min_components` (1), `max_components` (6), `standardize` (false), `start` (`"k-means"` or `"hierarchical"`), `starts` (5), `seed` (1). Saved columns: `probability_1`, `probability_2`, …, `cluster`, `uncertainty` |
| `embedding` | `method` (`"classical-mds"`, `"metric-mds"`, `"t-sne"`, `"umap"`), `columns`, `distances` (true: the columns are a distance matrix, one per row), `standardize` (true), `dimensions` (2), `perplexity` (30, t-SNE), `neighbours` (15, UMAP), `min_dist` (0.1, UMAP), `iterations` (`()` for the method's default), `seed` (1), `color` (a column name or `()`). Saved columns: `dim1`, `dim2`, … |
| `principal-coordinates` | `columns`, `distances` (true: the columns are a distance matrix), `metric` (as for the `distance-matrix` table operation; `"gower"`), `standardize` (`false`), `correction` (`"none"`, `"lingoes"`, `"cailliez"`), `color` (a column name or `()`). Saved columns: `pco1`, `pco2`, `pco3`. |
| `item-analysis` | `items` (two or more columns), `reversed` (item columns scored low + high − x), `scale` (`[low, high]`, or `()` for the smallest and largest answer), `level` (of alpha's interval, 0.95) |
| `factor-analysis` | `columns` (three or more numeric), `factors` (a number; omit it for the eigenvalues above 1), `extraction` (`"maximum-likelihood"` or `"principal-axis"`), `rotation` (`"varimax"`, `"promax"` or `"none"`), `matrix` (`"correlation"` or `"covariance"`), `scores` (saved scores: `"regression"` or `"bartlett"`), `promax_power` (4). Saves `factor1`, `factor2`, …. |
| `discriminant-analysis` | `response` (categorical), `predictors` (numeric), `rule` (`"linear"` or `"quadratic"`), `priors` (`"proportional"` or `"equal"`), `validation` (as for `decision-tree`; validation and test rows are only classified, k-fold adds held-out predictions). Saves the same columns as `random-forest` plus `canonical1`, `canonical2`, … (canonical scores). |
| `functional-data` | `curve`, `time`, `value` (long data, one row per reading), `factors` (curve-level columns for the score model; `[]` for none), `basis` (`"b-spline"`, the default, or `"fourier"`), `order` (4, B-splines), `functions` (omit for automatic), `period` (Fourier; omit for the time range), `smoothing` (`"gcv"`, `"fixed"` with `lambda`, or `"none"`), `landmarks` (`[#{ feature: "maximum" or "minimum", from, to }, …]`; leave out `from`/`to` to search the whole range), `continuous_registration` (false), `warp_functions` (6), `warp_penalty` (0.0001), `components` (omit for 90% of the variation, at most six), `interactions` (false), `confidence` (0.95). Saved columns: `fpc1`, `fpc2`, …, `smoothed`, `registered_time` (when registered). |
| `control-chart` | `column`, `chart` (`"mean-range"`, `"mean-std-dev"`, `"individuals-moving-range"`, `"moving-average"`, `"three-way-range"`, `"three-way-std-dev"`, `"zone"`, `"short-run"`), `subgroup_size` (used when there is no `subgroup` column; 1 plots individual values on a moving-average or zone chart), `subgroup`, `phase`, `label`, `part` (optional columns; `part` is required for `short-run`), `span` (moving-average span, default 5), `sigma_multiple` (k of the kσ limits, 3; individuals, X̄–R and X̄–S), `range_span` (values per moving range of an individuals chart, 2), `standard` (`#{ center, sigma }`: limits from standard values, or `()`), `last` (`#{ count, limits_from: "all" or "last" }`: show only the last subgroups), `options` (display switches, `show_run_chart` and `run_center` (`"median"` or `"mean"`), and `location_rules` / `dispersion_rules` / `within_rules`: the run-rule numbers 1–8 applied to each chart, `within_rules` for a three-way chart's within-subgroup chart). Zone-chart points carry `note` (the cumulative score) and `signal`. Its chart sections' values carry `points` (with `label`), `limits` (`#{ from, to, center, lower, upper }`, one per stretch of equal limits) and `phases` (`#{ at, label }`). |
| `ewma-chart`, `cusum-chart` | `column`, `lambda`/`width` or `k`/`h` |
| `attribute-chart` | `chart`, `count`, `size` |
| `pareto-chart` | `category`, `frequency` and `group` (optional columns), `options` (`other_percent`: pool categories below this percentage into "Other", 0 for none; `percent_scale`). The chart section's value has `panels`, each with `label`, `total` and `bars` (`label`, `value`, `share`, `cumulative`, `other`, `rows`). |
| `cause-and-effect` | `causes` (one to three columns: branches, causes, sub-causes), `effect`. The diagram section's value has `effect` and `branches` (each `label`, `rows`, `children`). |
| `multi-vari` | `response`, `factors` (one to four columns, outermost first), `options` (`show_values`, `show_ranges`, `show_cells`, `replicates`: `"in-order"` (default) or `"stacked"`) |
| `multivariate-chart` | `chart` (`"hotelling-t2"`, `"generalized-variance"`, `"mewma"`), `columns` (two or more), `subgroup` (an ID column) or `subgroup_size` (1: individual observations), `label`, `reference` (points in the reference sample, from the start; `()` for all), `alpha`, `lambda`, and `mewma_limit` (a value of H) or `mewma_arl` (in-control average run length; 200 by default) |
| `pca-chart` | `columns` (two or more), `label`, `reference` (rows in the reference period, from the start; `()` for all), `standardize` (true: correlations), `components` (a number, or `()` to choose by cross-validation), `folds` (7), `alpha` (0.0027), `spe_limit` (`"jackson-mudholkar"`, `"scaled-chi-square"`) |
| `monitoring-limits` | `value`, `readings` (`"counts"`, `"positives"`, `"continuous"`), `samples` (positives: a column or a number), `location`, `order` (optional columns), `limits_by` (`"all"`, `"location"`, or a column name), `family` (a family key such as `"negative-binomial"`, `"zero-inflated-poisson"`, `"beta-binomial"` or `"lognormal"`; `()` for the best by AIC), `levels` (`"quantiles"` with `alert`/`action` probabilities, 0.975/0.9985; `"sigma"` with `alert`/`action` multiples, 2/3; `"typed"` with `typed: [#{ group, alert, action }]`, an empty group for every other; `"columns"` with `alert`/`action` column names), `alerts_in_window` (`[n, m]`, `()` to skip; `[2, 5]`), `rising_run` (increases in a row, `()` to skip; 3), `signals_only` (false) |
| `analysis-of-means` | `data` (`"means"`, `"proportions"`, `"rates"`), `response`, `group`, `second_group` (means only), `size` (trials or exposure: a column name or a number), `alpha` |
| `process-capability` | `column`, `lower`, `upper`, `target`, `within`, `subgroup_size`, `box_cox`, `distribution` (`normal`, a family key such as `weibull` or `lognormal-threshold`, `best-fit`, `johnson`, `nonparametric`), `by`, `group_order`, `probability_plot` (as below) |
| `between-within-capability` | `column`, `subgroup` (a column) or `subgroup_size`, `lower`, `upper`, `target`, `within` (`"pooled-sd"`, `"average-range"`), `between` (`"moving-range"`, `"variance-components"`) |
| `attribute-capability` | `model` (`"binomial"`, `"poisson"`), `count`, `size` (a column, a number, or `()` for one unit per row, Poisson only), `level` (0.95) |
| `acceptance-sampling` | `design` (`"single"`, `"double"` with `second_ratio`, `"variables"` with `sigma`: `"known"`/`"unknown"`, `"none"`), `model` (`"binomial"`, `"hypergeometric"`, `"poisson"`), `aql`, `alpha`, `ltpd`, `beta`, `lot_size`, `plans` (`[#{ type: "single", n, c }, #{ type: "double", n1, c1, n2, c2 }, #{ type: "variables", n, k, sigma }]`); uses no columns |
| `attribute-chart` | `chart` (`"p"`, `"np"`, `"c"`, `"u"`, `"laney-p"`, `"laney-u"`, `"g"`, `"t"`), `count`, `size` (not used by c, g and t), `phase` (optional column), `rules` (run-rule numbers 1–8; default `[1]`) |
| `process-capability` | `column`, `lower`, `upper`, `target`, `within`, `subgroup_size`, `box_cox`, `distribution` (`normal`, a family key such as `weibull` or `lognormal-threshold`, `best-fit`, `johnson`, `nonparametric`), `layout` (`"standard"` or `"summary"` for the summary page), `bootstrap` (`#{ resamples, seed }`: bootstrap intervals for the percentile indices of a non-normal model), `by` (a column whose groups are also studied one by one), `group_order` (`"labels"`, `"ascending"` or `"descending"` by Ppk, for the Ppk by group chart), `probability_plot` (`true` adds the probability plot with the limits to the standard layout) |
| `gauge-study` | `part`, `operator`, `measurement`, `study_multiplier`, `tolerance`, `pool_interaction` (crossed), `design` (`"crossed"`, the default, `"nested"` or `"expanded"`), `run_order` (a numeric column, or `()` for row order), and for an expanded study `factors` (`[#{ column, random: true \| false, nesting: "crossed" \| "within-part" \| "within-operator" }, …]`), `parts_within_operators` (false) and `interactions` (true); `options` takes `show_anova`, `show_part_plot`, `show_operator_plot`, `show_run_chart` |
| `gauge-linearity` | `reference`, `measurement`, `confidence` (0.95), `process_variation` (a number, or `()` for none) |
| `gauge-repeatability` | `measurement`, `reference`, `tolerance` (both required), `percent_of_tolerance` (K, 20), `spread` (L, 6), `confidence` (0.95), `direction` (a column with two values: the direction each reading approached from; adds hysteresis), `order` (a numeric time or order column for the drift; `()` for the row order), `show_drift` (`true`), `summary_statistics` (keys among `"n"`, `"mean"`, `"sd"`, `"se"`, `"min"`, `"median"`, `"max"`, `"range"`, `"six_sd"`, `"cv"`; default n, mean, sd, min, max, range) |
| `intraclass-correlation` | `part`, `operator`, `measurement`, `confidence` (0.95), `increment` (the recorded measurement increment, or `()` to read it from the data's decimals) |
| `attribute-agreement` | `part`, `appraiser`, `rating`, `trial` and `standard` (optional columns, `()` for none), `confidence` (0.95), `ordinal` (default: true for a numeric or ordinal rating column), `order` (category labels lowest first; others follow in natural order) |
| `attribute-gauge` | `reference`, `acceptance` (times accepted, or each check's result), `checks` (times checked; `()` for one row per check), `accept_label` (the result meaning accepted, one row per check), `limit` |
| `signal-to-noise` | `factors`, `responses` (one column per outer-array run or repeat), `ratio` (`"smaller"`, `"larger"`, `"nominal"`, `"nominal-variance"`) |
| `declared-design` | `factors` (column names, or `#{ column, low, high }` with `()` for the data's range), `responses`, and for a split-plot design `whole_plot` and `subplot` (columns) and `model` (the formula the design was built for) |
| `design-evaluation` | `factors` (as for `declared-design`), `model` (formula over the factor names such as `"A + B + A*B + A^2"`, or `"main effects"`, `"interactions"` (default), `"quadratic"`), `alias` (formula of alias terms; `""` for the default), `coefficient` (anticipated coefficient in units of σ, one number or a list with one per non-intercept term; 1), `alpha` (0.05), `compare` (column whose two levels are two designs to compare; `()` for none), `constraints` (constraints in the factors' actual units, one per line as for optimal designs; `""` (default) for the whole cube) |
| `factorial-analysis` | `factors` (as for `declared-design`), `response`, `terms` (`"all"`, the default: every estimable interaction; a number: interactions of up to that many factors; or a formula such as `"A + B + A*B"`), `response-type` (`"continuous"`, `"binary"`, `"count"`), `alpha` (0.05), `means` (`"data"` or `"model"`), `cube` (up to three factor columns; empty for the first three) |
| `sequential-optimization` | `responses`, `factors` (continuous or categorical), `goals` (one object per response: `goal` `"maximize"`, `"minimize"` or `"target"` with `target`, optional desirability `low` and `high`, and `importance`; maximize by default), `bounds` (one `[low, high]` or `null` per factor; `null`: the data's range), `constraints` (text, one per line, as for optimal designs), `round` (column numbering the rounds), `criterion` (`"expected-improvement"`, the default, or `"upper-confidence-bound"` with `kappa`), `batch` (4), `kernel`, `nugget`, `starts` (5), `stop_improvement` (fraction of the observed range; 0.01), `stop_value`, `candidates` (2000), `seed`. Saves no columns; add the proposed runs from the report's menu, or read them from its Proposed Runs section. |
| `survival` | `time`, `status`, `status_coding`, `group`, `options` (`options` take, among others, `trend` (log-rank test for trend), `risk_table` (number at risk under the plot) and `risk_censored` (censored counts too)) |
| `life-distributions` | `layout` (`"times"`: `time`, `censor` (optional; a censored row is still running), `censor_coding` (`"nonzero-censored"`, the default, or `"nonzero-event"`); `"intervals"`: `start` and `end`, empty start = failed by the end, empty end = still running, equal = exact), `frequency`, `group` (optional columns), `families` (keys as for `process-capability`: `"exponential"`, `"weibull"`, `"lognormal"`, `"log-logistic"`, `"gamma"`, `"normal"`, `"logistic"`, `"smallest-extreme-value"`, `"largest-extreme-value"`, and `"…-threshold"` forms), `detail` (the family shown in detail; absent for the best by AIC), `confidence` (0.95), `probabilities` (fractions failing whose times are estimated; `[0.01, 0.1, 0.5]`), `times` (times whose failure probabilities are estimated), `known_shape` (Weibull β for the known-shape fit; absent for none) |
| `life-stress` | the `life-distributions` layout keys (`layout`, `time`, `censor`, `censor_coding`, or `start` and `end`) and `frequency`; `stresses` (one or two objects: `column`, `relationship` — `"arrhenius"` (the default), `"eyring"`, `"inverse-power"` or `"exponential"` (also `"linear"`) — and, for temperatures, `unit`: `"celsius"` (the default) or `"kelvin"`), `use` (the use condition of each stress, e.g. `[10]`), `interaction` (`false`), `distribution` (`"weibull"`, the default, `"lognormal"`, `"log-logistic"`, `"exponential"`), `stress_dependent_scale` (`false`), `confidence` (0.95), `probabilities` (fractions failing whose times at use are estimated; `[0.01, 0.1, 0.5]`), `times` (times whose failure probabilities at use are estimated) |
| `fatigue` | `cycles`, `stress`, `runout` (non-zero = did not fail; absent: all failed), `frequency`, `form` (`"basquin"`, `"semi-log"`, `"quadratic-log"`, `"endurance-limit"`), `distribution` (`"lognormal"`, `"weibull"`, `"log-logistic"`), `stress_dependent_scale` (`false`), `confidence` (0.95), `probabilities` (fractions failing for the quantile curves; `[0.1, 0.5]`), `lives` (cycles for the fatigue strength; `[1e6, 1e7]`), `stresses` (for life quantiles; empty: the tested levels) |
| `degradation` | `layout` (`"repeated"`, the default: `unit`, `time`, `response`; `"destructive"`: `time`, `response`), `path` (`"linear"`, the default, with `response_transform` and `time_transform` each `"none"`, `"log"` or `"sqrt"`; `"asymptotic"`; `"power"`), `threshold` (required), `direction` (`"rising"`, the default, or `"falling"`), `method` (`"pseudo-failure-times"`, the default, or `"random-effects"`), `distribution` (life distribution of the pseudo failure times, as for `life-stress`; `"weibull"`), `response_distribution` (destructive: `"normal"`, the default, `"logistic"`, `"smallest-extreme-value"`, `"largest-extreme-value"`), `stress` (optional, an object as in `life-stress` `stresses`) with `use` (its use condition), `confidence` (0.95), `probabilities` (`[0.1, 0.5, 0.9]`), `times`, `bootstrap` (random-effects bootstrap replicates; 200, 0 for none) |
| `proportional-hazards` | `time`, `censor` (optional), `censor_coding` (`"nonzero-censored"`, the default, or `"nonzero-event"`), `predictors`, `effects` (as for `linear-model`), `strata` (optional column), `ties` (`"efron"` or `"breslow"`), `confidence` (0.95), `ph_transform` (time scale of the proportional-hazards test: `"km"`, `"rank"`, `"identity"`, `"log"`), `curve_by` (a predictor whose levels or percentiles get their own survival curve; optional), `times` (times at which survival is tabulated), `profile` (`"hazard-ratio"` or `"survival"`), `profile_time` (for the survival profile; the median time when absent), `show_profiler`, `show_surface_profiler`. Saves `linear_predictor`, `hazard_ratio`, `survival`, `martingale` and `deviance`. |
| `survival-regression` | `layout`, `time`, `censor`, `censor_coding`, `start`, `end`, `frequency` (as for `life-distributions`), `predictors`, `effects` (as for `linear-model`), `distribution` (`"weibull"`, `"lognormal"`, `"loglogistic"`, `"exponential"`), `confidence` (0.95), `curve_by` (optional predictor), `probabilities` (fractions failing whose times are predicted; `[0.1, 0.5]`), `times` (times whose failure probabilities are predicted), `profile_probability` (fraction failing the Response Explorer shows; 0.5), `show_profiler`, `show_surface_profiler`. Saves `predicted` (the time to `profile_probability` failing) and `cox_snell`. |
| `reliability-growth` | `time` (cumulative test time), `event` (optional; 0 marks an end-of-test row), `phase` (optional), `end_time` (end of a time-terminated test without an end row), `beta` (`"unbiased"` or `"maximum-likelihood"`), `confidence` (0.95) |
| `repairable-systems` | `system`, `time` (age at repair), `event` (optional; 0 marks the end of observation), `group` (optional), `variance` (`"robust"` or `"poisson"`), `limits` (`"log"`, the default, or `"normal"`), `confidence` |
| `competing-risks` | `time`, `cause` (the cause or failure-mode column), `censor_value` (the cause value meaning still running; absent when every row failed), `group`, `frequency` (optional columns), `covariates` (columns for subdistribution hazards regression; nominal ones are reference-coded), `regression_cause` (the cause modelled; absent for the first), `profiles` (covariate settings to predict at, each an array of values in covariate order), `stacked` (`false`), `mode_family` (family key, as for `life-distributions`, fitted to each failure mode; absent for none), `times` (times whose system failure probability is estimated), `confidence` (0.95) |
| `warranty-returns` | `layout` (`"triangular"`, the default: `shipped`, `returns` (one column per calendar period, in order), `label` (optional period label column); `"long"`: `ship_period`, `return_period`, `returns` (units returned) and `shipped` columns), `families` (keys as for `life-distributions`; Weibull, lognormal, exponential and log-logistic by default), `detail` (the family used for the forecast; absent for the best by AIC), `horizon` (periods to forecast; 12), `planned` (units shipped in future periods), `warranty` (periods of service in which returns are accepted; absent for no limit), `confidence` (0.95) |
| `series-description` | Every time series analysis takes `column` (the numeric series, rows in table order; an excluded or missing row inside it is an error), `time` (optional date-time or numeric column labelling the axis and forecast stamps) and `period` (observations per cycle; absent: from the time column's spacing, 12 for monthly dates). This one also takes `box_cox` (`"none"`, the default, `"guerrero"`, or λ), `differences` (0), `seasonal_differences` (0), `max_lag` (`()`: 10·log₁₀ n and at least two cycles), `bartlett` (`false`), `confidence` (0.95), `adf` (Dickey–Fuller terms: `"none"`, `"constant"`, the default, or `"trend"`), `unit_root` (`true`), `spectrum` (`false`), `spans` (Daniell spans smoothing the periodogram; `[3, 3]`, `[]` for none), `spectrum_axis` (`"frequency"` or `"period"`) |
| `decomposition` | `column`, `time`, `period` (at least 2), `method` (`"classical-multiplicative"`, the default, `"classical-additive"` or `"stl"`), `seasonal_window` (STL: an odd span of at least 7, or `"periodic"`, the default), `robust` (STL; `false`). Saves `trend`, `seasonal`, `remainder` and `adjusted` (seasonally adjusted). |
| `trend-models` | `column`, `time`, `period`, `model` (`"linear"`, the default, `"quadratic"`, `"exponential"`, `"logistic"` or `"moving-average"`), `span` (moving average; 3), `horizon` (periods forecast; 12), `confidence` (0.95, of the prediction limits). Saves `fitted` and `residual`. |
| `cross-correlation` | `column` (the output series), `input` (the input series), `time`, `period`, `max_lag` (`()`: 10·log₁₀(n/2)), `prewhiten` (`"none"`, the default, `"aic"`, or an autoregressive order), `confidence` (0.95). Lag k pairs the input at t with the output at t + k. |
| `fourier-spectrum` | Every signal analysis takes `column` (the signal), `x` (optional time column: rows are sorted by it, the sampling rate is read from its spacing, and uneven x–y pairs are resampled by linear interpolation onto an even grid), `group` (optional; one signal per level), `sampling_rate` (without `x`; 1). This one adds `window` (`"hann"`, the default, `"rectangular"`, `"hamming"`, `"blackman"`, `"flat-top"`, `"kaiser"` with `window_beta`, `"tukey"` with `window_alpha`), `detrend` (`"mean"`, the default, `"none"`, `"linear"`), `padding` (zero padding factor; 1), `power_of_two` (`false`), `sides` (`"one"`, `"two"`), `scaling` (`"density"`, `"spectrum"`), `decibels` (`true`), `phase` (`false`), `peaks` (5), `welch` (`false`), `spectrogram` (`false`), `segment_length` (256), `overlap_percent` (50) |
| `digital-filter` | the signal keys, `design` (`"butterworth"`, the default, `"chebyshev1"` with `ripple_db` (1), `"chebyshev2"` with `attenuation_db` (40), `"elliptic"` with both, `"fir"` with `taps` (61) and `window` (Hamming), `"fft"` with `cutoff_shape` (`"ideal"`, `"smooth"`), `"notch"` with `quality` (30)), `order` (4), `band` (`"lowpass"`, `"highpass"`, `"bandpass"`, `"bandstop"`), `low` and `high` (edge frequencies in the units of the sampling rate), `zero_phase` (`true`) |
| `signal-correlation` | the signal keys, `operation` (`"cross-correlation"`, the default, with `other`; `"autocorrelation"`; both with `scaling` (`"coefficient"`, `"biased"`, `"unbiased"`, `"none"`) and `max_lag`; `"convolution"` with `kernel` and `normalize` (`true`); `"deconvolution"` with `kernel` and `method` (`"wiener"` with `noise_ratio` (0.01), or `"richardson-lucy"` with `iterations` (30)); `"envelope"`) |
| `wavelet-analysis` | the signal keys, `wavelet` (`"db4"`; `haar`, `db1`–`db10`, `sym2`–`sym10`, `coif1`–`coif5`), `extension` (`"symmetric"`, `"periodic"`, `"zero"`), `levels` (as many as the length allows), `denoise` (`true`), `threshold_rule` (`"universal"`, `"sure"`, `"fixed"` with `threshold`), `shrinkage` (`"soft"`, `"hard"`), `noise` (`"finest"`, `"each-level"`), `scalogram` (`false`), `continuous_wavelet` (`"morlet"`, `"mexican-hat"`), `scales` (48) |
| `image-spectrum` | Every image analysis takes `column` (the pixel values), `x` and `y` (pixel positions, evenly spaced; y increases downwards) and `group` (optional; one image per level). This one adds `window` (`"separable-hann"`, the default, `"radial-hann"`, `"none"`), `padding` (`"none"`, the default, `"zero"`, `"mirror"`), `remove_mean` (`true`), `phase` (`false`), `radial` (`true`), `angular` (`true`), `profiles` (`true`), `profile_directions` (degrees, `[0, 90]`), `profile_half_width` (wedge half-width in degrees, 0 for a straight cut), `profile_peaks` (`false`; also profile each dominant frequency's direction), `sectors` (36), `peaks` (6), `min_frequency_fraction` (0.02 of the Nyquist frequency) |
| `image-filter` | the image keys, `band` (`"lowpass"`, the default, `"highpass"`, `"bandpass"`, `"bandreject"`, `"none"`), `low` and `high` (cut-off or band edges in cycles per unit of x; a quarter and a half of the Nyquist frequency), `shape` (`"butterworth"`, `"ideal"`, `"gaussian"`), `order` (2), `notches` (array of `[fx, fy]` pairs), `auto_notches` (0: notch this many of the strongest frequencies), `notch_radius` (two frequency steps), `notch_shape` (`"gaussian"`), `notch_pass` (`false`), `padding` (`"mirror"`) |
| `image-restoration` | the image keys and `operation`: `"deblur"` (the default) with `method` (`"wiener"`, `"least-squares"`, `"richardson-lucy"`), `lambda` (0.01), `iterations` (30), `sigma_x` and `sigma_y` (Gaussian blur in pixels; 2); `"register"` with `other` (the image to align) and `upsample` (20); or `"autocorrelation"` with `remove_mean` (`true`) |
| `exponential-smoothing` | `column`, `time`, `period` (as for `series-description`), `model` (`"automatic"`, the default, `"simple"`, `"holt-linear"`, `"damped-trend"`, `"holt-winters-additive"`, `"holt-winters-multiplicative"`, `"custom"` with `error` (`"additive"`/`"multiplicative"`), `trend` (`"none"`/`"additive"`/`"damped"`) and `seasonal` (`"none"`/`"additive"`/`"multiplicative"`)), `allow_multiplicative` and `allow_damped` (automatic only; `true`), `alpha`, `beta`, `gamma`, `phi` (values held fixed; a chosen model only), `horizon` (12), `levels` (`[0.8, 0.95]`) |
| `arima` | `column`, `time`, `period` (as for `series-description`), `orders` (`"automatic"`, the default, with `stepwise` (`true`) and `allow_constant` (`true`); or `[p, d, q]` with `seasonal_orders` (`[P, D, Q]`, using `period`) and `constant` (`false`; a mean when d + D = 0, a drift when 1)), `box_cox` (`"none"`, the default, `"guerrero"`, or λ; forecasts are back-transformed, so the point forecast is the median), `horizon` (12), `levels` (`[0.8, 0.95]`), `inputs` (input series with transfer functions: a list of maps with `column`, `delay` (a number, or `"automatic"`, the default: the first lag of significant prewhitened cross-correlation), `numerator` (0) and `denominator` (0); rows after the series with the inputs filled in supply their future values) |
| `signal-smoothing` | `y` (the signal), `x` (optional: orders the points; absent: row order), `group` (optional: one signal per level), `method` (`"savitzky-golay"`, the default, with `window` (11, odd), `order` (3) and `derivative` (0); `"moving-average"` or `"trailing-average"` with `window`; `"exponential"` with `alpha` (0.3); `"running-median"` with `window`; `"running-percentile"` with `window` and `percentile` (50); `"gaussian"` with `sigma` (2, in points)) |
| `curve-calculus` | `x`, `y`, `group` (optional), `derivative` (`"central"`, the default, or `"savitzky-golay"` with `window` (11) and `order` (3)), `integral` (`"trapezoid"`, the default, or `"simpson"`), `resample` (absent for none, or `"linear"`, `"spline"`, `"natural-spline"` or `"pchip"`, with `points` (101)) |
| `spectrum-peaks` | `x`, `y`, `curve` (optional: one spectrum per level), `baseline` (`"arpls"`, the default, with `lambda` (10⁵); `"asls"` with `lambda` (10⁵) and `p` (0.01); `"snip"` with `half_window` (20), `decreasing` (`true`) and `lls` (`false`); `"column"` with `baseline_column`; `"none"`), `smooth_window` and `smooth_order` (Savitzky–Golay smoothing before the search; 11 and 3, `()` for none), `min_height` (3) and `min_prominence` (5), in noise levels, `min_separation` (0, in x units), `fit` (absent: peaks only; or a map with `shape` (`"gaussian"`, the default, `"lorentzian"`, `"pseudo-voigt"`, `"voigt"`, `"split-gaussian"`, `"split-lorentzian"`), `baseline` (`"constant"`, the default, `"none"` or `"linear"`), `peaks` (how many of the most prominent; absent: every peak found, at most 12), `centres` (starting centres instead), `compare` (`true`), `common_width` (`false`), `spacing` (`"free"`, `"equal"` or a number)) |
| `reliability-test-plan` | `plan`: `"demonstration"` with `reliability`, `time`, `confidence`, `shape` (Weibull β), `failures` and either `test_time` (find the units) or `units` (find the test time); `"estimation"` with `distribution` (`"weibull"`, `"lognormal"`), `spread` (β or σ), `fraction_failing`, `percentile`, `confidence`, `precision`; `"accelerated"` with `distribution`, `spread`, `relationship` (`"linear"`, `"arrhenius"`, `"inverse-power"`), `use_stress`, `high_stress`, `p_use`, `p_high`, `percentile`, `confidence`, `precision`, `levels` (`[[stress, share], …]`, optional). Uses no columns. |
| `permutation-test` | `design` (`"two-groups"` with `response`, `group` and `statistic` `"mean"` or `"median"`; `"paired"` with `first`, `second`; `"correlation"` with `x`, `y` and `statistic` `"pearson"` or `"spearman"`), `alternative` (`"two-sided"`, `"less"`, `"greater"`), `exact_limit` (enumerate every arrangement up to this many; 100000), `resamples` (random arrangements otherwise; 9999), `seed` (1) |
| `bootstrap` | `report` (the bootstrapped report's own settings, with its `analysis`), `section` (title of one of its tables), `columns` (headings of the cells to collect), `rows` (labels in the table's first column; empty or absent for every row), `replicates` (1000), `seed` (1), `level` (0.95), `stratify` (a column resampled within its levels, or `()`) |
| `scatter-3d` | `x`, `y`, `z` (numeric columns), `color`, `size` (numeric), `shape` (optional columns; `()` for none), `drop_lines` (`false`), `camera` (`#{ turn, tilt }` in degrees). Its sections are the plotted-columns table and a note of rows left out; the 3-D plot itself is drawn only in the application. |
| `data-surface` | `x`, `y`, `z` (the response), `method` (`"triangulation"`, `"spline"` for the interpolating thin-plate spline, `"smoothing-spline"` with λ by generalized cross-validation), `grid` (divisions per axis, 8–120; 40), `levels` (contour levels; 10), `contour_plot`, `surface`, `filled`, `surface_contours`, `floor_map`, `points` (display switches), `camera` (`#{ turn, tilt }`). Its sections are the surface-fit table and the contour plot (its `contours`: the levels and each level's lines, and the observations). |
| `image-plot` | `x`, `y` (numeric columns placing each row on an evenly spaced grid), `value` (the column coloured), `scale` (`"sequential"`, the default, `"heat"`, `"spectrum"`, `"diverging"`, `"grey"`), `limits` (`[low, high]` for a fixed colour range, or `()` for the data range), `reverse` (`true`: the colours run from the scale's high end to its low end, so the grey scale goes from black for low values to white for high ones), `y_down` (`true`: y grows downwards as in image files), `equal_aspect` (`true`). Its picture section's value is `#{ kind: "image", x_label, y_label, value_label, x_range, y_range, y_down, values }`, where `x_range` and `y_range` are the outer edges of the grid (low, high) and `values` lists the grid row by row, the first row at the top of the picture (NaN for empty cells); its table is the Grid summary. |
| `graph` | the graph canvas's JSON (`encoding`, layers…), as saved in projects and pipeline files. `encoding.y2` lists right-axis columns; `axes` (optional) holds `title`, `x_title`, `y_title`, `y2_title` (strings, read as math markup: `x_1`, `x^2`, `\alpha`, `\frac{a}{b}`; see the Reference Manual, "Math in labels"), `x_break` and `y_break` (`[from, to]`, in data units: the stretch of the axis left out) and `markup_labels` (`true` reads column names, levels and labels as markup too). Files without these members draw as before. Its `form` object's `kind` is `"panels"` (default), `"treemap"` (`size_by_sum`, `labels`), `"parallel"`, `"multi-vari"`, `"scatter-matrix"`, `"pie"` (`donut`, `size_by_sum`, `labels`), `"radar"` (`group_means`, `fill`), `"polar"` (`angle`: `"compass"`, `"degrees"` or `"radians"`; `connect`), `"wind-rose"` (`sectors`, 4–72, 16 by default; `percent`), `"ternary"`, `"waterfall"` (`total`, `labels`), `"ridgeline"` (`height`, percent of the gap between ridges, 20–400, 150 by default; `rug`) or `"vector-field"` (`scale`, percent of the automatic arrow length, 10–1000); missing settings take their defaults and numbers out of range are clamped. The bars layer's `layout` is `"side_by_side"`, `"stacked"`, `"percent"` or `"lollipop"`. |

An analysis that cannot be computed stops the script with the analysis's own
message (catch it with `try { … } catch (e) { … }` to carry on). Each
successful analysis is stored with the table, so `t.save_project` keeps it.

`t.reports()` lists the table's reports, newest first as the application
lists them (those run in the script come before those it was opened with),
re-run on the current data.

| Function | Effect |
|---|---|
| `t.update_report(i, #{ settings })` | Change report `i` (numbered as in `t.reports()`) to new settings and display options, in the form `t.analyze` takes; returns the new report. |
| `t.close_report(i)` | Remove report `i`. |

A report's **⋯ Options ▸ Copy as script** in the application copies the
`t.analyze(…)` call that recreates it, every setting and display option
included.

## Reports

| Function | Result |
|---|---|
| `r.title`, `r.analysis` | Report heading; analysis identifier. |
| `r.sections()` | Section headings, in order. |
| `r.has_section(title)` | Whether a section exists. |
| `r.section(title)` | A section's content as data (see below). |
| `r.value(title, row)`, `r.value(title, row, column)` | One cell of a table section: the row whose first cell is `row`, in the second column or the named one, e.g. `r.value("Fit Statistics", "R²")`. |
| `r.request()` | The analysis settings, as a map (the project-file form). |
| `r.text()` or `print(r)` | The report as Markdown text. |
| `r.save(path)`, `r.save(path, #{ paper: "a4" })` | `.md` or `.txt`: the report as text. `.html` (or `.htm`): the report as one self-contained web page, every section included (tables as HTML tables, plots as inline SVG, light and dark appearance, fonts embedded; nothing is loaded from elsewhere). `.pdf`, `.docx`, `.pptx`, `.xlsx`: the report as a PDF, Word, PowerPoint or Excel document, every section included, as File ▸ Export reports… writes it (`paper`: `"letter"` or `"a4"`). `.png`, `.svg`, `.pdf`, `.eps`: a graph canvas as a figure (PNG is a 300 ppi picture; a graph canvas's `.pdf` is its figure). |
| `r.save_section(title, path)` | One section: its data as `.csv`, or tab-separated as `.tsv`, `.tab` or `.txt` (a table as shown; a plot's points with a `Row` column of 1-based row numbers, as in the data view, unlike `t.row(i)`), or a plot as a `.png` (300 ppi) picture or an `.svg`, `.pdf` or `.eps` figure (titled with the section title; excluded rows drawn hollow and gray). |
| `r.saveable_columns()` | The columns the report can add to its table, as `#{ key, name }` maps (empty if none). |
| `r.save_columns()`, `r.save_columns([keys…])` | Add all of those columns, or the named ones, to the table the report was run on. Returns the new column names (a number is added to a name already in use). |

Columns reports offer: linear models `predicted`, `prediction_formula`
(every row with its predictors present, rows without a response too),
`residual` (every row with complete data, excluded rows too); generalized
linear and penalized models `predicted`, `prediction_formula`; nonlinear
regression `predicted`, `prediction_formula`, `residual`;
`prediction_formula` adds a [formula column](formulas.md#prediction-formulas)
of the fitted equation; mixed models `conditional`, `marginal`,
`residual`; principal components `pc1`, `pc2`, …; clustering `cluster`;
outlier distances `mahalanobis`, `jackknife`, `t2`; item analysis `total`
(the scale score, reversed items flipped);
factor analysis `factor1`, `factor2`, … (factor scores); discriminant
analysis `probability_1`, … `most_likely`, `validation`, `canonical1`, …;
functional data `fpc1`, `fpc2`, …, `smoothed`, `registered_time`;
distribution of positive data `box_cox`; p-value adjustment `bonferroni`, `sidak`, `holm`, `hochberg`, `bh`, `by`, `q`;
area under a curve `baseline_subtracted`, `percent_of_range`,
`fraction_of_total`. A report saves columns only while
its columns still exist under the names it was run with.

Section titles match exactly, then without regard to case, then by the name
before any details in parentheses (`"Influence Diagnostics"` finds
`"Influence Diagnostics (2 of 60 rows flagged; …)"`).

`r.section(title)` returns:

- **tables:** an array of row maps keyed by column heading. Cells that read as
  numbers are numbers (as shown in the report, so rounded to its displayed
  digits); empty cells are `()`; others (such as `"<.0001"`) are text.
- **plots:** a map with `kind: "plot"`, `x_label`, `y_label`, `points`
  (`#{ row, x, y }`, rows numbered from 0), `line` and `band` (`[x, y]`
  polylines, or `()`).
- **histograms and bar charts:** `kind: "bars"` and `bars` (`#{ center, width, count, rows }`).
- **box plots, control charts, survival plots:** their statistics and points
  (`kind` says which).
- **heat maps** (such as "Correlation Heat Map"): `kind: "heat-map"`,
  `measure`, `rows`, `columns` (labels) and `values` (an array of rows of
  numbers).
- **notes and formulas** (such as a model's "Prediction Formula"): the text.

### Functions that use a table inside a chain

Rhai keeps a variable in use for the whole of a chain of calls on it, so a
function inside the chain cannot use the same variable:
`t.columns().filter(|c| t.level(c) == "nominal")` fails with “Data race
detected”. Give the first call its own statement:

```rhai
let columns = t.columns();
let nominal = columns.filter(|c| t.level(c) == "nominal");
```

## Statistical functions

These work on arrays of numbers (such as `t.column("Weight")`) and return a
map of results. Missing values (`()`) are left out. The same methods run as
reports through `t.analyze` (tolerance intervals, outlier and runs tests as
`distribution` options; Dunnett and the median test in `compare-groups`;
`best-subsets`; the p-value functions as `adjust-p-values`).

| Function | Options (all optional) | Result |
|---|---|---|
| `tolerance_interval(values, #{…})` | `proportion` (0.90), `confidence` (0.95), `sides` (`"two-sided"`, `"lower"`, `"upper"`), `method` (`"normal"`: exact normal-theory factor; `"distribution-free"`: order statistics) | `lower`, `upper` (±∞ for an open side), `factor`, `mean`, `std_dev`, `n`, `achieved_confidence` (distribution-free intervals reach at least the confidence asked for, or say how much less a small sample gives) |
| `grubbs_test(values, #{…})` | `side` (`"two-sided"`, `"greater"` for the largest value, `"less"` for the smallest), `alpha` (0.05) | `statistic` (G), `p_value`, `critical_value`, `value`, `index` (position in `values`, from 0), `mean`, `std_dev` |
| `dixon_test(values, #{…})` | `ratio` (`"r10"`, `"r11"`, `"r21"`, `"r22"`; by sample size when left out), `side`, `alpha` | `ratio`, `statistic`, `p_value`, `critical_value`, `value`, `index` |
| `esd_test(values, #{…})` | `max_outliers` (⌊n/10⌋, at least 1), `alpha` | `outliers` (how many), `indices`, `steps` (`#{ index, value, statistic, critical_value }` per step) |
| `best_test([group, …], #{…})` | `best` (`"largest"` or `"smallest"`), `level` (0.95) | `df`, `mse`, `comparisons` (`#{ group, mean, difference, critical_value, lower, upper, conclusion }`; conclusion `"the best"`, `"not the best"` or `"may be the best"`) |
| `dist_density(family, [parameters], x)`, `dist_cdf(…)`, `dist_quantile(family, [parameters], p)` | — (`x` and `p` may be a number or an array) | the density (probability mass for counts), P(X ≤ x), or the quantile; families by key, parameters in the calculator's order (e.g. `dist_cdf("weibull", [2, 10], 5)`) |
| `random_values(family, [parameters], n, seed)` | — | an array of `n` seeded draws (the same seed gives the same values) |
| `dunnett_test(control, [group, …], #{…})` | `level` (0.95), `alternative` (`"two-sided"`, `"greater"`: above the control, `"less"`) | `critical_value`, `df`, `mse`, `comparisons` (`#{ group, difference, std_error, t, p_value, lower, upper }`, groups numbered from 0) |
| `median_test([group, …], #{…})` | `ties` (values equal to the grand median count `"below"`, `"above"`, or `"ignore"`) | `median`, `above`, `below` (counts per group), `chi_square`, `df`, `p_value`, `min_expected` |
| `runs_test(values, #{…})` | `reference` (the median when left out), `alternative` (`"less"`: too few runs, `"greater"`: too many, `"two-sided"`) | `runs`, `n`, `above`, `below`, `reference`, `expected`, `std_dev`, `p_fewer`, `p_more`, `p_value`, `exact` |
| `up_down_runs_test(values, #{…})` | `alternative` | as `runs_test`, for runs of increases and decreases (exact up to 400 values) |
| `p_adjust(p_values, method)` | — (`method`: `"bonferroni"`, `"sidak"`, `"holm"`, `"hochberg"`, `"bh"`, `"by"`, or `"q"` for Storey's q-values with λ = 0.5) | an array of adjusted p-values in the same positions; missing values stay `()` and are not counted |
| `ranks(values)`, `ranks(values, ties)` | `ties`: `"average"` (default), `"min"`, `"max"`, `"dense"` or `"first"` (row order) | an array of ranks from 1 in the same positions; missing values stay `()` and are not ranked |
| `q_values(p_values, #{…})` | `lambda` (0.5) | `pi0` (Storey's estimate of the share of true nulls), `lambda`, `q_values` (in the same positions) |
| `combine_p_values(p_values, #{…})` | `weights` (an array, one per p-value, for Stouffer's weighted Z) | `fisher` and `stouffer`, each `#{ statistic, df, p_value, k }` (`df` is `()` for Stouffer) |
| `best_subsets(y, #{ name: values, … }, #{…})` | `models_per_size` (3), `max_size`, `forced` (names in every model) | an array of models, by size and then best first: `#{ predictors, size, rss, r_squared, adj_r_squared, cp, aicc, bic }`; rows with a missing value are left out |
| `smooth(values, #{…})` | `method` and its settings as for the `signal-smoothing` analysis; `spacing` (1: the x step, for Savitzky–Golay derivatives) | an array of smoothed values in the same positions (missing values are skipped and stay `()`) |
| `derivative(x, y)` | — | dy/dx by central differences (any spacing), in the same positions |
| `cumulative_integral(x, y, #{…})` | `rule` (`"trapezoid"` or `"simpson"`) | the running integral from the smallest x, in the same positions |
| `interpolate(x, y, at, #{…})` | `method` (`"linear"`, `"spline"`, `"natural-spline"`, `"pchip"`) | y at `at` (a number or an array); `()` outside the range of x |
| `baseline(values, #{…})` | `method` (`"arpls"`, the default, `"asls"`, `"snip"`), `lambda` (10⁵), `p` (0.01), `half_window` (20), `decreasing` (`true`), `lls` (`false`) | the estimated baseline, in the same positions |
| `noise_level(values)` | — | σ̂ = 1.4826·MAD of the successive differences / √2 |
| `find_peaks(values, #{…})` | `min_height`, `min_prominence` (0), `min_distance` (1, in points) — in the values' own units | an array of `#{ index, height, prominence, left_base, right_base, width }` (positions from 0; width at half prominence, in points) |
| `fit_peaks(x, y, #{…})` | `shape` (`"gaussian"` …, as for `spectrum-peaks`), `baseline` (`"linear"`, the default, `"constant"`, `"none"`), `centres` (starting centres; else the `peaks` most prominent maxima, at least 5 noise levels prominent), `fwhm` (starting width with `centres`), `common_width`, `spacing`, `compare` (`false`) | `peaks` (`#{ centre, centre_se, height, height_se, fwhm, fwhm_se, area, area_se, parameters, parameter_se }`), `parameter_names`, `baseline`, `baseline_se`, `n`, `parameters`, `sse`, `sigma`, `r_squared`, `aicc`, `bic`, `converged`, `termination`, `fitted`, and with `compare` a `comparison` array (`#{ peaks, aicc, delta_aicc, f, p_value }`) |

```rhai
let t = open_sample("Hald cement");
let x = #{};
for name in t.columns() {
    if name != "Heat (cal/g)" { x[name] = t.column(name); }
}
for m in best_subsets(t.column("Heat (cal/g)"), x, #{ models_per_size: 2 }) {
    print(`${m.predictors}: Cp = ${m.cp}`);
}
```

## Signal processing functions

These work on arrays of evenly spaced samples (such as
`t.column("Microphone")`), which must be complete (no missing values), and
return arrays or maps of arrays. Options use the same names as the signal
reports' settings below; `sampling_rate` (1) sets the unit of frequency.

| Function | Options (all optional) | Result |
|---|---|---|
| `fft(x)`, `fft(re, im)`, `ifft(re, im)` | — | `re`, `im`, `amplitude`, `phase` of the discrete Fourier transform (any length; the inverse is scaled by 1/n) |
| `spectrum(x, #{…})` | `sampling_rate`, `window` (`"rectangular"`, the default here, `"hann"`, `"hamming"`, `"blackman"`, `"flat-top"`, `"kaiser"` with `window_beta`, `"tukey"` with `window_alpha`), `detrend` (`"none"`, `"mean"`, `"linear"`), `fft_length`, `sides` (`"one"`, `"two"`), `scaling` (`"density"`, `"spectrum"`) | `frequency`, `amplitude`, `phase`, `power`, `resolution` |
| `welch_spectrum(x, #{…})`, `spectrogram(x, #{…})` | as `spectrum`, plus `segment_length` (256), `overlap` (samples; half a segment for Welch, an eighth for the spectrogram); Hann and Tukey (α = 0.25) windows by default | Welch: `frequency`, `power`, `segments`; spectrogram: `frequency`, `time` (segment centres), `power` (one array per segment) |
| `filter_signal(x, #{…})` | `design` (`"butterworth"`, `"chebyshev1"`, `"chebyshev2"`, `"elliptic"`, `"fir"`, `"fft"`, `"notch"`), `band` (`"lowpass"`, `"highpass"`, `"bandpass"`, `"bandstop"`), `low` (or `cutoff`) and `high`, `order` (4), `ripple_db` (1), `attenuation_db` (40), `taps` (61), `window` (Hamming), `cutoff_shape` (`"ideal"`, `"smooth"`), `quality` (30), `zero_phase` (`true`), `sampling_rate` | the filtered array |
| `design_filter(#{…})` | as `filter_signal`, plus `points` (512) | `frequency`, `gain`, `gain_db`, `phase` (one forward pass), and `sections` (`[b₀, b₁, b₂, a₀, a₁, a₂]` each) or `taps` |
| `convolve(a, b)`, `convolve(a, b, mode)`, `circular_convolve(a, b)` | `mode`: `"full"` (the default), `"same"` (centred, the length of `a`), `"valid"` | the convolution |
| `cross_correlation(x, y, #{…})`, `autocorrelation(x, #{…})` | `scaling` (`"coefficient"`, the default, `"biased"`, `"unbiased"`, `"none"`), `max_lag` | `lag`, `value`, `lag_of_max`, `max_value`; r(ℓ) = Σ x(t+ℓ)·y(t), so a positive lag means `x` follows `y` |
| `deconvolve(y, kernel, #{…})` | `method` (`"wiener"`, the default, with `noise_ratio` (0.01); `"richardson-lucy"` with `iterations` (30), for non-negative data) | the deconvolved array |
| `hilbert(x, #{…})` | `sampling_rate` | `envelope`, `phase` (unwrapped, radians), `frequency` (instantaneous, one fewer value), `imaginary` (the Hilbert transform) |
| `wavelet_decompose(x, #{…})`, `wavelet_reconstruct(decomposition)` | `wavelet` (`"db4"`), `extension` (`"symmetric"`, `"periodic"`, `"zero"`), `levels` (as many as the length allows) | a map with `approximation`, `details` (coarsest first), `wavelet`, `extension`, `length`, which `wavelet_reconstruct` turns back into the signal |
| `wavelet_denoise(x, #{…})` | `wavelet`, `extension`, `levels`, `threshold_rule` (`"universal"`, `"sure"`, `"fixed"` with `threshold`), `shrinkage` (`"soft"`, `"hard"`), `noise` (`"finest"`, `"each-level"`) | `signal`, `thresholds` and `noise_sd` (per detail level, coarsest first), `share_zeroed` |
| `cwt(x, #{…})` | `wavelet` (`"morlet"`, `"mexican-hat"`), `scales` (an array of scales in samples, or a count (32) spaced on a log scale from 1 to n/4), `sampling_rate` | `scales`, `frequency`, `coefficients` (one array per scale) |

```rhai
let t = open_sample("Test bench signals");
let mic = t.column("Microphone").extract(0, 1000);   // run A
let s = spectrum(mic, #{ sampling_rate: 500.0, window: "hann" });
let low = filter_signal(mic, #{ band: "lowpass", cutoff: 80.0, sampling_rate: 500.0 });
```

## Other functions

| Function | Result |
|---|---|
| `parse_json(text)` | JSON text as script values (`null` is `()`). |
| `to_json(value)` | A value as JSON text. |
| `args` | Command-line arguments after the script path. |

## Limits and safety

Each run is limited to 1 000 000 000 operations (time inside an analysis
counts as one) and 64 nested calls. Programs embedding the engine can also
cap string, array and map sizes (`stat_script::Limits`); these are off by
default because Rhai enforces them by re-measuring every array after each
change, which makes building a large array very slow. Scripts read and write
files only through the functions above: `import` and `eval` are not
available.

## Performance

`examples/scripts/benchmark.rhai` times each kind of script work and prints
one line per step in milliseconds:

```text
cargo build --release -p stat-script
target/release/anovacraft-run examples/scripts/benchmark.rhai 100000
target/release/anovacraft-run examples/scripts/benchmark.rhai 200000
```

Doubling the row count should roughly double every line; a step whose time
quadruples does not scale. Use a release build: a debug build is many times
slower. Reference times from a cloud Linux container, release build:

| Step | 100 000 rows | 200 000 rows |
|---|---|---|
| Script arithmetic loop | 17 ms | 33 ms |
| Build two arrays element by element | 178 ms | 313 ms |
| Make a three-column table | 51 ms | 114 ms |
| Select rows with a test function | 101 ms | 279 ms |
| Sort by two keys | 71 ms | 221 ms |
| Summarize by group | 20 ms | 52 ms |
| Linear model, continuous and 5-level factor | 265 ms | 533 ms |
| 100 small model fits (36 rows each) | 7 ms | 7 ms |

Analyses run the same compiled code as the application, so a script adds
only the cost of turning its settings into a request (microseconds). Time
goes into script-level loops over rows: a test function in `select` or
`exclude` builds a map of the row's values for every row, so prefer table
operations (`subset` with `matching` conditions, `summarize`, `sort`) on large
tables. `tests/scripts.rs` checks that building a 100 000-element array stays
fast, so a quadratic slowdown is caught.
