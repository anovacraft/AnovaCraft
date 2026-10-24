# AnovaCraft — Feature List

What the application does today, organized by menu. Each analysis opens a
report of collapsible sections; the sections listed as *options* are added or
removed in place from the report's **⋯ Options** menu. Every plot takes part in
linked brushing: selecting rows anywhere highlights them everywhere.
Right-clicking any plot (or its section heading) saves it as a PNG picture
(drawn from the plot itself at 300 ppi, not a screenshot) or as an SVG, PDF or
EPS figure, at the plot's on-screen proportions and current axis ranges
(titled with the section; groups keep their colour and marker shape; excluded
rows drawn hollow and gray, with a key entry). The same menu on a Response
Explorer panel saves every panel at its current settings, and on the 3-D
Response Surface saves the surface at its current angle and settings. It also
copies the plot (**Copy plot**: PDF, PNG and SVG on the clipboard at once) and
opens **Save plot as…** for a size on paper, resolution, background, font
and text size (see Plots (everywhere) ▸ Figure output).
Right-clicking a report table (or its heading) copies it as tab-separated text
for a spreadsheet or document, or saves it as a data table in the current
project or in a new tab, or
bootstraps it (**Bootstrap this table…**, also under ⋯ Options ▸ Bootstrap a
table; see Compare ▸ Bootstrap of any report table); numeric
columns in report tables are right-aligned (Settings can add thousands
separators and line up decimal points; copies keep the plain numbers). Collapse all and Expand all above
the reports close every report to its title or open everything; a report's
⋯ Options menu collapses or expands its sections, and the **Export…** button
beside it (also ⋯ Options ▸ Export report…) saves the report as a document
(see Reports as documents under Data and documents), and the × beside it removes
the report (Undo remove in the status bar puts it back for 30 seconds). A
faint note beside each report says how many rows are excluded; clicking it
selects them. With two to four reports, a Go to line lists their titles and
scrolls to the one clicked; with more, a Go to ▾ list holds them. The line
on how to work plots shows only while the getting-started tips are on.
**Report row filter:** ⋯ Options ▸ Filter rows… gives any analysis report
its own filter (ticked levels of categorical columns, intervals of
continuous ones, shown as editable chips under the title); the report
recomputes from the matching rows and notes how many it filtered out, while
the table's row states, its grid and other reports are untouched. Saved with
the report in projects and scripts (`row_filter`). **Filter panel** (on the
Row filter line) shows the conditions as a form: a level list with all /
none per categorical column, from / to sliders per continuous one, and
**Step through** one level at a time (a continuous column as a moving
window of fixed width) with Previous, Next, Play / Pause and seconds per
step; each step recomputes the report. **Dropping a column on an axis:**
dragging a column from the variables list onto a report plot's axis that
shows the report's own column (a model's response, a distribution's or a
chart's column) outlines the axis and says what will happen; letting go
re-runs the report on the new column, as the column picker does (columns
of another kind, or used in another role, are refused with a note).
**Column picker:**
reports built on one response or measured column (linear, generalized
linear, penalized and stepwise models, distributions, control charts,
capability, bivariate Y, multi-vari, analysis of means) step through other
columns of the same kind with Previous / Next or a list, keeping every other
setting; capability studies keep specification limits per column, edited in
place beside the picker.
P-values below 0.05 are marked with a ★ in strong
text (Settings can turn this off).

Every dialog works the same way: it opens centred but can be moved and
resized (and reopens where it was left); column roles are chosen from lists
with measurement-level icons, a filter box, All / None, Shift-click ranges
and drops from the sidebar's variables, starting from the columns selected in
the grid; a list with more entries than it shows fades out under a ▾ (▴ once
scrolled) and scroll bars are always drawn wherever there is more to see; the foot says what is missing (⚠, also on the button's hover) or
what will run on how many rows (excluded rows counted); **Run** for analyses,
**Create** for designs and tables, Cancel and Reset; Enter runs
(⌘/Ctrl+Enter in a text field) and Escape cancels; settings most people leave
alone sit under a closed **More options**; and each dialog remembers its last
settings per document while the app runs, including a dialog left open when
switching to another document. Analysis dialogs have a **Script** toggle
showing the Rhai call their settings make, live, with Copy script. The dialog
in front dims the workspace behind it (clicks and drops still reach it).
Random seeds are entered up to 2⁵³ − 1 (and
clamped there when read from a file or script), so every seed saves exactly. **Edit analysis…** in a report's
⋯ Options menu reopens its dialog filled from the report: **Update**
replaces it in place, keeping its display options and plot sizes, **Run as
new** keeps both.

Keep this file current: any change a user can see updates it, and the user
manual (`docs/manual`), in the same commit (see CLAUDE.md). Methods follow published definitions, cited in the source;
"Verified" notes the independent reference each area is tested against.

---

## Data and documents

- **Documents and projects:** several projects open at once, each holding one
  or more tables, as tabs along the bottom edge and in the left sidebar
  (right-click a table: save its project, export, close; right-click a
  project's name: save or close the project). A one-table project shows as a
  single entry; a project of several tables shows as a foldable tree, with
  tables made from another indented under it, and its tabs framed as a
  group under a header with the project's name (click folds the group's tabs
  away, keeping the one on screen; right-click saves or closes the project). A project with unsaved changes shows a • on its tab and
  sidebar entry, and "(edited)" in the window title (checked by revision
  numbers, not by writing the project out, so it costs nothing on large
  tables; selecting rows is not a change, and undoing back to the saved state
  clears the mark). Asks once before closing
  a project with unsaved changes; closing one table of a project removes it
  from the project, asking first if other tables were made from it. File ▸ Open recent lists
  the last 10 files opened or saved. Tables and projects are renamed in place
  (double-click the name in the sidebar or on a tab, or right-click ▸
  Rename…; Enter keeps, Escape drops): a table's new name is an undoable
  edit, a project's is saved in its file (`name`) and offered by Save project
  as…. File ▸ Add to this project (Empty table, Table from clipboard, Data
  file…) and the Open Data dialog's "Add to the open project" box put new
  tables in the active project instead of a new one.
- **Duplicate on another table…** (a report's or graph canvas's ⋯
  Options): the same report, with its display options, row filter, column
  picker and plot styles, on another open table. Columns of the same name
  match at once, names differing only in case, spaces, underscores or dots
  are offered, and a list per column asks for the rest (with a note when a
  chosen column holds another kind of values).
- **Project templates:** File ▸ Save project as template… writes a
  `.acrafttemplate` file (the project's reports, graph canvases, dashboards,
  scripts, notes and plot styles, and per table the columns they read and
  the formula columns they use, without data, row states, layout or derived
  tables). File ▸ New project from template… builds it on an open table or a
  data file per template table, or for a one-table template on each data
  file in a folder (one project, a table per file), with the same column
  mapping; formula columns are recomputed and renamed columns followed in
  formulas, reports and dashboard filters. Sample project: Tensile tests
  (invented).
- **Project tree and folders:** each table in the sidebar lists its reports
  and graph canvases under it (click one to show it, scrolled into view or
  its window brought forward; the ▾ before the table folds them away), and a
  project's notebooks are listed with its dashboards. Folders group a
  project's tables, reports, canvases, dashboards and notebooks: right-click
  the project's name ▸ New folder, or any item ▸ Move to folder ▸ New folder
  (named in place), drag items or folders onto a folder (onto the project's
  name to take them out again) or use Move to folder; folders nest, rename
  in place and fold. Clicking a folder brings the windows of what it holds
  to the front (or shows its first table); right-click ▸ Open each in a
  window, New folder inside, Delete folder (what it held moves up a level).
  Folders are saved with the project (`folders`; optional, older projects
  open unchanged).
- **Start page:** shown at startup and whenever nothing is open (every menu
  stays in the menu bar). Start: New data table, Open a file or project,
  Import (text, spreadsheet workbook, database, web page), Paste from
  clipboard. Recent projects, most recent first, each with when it was last
  opened or saved ("Today, 14:32"), its tables and the first line of its
  notes; data files show rows and columns; Show all recent lists the rest.
  "What do you want to do?": six task cards naming their menu items
  (Distribution, Two or more groups, Linear model, Control chart, a design
  from the Design menu with no data needed, Exponential smoothing); with no
  table open a data task shows an "Open data for …" box in front (Choose a
  file…, Start an empty table, or a sample; Cancel or Escape) and opens its
  dialog on the table that opens; cards in a row share one height at any
  window width and the badge never wraps. Learn: Sample data (every sample by topic), Quick guide, Reference manual, and a
  rotating tip with Next (on its own line when the window is narrow). Icons are Lucide's, in light and dark.
- **Project notes:** free-text notes in the sidebar below the row counts,
  saved in the project file (version 3 form), following the project of the
  table you work in; they can move into a window of their own, which follows
  the project too.
- **Views in windows of their own:** a dataset's data grid (New window beside
  the Data / Results / Side by side tabs, with Data shown), its results (New
  window with Results or Side by side shown) or a single report (Open in new
  window in its ⋯ Options and title right-click menus) opens in a native
  window titled "dataset · view · AnovaCraft", with Back to main window. The
  main window shows a panel in its place (Show window, Bring back here; in
  Side by side the other view fills the width under a one-line note); the
  dataset keeps its tab and list entry, marked with a window symbol (tooltip
  names the views). Closing such a window (close button, ⌘/Ctrl+W, Back to
  main window) puts the view back; closing the dataset closes its windows.
  Menus, dialogs and the sidebar act on the dataset of the window used last
  (the sidebar and status bar name it); shortcuts work in every window on its
  dataset; selections show in every window at once; dialogs open in the main
  window. A graph canvas in its own window lists the columns to drag onto its
  shelves. The window layout (table shown, each table's view, windows with
  position and size) is saved in the project; reopening puts windows back,
  moved onto the screen if saved on a monitor that is not there.
- **Dashboards:** a project can hold dashboards: reports and notes from its
  tables arranged on a 12-column grid, listed in the sidebar under the
  project's tables and as tabs after them (right-click: show, open in a new
  window, export as HTML, remove; removing asks first and Edit ▸ Undo
  restores it; closing a table that leaves a dashboard empty asks). File ▸ New dashboard, the project's
  right-click menu, or a report's ⋯ Options ▸ Add to dashboard (to an
  existing dashboard of its project or a new one) make them. A report tile is
  its own copy of a report bound to one table (its display options change
  the tile only); + Add tile adds a copy of any of the project's reports, an
  empty graph canvas, a heading or a note (double-click to edit). Tiles move
  by dragging their strip and resize by dragging their corner, on whole grid
  cells (an outline shows where; overlapping places are refused), and are
  removed with their ×; every change (tiles, filters, pickers, text,
  display options, name) is undoable from the toolbar, ⌘/Ctrl+Z or the Edit menu. Filters in a
  strip above the tiles are keyed by column name (levels to keep, or a range
  with sliders) and apply to every tile whose table has that column; a tile
  whose table lacks it says "not filtered by …". Filters never change the
  tables' own row states: tiles run on filtered copies, and their reports
  count the rows left out as "filtered out". Shared column pickers in a
  strip below the filters name a column and the columns to show instead
  (◀, list, ▶, choices…, ×; + Add picker offers the project's columns of
  the same type): every tile that reads the column shows the chosen one in
  its place, tiles whose table lacks it or that already read it are left as
  they are and say "not switched", and the tiles keep their own reports, so
  choosing the picker's own column shows them as before; the HTML export
  lists the switched columns. Selecting points in a tile
  selects those rows in its own table only. A dashboard opens in a window of
  its own (New window) and comes back (Back to main window, ⌘/Ctrl+W).
  Export as HTML… writes one self-contained page with the tiles on the same
  grid (CSS grid), each report's open sections in its tile and plots as SVG.
  Dashboards, the one shown and their windows are saved in the project.
  Publish as web app… writes a folder (index.html, the project and the web
  viewer) that any web server can serve: the browser shows the project's
  dashboards read-only, recomputed there, with filters, column pickers,
  selections and display options still working (nothing is saved); tiles
  cannot be added, moved, resized or edited. The viewer is the app built
  for WebAssembly (`scripts/build-web-viewer.sh`, which Publish offers to
  run when the viewer has not been built yet; `--serve` publishes the
  sample and serves it locally); `app-gui --viewer project.acraft` shows the
  same viewer on the desktop.
  Sample: Greenhouse trial (File ▸ Open sample data ▸ Projects).
- **Crash safety:** a panic inside one analysis becomes that report's error
  ("stopped with an internal error"; it computes again when its data or
  settings change) instead of closing the program. When the program does
  stop, a crash report (message, place and backtrace) is written to
  `~/Library/Logs/AnovaCraft` (`%LOCALAPPDATA%\AnovaCraft\Logs` on Windows,
  `~/.local/state/anovacraft/logs` on Linux; the newest 20 are kept), and
  Help ▸ Show crash reports opens that folder. Every two minutes, projects
  with changes since they were opened or saved get a recovery copy (written
  on a background thread, without row selections or window layout); saving
  removes it, and a normal exit removes all of them. After a run that ended
  abnormally (a crash, a forced quit, a power cut) the next launch offers
  **Restore unsaved work**: Restore opens the copies as projects with unsaved
  changes (saving writes to the file each came from), Discard deletes them,
  Decide later asks again next time, and Show crash report opens the report.
- **Getting-started tips** above the workspace on first use (Got it hides
  them; Settings brings them back).
- **Help:** Help ▸ Quick guide (every menu item in plain language) and
  Reference manual (every option and method in detail): both manuals are
  built into the app as PDFs (with linked contents and bookmarks), written to
  the app's data folder and opened in the system's PDF viewer, so they work
  offline and match the installed version; About links to them and to their
  pages online. Keyboard
  shortcuts (a window listing every shortcut), Show crash reports and About. Shortcuts include
  ⌘/Ctrl+N new table, ⌘/Ctrl+Shift+S Save as, ⌘/Ctrl+F Find, and ⌘/Ctrl+1/2/3
  for the Data, Results and side-by-side views.
- **Downloads:** each release on GitHub carries a universal macOS app
  (`AnovaCraft-<version>-macos-universal.zip`, Apple silicon and Intel) and a
  64-bit Windows folder (`AnovaCraft-<version>-windows-x64.zip`), built by
  `scripts/build-macos-app.sh universal` and `scripts/build-windows-exe.sh`.
  Neither is code-signed yet (README.md says how to open them the first
  time).
- **Licences:** AnovaCraft is licensed under Apache-2.0 or MIT, at the
  user's option (`COPYRIGHT`, `LICENSE-APACHE`, `LICENSE-MIT`). About names
  the licence, notes that AnovaCraft is independent of any commercial
  statistics package, and its **Open-source licences** button shows
  `THIRD-PARTY-NOTICES.txt`: the copyright notices and licence texts of the
  bundled fonts (Inter, JetBrains Mono, Liberation, DejaVu, egui's own),
  Lucide icons, the VADER lexicon and every Rust crate in the shipped programs.
  The notices are built into the app and `anovacraft-run` (`--licences`
  prints them), copied into the macOS bundle and the Windows folder, and
  written beside the web viewer into every folder that Publish as web app…
  writes.
  `scripts/build-notices.py` regenerates them; a test fails when Cargo.lock
  changes without them.
- **Knowledge pack for chat assistants** (`knowledge-pack/` in the repository):
  Markdown files with an `llms.txt` index that a user can attach to any web
  chat assistant so it can answer questions about using, designing,
  coding and developing AnovaCraft: the whole pack as one self-contained
  file, a short essentials file for assistants with small limits, or the
  separate parts (optionally zipped). Generated by `scripts/build-knowledge-pack.py`.
- **Variables sidebar:** click a variable to select its column in the grid and
  scroll to it; right-click for the column's menu (rename, sort, type, level,
  formula, insert, delete); a filter box narrows long lists by name.
- **Import:** CSV and other delimited text (delimiter detected, `.csv`
  included), TSV, JSON arrays of row objects, spreadsheet workbooks (`.xlsx`,
  `.xlsm`, `.xlsb`, `.xls`, `.ods`; one project per workbook, one table per
  chosen sheet). UTF-8, UTF-16 and Windows-1252 text. Blank and repeated
  column names are renamed (`Column 3`, `value (2)`) with a note, in every
  format. Missing-value markers such as
  `NA`, `N/A`, `NaN` and `null` (pandas' default list) are missing in number
  and date columns, with an import note; text columns keep them as text.
  A newly opened or imported table opens on the Data tab with no reports;
  nothing runs until the user chooses an analysis (projects and samples
  open with their saved reports).
- **Open Data dialog** (every text file, workbook, web-page table or web
  text, and File ▸ New table from clipboard): a preview of the raw cells with
  row and column numbers that shades the column names and the values; the
  row of names (or none), rows of names (joined, merged headings carried
  across), first and last rows of values and first and last columns, typed
  or set by clicking the preview (top-left cell, names row, first or last
  row, first or last column); starts from a detected block (title lines,
  side headings and footnotes left out). Text: separator (detected, or
  comma, tab, semicolon, vertical bar, space), decimal point or comma
  (detected), extra missing markers. Workbooks: sheets to open, one setting
  for all or per sheet, stack the chosen sheets into one table with a
  sheet-name column. Settings can skip it for files whose table starts in
  the top-left cell; web tables keep the choices for Refresh from source;
  scripts take them as `open_table(path, #{…})` and recordings write them.
  Data ▸ Rows ▸ Use row as column names (undoable) fixes a table already
  open. Example file: `examples/data/shift_weights_report.xlsx`.
- **Images as tables:** PNG (8- or 16-bit, palette), JPEG, TIFF (8- or 16-bit,
  32-bit float) and BMP files open as long tables, one row per pixel from the
  top row down: `x`, `y` (pixel-centre positions times an optional pixel size,
  y downwards) and `intensity`, or for colour images `red`, `green`, `blue` and
  `luminance` (Rec. 709 weights), in the file's own units. The Open image
  dialog shows a preview; an image larger than 1024 pixels on a side offers
  Reduce (average k × k blocks, k = ⌈side / 1024⌉, coordinates still in
  original pixel units), Crop (a region at full resolution placed by dragging
  in the preview, original coordinates) or Keep full size (up to 4096 × 4096),
  with the table's size and memory. What was done is noted in the status line
  and the project's notes. Plain grey images within 1024 pixels open directly
  when Settings says so; scripts read images with `open_table(path)`.
- **Databases and the web** (File ▸ Import from database… / Import from the
  web…): SQLite files, PostgreSQL and MySQL/MariaDB servers (TLS optional,
  preferred or required) and ODBC data sources (an optional build feature,
  `--features odbc`); list a database's tables, run an SQL query into a new
  table. Web import fetches JSON, delimited text or a table on a web page
  from an address with optional request headers; nested JSON objects become
  dotted column names, and a path picks the rows inside a larger document; a
  page's table is chosen by number, id or the address's `#section` (else the
  largest), with spanned heading and body cells filled in. Requests identify
  themselves as a desktop browser does (a User-Agent header given replaces
  it), and on macOS and Windows a dropped secure connection is retried
  through the system's own TLS; network failures are explained in words. Data ▸ Refresh from
  source reads the query or address again and replaces the rows (one undoable
  edit). Projects remember the source but never a password, secret header or
  key in the address's query (`api_key=`, `token=`…), which are asked for
  again when refreshing after reopening.
- **Dates and times:** a column type with four displays (date, date and time,
  time of day, duration), shown at a chosen UTC offset with up to six
  fractional-second digits. Import recognises ISO 8601 / RFC 3339 text, slash
  and month-name dates, and spreadsheet date cells (1900 and 1904 serial
  systems; `.ods` times of day arrive as durations). Slash dates whose order
  cannot be told apart are read month first, and the import says so; Swap day
  and month in the column's type menu fixes a day-first column. Text and JSON
  export write ISO 8601; `.xlsx` export writes date cells with number formats.
  Date columns convert to and from text and numbers (seconds since 1970) and
  analyse as continuous values. A time of day typed or converted into a
  Date column is dropped, and recoding into a new column keeps the time of
  day of unchanged values.
- **Export:** CSV, TSV, JSON, `.xlsx`; optionally with value labels in place of the stored values.
  Export data as ▸ Image… writes a value column placed by x and y columns as
  a grey 8-bit PNG or 16-bit TIFF between chosen black and white values
  (excluded rows and empty cells black).
- **Reports as documents:** a report's Export… button (or ⋯ Options ▸ Export
  report…), Export all… at the top of the Results pane (works with every
  report collapsed) and File ▸ Export reports… (⌘/Ctrl+Shift+E; also in a
  table's right-click menu) open one dialog, in the window it was asked from: every
  report of the table (the default) or only this report, whether to leave
  out collapsed sections and reports (all sections are exported by
  default), the format, and for PDF and Word the paper size (Letter or A4;
  the format and paper are remembered). Each export starts with a heading
  naming the table, its row counts and the date:
  - **PDF:** paged, plots as vector graphics, bundled fonts embedded (subset),
    bookmarks for every report and section, page numbers; numeric columns
    right-aligned in the monospace font; long tables repeat their header row
    on each page; wide tables are set smaller, then wrap their text columns,
    then split into column groups that each repeat the first column.
  - **Word (`.docx`):** heading styles (navigation pane, table of contents),
    tables with repeating header rows, plots as pictures (PNG with the SVG
    alongside for Word 2016 and later), page numbers.
  - **PowerPoint (`.pptx`):** 16:9 slides: a title slide, a slide per report
    when there are several, then a slide per plot, table or note titled with
    its section; long tables continue over slides and wide ones split into
    column groups.
  - **HTML:** one self-contained page (tables as HTML tables, plots as inline
    SVG, light and dark appearance, the bundled fonts embedded unless the page
    would pass 1.5 MB; nothing is loaded from the web).
  - **Markdown:** GitHub-flavoured tables, plots as PNG files in a
    `<name>_files` folder beside the file.
  - **Excel (`.xlsx`):** a sheet per report; cells showing plain numbers are
    stored as numbers with the decimals shown; plots as pictures.
- **Missing-value imputation** (Data ▸ Impute missing values…): column mean or
  median, multivariate normal by the EM algorithm, or iterative low-rank SVD
  (rank chosen on held-out values or fixed; regularized by default so the
  refitting settles — regularized iterative PCA — or with a soft-impute
  shrinkage). A fit that stops before converging gets a warning at the top
  of the report.
  The report shows missing counts per column, the missing-data patterns as a
  table and a map, EM's mean and covariance or the SVD's singular values,
  convergence, the held-out error by rank, the imputed values and a scatter
  plot marking them; save completed columns and a count of imputed values per
  row. Excluded rows are not used and stay missing. Sample: Air quality
  (simulated).
- **Multiple imputation** (Data ▸ Impute missing values…, the two *Multiple
  imputation* methods): chained equations (predictive mean matching with 5
  donors, Bayesian linear regression, logistic and multinomial models, for
  continuous and categorical columns, with an editable model per column,
  predictor matrix and visit order) or the multivariate normal model by data
  augmentation started from the EM fit; 20 imputations by default, seeded.
  The report adds each column's model, means pooled across the imputations,
  convergence (trace) plots, strip plots of observed and imputed values,
  observed-vs-imputed densities and category shares. **Make stacked table**
  (also a tick in the dialog, on by default) adds a table to the project with
  the original rows (Imputation 0) and every completed copy, linked to its
  source (Update remakes it from the same seed).
- **Pooled analyses across imputations:** linear models, GLMs and logistic
  regression, mixed models, one-sample, two-group and paired comparisons and
  correlations on a stacked imputation table pool across the imputations
  (⋯ ▸ Pool across imputations, on for new reports): Rubin's rules with
  Barnard–Rubin degrees of freedom, within/between variance, relative
  increase in variance, λ and fraction of missing information, D₁ and (linear
  models) D₃ effect tests, pooled R², and estimates by imputation, followed by
  the complete-case analysis. Matches R mice. Scripts:
  `t.multiple_imputation(...)` and `pool_imputations`. Sample project:
  Clinical trial with dropout (simulated).
- **Projects (`.acraft`):** one or more tables, each with its data,
  measurement levels, row states, and every report's request and display
  options, plus the window layout; reopening re-runs each report exactly. A
  one-table project without a saved layout is written in the one-table form
  (version 1, or 2 with date columns); several tables or a layout make
  version 3; older files load. A table made by a table operation also stores
  the operation, its source tables and a fingerprint of their data.
  A report that cannot be restored (a hand-edited file naming a missing
  column) is left out with a note, and formula columns that cannot be
  computed keep their saved values, so the rest of the project opens.
  Projects can carry Rhai scripts (Script window ▸ Keep in project), listed
  under Scripts in the table's right-click menu and the File menu; scripts
  set to run when the project opens are run only after asking (Run, Don't
  run, or Show in Script window).
- **Sample data** (File ▸ Open sample data, one submenu per topic: describe
  and compare, models, multivariate, designed experiments, quality,
  measurement systems, reliability, time series, graphs): 60+ datasets, each labelled with
  what it demonstrates — NIST Longley, Norris, Michelson, silicon resistivity
  and beam deflection; Hald cement; leukemia remission; sleep-drug trial; plus
  simulated sets for ANCOVA, factorials, skewed data, surveys, SPC, gauge R&R,
  clustering, GLMs, life testing, response surfaces, mixtures, attribute
  charts, decision trees, assay curve templates, non-detects (Well water
  arsenic), pharmacokinetic profiles (PK profiles), mixed-type traits for
  distances (Shrub traits) and fatigue tests with run-outs (Steel fatigue). A Projects submenu opens sample projects of
  several tables (Greenhouse trial: two tables and a dashboard).
- **Editing (Edit):** spreadsheet-style grid (cell cursor; double-click, Enter
  or typing to edit; a value wider than its cell or with several lines opens
  in a larger box over the cell, Shift+Enter for a new line in text columns;
  typing in the main grid edits the table it shows; Enter/Tab to commit; arrows, ⌘/Ctrl+arrows to the edge,
  Home/End, Page Up/Down), type-checked entry, add/delete rows and columns
  (including date columns; toolbar + Column inserts numbers, text or dates
  left or right of the selected columns or the cell cursor, or at the end, and
  a formula column in the same three places), rename, sort ascending/descending from the column heading, data
  type and measurement level (continuous, nominal, ordinal) changes (an
  ordinal column stays ordinal when converted between text and numbers;
  `NA` and other missing-value markers become missing), date
  display and offset changes, undo/redo of the last 500 edits (fewer when they hold more than 512 MB, such as many whole-column changes of a large table). Long computed
  numbers show to eight significant digits (full value on hover); whole numbers
  always show in full. Selection:
  drag across cells, Shift-click or Shift+arrows for a block, ⌘A / Ctrl+A for
  all cells; Edit ▸ Find… (⌘F / Ctrl+F) opens a Find bar that steps through
  matching cells (Next or Enter; Match case) and selects every row with a
  match (in the selected columns, or all); Replace and Replace all (one
  undoable step; cells whose new text is not a valid value are left alone).
  Value labels… (Data menu or a column heading's right-click menu): names for
  a column's stored values (1 = Female, 2 = Male). The grid shows the labels
  (hover a cell for its stored value; **Stored values** on the grid toolbar
  shows the codes), typing a label stores its value, and analyses, plots and
  keys name levels by label while the data keep the codes. Export data as ▸
  **Write value labels** writes labels instead of codes. Undoable; saved in
  projects; set from scripts (`t.set_value_labels`). **Show on hover in
  plots** (column heading menu) adds a column's value to every point tooltip
  in report plots and on the graph canvas, which now has point tooltips.
  **Column charts** (grid toolbar) draws a small chart under each heading: a
  histogram of a continuous column, level counts of a categorical one (at
  most eight bars, the rest pooled) or the values in row order for a date
  column, over the rows analyses use; clicking a bar selects its rows. Column
  headings can be dragged onto dialog and canvas drop targets.
  Recode values… (column heading menu, sidebar, or Data menu): a list of each
  distinct value with its row count and a new value, merging values given the
  same new value, with trim-spaces and case helpers, into a new column or in
  place (undoable). Every add or insert of rows or columns (Edit ▸ Add rows and
  Add column, toolbar + Row (above or below the selected rows or the cell cursor, or at the end; opening it or + Column ends a cell edit) and + Column (left or right of the selected columns or the cell cursor, or at the end), row and column heading menus) has a
  How many field, remembered for the session and set to the size of the
  selection it applies to (four selected rows insert four, beside the
  selection); column inserts also take a type and a name (Rep gives Rep 1,
  Rep 2, …). Enter on the last row adds a row; ⌘⇧I / Ctrl+Shift+I inserts rows
  above the cell block, or columns left of the selected headings. The rows or
  columns added together undo as one step, and the status bar says what was
  added. Delete selected columns (toolbar, Edit menu, heading menu) removes
  every selected column in one undoable step. Selecting rows ends a column
  selection and selecting columns or clicking a cell ends a row selection. A rejected entry is
  outlined with the reason under the cell. Data ▸ New random column… (also Edit ▸ Add column ▸
  Random values…) adds a column of seeded random draws (one per row) from any of the 22
  distributions of the distribution calculator (undoable; the same seed gives
  the same values). Data ▸ New rank column… adds a formula column of ranks
  (optionally within groups) with a tie rule: average, minimum, maximum,
  dense or first (row order). The status bar shows the cursor
  cell's column and value and the row counts. Click a column heading to select the column (Shift-click a run,
  ⌘/Ctrl-click to add, or drag across headings). Clear cells (Delete), Cut
  (⌘X / Ctrl+X) and Fill down (⌘D / Ctrl+D) act on the selection as one
  undoable step, leaving formula cells alone. Headings show the measurement
  level badge; numbers and dates are right-aligned and a missing value shows
  as a faint dot. Edits re-run dependent reports; a column in use by a report
  cannot be deleted: asking to delete one shows a "Column not deleted" box
  naming the reports that use it, with Show the reports.
- **Formula columns (Edit, column menu):** a column computed per row from a
  Rhai expression over the other columns, recomputed after every edit, paste,
  sort, row change, undo and redo, in dependency order (cycles refused);
  formulas that read only their own row recompute just the rows edited or
  added, so cell edits stay quick on large tables.
  Library: conditions, missing values, mathematics, normal/t/χ²/F/binomial/
  Poisson distributions, text and regular expressions, dates (parts,
  differences, adding intervals, formatting), other rows (`row()`, `lag`,
  `lead`), column summaries optionally by group (mean, SD, min, max, sum,
  count, quantile), cumulative sums, ranks (with a tie rule: `rank_ties`), p-values adjusted for multiple
  testing (`p_adjust`, optionally by group), curve transforms (`percent_of_range`,
  `fraction_of_total`, `subtract_baseline`), signal processing optionally by
  group (`lowpass_filter`, `highpass_filter`, `bandpass_filter`,
  `bandstop_filter`, `envelope`, `instant_phase`, `wavelet_denoise`) and
  seeded random numbers. The
  formula editor has column and function browsers that insert at the cursor,
  a live preview of the first rows, the formula typeset as mathematics
  under the text (**Reads as**: fraction bars, powers, square roots, |x|,
  Σ for column sums, row subscripts for `lag`/`lead`, with the parentheses
  that show how it is computed), errors marked at their place, and a
  choice of stored type (including dates). Formula cells cannot be typed into;
  a column a formula uses cannot be deleted; renames rewrite formulas;
  Convert formula to values keeps the values. Saved in projects, set from
  scripts and recorded. Reference: `docs/formulas.md`.
- **Clipboard (Edit, File):** Copy (⌘C / Ctrl+C) puts the selected columns
  (with their names), the block of cells, two or more selected rows (with a
  line of column names), or the cursor cell on the clipboard as tab-separated
  text; Cut copies the selected columns or block and then clears it; Edit ▸
  Copy selected rows with column names copies any selection. Paste (⌘V / Ctrl+V) reads tab- or comma-separated text into the
  grid from the cursor, adding rows and columns as needed (new columns typed
  from their values); text without tabs is split at commas only when not
  every comma is followed by a space, the table has several columns and the
  pieces fit (the summary says so); every value is checked against its column's type and a
  bad one stops the whole paste; a first line repeating the column names is
  skipped; one Undo removes the paste. File ▸ New table from clipboard makes
  a table from the text through the Open Data dialog, taking a first line of
  names as column names.
- **Table operations (Data):** *Rows* — sort by several columns (ascending
  or descending, missing values last, text in natural order), remove duplicate
  rows (all or chosen columns; first occurrence kept), subset (all, selected,
  included or matching rows; chosen columns) with an optional random sample
  (count or fraction, stratified by columns with proportional allocation,
  seeded). *Reshape* — stack columns (wide → long), unstack by a label column
  (long → wide, rows matched by ID columns), transpose, distance matrix (the
  distance between every pair of included rows as a new table, one column
  per row named by a label column: Euclidean or Manhattan, optionally
  standardized, Gower for mixed types with missing values, Jaccard,
  Bray–Curtis, Hamming or simple matching; *verified* against SciPy `pdist`
  and R `cluster::daisy`). *Merge* — combine rows
  of open tables (columns matched by name, optional source column) and join on
  key columns (inner, left, right, full outer; many-to-one and one-to-one
  checks; a missing key never matches; a join with no matching rows stops; a clashing right column gets its
  table's name in parentheses).
  *Summarize by group* — N, missing, mean, SD, min, max, median, quantiles
  and sum per column, one row per group (columns to summarize listed on the
  left, grouping columns on the right, the order every two-list dialog
  follows; also listed in the Describe menu). Every dialog previews its result.
  Sort and remove duplicates change the current table as one undoable edit
  (row states move with their rows) or, optionally, make a new table; the
  other operations open a new table that keeps its source rows' states where
  rows map one-to-one and records how it was made (shown on its tab's tooltip
  and saved in the project). New tables get short names ("Summary by
  Fertilizer", "Fertilizer A only") and, with **Keep in this project** (ticked
  by default; also in Augment design), join the source's project, listed
  under the source. When the source is edited later, the made table is marked
  ⚠ in the sidebar and tab, the status bar counts tables that can be updated,
  and a "Source changed" banner above its grid offers **Update**, which re-runs
  the operation as one undoable edit; nothing updates by itself.
- **Row states (Data ▸ Row states):** select all / clear / invert; exclude and include rows
  (excluded rows leave every analysis); hide rows in plots; select excluded rows.
  In the grid, click a row number to select the row, Shift-click for the run
  of rows between two clicks, ⌘/Ctrl-click to add or remove one, or drag down
  the row numbers (the grid scrolls when the drag passes its edge); a click
  in a data cell only moves the cell cursor.
  Sidebar summary of total, selected, excluded and hidden rows.
- **Row colours and markers (Data ▸ Row states ▸ Colour and marker by
  column…, or a column heading's right-click menu):** every row takes a
  colour and, for a categorical column, a marker shape from its value (seven
  colours × eight shapes, so 56 levels stay distinct; colour is never the only
  cue), or a colour ramp from a continuous column's smallest to largest
  value. The dialog previews each level's swatch and row count. Every report
  scatter plot and canvas point layer without a grouping of its own draws the
  rows in their colours and markers and keys them by level ("Arm = Placebo")
  or by the ramp's ends. **Clear row colours and markers** removes them.
  Saved in projects, recorded, and set from scripts.
- **Row subsets (Data ▸ Row subsets, or the Subset line under the sidebar's
  row counts):** save the selected rows under a name (up to 64 per table),
  switch between saved subsets or back to All rows, select a subset's rows,
  replace it with the selection, rename or delete it. While a subset is
  active, rows outside it are left out of every report, plot, dashboard and
  script exactly as excluded rows are (reports re-run at once); the grid marks
  them ⊘ and the sidebar names the active subset. Your own exclusions and
  hidden rows are kept apart and come back unchanged. Subsets and the active
  one are saved in the project (`"subsets"` and `"active_subset"` in its row
  states), recorded, and scriptable (`t.save_subset`, `t.use_subset`,
  `t.use_all_rows`, `t.remove_subset`, `t.subset_names`, `t.subset_rows`,
  `t.active_subset`).
- **Text pane** (Text pane toggle in the grid's toolbar): the cursor cell's
  whole value in a multi-line editor above the grid, for long text such as
  comments; Save (⌘/Ctrl+Enter), Revert, or moving to another cell stores the
  edit as one undoable cell edit, checked against the column's type.
- **Settings:** colour scheme (follow the system, light or dark); text size
  (standard, large, extra large); tooltip delay (½, 1 or 2 seconds, default 1); report tables: mark significant p-values,
  separate thousands, line up decimal points; getting-started tips; open
  files directly when the table starts in the top-left cell (off by default);
  show frame time (off by default: the status bar shows how long the last
  frame took and the slowest report in it).
- **Responsiveness on large tables:** a report's sections are built once per
  computation and display options rather than on every frame, plots without
  axis breaks draw their points without copying them, reports scrolled well
  out of view keep their place without being drawn, and the row counts in the
  status bar and sidebar take one pass over the rows.
- **Background computation:** reports are computed on background threads,
  so edits, exclusions and filters never wait for an analysis; a slow report
  shows **Updating…** (with a step count for forests, boosting, Bayesian
  sampling, bootstrap and model search) over its dimmed previous result, and
  **Cancel** keeps that result with **Compute again**; an analysis that fails
  unexpectedly keeps the previous result and shows the error.
- **Grid headings:** a measurement-level icon (Lucide ruler, tag or numbered
  list) and the whole column name, with level and data type on hover; columns
  open wide enough for their name and first values (up to a limit).
- **Untitled table:** a new table left blank closes by itself when a file,
  sample, design or other table opens.

## Scripting

- **Rhai scripts** (`.rhai`, run headless with `anovacraft-run script.rhai
  [arguments]`; reference in `docs/scripting.md`, examples in
  `examples/scripts`): open CSV, TSV, delimited text, JSON, workbooks (a sheet
  by name, or every sheet) and project files,
  built-in samples or new tables; add, replace, rename and remove columns,
  set cells and measurement levels, insert, remove and reorder rows, sort;
  select, exclude, hide and colour rows with a test function or by number; every Data-menu table operation (subset
  and random samples, remove duplicates, stack, unstack, transpose, combine,
  join, summarize by group); run any analysis with the settings a project file
  stores (`t.analyze("linear_model", #{ response: "y", predictors: ["x"] })`),
  giving the same sections as its dialog, and change or close a table's
  reports (listed newest first, as in the app); read report sections as data
  (tables as row maps, plots as points), single values by row label, and the
  request; save reports as PDF, Word, PowerPoint, Excel, HTML or Markdown
  (`r.save("x.pdf")`, every report of a table with `t.save_reports("x.docx")`,
  paper size with `#{ paper: "a4" }`), sections as CSV/TSV, any report
  plot or graph canvas as PNG/SVG/PDF/EPS, the columns a report offers (predicted
  values, residuals, component scores, clusters, Box–Cox) back into its table,
  and tables as CSV, TSV, JSON, `.xlsx` or projects that reopen with every
  report. Read database queries and tables (`read_database`,
  `database_tables`) and web data (`read_url`); credentials are passed in the
  call and never saved. Signal processing functions on arrays: `fft`, `ifft`,
  `spectrum`, `welch_spectrum`, `spectrogram`, `filter_signal`,
  `design_filter`, `convolve`, `circular_convolve`, `cross_correlation`,
  `autocorrelation`, `deconvolve`, `hilbert`, `wavelet_decompose`,
  `wavelet_reconstruct`, `wavelet_denoise`, `cwt`.
  `examples/scripts/benchmark.rhai` times each kind of work
  (reference times in `docs/scripting.md`).
- **Script window** (Scripts ▸ Script window, Scripts ▸ Open script…, or open or
  drop a `.rhai` file): editor with Rhai highlighting and line numbers; New,
  Open, Save, Save As; Run (Ctrl/⌘+Enter), Run selection (Ctrl/⌘+Shift+Enter,
  keeping line numbers) and Stop; an output log of `print`/`debug` output and
  what each run changed, with errors marked on their line (margin glyph and
  tint) and clickable to jump there. Scripts reach the open tables
  (`current_table()`, `open_tables()`, `table(name)`); their data edits merge
  as one undoable step with reports following their columns, and row states
  and new, changed or closed reports are applied, and `t.show()` opens other tables
  as new tabs behind the current one. Runs in the background on copies, so the app stays usable; a
  table edited meanwhile keeps its edits and gets the script's version as a
  new tab. Unsaved scripts are asked about before quitting. Keep in project
  stores the script in the current table's project, to run on request or when
  the project opens (after asking). Examples:
  `examples/scripts/flag_outliers.rhai`, `examples/scripts/model_outputs.rhai`.
  Editing aids: matching brackets tinted at the cursor; ⊟/⊞ in the margin
  fold and unfold each `{ … }` block (shown as ⋯; Fold ▸ Fold all blocks,
  Unfold all; an error unfolds); completion of script functions (with their
  signatures) and of the active table's column names inside quotes, as you
  type or with Ctrl+Space (arrows, Tab or Enter, Esc); Find… (⌘/Ctrl+F) with
  Replace, Replace all, Match case and Whole names, which also works on a
  recorded action list, since recordings open in this window.
- **Script notebooks** (Scripts ▸ New notebook; reopen from Scripts ▸
  Notebooks): a document of text cells and Rhai code cells kept in the
  current table's project. Run a cell (▶ Run, ⌘/Ctrl+Enter; Shift+Enter runs
  and moves on) or Run all; cells share variables and functions, which last
  until Restart. Each cell shows its run number and outputs under it: printed
  lines, errors (click to go to the line), its last value (a table as its
  first 50 rows, an array of maps as a table, a map as name/value pairs, a
  report as its tables), and the reports it opened (click to show them).
  Changes to tables merge like a Script window run. Cells can be added,
  deleted and moved; Notebook ▸ Clear outputs, Copy as one script (text cells
  as comments), Rename…, Delete notebook. Notebooks and their outputs are
  saved in the project file (`"notebooks"`).
- **Recording** (Scripts ▸ Record actions, then Scripts ▸ Stop recording; the status
  bar shows ⏺ Recording meanwhile): what you do is written as a script that
  repeats it exactly: opening files, samples and projects, data edits
  (including undo and redo), row selection, exclusion, hiding and colours,
  new analyses, report option changes and closed reports, table operations,
  and saving. The script opens in the Script window to run or save. The app
  and scripts carry out these steps through one shared action model, so a
  replay reaches the same data, row states and reports. **Copy as script**
  (a report's ⋯ Options menu) copies the call that recreates that report.
- Errors name the line and column; limits on operations and call depth
  (optional string, array and map size limits for embedding programs); a
  cancel flag; no file access except through these
  functions (no `import` or `eval`).
- Verified: every sample report run from a script matches the same request run
  directly, section for section; exclusions and dialog defaults likewise; a
  recorded session (edits, undo and redo, row states, reports, table
  operations, typed-in data) replays to identical tables.
- **Statistical functions** on arrays of numbers (the same methods as the
  menus below): normal (exact factors) and distribution-free
  tolerance intervals; Grubbs', Dixon's (r10, r11, r21, r22 with exact
  p-values) and generalized ESD outlier tests; Dunnett's comparisons with a
  control; Hsu's comparisons with the best; Mood's median test; runs tests above and below a value and up and
  down (exact); best-subsets regression by branch and bound with R²,
  adjusted R², Mallows' Cp, AICc and BIC; `p_adjust`, `q_values` and
  `combine_p_values` (multiple-testing adjustments, Storey q-values, Fisher
  and Stouffer combined tests). *Verified:* NIST e-Handbook
  examples, Dunnett's and Dean–Dixon tables, SciPy, exhaustive enumeration
  and Draper & Smith's Hald subsets.
- **Distribution functions:** `dist_density`, `dist_cdf`, `dist_quantile`
  (of a number or an array) and seeded `random_values` for every distribution
  of the calculator, named by key (`"weibull"`, `"noncentral-t"`, …).
- **Script library** (sidebar ▸ Scripts): the `.rhai` files of a scripts
  folder (chosen once, remembered in the settings) and the scripts kept in
  the active project. Click to open in the Script window; right-click to run
  a script, keep a folder script in the project, or remove a project script.
- **Batch runs** (Scripts ▸ Run script over a folder…, or `anovacraft-run
  --batch FOLDER [--pattern "*.csv"] [--results results.csv] script.rhai`):
  one script runs once per data file in a folder (every file the app opens,
  or those matching a `*`/`?` pattern), with `batch_file`, `batch_name` and
  `batch_index` set; the value of each run's last line (a map gives one
  column per key) becomes its row of a results table (`File`, `Status`,
  `Error`, then the returned values), which opens as a new table in the app
  or is written as CSV, TSV, JSON or `.xlsx` from the command line. A failed
  run is recorded and the batch carries on; Stop ends it between files.

## Describe

- **Distribution** (one column): summary statistics (moments, skewness,
  excess kurtosis, CV, CI for the mean); *options:* quantiles (Hyndman–Fan
  types 1–9), robust estimates (median, MAD, IQR, trimmed and winsorized
  means), histogram, box plot, normal fit with Shapiro–Wilk and
  Anderson–Darling tests, normal quantile plot, Box–Cox transformation (save
  the transformed column), tolerance interval (normal with exact factors, or
  distribution-free from order statistics; two-sided or one limit; drawn on
  the histogram), outlier tests (Grubbs, Dixon r10–r22 with exact p-values,
  generalized ESD; select the flagged rows), runs tests for randomness in row
  order (above and below the median or a value, and up and down; exact),
  frequency table and chart for categorical columns (levels in natural order).
  Date columns report locations (mean, quantiles, bounds) as dates and spreads
  (standard deviation, MAD, IQR) as lengths of time.
  - *Censored values* (non-detects and detection limits): a flag column
    marks values that are only known to lie below (left-censored, the
    default) or above (right-censored) the recorded value, or a second
    column holds the upper end of an interval (empty ends open). The report
    then gives counts of exact and censored values with the detection
    limits; a nonparametric distribution function (the Kaplan–Meier
    estimate reversed for left censoring, Turnbull's estimate for mixed
    censoring; a non-detect "< d" never includes d) with its percentiles
    ("< 1" when only a bound is known) and restricted mean; maximum-likelihood
    fits of the normal, lognormal, Weibull, gamma, exponential, logistic and
    log-logistic ranked by AIC; parameters, median and mean of each fit with
    intervals; a distribution-function plot of the estimate with the fitted
    curves; and, for whole-number data, Poisson and negative binomial fits
    with censored counts. Sample: Well water arsenic. *Verified:* R
    `fitdistrplus::fitdistcens` and `survival::survfit`; count fits against
    SciPy.
- **Identify distribution** (one continuous column): maximum-likelihood fits
  of the normal, lognormal, exponential, Weibull, gamma, logistic,
  log-logistic, smallest and largest extreme value distributions, and the
  threshold forms of the lognormal, exponential, Weibull, gamma and
  log-logistic (profile likelihood over the threshold). Report: goodness of
  fit ranked by AIC with Anderson–Darling A² and p-values (published formula
  for the normal; Monte Carlo simulation, cached per family and sample size,
  for the others) and the likelihood-ratio test of each threshold; parameter
  estimates; histogram with the best density; quantile plots of the best fits
  (brushable); Johnson (SB/SL/SU, percentile method) and Box–Cox
  transformations to normality with normal quantile plots. Sample: Seal
  strength. *Verified:* closed-form and score-equation solutions, recovery of
  known parameters, simulated percentage points against D'Agostino &
  Stephens' tables, uniform p-values under the null, and Johnson fits that
  reproduce their four defining percentiles.
- **Bivariate** (two columns, by type):
  - continuous × continuous — Pearson and Spearman correlations with tests and
    95% intervals, covariance, scatter plot;
  - categorical × categorical — contingency table with margins and expected
    counts, Pearson and likelihood-ratio χ² tests, Cramér's V and (2×2) φ
    with 95% intervals by inverting the noncentral χ² distribution,
    small-count warning; adjusted (standardized) residuals with ▲ or ▼
    beside cells beyond ±1.96 (so the sign never rests on colour alone);
    differences of proportions between every pair of row groups with
    Newcombe hybrid-score intervals and a forest plot (up to 10 groups);
  - continuous × categorical — means by group with intervals, one-way ANOVA,
    pooled and Welch t (two groups) or Welch ANOVA, tests of equal
    variances, rank tests, pairwise comparisons of three or more groups
    (Tukey–Kramer by default; Games–Howell, Holm-adjusted t, Bonferroni,
    Šidák, Holm–Šidák, Fisher's LSD, Newman–Keuls, Tamhane's T2, Dunnett's
    T3 or none from the options menu), values-by-group plot.
- **Multi-way table** (several categorical columns): nested row and column
  classifications (only combinations that occur), each cell holding any of
  count, row %, column % and total %, and N, mean, SD, median, min, max and
  sum of chosen continuous columns; optional totals (All row and column) and
  subtotals of outer levels; optional frequency column. Settings editable in
  place. Sample: Multi-centre trial. *Verified:* hand-worked tables;
  frequency weights equal repeated rows.
- **Area under a curve:** x and response columns, optionally one curve per
  level of a group column; trapezoid area of the mean at each x above a
  baseline (a value, or the mean of the first points), Bailer's (1988)
  standard error from replicates with a normal interval, areas above and
  below the baseline split at crossings, peaks above the baseline (start,
  end, peak x, height, area; small peaks can be left out), a plot per curve;
  save the response minus baseline, as % of range or as a fraction of total
  (also the formula functions `subtract_baseline`, `percent_of_range`,
  `fraction_of_total`). Sample: Plasma concentration. *Verified:* hand
  calculation; R for Bailer's standard error.
- **Multiple response** (check-all-that-apply answers): items kept as one
  text column of lists (separator `;` by default, any text allowed; spaces
  around items ignored, an item ticked twice counts once) or as one
  indicator column per item (a non-zero number, yes, true, x or ✓ counts as
  ticked). Report: each item's count with % of cases (adding to more than
  100) and % of responses; respondents, cases with no answer and items per
  respondent; a bar chart (click a bar to select the cases that ticked the
  item); with a group column, the cross-tab of counts and % of each group's
  respondents and a 2 × G Pearson χ² test of each item with a
  Bonferroni-adjusted overall test of marginal independence (Agresti & Liu
  1999; Bilder & Loughin 2004). Items most ticked first, or as found.
  Filters: the formula functions `has_item(s, "Email")` and `item_count(s)`
  make row filters or one yes/no column per item. Sample: Visitor survey.
  *Verified:* hand-computed counts and χ² values.
- **Pharmacokinetic parameters** (non-compartmental analysis): long data
  with subject, time and concentration columns, an optional treatment or
  period column (one profile per subject and level) and a dose (none, one
  value, or a column). Per profile: Cmax, Tmax, Clast, Tlast, AUC₀–t and
  AUMC₀–t (linear trapezoid, or linear up / log down), the terminal rate
  constant λz (best fit by adjusted R² over the last three or more points
  after Cmax, the last *k* points, or a time range) with its regression,
  t½, AUC₀–∞ with the % extrapolated, AUMC₀–∞, MRT, and with a dose CL (CL/F)
  and Vz (Vz/F); extravascular dosing, intravenous bolus (C0
  back-extrapolated) or infusion. Summary by treatment: N, geometric mean
  and CV, mean, SD, median and range. Concentration–time profiles on linear
  and semi-log axes with the terminal lines. Sample: PK profiles.
  *Verified:* an independent NumPy implementation (every parameter of 24
  profiles under both area rules, 10⁻¹⁰) and hand-worked closed forms.
- **Distribution calculator** (uses no data): density or probability mass,
  P(X ≤ a), P(X ≥ a), P(a ≤ X ≤ b) or P(X < a or X > b), quantiles and
  moments for 22 distributions — normal, lognormal, exponential, gamma,
  Weibull (with thresholds), logistic, log-logistic, smallest and largest
  extreme value, uniform, triangular, beta, t, χ², F, noncentral t, χ² and F,
  studentized range, binomial, Poisson and hypergeometric — with a plot that
  hatches the region (marks the region's sticks for discrete families).
  *Verified:* SciPy 1.18 values for every family; moments, density
  integration, quantile inversion and seeded draws.

## Compare

- **One sample vs a value:** one-sample t and z, Wilcoxon signed-rank, sign
  test, test of a standard deviation; equivalence (TOST).
- **Two or more groups** (stacked or side-by-side columns): pooled and Welch t,
  Wilcoxon rank-sum (Mann–Whitney), two-sample Kolmogorov–Smirnov (exact
  without ties for small samples, asymptotic otherwise; one- or two-sided),
  one-way and Welch ANOVA, Kruskal–Wallis;
  equal-variance tests (variance ratio, Levene, Brown–Forsythe, Bartlett);
  Mood's median test (choice of how values at the median count); effect sizes
  (Cohen's d, Hedges' g, Glass's Δ; for the one-way ANOVA η², ω² and
  Cohen's f with noncentral-F intervals, Steiger 2004); equivalence (TOST);
  all-pairs comparisons by Tukey–Kramer (default), Games–Howell, Holm-adjusted t, Bonferroni, Šidák,
  Holm–Šidák, Fisher's LSD, Newman–Keuls (stepwise, coherent p-values),
  Tamhane's T2 or Dunnett's T3 (unequal variances) or Dunn; a linear-trend
  contrast across ordered groups with a test of nonlinearity; comparisons with a
  control group by Dunnett's method (two- or one-sided, simultaneous
  intervals, interval plot; up to 100 groups); comparisons with the best by Hsu's method
  (larger or smaller is better; constrained simultaneous intervals for each
  group's difference from the best of the others, a conclusion per group and
  an interval plot); for two groups, the ratio of means (first / second)
  tested against 1 with Fieller's interval, pooled and unequal variances
  (unbounded when the second mean is not clearly different from zero).
  Sample: Paint drying times.
- **Two groups, many variables:** the same Welch t, pooled t or rank-sum
  test of two groups on every chosen column (analysed columns left, grouping
  column right), p-values adjusted by any method below (Benjamini–Hochberg
  by default), a table ordered by column, p-value or size of difference, and
  a volcano plot (difference against −log₁₀ p; columns significant after
  adjustment coloured, marked and named). Sample: Gene screen (simulated).
  *Verified:* R `t.test`, `wilcox.test` and `p.adjust`.
- **Adjust and combine p-values:** a column of p-values (optional test-name
  and weight columns) adjusted by Bonferroni, Šidák, Holm, Hochberg,
  Benjamini–Hochberg and Benjamini–Yekutieli, plus Storey's q-values (λ
  chosen; 0.5 by default); a summary of rejections per method at α; Fisher's
  and Stouffer's (optionally weighted) combined tests; ordered p-values
  against the Benjamini–Hochberg line; save any adjusted column. Sample:
  Trial endpoints (invented). *Verified:* R `p.adjust`, hand calculation of
  q-values, and R for the combined tests.
- **Paired or repeated measures:** paired t, signed-rank (Hodges–Lehmann
  estimate), sign test; repeated-measures ANOVA, Friedman; equivalence of the
  mean difference or, for positive data, of the ratio of geometric means
  (TOST on logs; bounds such as 0.80–1.25). Repeated-measures ANOVA with
  three or more conditions adds Mauchly's test of sphericity and
  Greenhouse–Geisser and Huynh–Feldt (Lecoutre) corrected F tests.
  *Verified:* R `car::Anova` (O'Brien–Kaiser data).
- **Factorial repeated measures:** two- and three-way repeated-measures
  ANOVA from stacked data (one to three within-subject factors, any
  between-subject factors, four in all; replicates averaged, incomplete
  subjects dropped): design, cell means, univariate tests of every effect
  with its own error term (Type III for unbalanced groups), Mauchly's test
  and GG/HF ε with corrected p-values per within-subject effect, partial
  and generalized η², simple effects of one factor at each level of another
  and a profile plot. Sample: Reaction times by task and cue. *Verified:*
  R `car::Anova` on the O'Brien–Kaiser data (all 15 effects, both ε,
  Mauchly) and per-level simple effects.
- **Nested comparison:** nested t test or nested one-way ANOVA with
  replicate measurements within subjects (subject labels read within their
  group): REML mixed model with Satterthwaite degrees of freedom for the
  overall F test, group means and pairwise differences; variance components
  and intraclass correlation; the classical nested ANOVA when balanced; a
  plot of measurements by subject. Sample: Dendrite lengths per mouse.
  *Verified:* R `lmerTest` (`contestMD`, same contrasts) and `aov` with an
  Error stratum.
- **Proportions:** exact binomial, two-proportion (score z, Pearson and G²
  tests of equal proportions across groups), Fisher exact, McNemar for paired
  outcomes.
- **Event rates:** exact Poisson tests of a rate and of a rate ratio.
- **Stratified tables and exact tests:** a row × column table in each stratum
  (combinations of one or more stratum columns; optional frequency column;
  levels from the rows counted, so excluded rows add no empty level):
  counts by stratum; per stratum and pooled, the odds ratio (2 × 2), Pearson
  χ² and Fisher's exact test for r × c tables (Mehta–Patel network
  algorithm; a seeded Monte Carlo estimate from Patefield's random tables,
  flagged in the report, when a table is too large, at a cost that does not
  grow with the total count; a table whose total exceeds 2⁵³ shows "exact
  test not available" with the reason); generalized Cochran–Mantel–Haenszel statistics
  (nonzero correlation, row mean scores, general association; numeric labels
  as scores) and, for 2 × 2 tables, the continuity-corrected Mantel–Haenszel
  test, the Mantel–Haenszel common odds ratio with the Robins–Breslow–
  Greenland interval beside the crude odds ratio, and the Breslow–Day test
  with and without Tarone's correction. Optionally each stratum's Pearson
  and exact p-values adjusted across strata for the false discovery rate
  (Benjamini–Hochberg, Benjamini–Yekutieli, or Storey q-values with π̂₀).
  Without strata: one table with its exact test. Sample: Multi-centre trial. *Verified:* statsmodels
  `StratifiedTable`, Agresti's job satisfaction table (general association
  10.2 on 9 df), an independent NumPy implementation, exact enumeration and
  R's published Fisher example (p = 0.7827).
- **Goodness of fit for counts:** Pearson χ² and likelihood-ratio G² tests of a
  category column against equal or given proportions (ratios such as
  9 : 3 : 3 : 1), or of a count column against a Poisson distribution with the
  mean estimated (neighbouring counts pooled until each cell expects at least
  a chosen minimum, 5 by default; one degree of freedom for the mean); optional
  frequency column; observed, expected, contribution to χ² and Pearson
  residual per cell, and a plot of observed against expected counts. Sample:
  Horse-kick deaths. *Verified:* Mendel's peas and von Bortkiewicz's horse
  kicks against SciPy.
- **Crossover trial and equivalence:** a two-period, two-treatment crossover
  from stacked data (response, subject, period and treatment columns):
  sequence summary, treatment, period and carryover effects with t tests,
  the analysis of variance (carryover against subjects within sequences),
  average equivalence by two one-sided tests on the difference or the ratio
  of geometric means (log scale, with the within-subject CV), and subject
  profiles by sequence. Sample: Bioequivalence crossover. *Verified:* against
  an independent least-squares fit of the crossover model (NumPy).
- **Power and sample size:** one-sample t and z, paired t (from the SD and the
  correlation), two-sample t, one-way ANOVA, one and two proportions, one and
  two Poisson rates, one standard deviation, two variances, equivalence (one
  or two samples); margins of error for a mean, proportion (Wilson), standard
  deviation or rate, as the probability that the interval's margin is at most
  E; tolerance intervals (probability that the content stays below a chosen
  limit; one- or two-sided); distribution-free tolerance intervals between
  order statistics (the confidence of covering a proportion P with m order
  statistics outside, from the exact binomial sum; Wilks 1941, Guenther
  1977); two-level factorial (full or fractional, center
  points, main effects or two-factor interactions), screening (Plackett–Burman
  runs) and general full factorial designs, in replicates (noncentral F).
  Solve for power, sample size or detectable effect (smallest margin or
  content limit); power curves by sample size and by effect for every design.
  *Verified:* closed forms evaluated with SciPy 1.18, an exact binomial sum
  for the proportion margin, quadrature for two-sided tolerance intervals, and
  simulation.
- **Permutation test:** two independent groups (difference of means or of
  medians), paired columns (sign-flip test of the mean difference) or the
  correlation of two columns (Pearson or Spearman); exact enumeration of every
  arrangement when there are at most a chosen number (100 000 by default),
  otherwise seeded Monte Carlo with the (b + 1)/(B + 1) p-value, run in
  parallel and reproducible from the seed; one- or two-sided; a histogram of
  the null distribution with the observed value marked. Sample: Repair times.
  *Verified:* exact p-values against SciPy 1.18 `permutation_test` (full
  enumeration) for all three designs; Monte Carlo within binomial error of
  the exact p-value.
- **Bootstrap of any report table** (right-click a report table ▸ Bootstrap
  this table…, or ⋯ Options ▸ Bootstrap a table): re-runs the report on B
  row resamples (with replacement from the included rows with values in the
  report's columns; optionally within the levels of a stratum column),
  seeded per replicate and in parallel, and collects the chosen cells (rows
  and columns of that table): original estimate, bootstrap mean, bias,
  standard error, percentile and BCa intervals (jackknife acceleration, up to
  500 rows), the count of failed replicates, and a histogram of each
  statistic's bootstrap distribution with the estimate and interval marked.
  Saved in projects with the bootstrapped report. Sample: Repair times.
  *Verified:* BCa and percentile limits against SciPy 1.18 on identical
  replicates (one sample and stratified two-sample statistics); bootstrap SE
  of a mean ≈ s/√n·√((n − 1)/n); coverage on simulated normal and
  exponential samples.
- *Verified:* published textbook examples and SciPy.
- **Bayesian** (Compare ▸ Bayesian, also under Model; `stat-bayes`):
  - *Bayesian estimation:* closed-form posterior of a mean (Jeffreys or an
    informative prior; one group, two groups with pooled or Behrens–Fisher
    posterior, paired columns), a proportion (Jeffreys, flat or informative
    beta; labelled outcomes with an event and frequency, or successes out of
    trials; two groups by difference, ratio or odds ratio) or an event rate
    (gamma prior, exposure column; two groups by an exact scaled-F rate
    ratio); posterior summary with equal-tailed and HPD intervals (a ratio's
    mean and SD shown as — with a note when they do not exist), posterior
    probabilities about a reference value and a region of practical
    equivalence, the Savage–Dickey Bayes factor for proper priors, prior and
    posterior density plots.
  - *Bayes factor tests:* one-sample, paired and two-sample t (JZS Cauchy
    prior, scale r), Pearson correlation (stretched beta, width κ), and
    contingency tables (Dirichlet, all four sampling plans); two- or
    one-sided; evidence labels, posterior of the effect size, and a
    robustness plot of BF₁₀ over the prior scale.
  - Samples: Response rates by site, Stroop reaction times.
  - *Verified:* Bayes factors against R BayesFactor 0.9.12; posteriors
    against closed forms and quadrature identities.

## Model

The menu groups its items under headings: linear models; generalized,
nonlinear and mixed; method comparison and stability; predictive models;
compare and optimize.

- **Linear model:** continuous and categorical predictors, interactions and
  powers; models kept hierarchical (heredity enforced in the dialog; a note when
  a model breaks it). Report: fit statistics (R², adjusted R², RMSE, AICc, BIC),
  Effect Pareto (−log₁₀ p), parameter estimates, effect tests, fitted line or
  observed-vs-predicted plot. *Options:* ANOVA table, confidence limits,
  confidence band, residual plot, influence diagnostics (leverage, Cook's
  distance, residual quantile plot), effect sizes (η², partial η², ω² and
  Cohen's f per effect, with noncentral-F intervals; Steiger 2004), Response
  Explorer, 3-D surface profiler.
  *Spline terms* (More options): a continuous predictor entered as a natural
  cubic spline or a linear, quadratic or cubic B-spline, with a number of
  interior knots at quantiles or knots at typed values; its effect test is
  the joint test of its columns, and saved prediction formulas include the
  basis (`natural_spline`, `bspline`). *Comparisons within slices*
  (options menu or dialog): for each two-factor interaction of categorical
  factors, each factor tested at every level of the other with F tests from
  the model's error, and pairwise differences of least-squares means within
  each slice (Tukey, Holm, Bonferroni, or tests only). *Slopes by group*:
  for a factor × continuous effect, each group's slope with its interval,
  the test of equal slopes, and pairwise slope differences with the same
  adjustments. Save predicted values and residuals as new columns (every row with complete
  data, excluded rows included), or **Save prediction formula**: the fitted
  equation as a formula column, so predictions follow edits to the data and
  are computed for new rows. Linear, generalized linear, penalized model and
  decision tree reports end with a Prediction Formula section showing that equation, with a
  Copy formula button.
  - *One-way models* (a single categorical factor): tests of equal variances
    (Levene, Brown–Forsythe, Bartlett) and a choice of pairwise comparisons —
    Tukey–Kramer (default), Games–Howell, Holm-adjusted t, Bonferroni, Šidák,
    Holm–Šidák, Fisher's LSD, Newman–Keuls (stepwise, coherent p-values),
    Tamhane's T2 or Dunnett's T3 (unequal variances), or none;
    a linear-trend test across ordered levels; comparisons with a control level (Dunnett); and comparisons with the best
    (Hsu; larger or smaller is better).
- **Response surface:** full second-order model on coded factors; stationary
  point and canonical analysis; ridge paths of maximum and minimum response;
  contour plots over any two factors (back to the first two when an edit
  leaves fewer factors), filled with a colour bar; Response Explorer and 3-D
  profiler.
- **Mixture model:** Scheffé linear, quadratic and special-cubic models;
  mixture Response Explorer (moving one component rescales the others);
  ternary plots filled as contour maps.
- **Stepwise selection:** forward, backward or mixed, by p-values, AIC or BIC,
  with heredity rules; step history; open the selected model as a linear model.
  *Several responses, each with its own search:* the same candidates searched
  separately for each response, the selected models opened together in a
  several-responses report (desirability, best settings, simulation).
- **Screen many responses:** the same model fitted to every chosen response
  column against the same factors — linear model, Poisson or negative
  binomial regression for counts, or Kruskal–Wallis for one factor — with
  each effect's p-values adjusted across the responses for the false
  discovery rate (Benjamini–Hochberg, Benjamini–Yekutieli or Storey
  q-values). Report: summary with discoveries per effect, a results table per
  effect (sorted by p-value, effect size or column order; discoveries
  marked), a volcano plot per effect (effect size against −log₁₀ p, the
  discoveries named), and for two-level factors the difference (or rate
  ratio) with its interval and a forest plot. Optional subgroups screen each
  level of a column separately. ⋯ Options opens any response's full report,
  smallest q first. Sample: *Transcript counts (simulated)*.
- **Best subsets:** the best models of each size among every subset of the
  candidate effects (branch and bound), with forced effects, heredity rules,
  models kept per size and a largest size; R², adjusted R², Mallows' Cp, AICc
  and BIC for each; the best overall by a chosen criterion, a plot of the
  criterion against size, and any model opened as a linear model.
- **Generalized linear model:** binomial (logit, probit or complementary
  log-log link), Poisson (log or identity), Gaussian (identity or log) and
  Gamma (inverse, log or identity; Pearson dispersion, t tests) families;
  categorical predictors; the response is chosen first and suggests a
  distribution (binomial, Poisson or normal) from its values; an **Event**
  choice sets which of a binomial response's two values is modelled
  (`event` in scripts and projects); two-factor interactions, spline terms,
  comparisons within slices and slopes by group as in the linear model (Wald
  F slice tests, differences on the link scale); likelihood-ratio test per term; fitted curve or
  observed-vs-fitted plot; profilers. Save predicted values (the fitted mean),
  or the prediction formula as a formula column (the inverse link of the
  linear predictor: `logistic(…)`, `normal_cdf(…)`, `exp(…)`, …).
- **Ordinal and nominal logistic regression:** a response with several
  categories, any column. Ordinal: the proportional-odds (cumulative logit)
  model with the categories in an order set in the dialog, thresholds and
  common coefficients, and the score test of parallel lines. Nominal:
  baseline-category logits against a chosen reference category. Effects,
  interactions and heredity as in the linear model; Effect Pareto and
  likelihood-ratio test per effect; Wald estimates and odds ratios with
  intervals; −2 log-likelihood, whole-model test, AIC, BIC, McFadden and
  Nagelkerke pseudo-R²; classification table; predicted probabilities against
  a continuous predictor. Save each category's probability and the most
  likely category as formula columns.
- **Dose–response (probit) analysis:** proportion responding (counts out of
  subjects, or 0/1 rows) against log₁₀, ln or linear dose with a probit, logit
  or complementary log-log line; effective doses (ED50 and any other
  percentages) with Fieller fiducial limits and delta-method limits; Pearson
  χ² test of fit and Finney's heterogeneity factor (with t) when it is
  significant; observed and expected responses; curve with the effective doses
  marked. Zero-dose controls are left out on a log scale.
- **Treatment effects from observational data** (Model ▸ Treatment effects
  (observational data)…): a two-level treatment's effect on a continuous,
  binary (0/1) or count outcome adjusted for covariates through a logistic
  propensity-score model. Inverse-probability weighting (normalized;
  stabilized weights optional), augmented inverse-probability weighting
  (doubly robust, with normal, logistic or Poisson outcome models in each
  group), or nearest-neighbour matching on the logit of the score (1:k, with
  or without replacement, caliper in SDs of the logit); ATE or ATT; trimming
  of extreme scores. Mean, risk or rate difference, and risk, rate and odds
  ratios, with sandwich intervals that allow for the estimated scores (robust
  clustered intervals for matched data) or stratified bootstrap percentile
  intervals. Covariate balance table (standardized mean differences and
  variance ratios before and after adjustment, every level of a categorical
  covariate) and balance dot plot with the 0.1 threshold; brushable
  propensity-score overlap plot; outcome means, effective sample sizes;
  subgroup estimates (each subgroup refitted) with a forest plot and
  Cochran's Q test of equal effects. Save propensity scores, weights and
  matched sets. Sample: Cardiac rehabilitation (invented).
- **Nonlinear regression:** least squares for a curve from the library —
  exponential growth or decay, exponential with an asymptote, logistic with 3
  or 4 parameters, Gompertz, Michaelis–Menten, power, rational (linear over
  linear), straight line, four-parameter dose–response on log dose or dose,
  five-parameter asymmetric logistic, Weibull and beta growth, segmented line
  with one breakpoint, sum of two exponentials, Gaussian and Lorentzian peaks
  — with automatic starting values and exact derivatives, for a
  differential equation dy/dx = g(x, y; θ) with the starting value as a
  parameter (adaptive Dormand–Prince 5(4)), for an implicit curve
  F(x, y; θ) = 0 by orthogonal distance, or for any
  model formula with named parameters (written like a formula column, columns
  and parameters as variables; derivatives by central differences). Starting
  values, lower and upper bounds per parameter (equal bounds fix a parameter
  at a known value: it is not estimated and uses no residual degree of
  freedom), iteration limit; a formula in two columns is a fitted surface
  (contours, Response Explorer and 3-D surface profiler). Global fits of
  several data sets with each parameter shared or fitted per data set;
  weights 1/Y², 1/Y or a weight column; robust (Cauchy-loss) fitting, or
  least squares after removing ROUT outliers at a chosen Q; model comparison
  by the extra sum-of-squares F test and AICc (does a parameter differ
  between data sets, one curve for all, or another library curve); inverse
  prediction of x for rows with a response but no x, with intervals
  (standard curves); Hougaard skewness of each estimate;
  Levenberg–Marquardt with Gauss–Newton steps and Bates & Watts' relative
  offset convergence criterion. Report: fit summary (SSE, residual standard
  error, df, iterations, convergence), warnings (no convergence, a parameter
  on a bound, an open profile limit), estimates with asymptotic standard
  errors, t ratios, Wald intervals and profile t (likelihood-ratio)
  intervals, correlation of estimates, fitted curve with the confidence band
  for the mean and prediction limits (delta method; observed by predicted
  when the formula reads several columns), residuals by predicted, and the
  fitted model as a Prediction Formula. Save predicted values, residuals,
  inverse predictions, distances to an implicit curve, or the prediction
  formula as a formula column.
- **Curve templates** (Model ▸ Curve templates): 37 ready-made assay
  models in five experiment types, fitted as nonlinear regressions with
  starting values computed from the data, default bounds, sharing and
  weighting. Immunoassay standard curves (4PL and 5PL, 1/Y² weights):
  back-calculated standards with recovery against ICH M10 limits (±20%,
  ±25% at the lowest and highest standard; at least 75% of standards and
  six levels; all adjustable), precision profile with the lower and upper
  limits of quantitation at a CV limit, and unknown samples read off the
  curve times a dilution factor with intervals and in-range flags. Receptor
  binding: saturation (one site, two sites, total and nonspecific fitted
  together), competition (one and two sites with Ki by Cheng–Prusoff,
  homologous competition), association (one or several ligand
  concentrations), dissociation and competitor (Motulsky–Mahan) kinetics.
  Enzyme kinetics: Michaelis–Menten with kcat, substrate inhibition,
  allosteric (Hill), competitive, uncompetitive, noncompetitive and mixed
  inhibition as global fits with the four mechanisms compared by AICc and
  Akaike weights, Morrison tight binding, and the integrated
  Michaelis–Menten progress curve (Lambert W). Dose–response: agonist (EC50,
  pEC50, ECF) and inhibitor curves, bell-shaped and biphasic curves,
  Gaddum–Schild global fit with pA2 and a Schild plot and slope test, the
  operational model of agonism (τ, KA), and relative potency from parallel
  curves with a parallelism test. Growth, decay and survival: modified
  Gompertz and Baranyi–Roberts growth with doubling times, one- and
  two-phase decay, one-phase association, plateau then decay, linear-quadratic
  cell survival (α/β, SF2, D10), Boltzmann and Arrhenius. Derived quantities
  with intervals (a parameter's profile interval mapped through the
  function, or the delta method), per data set when parameters are not
  shared. Reopens in its dialog from Edit analysis; scripts give a
  `template` member. Samples (Curve fitting): ELISA plate, receptor
  saturation, competition binding, enzyme inhibition, antagonist shift,
  bacterial growth, clonogenic survival and potency assay.
- **Method comparison:** Deming regression with a known error-variance ratio
  (orthogonal at 1) and jackknife intervals; Passing–Bablok regression with
  its cumulative-sum linearity test (an undefined slope limit for negatively
  related methods shows as ∞ with a note); constant and proportional differences read
  from the intervals; scatter plot with the identity and fitted lines;
  difference (Bland–Altman) plot with limits of agreement.
- **Shelf life:** stability data of several batches; ICH Q1E poolability
  tests of slopes and intercepts at 0.25 choosing separate, parallel or pooled
  lines (with two times per batch the slope test is skipped and slopes assumed
  common, with a note); shelf life where the one-sided 95% bound meets a lower or upper limit
  (two-sided bounds with both limits), per batch and overall; the unit of
  time (months by default) sets ICH Q1E's extrapolation limit of twice the
  data and no more than 12 months beyond them, and the report gives the shelf
  life within that limit; plot with the bound, limits and shelf life; note on
  ICH extrapolation limits. Batches can instead be a random sample: random
  intercepts and slopes by REML, the shelf life where the one-sided bound for
  a future batch (or for the mean of all batches) meets the limit, with
  Satterthwaite df, batch-to-batch variances and each batch's predicted line.
- **Mixed model:** linear and generalized linear mixed models — random
  intercepts and correlated or independent slopes for crossed or nested
  grouping columns; normal (REML or ML, Satterthwaite t/F tests), binomial,
  Poisson, Gamma and negative binomial families (Laplace, or adaptive
  Gauss–Hermite quadrature for one random intercept); variance components with
  Satterthwaite χ² limits, random-effect correlations, conditional modes;
  comparisons within slices and slopes by group of the fixed effects
  (Satterthwaite F tests for normal models); save
  population-level predictions, predictions with random effects, or residuals.
  Frequency and weight columns (all families; no weights for binomial) and an
  offset (GLMMs). Normal family: residual variances that differ by a grouping
  column, with a likelihood-ratio test of one variance; between–within
  denominator df beside Satterthwaite; CR2 cluster-robust standard errors on
  the outermost grouping, with Bell–McCaffrey df and HTZ effect tests.
  Ordinal family (cumulative logit; Laplace or adaptive quadrature):
  thresholds, odds ratios, observed and predicted category probabilities;
  save a probability column per category and the most likely category.
- **Bayesian** (Model ▸ Bayesian, also under Compare; `stat-bayes`):
  - *Bayesian linear model or ANOVA:* every model that keeps heredity
    compared by Bayes factors under Zellner–Siow or hyper-g priors for
    continuous effects and g-priors with orthonormal contrasts for factors;
    uniform or beta-binomial model prior; posterior model probabilities,
    effect inclusion probabilities and inclusion Bayes factors with a plot,
    model-averaged and best-model estimates with credible intervals.
  - *Bayesian regression or hierarchical model:* normal, logistic, Poisson
    and negative binomial models with group effects (intercepts and slopes,
    nested groupings), trials, offsets or known SDs (meta-analysis);
    weakly informative autoscaled priors (normal, t or Cauchy coefficients;
    exponential, half-normal, half-t or half-Cauchy SDs); NUTS with step-size
    and mass-matrix adaptation, 4 seeded chains in parallel; posterior
    summary with rank-normalized R̂, bulk and tail ESS and MCSE, group
    effects, sampler diagnostics and plain-language warnings, PSIS-LOO and
    WAIC, coefficient interval plot, trace, rank, autocorrelation and density
    plots of a chosen parameter, posterior predictive check.
  - Samples: Eight schools (Rubin 1981), Response rates by site, Stroop
    reaction times. Scripts: `t.analyze("bayesian-model", #{...})` and the
    other three keys.
  - *Verified:* model Bayes factors against R BayesFactor; R̂/ESS/MCSE
    against the R posterior package; LOO/WAIC against R loo; posteriors
    against long PyMC runs; simulation-based calibration of the sampler.
- **Penalized regression:** lasso, ridge and elastic net with the penalty
  chosen by k-fold cross-validation (λ_min and λ_1SE); coefficient paths;
  coefficients in original units; profilers. Save predicted values or the
  prediction formula (the λ_1SE fit; dropped predictors left out).
- **Decision tree** (Model ▸ Decision tree): classification trees for a
  categorical response and regression trees for a continuous one (Breiman,
  Friedman, Olshen & Stone 1984).
  - *Growing:* Gini or entropy splits (classification), squared-error splits
    (regression); categorical predictors split into two groups of levels
    (exhaustively for up to 10 levels with three or more classes, otherwise by
    ordering levels on the response); minimum node and leaf sizes and a
    maximum depth.
  - *Missing values:* surrogate splits send a row with a missing predictor
    the way its best-agreeing other predictor goes, or the majority way.
  - *Pruning:* weakest-link cost-complexity pruning, with the size chosen on
    validation rows, or by cross-validation when there are none: the
    smallest tree within one standard error of the best (default), the
    lowest held-out error, or no pruning.
  - *Report:* summary, fit measures by role, confusion matrix, a tree diagram
    (each node's rule, count, prediction and class-share bar; click a node to
    select its training rows in every view, shift or ⌘/Ctrl to add; − / +
    hides or shows a branch), variable importance, a leaf table with each
    leaf's full rule, held-out error against tree size, the pruning
    sequence, and the prediction formula. Response Explorer and 3-D profiler.
  - *Save:* predicted values, or each class's probability and the most likely
    class, as formula columns (nested `if` expressions that route missing
    values as the tree does), and the validation roles as a column.
  - Sample: *Loan applications* (a three-class outcome with training,
    validation and test rows from a column, and a regression tree of the
    interest rate sized by 10-fold cross-validation).
- **Random forest** (Model ▸ Random forest): many unpruned trees, each grown
  on a bootstrap sample of the training rows and choosing each split from a
  random subset of predictors (Breiman 2001); classification forests average
  the trees' class probabilities, regression forests their predictions.
  - *Settings:* number of trees, predictors tried per split (default √p for a
    categorical response, p/3 for a continuous one), bootstrap sample size as
    a share of the training rows, minimum node and leaf sizes, maximum depth,
    Gini or entropy, and a seed (trees are grown in parallel and the result
    does not depend on the number of threads). Missing values follow
    surrogate splits.
  - *Report:* summary (out-of-bag error, mean leaves per tree), fit measures
    by role with an out-of-bag row (or cross-validated measures when k-fold
    is chosen: one forest per fold), confusion matrix, variable importance
    (permutation importance on each tree's out-of-bag rows with its standard
    error, and impurity reduction per tree with its share), and out-of-bag
    error against the number of trees. Response Explorer and 3-D profiler.
  - *Save:* predicted values, or each class's probability and the most likely
    class, and the validation roles.
  - Sample: *Loan applications* (a forest of the outcome on the same roles as
    the tree).
- **Boosted trees** (Model ▸ Boosted trees): gradient boosting (Friedman
  2001), a sum of small regression trees each fitted to the gradient of the
  loss at the fit so far and shrunk by a learning rate.
  - *Losses:* squared or absolute error (absolute: leaf values are medians,
    robust to outlying responses) for a continuous response; logistic for two
    classes; multinomial (one tree per class per round) for more.
  - *Trees:* second-order (Newton) split gains and leaf values with an L2
    penalty (Chen & Guestrin 2016); continuous predictors binned at
    training-row quantiles so splits scan histograms; categorical predictors
    split into two groups of levels; missing values go the way each split
    learned for them.
  - *Settings:* greatest number of rounds, learning rate, tree depth, rows
    per leaf, L2 penalty, share of rows per round (stochastic gradient
    boosting), rounds without improvement before stopping, seed.
  - *Rounds kept:* the smallest validation loss (boosting stops once
    validation rows stop improving), or the smallest k-fold cross-validated
    loss, or every round without validation.
  - *Report:* summary, fit measures by role (cross-validated with k-fold),
    confusion matrix, variable importance (total gain, share, number of
    splits), and training and held-out loss by round with the kept round
    marked. Response Explorer and 3-D profiler.
  - *Save:* predicted values, or each class's probability and the most
    likely class, and the validation roles.
  - *Verified:* a one-round stump against hand-computed Newton leaf values
    and gain; the same seed gives the same model.
  - Sample: *Loan applications* (boosted trees of the outcome, stopped on
    the validation rows).
- **Partial least squares** (Model ▸ Partial least squares): regression on a
  few latent factors, weighted sums of the predictors chosen to explain both
  the predictors and the response (Wold; de Jong 1993). Suited to many
  strongly correlated predictors, even more predictors than rows.
  - *Methods:* SIMPLS (default) or NIPALS; predictors centered and optionally
    standardized to unit variance.
  - *Number of factors:* fixed, or the fewest factors whose held-out mean
    squared error is within one standard error of the smallest (on the
    validation rows, or by k-fold cross-validation; 10 folds when every row
    trains).
  - *Categorical response:* PLS discriminant analysis, one indicator per
    class; class probabilities are the fitted indicators clipped to [0, 1]
    and rescaled to sum to one.
  - *Report:* summary, factors table (X and Y variance explained, cumulative,
    held-out PRESS and RMSE, ◀ marks the number used), PRESS by number of
    factors, fit measures by role, confusion matrix, variable importance in
    projection (VIP; above 1 important), coefficients in original units, X
    loadings, and a score plot of the first two factors (points grouped by
    class for PLS-DA, every point a data row for brushing). Response
    Explorer and 3-D profiler.
  - *Save:* predicted values with a prediction formula (continuous
    response), or each class's probability and the most likely class, and
    the validation roles.
  - *Verified:* with every factor, PLS equals least squares (to 10⁻⁹); one
    factor matches its closed form; NIPALS and SIMPLS agree for one
    response; scores are orthogonal; VIP has mean square 1.
  - Sample: *Fuel spectra* (octane from 40 near-infrared absorbances, factors
    chosen by 10-fold cross-validation); *Flower measurements* (PLS-DA of
    the variety).
- **Support vector machine** (Model ▸ Support vector machine): C-support
  vector classification for a categorical response (Cortes & Vapnik 1995)
  or ε-support vector regression for a continuous one, solved by SMO with
  second-order working-set selection (Platt 1998; Fan, Chen & Lin 2005).
  - *Kernels:* radial basis exp(−γ‖u − v‖²) (default), polynomial
    (γuᵀv + 1)ᵈ of degree 2 to 5, or linear.
  - *Inputs:* continuous predictors standardized on the training rows;
    categorical predictors as one indicator per level; rows missing a
    predictor are left out. For regression the response is standardized
    too, so ε is in response standard deviations (default 0.1).
  - *Tuning:* C over 0.25, 1, 4, 16 and 64, and γ over g/16, g/4, g and 4g
    (g = 1 / number of inputs), by the smallest held-out error on the
    validation rows or by k-fold cross-validation (5 folds when every row
    trains); either can be fixed instead.
  - *Several classes:* one machine per pair of classes, the class with the
    most votes predicted. *Probabilities:* Platt's sigmoid fitted to 5-fold
    cross-validated decision values of each machine (Lin, Lin & Weng 2007),
    combined across pairs by pairwise coupling (Wu, Lin & Weng 2004).
  - *Report:* summary (kernel, C, γ, support vectors and how many sit at
    the bound), tuning table with ◀ marking the setting used, held-out
    error by cost with one line per γ (each labelled), fit measures by role
    (cross-validated with k-fold), confusion matrix, and for a linear
    machine with one decision function its weights. Response Explorer and
    3-D profiler.
  - *Save:* predicted values, or each class's probability and the most
    likely class, and the validation roles.
  - *Verified:* the closed-form maximum-margin solutions of small separable
    problems (weights, offset, and support vectors on the margin); ε-SVR
    residuals within ε on exact linear data; Platt scaling and pairwise
    coupling recover known probabilities; seeded fits repeat exactly.
  - Sample: *Pump sensors* (radial and linear kernels of a fault flag set
    outside an elliptical safe zone, compared on the test rows, and an
    ε-regression of efficiency).
- **Multivariate adaptive regression splines** (Model ▸ Adaptive regression
  splines): Friedman's (1991) method for a continuous response, a sum of hinge
  functions max(0, x − t), max(0, t − x) with knots found in the data, and
  their products up to the chosen interaction degree (additive, pairs, or
  three predictors).
  - *Forward pass:* adds the reflected pair (on any parent term, any
    predictor not already in it, any allowed knot) that most reduces the
    residual sum of squares, using Friedman's fast knot update; stops at
    the maximum number of terms, a gain in R² below the threshold (0.001),
    R² of 1 − threshold, or no improvement. Knots respect Friedman's
    minimum span and end span (α = 0.05), or set spans.
  - *Predictors:* continuous ones through hinges, or only linearly when
    listed as linear; categorical ones through indicators of each level but
    the first. Training rows missing a predictor are left out.
  - *Pruning:* backward elimination gives the best subset of each size; the
    size is chosen by GCV with a penalty per knot (2 additive, 3 with
    interactions; settable), by the validation rows, or by k-fold
    cross-validation (each fold repeats both passes).
  - *Report:* summary (terms, stop reason, RSS, R², GCV, generalized R²),
    fit by role, basis functions with coefficients, variable importance
    (number of subsets using each predictor, GCV and RSS reductions), GCV
    (and held-out error) by number of terms with the size kept marked, the
    forward-pass steps. Response Explorer and 3-D profiler.
  - *Save:* predicted values, the prediction formula as a formula column
    (hinges are `max(…, 0.0)` expressions, levels a `switch`), and the
    validation roles.
  - *Verified:* known hinges are recovered exactly; additive fits match the
    R package earth to rounding (see `docs/validation.md`).
  - Sample: *Fuel consumption* (degree-2 and additive models sized on the
    validation rows, boosted trees, and the three compared on the test
    rows).
- **Neural networks** (Model ▸ Neural network): fully connected
  feed-forward networks with one or two hidden layers of tanh, logistic,
  rectified linear or identity nodes, for a continuous response (linear
  output, squared error) or a categorical one (a logistic output for two
  classes, softmax for more; cross-entropy).
  - *Inputs:* continuous predictors standardized on the training rows,
    categorical ones as one indicator per level; a continuous response is
    standardized and predictions returned in its units.
  - *Fitting:* weight decay (α / 2n) Σ w², with α tuned over 10⁻⁴ … 10 on
    the validation rows or by k-fold cross-validation (5 folds when every
    row trains), or fixed; L-BFGS (up to 2,000 rows) or Adam mini-batches
    (above), or either chosen; several seeded random starts (Glorot
    initialization) with the best kept, run in parallel and repeating
    exactly.
  - *Report:* summary (layers, output, penalty and how it was chosen,
    solver and iterations, every start's objective), network diagram
    (layers as columns of marked nodes, inputs named on the axis), penalty
    tuning table and held-out error by log₁₀ α, fit measures by role
    (cross-validated with k-fold), confusion matrix, a heat map of each
    layer's weights with values printed. Response Explorer and 3-D
    profiler.
  - *Save:* predicted values, or each class's probability and the most
    likely class; for a continuous response the prediction formula as a
    formula column (hidden nodes as `tanh`, `logistic` or `max` of their
    weighted sums); the validation roles.
  - *Verified:* objective, gradient and predictions at scikit-learn's
    fitted weights for regression, two-class and three-class networks;
    fits from random starts reach scikit-learn's objective (see
    `docs/validation.md`).
  - Sample: *Polymer batches* (networks of strength and grade tuned on the
    validation rows, boosted trees of strength, and the two strength models
    compared on the test rows).
- **Gaussian processes** (Model ▸ Gaussian process): a continuous response
  as a Gaussian process with a constant mean, Matérn 5/2 (default) or
  squared-exponential covariance and one length scale per input (automatic
  relevance; continuous inputs scaled to 0–1, categorical ones as one
  indicator per level), and a nugget estimated or fixed.
  - *Fitting:* signal variance, length scales and nugget by maximum
    likelihood (BFGS on log parameters within bounds, analytic gradient),
    from several Latin-hypercube starts with the best kept; up to 2,000
    rows.
  - *Report:* summary (kernel, signal and noise SD, log likelihood, every
    start's log likelihood), length scales in fractions of the range and in
    data units with sensitivities, fit measures by role with exact
    leave-one-out predictions (or validation roles, holdout or k-fold),
    actual by predicted, contour maps of the predicted mean and its standard
    error over the first two continuous inputs with the runs marked.
    Response Explorer with confidence bands and 3-D profiler.
  - *Save:* predicted values, standard errors, validation roles.
  - *Verified:* log marginal likelihood, its gradient, fitted
    hyperparameters and predictions against scikit-learn's
    `GaussianProcessRegressor` (see `docs/validation.md`).
  - Sample: *Reaction yield (simulated)*.
- **Model search** (Model ▸ Model search): fits every predictive model that
  suits the response (decision tree, random forest, boosted trees, neural
  network, support vector machine, adaptive regression splines for a
  continuous response, partial least squares, discriminant analysis for a
  categorical one), each with its defaults and the same roles, and ranks
  them by held-out error.
  - *Ranking:* RMSE, or log loss (ties by misclassification) for classes, on
    the validation rows or by k-fold cross-validation (5 folds when every
    row trains); test rows give an honest estimate. Models are fitted in
    parallel and seeded, so a search repeats exactly; a model that cannot be
    fitted is listed with the reason.
  - *Report:* summary (roles, measure, best model with held-out and test
    error, models not fitted or not applicable), ranking table (held-out
    and test measures, training error, ◀ best), held-out error by model.
  - *Repeated and nested cross-validation* (More options): k-fold
    cross-validation repeated with fresh folds (seeds seed, seed + 1, …; up
    to 50 repeats), each model's mean error with a 95% t interval, best and
    worst repeat, and a forest plot; *nested* holds each outer fold out while
    the model tunes itself on the other rows only.
    A new search opens the best model's full report below it; ⋯ Options
    opens any ranked model's report.
  - *Verified:* on simulated data with a known model class, the true class
    wins (hinge sums by adaptive regression splines, a tanh boundary by the
    neural network); ranked rows match each model's own report.
  - Sample: *Polymer batches* (a search of tensile strength).
- **Model comparison** (Model ▸ Compare models): compares models through the
  prediction columns they saved, on the rows where every model and the
  response are present.
  - *Measures* (by validation role when a column of roles is given; ◀ marks
    the best model in each): RMSE, MAE and R² for a continuous response;
    for a categorical one, each model's probability of one event class
    (default the last class) against the rest: misclassification at 0.5,
    log loss, Brier score and AUC.
  - *Plots* (on test rows when there are any, else validation rows, else all
    rows; each model a colour and marker shape, every point a data row so
    brushing links them): ROC curves with the chance diagonal, lift and
    cumulative gains by depth, and a calibration plot of observed event rate
    against mean predicted probability in tenths; actual by predicted and
    residual by predicted for a continuous response. Every plot exports as
    PNG, SVG, PDF and EPS.
  - Sample: *Loan applications* (the tree's, the forest's and the boosted
    trees' saved default probabilities compared on the test rows); *Pump
    sensors* (radial against linear support vector machines); *Fuel
    consumption* (two adaptive spline models against boosted trees); *Polymer batches*
    (a neural network against boosted trees).
- **Validation and fit measures** (predictive models): each row trains,
  validates (chooses the model's size) or tests (kept out of both). Roles
  come from a column (0/1/2 or Training/Validation/Test), from a seeded random
  draw (stratified by the response's classes or quartiles, or by whole groups
  of a column so related rows stay together), or k-fold cross-validation is
  used instead. Fit measures per role: RMSE, MAE and R² (continuous);
  misclassification rate, log loss, AUC (one class against the rest, averaged
  for three or more classes) and the confusion matrix (categorical).
  - *Verified:* AUC against direct counting of positive–negative pairs and
    the trapezoid area under the ROC curve; the pruning sequence of a small
    tree against hand computation; saved formulas reproduce every row's
    prediction, missing values included; forests reproduce exactly from
    their seed on any number of threads, and out-of-bag predictions average
    exactly the trees that left the row out.
- **Several responses** (Model ▸ Several responses): the same model (main
  effects, with interactions, or quadratic) fitted to each of several
  responses on shared factors, explored together.
  - *Terms selected stepwise for each response:* instead of every term, a
    separate stepwise search per response (forward, backward or mixed; by
    p-values, AIC or BIC; heredity rules) among the model form's terms
    (interactions and squares included) or candidates chosen in the
    stepwise dialog. Report: Term Selection (each response's selected
    effects, steps and criterion), the candidates, and a step history per
    response; a response with no term selected is predicted by its mean
    (t interval). The explorer, goals, best settings and simulation use the
    selected models.
  - *Linked Response Explorer:* one row of slice plots per response, plus a
    row for the overall desirability; moving a factor in any row moves it in
    all of them. Right-click saves every row as one figure.
  - *Goals (desirability, Derringer & Suich 1980):* per response maximize,
    minimize, hit a target or ignore, with lower and upper limits, shape
    exponents (separate above a target) and an importance; the overall
    desirability is the importance-weighted geometric mean, shown live with
    each response's prediction and desirability.
  - *Find best settings:* maximizes the overall desirability within the
    factor ranges by Nelder–Mead from a Latin hypercube of starts, every
    combination of categorical levels searched; the top five distinct
    solutions in a table, each with a "Set to" button.
  - *Simulation:* each continuous factor fixed or varying (normal, uniform
    or triangular around its setting), normal noise on each response (or the
    model's RMSE), optional specification limits; simulated runs at the
    current settings give each response's mean, SD and defect rate, and the
    share of runs with any response out of specification, with 95% Wilson
    intervals and a histogram per response. It re-runs as settings change.
    *Find robust settings* moves the continuous factors to the lowest
    simulated defect rate (common random numbers); the simulated runs can be
    saved as a new table.
  - *Constraints on the factors:* linear limits such as
    `Temperature + 2*Time <= 230` and excluded combinations, in the factors'
    units (the design dialogs' syntax); best and robust settings keep to
    them, and the best-settings title counts them.
  - *Specification region:* a low and high per continuous factor (a box of
    operating settings, by default the current settings ± 5% of each range).
    At every corner (up to 8 varying factors, 256 corners) the probability
    that every response is within its limits, from the normal noise
    (∏ [Φ((U − ŷ)/σ) − Φ((L − ŷ)/σ)]) and from simulation with common random
    numbers and a Wilson interval; the worst corner is marked (Peterson 2004;
    del Castillo 2007). A contour plot of each specification limit over
    the first two varying factors shows the box with its corners, the worst
    marked.
  - Options menu opens each response's full linear model report.
  - Sample: *Coating formulation* (three factors, three responses, a
    central composite design, with a constraint and a specification region;
    a second report selects each response's terms stepwise).
  - *Verified:* desirability functions against hand-computed values;
    simulated defect rates against the analytic normal tail
    probabilities; uniform and triangular factor spreads against their
    theoretical standard deviations; region corner probabilities against
    normal tails and simulation; constrained optima of known problems.
- **Response Explorer** (all models): one slice plot per factor with its
  confidence band; drag a factor's reference line and every panel updates.
  One grip under the panels resizes them all together.
- **3-D surface profiler:** rotatable fitted surface over two factors, with
  observed points.
- *Verified:* NIST StRD linear regression and ANOVA suites (log relative error
  ≥ 10, documented representation-limit exceptions); mixed models against an
  independent dense reference implementation and the Dyestuff data.

## Multivariate

Menu groups: Relationships, Reduce dimensions, Clusters, Groups, Text.

- **Correlations:** Pearson matrix with pairwise-complete or listwise rows;
  every pair's r, n, Fisher-z confidence interval and t-test p-value;
  heat map with the value printed in each cell (diverging ramp, so colour is
  never the only cue); optional Spearman's ρ with p-values, partial
  correlations (each pair given all other columns, listwise) and the
  covariance matrix.
- **Scatterplot matrix** (also in the Visualize menu): every pair of chosen continuous columns as a grid
  of linked graph-canvas panels, density ellipses off the diagonal and
  histograms on it; colour and marker by a grouping column; opens in the
  graph canvas, so layers can be changed and brushing is shared.
- **Outlier distances:** Mahalanobis, jackknifed Mahalanobis and Hotelling T²
  per row, with beta/F reference limits at a chosen α; distance-by-row plot
  with box selection and click brushing; table of rows above the limit; mean
  and covariance; save any of the three distances as columns.
- **Item analysis:** Cronbach's alpha (raw and standardized) with a Feldt
  confidence interval; per item mean, SD, item–total and item–rest
  correlations, alpha if deleted; reverse-scored items (low + high − x, with
  the scale's ends given or taken from the data); inter-item correlation heat
  map; save the total score.
- **Principal components:** standardized (correlation) or covariance based; eigenvalues, scree
  plot, loadings, score plot with variable loading lines; save PC scores.
- **Clustering:** k-means and hierarchical (single, complete, average, Ward)
  with dendrogram; cluster means and plots; save cluster numbers. k-means
  reports each cluster's average silhouette width and can compare numbers
  of clusters: the gap statistic (reference sets drawn over the data range
  or the principal-component box, seeded, Tibshirani's 1-SE rule) and the
  average silhouette for k = 1 … K, with plots and both suggested k.
  Starting centers for k-means typed in or read from an open table (one row
  per center), alone or beside the seeded starts, with a Starting Centers
  table saying which start won.
  *Verified:* R `cluster::silhouette` and `cluster::clusGap` (gap from
  given reference sets); R `kmeans(centers =, algorithm = "Lloyd")`.
  Hierarchical clustering can also use any distance of Data ▸ Reshape ▸
  Distance matrix (Gower takes text and ordinal columns and missing values)
  or a distance matrix stored as columns; the cluster plot then maps the
  rows by their first two principal coordinates.
  *Verified:* R `hclust` and SciPy `linkage` on Gower distances.
- **Correspondence analysis:** inertia table and symmetric map of a two-way
  table.
- **Multivariate analysis of variance (MANOVA):** two or more continuous
  responses on a linear model's effects (continuous and categorical
  predictors, interactions and powers, hierarchy kept as in the linear
  model dialog); per effect Wilks' Λ, Pillai's trace, Hotelling–Lawley trace
  and Roy's largest root with F approximations (exact, approximate or upper
  bound, labelled); Effect Pareto by a chosen statistic; univariate F-tests of
  each response; contrasts among a factor's levels (least-squares means,
  weights adding to zero) with estimates; canonical analysis of each effect
  (eigenvalues of E⁻¹H, canonical correlations, standardized coefficients);
  canonical plot of the rows for a categorical factor, marked by level with
  level means, linked to the data; residual (partial) correlations; Box's M
  test of equal covariance matrices when every predictor is categorical.
- **Factor analysis:** maximum-likelihood (with Heywood cases held at a
  bound and flagged) or principal-axis (iterated communalities; Heywood
  cases held at communality = variance, uniqueness 0) extraction
  from the correlation or covariance matrix; number of factors from the
  eigenvalues above 1 or set; varimax (Kaiser normalization) or promax
  (power 4, adjustable) rotation, with pattern and structure loadings and the
  factor correlations of an oblique solution; eigenvalue table and scree
  plot, unrotated and rotated loadings, communalities and uniquenesses,
  variance explained, chi-square test that the factors suffice (Bartlett
  correction), loading plot of factor 1 by factor 2; save regression or
  Bartlett factor scores.
- **Discriminant analysis:** linear (pooled covariance) or quadratic
  (per-class covariance) normal-theory classification with proportional or
  equal priors; validation roles of the predictive models (training,
  validation and test rows, or k-fold) with fit measures and confusion
  matrices by role and a list of misclassified rows; Fisher's canonical
  discriminant functions (eigenvalues, canonical correlations, Wilks' Λ
  tests, standardized and raw coefficients, structure correlations) and a
  canonical plot of the rows by class (colour and marker) with the class
  means; save posterior probabilities, the most likely class (usable in
  Compare models), the validation roles and canonical scores. Also listed
  under Model with the other classifiers.
- **Gaussian mixture clustering:** EM with six covariance structures
  (spherical, diagonal or full; equal or varying across components: EII,
  VII, EEI, VVI, EEE, VVV), several seeded starts (k-means or Ward
  hierarchical first start), optionally on standardized columns; BIC table
  and BIC-by-components plot (one line and marker shape per structure) for a
  chosen range of components, the smallest BIC chosen; log-likelihood, BIC,
  mixing proportions, means and covariance matrices in the columns' units; rows on the first two
  columns by most likely component with each component's 95% ellipse,
  brushable; save membership probabilities, most likely component and
  classification uncertainty.
- **Latent class analysis:** hidden classes behind several nominal or
  ordinal items (any number of levels each) under local independence,
  fitted by EM from several seeded random starts for each number of classes
  in a range, the smallest BIC chosen; blank answers skipped in the
  likelihood; optional Dirichlet smoothing. Model-selection table
  (log-likelihood, parameters, AIC, BIC, sample-size-adjusted BIC, entropy
  R², G², starts reaching the best fit) and an information-criteria plot;
  chosen model's fit with G² and Pearson X² over the response patterns;
  class sizes and item-response probabilities with standard errors
  (observed information, delta method); response-probability profile plot
  (one marker and stem per class at each level, a pane per item); average
  posterior probabilities by most likely class; save the most likely class
  and the posterior class probabilities.
- **Dimension reduction:** classical (Torgerson) and metric (SMACOF)
  multidimensional scaling, t-SNE (exact gradient up to 600 rows,
  Barnes–Hut beyond; perplexity, early exaggeration) and UMAP (neighbours,
  minimum distance; spectral start); from Euclidean distances between rows
  of chosen columns (optionally standardized) or from a distance matrix
  stored as columns; 1–3 dimensions, seeded and reproducible; fit summary
  with stress-1, KL divergence or eigenvalues and the trustworthiness of the
  map; map of the rows, brushable, coloured and marked by an optional
  column; Shepard diagram for the scaling methods; save the coordinates as
  columns.
- **Principal coordinates** (Reduce dimensions): classical scaling of any
  distance between rows (Euclidean, Manhattan, Gower, Jaccard, Bray–Curtis,
  Hamming, simple matching; numeric columns optionally standardized) or of a
  distance matrix stored as columns; every eigenvalue including negative
  ones, with relative and cumulative shares and the broken-stick
  expectation; Lingoes or Cailliez correction for negative eigenvalues;
  ordination plot of the first two axes, brushable, coloured and marked by
  an optional column; correlations of the numeric columns with the axes;
  save up to three coordinates. Samples: Shrub traits, Town distances.
  *Verified:* R `cmdscale` and `ape::pcoa`.
- **Text exploration** (Text group): each non-empty cell of a text column
  is a document (excluded rows left out). Words split at Unicode word
  boundaries, lower-cased, English stop words and extra words of your own
  left out, optional Porter stems (labelled by their commonest word), a
  shortest length, numbers dropped or kept, terms kept by the number of
  documents they appear in and a cap on the vocabulary. Summary of words,
  terms and settings; term table (count, documents, % of documents, weight
  total); word-frequency bar chart whose bars select the documents
  containing each term; word cloud (type area proportional to the count,
  spiral packing, no overlaps; click a word to select its documents, words
  of selected documents underlined; exported like any plot); phrases of 2–6
  words that do not cross punctuation or start or end with a stop word;
  semantic dimensions by latent semantic analysis (truncated SVD of the
  count, binary, log, TF-IDF or log TF-IDF matrix with unit-length rows;
  singular values and shares, highest and lowest loading terms, a document
  map with the most prominent terms); document clusters by k-means on the
  documents' directions, with sizes and characteristic terms, shown on the
  map by colour and marker; save the semantic dimensions and the cluster.
- **Topic model** (Text group): latent Dirichlet allocation fitted by
  collapsed Gibbs sampling (2–50 topics; α = 50 ÷ topics by default, β =
  0.1; sweeps, burn-in and seed), estimates averaged over samples after the
  burn-in; summary with log-likelihood and perplexity; top terms of each
  topic with their probabilities and share of terms; documents by most
  likely topic (bars select the documents); the sampler's log-likelihood by
  sweep; the most representative documents of each topic; save each
  document's topic proportions and its most likely topic.
- **Sentiment scores** (Text group): a score from −1 to 1 for each document
  from the bundled sentiment lexicon (about 7,500 words and emoticons rated
  −4 to +4; Hutto & Gilbert's VADER lexicon, MIT licence), sentiment words
  within three words after a negation reversed and damped, entries of your
  own added or replacing the lexicon's; summary with positive, neutral and
  negative shares (neutral band adjustable); score histogram (bars select
  documents); the words driving the scores with their total contribution;
  the most positive and most negative documents; save the score, the sum of
  valences, the category and the counts of positive and negative words.
- **Functional data** (Curves heading): curves in long form (curve
  identifier, time, value; uneven times and missing readings allowed)
  smoothed by B-splines (order 2–6) or Fourier series, with a
  roughness penalty (second derivative, or harmonic acceleration for
  Fourier) chosen by GCV, fixed, or none (least squares in the basis);
  landmark registration on each curve's maximum or minimum in a window
  (monotone piecewise-cubic warps) and continuous registration to the mean
  curve (Ramsay–Li minimum-eigenvalue criterion, smooth monotone warps);
  functional principal components (eigenvalues, eigenfunctions, scores,
  effect plots of the mean ± 2 SD of each score, reconstruction and its
  error per curve); component scores modelled on curve-level factors
  (multivariate regression with Wilks' Λ, p per component, Effect Pareto)
  with a Curve Explorer and 3-D profiler of the predicted curve and its
  confidence limits as the factors change; save scores, smoothed values
  and registered times. Sample: Exotherm profiles.

## Design

- **Factorial:** 2-level and 3-level full factorials; full factorials with a
  different number of levels per factor (mixed levels, evenly spaced from
  each factor's lowest to highest setting); fractional factorials with
  resolution shown.
- **Screening:** Plackett–Burman; definitive screening designs.
  *Definitive screening options:* extra runs (added in pairs from a larger
  conference matrix whose extra columns are left out; Jones & Nachtsheim
  2013) and the total number of center runs (default 1). *Compare run
  sizes* tabulates nearby choices: runs, error degrees of freedom after the
  main effects and after the main and quadratic effects, main-effect
  D-efficiency, the largest |r| among quadratic effects and between
  two-factor interactions, and the mean standard error of a quadratic
  effect (σ units). Script: `definitive_screening_design(#{ … })`,
  `compare_definitive_screening(#{ … })`.
- **Screening with categorical factors** (Design ▸ Screening): two-level
  continuous factors together with categorical factors of 2 to 12 levels,
  for a main-effects (or main effects and two-factor interactions) model,
  built by coordinate exchange maximizing det(XᵀX) with orthonormal
  (normalized Helmert) contrasts, so a design that balances every pair of
  factors (an orthogonal array) scores 100% D-efficiency. Default runs:
  terms + 4, rounded up to a multiple of the level counts' least common
  multiple when that costs at most half again. Categorical factors come out
  as nominal text columns L1, L2, …; the notes give each level's run
  count, the D-efficiency and ln det. Script: `screening_design(#{ … })`.
- **Response surface:** central composite (rotatable, spherical,
  face-centered or custom α) and Box–Behnken. A rotatable design's default
  center runs give uniform precision (Box & Hunter 1957) for its factorial
  portion, computed for every portion up to 11 factors.
- **Optimal:** D- and I-optimal designs by coordinate exchange.
  *Constraints:* linear inequality constraints on the factors and
  disallowed combinations (`exclude Temp > 180 and Time > 25`), typed in the
  factors' units one per line; every run satisfies them, they are listed in
  the design's notes, and an I-optimal design's average prediction variance
  is taken over the constrained region (estimated from 20 000 quasi-random
  points). The same constraints limit the best settings of a several-
  responses report.
- **Split-plot designs** (Design ▸ Optimal ▸ Split-plot (hard-to-change
  factors)…, or the D-optimal dialog's per-factor **Changes** choice: easy,
  hard, very hard): D-optimal split-plot and split-split-plot designs by
  candidate-set-free coordinate exchange (Goos & Vandebroek 2003; Jones &
  Goos 2007, 2009), maximizing det(XᵀV⁻¹X) for the given numbers of whole
  plots and subplots and a prior variance ratio η (whole plot ÷ run, default
  1). Hard factors stay fixed within whole plots (very hard within whole
  plots and hard within subplots); the table gets nominal Whole Plot (and
  Subplot) columns and a run order randomized within those restrictions;
  the notes give the D-efficiency, ln det(XᵀV⁻¹X), the average and largest
  prediction variance and the whole-plot error degrees of freedom. The new
  table opens with the design declared (whole-plot columns and model
  recorded), Model ▸ Mixed model filled in for it (the design's model with
  random intercepts for the plots, REML, Satterthwaite F tests, so
  whole-plot factors are tested against whole-plot error), and a project
  script "Fit split-plot model". Script: `split_plot_design(#{ … })`.
- **Space filling** (computer experiments): maximin Latin hypercube
  (φ₅₀ criterion minimized by simulated annealing; Morris & Mitchell 1995),
  maximin distance (point exchange over a dense candidate set, then
  continuous refinement), minimax distance (clustering; Mak & Joseph 2018)
  and uniform designs (centered L2 discrepancy minimized by threshold
  accepting; Fang et al. 2000). Runs and seed; factors keep their low/high
  ranges; no center points; the preview gives each design's minimum
  distance, φ₅₀, centered L2 discrepancy and fill distance. Sample: borehole
  water flow (Worley 1987) on a 40-run maximin Latin hypercube with a
  surrogate linear model.
- **Mixture:** simplex lattice and simplex centroid; extreme-vertices
  designs for a region with lower and upper bounds per component (McLean &
  Anderson 1966), with optional edge and face centroids, the overall
  centroid and axial check blends.
- **Robust design (orthogonal arrays):** control factors in a standard
  orthogonal array (L4, L8, L9, L12, L16, L18, L27) crossed with noise
  factors in an outer array, or repeated runs; the layout opens as a table
  with one response column per outer-array run.
- **Signal-to-noise analysis** (with a table open): smaller-the-better,
  larger-the-better and nominal-the-best (mean²/variance or variance only)
  ratios per run; response tables of the ratio and of the mean with deltas
  and ranks, main-effects plots, the best setting of each factor and the
  additive-model prediction there (Phadke 1989). *Options:* ratio edited in
  place.
- **Declare table as design** (with a table open): choose existing columns
  as factors (numeric with optional low/high settings, or categorical with
  two levels coded −1/+1) and numeric columns as responses;
  reports levels and out-of-range runs per factor, distinct and repeated
  settings (pure-error df), correlations among the coded factors, whether
  the design is orthogonal, and which models (main effects, two-factor
  interactions, quadratic) it can estimate, with error df and D-efficiency.
- **Augment design** (with a table open): add runs to the design in the
  active table, keeping the runs made — full fold-over or fold-over on one
  factor (two-level text factors swap levels), center runs, axial runs at
  ±α with center runs (a factorial becomes a central composite), or
  D-optimal runs for a (larger) model with the existing runs fixed, within
  optional constraints. Notes give the aliases a fold-over removes and the
  D-efficiency before and after; the result opens as a new table with the
  new runs in actual units (in random order with a seed) and a nominal
  Block column (1 original, 2 added).
  *New whole plots* (split-plot designs): choose the whole-plot column, how
  many whole plots to add, the runs in each and η; factors constant within
  every existing whole plot are the hard-to-change ones and stay fixed
  within each new whole plot, and the new settings maximize det(XᵀV⁻¹X) of
  all runs (Goos & Jones 2011, ch. 10). The notes give the D-efficiency
  before and after and the whole-plot error degrees of freedom; the new
  whole plots are numbered after the existing ones and run in random order,
  runs randomized within each. Script: `t.augment_whole_plots(#{ … })`.
- **Simulate responses** (with a table open): made-up responses for the
  design in the active table from random true coefficients under effect
  sparsity, hierarchy and heredity (Chipman, Hamada & Wu 1997): main effects
  active with probability p; a higher-order term with p₁₁, p₀₁ or p₀₀ when
  all, some or none of its parent factors are active (presets Strong, Weak,
  None); active coefficients N(0, τ²) in coded units, inactive ones zero
  (or small), normal noise. Up to 1000 responses at once; they are added to
  the table as columns in one undoable edit, and a table of the true
  coefficients (with each response's active terms) joins the project.
  Script: `t.simulate_responses(#{ … })`.
- **Evaluate design** (with a table open), before any responses are
  collected: for chosen factor columns and a model (main effects,
  interactions, full quadratic or a typed formula), the power of the
  two-sided test of each term for an anticipated coefficient in units of σ
  (common or per term) at a chosen α, with relative standard errors and
  variance inflation factors; the relative prediction variance profile along
  each factor, its largest and average value over the cube, and the
  fraction-of-design-space curve with quantiles (Zahran, Anderson-Cook &
  Myers 2003); the alias matrix for terms left out (default: the two-factor
  interactions not in the model, or the three-factor ones; or typed); a
  heat map of absolute correlations among model and alias terms; D, A, G
  efficiencies and the I criterion. A column with two levels splits the
  rows into two designs compared side by side, with relative D, A, G and I
  efficiencies, overall and per run. *Region constraints* (linear limits
  and disallowed combinations in actual units, as for optimal designs) make
  the largest and average variance and the fraction of design space refer
  to the constrained region, as the report states; a D- or I-optimal
  design created with constraints opens with this evaluation set up.
  *Options:* model, alias terms, coefficient, α, constraints and factor
  settings edited in place.
- **Factorial analysis** (with a table open; Design ▸ This table): two-level
  factor columns (numeric with optional low/high settings, or categorical
  with two levels) and one response; model of every estimable interaction
  (aliased terms of a fraction left out and listed), interactions up to an
  order, or a formula. Effects (2 × coded coefficient) fitted at the coded
  settings actually run, with a note when runs missed their targets;
  t tests when there are error df, otherwise Lenth's (1989) pseudo standard
  error with margin of error and simultaneous margin of error; Effect
  Pareto; normal and half-normal plots of effects (Daniel 1959) with term
  labels, the line of inert effects and ME/SME marks, active effects by
  marker shape and colour; main-effects plot (one pane per factor),
  interaction plots of each pair (one line per level of the second factor)
  and a cube (or square) plot of corner means, of the data or the fitted
  model. Dispersion effects: ln s per setting with its own effects and
  normal plot for replicated designs (Box & Meyer 1986); Bergman–Hynén
  (1997) F tests and Box–Meyer ln ratios for unreplicated ones. Binary
  (logistic) and count (Poisson) responses through the GLM, effects on the
  link scale with Wald z tests, odds or rate ratios; a saturated Poisson
  model is fitted exactly and judged by Lenth's method too. *Options:*
  terms, response type, α and data means or model predictions edited in
  place. Samples: Filtration rate (Montgomery), Coating thickness.
- **Propose next runs** (with a table open; Design ▸ This table):
  sequential optimization of an experiment from the runs made so far. One
  Gaussian process per response (Matérn 5/2 or squared exponential, nugget
  estimated or fixed) is fitted to the runs with every response measured;
  rows with the factors set but a response missing are runs in progress,
  which the proposals avoid.
  - *Goals:* maximize, minimize or match a target; several responses
    through the overall desirability (limits from the data or set, with
    importances).
  - *Factors:* continuous between the data's range or set bounds,
    categorical by level; linear constraints and disallowed combinations in
    the factors' units, as for optimal designs; an optional round column.
  - *Criterion:* expected improvement (closed form; for a target, the
    improvement in distance from it; for several responses, Monte Carlo
    expected improvement in desirability with common draws) or an upper
    confidence bound with κ standard errors. Batches by the constant-liar
    heuristic; candidates from a Latin hypercube with rejection, refined by
    bounded Nelder–Mead and a sweep of each categorical factor's levels.
  - *Report:* summary, each response's model (length scales, noise SD,
    leave-one-out RMSE), the best run so far, the proposed runs with
    predictions, standard errors and criterion values, a progress chart of
    each run and the best so far by round (or run), and a stop rule (the
    best run reaches a set value, or the largest expected improvement falls
    below a fraction of the observed range).
  - *Options:* batch size, criterion and stop rule edited in place; *Add
    proposed runs to the table* appends them as new rows with the next
    round number (undoable).
  - *Verified:* expected improvement against numerical integration; the
    Branin and Hartmann 3 test functions reach their optima (see
    `docs/validation.md`).
  - Sample: *Reaction yield (simulated)* (yield and impurity as formula
    columns standing in for the reaction, under a heat-budget constraint;
    round 2 follows the first report's proposals).
- **Gauge study worksheet:** a randomized run sheet for a crossed or nested
  gauge study (parts × operators × replicates; standard order, fully random
  or random within each replicate, with a seed) opened as a new table with
  an empty measurement column. Also under Quality.
- Live preview as settings change; center points; randomized run order with a
  seed; the design opens as a new data table ready for responses.

## Quality

Menu groups: Control charts, Process capability, Measurement systems,
Acceptance sampling, Problem-solving tools.

- **Control charts:** X̄ and R, X̄ and S, individuals and moving range, built
  from column roles chosen in the dialog (or dragged onto it from the variables list): measurement,
  subgroup ID (subgroups of varying size, with limits that step with the
  size), phase (each phase gets its own center line and limits, run rules
  restart, and the chart separates phases with a solid rule, shades every
  other phase and names each in a label along the top, on screen and in
  exports) and point label (shown
  on hover and in the flagged-point table). Eight run rules (Nelson) with
  flagged points labelled by rule number; excluded rows leave the limits.
  *Options:* chart type and subgroup size switched in place, the run rules
  applied to each chart, dispersion chart, 1σ/2σ zone lines, table of
  flagged points, a run chart of every value (subgroup medians or means
  joined, center at the median or mean), and "Save limits as data table"
  (center, limits, process center and σ per phase). Individuals, X̄–R and
  X̄–S charts can put the limits at kσ, take moving ranges over w values
  (σ̂ = M̄R/d₂(w)), and set the limits from standard (historical) values
  typed in or read from a saved limits table; any chart can show only its
  last N subgroups, with limits from all of them or from those N.
  - **Moving-average chart:** averages of the last *w* points (span set in
    the dialog or options, default 5) of individual values or subgroup
    means, with limits that widen over the first points of each phase and
    follow varying subgroup sizes; rule 1 only (the averages overlap).
  - **Three-way (between/within) charts:** subgroup means charted as
    individual values against their own moving range, the moving-range
    chart of the means, and a within-subgroup range or SD chart, for
    processes whose subgroup means vary more than the within-subgroup
    spread allows (Wheeler 1995).
  - **Zone chart:** each point scored 1, 2, 4 or 8 by its σ zone, scores
    added while points stay on one side of the center line, a signal at a
    cumulative score of 8 (Jaehn 1987); the score is printed above each
    point and listed with the flagged points.
  - **Short-run Z and moving-range chart:** a part-number column (any
    type); each value standardized by its own part's mean and σ (from the
    moving ranges of consecutive values of that part, per phase), so
    several part numbers share one chart; a Part Estimates table
    (Griffith 1996).
- **Count and rare-event charts:** p, np, c and u charts; Laney p′ and u′
  charts, whose limits are widened by the between-subgroup variation σ_z of
  the standardized values for large overdispersed subgroups (Laney 2002); g
  chart of the units between rare events (geometric, ḡ ± 3√(ḡ(ḡ+1));
  Benneyan 2001); t chart of the times between events, charted on the scale
  t^(1/3.6) and drawn on the original scale (Nelson 1994). u and u′ chart
  sizes are inspection units and may be fractional. An optional phase
  column gives each phase its own limits; run rules can be applied (on the
  standardized or transformed scale). *Options:* chart type and run rules
  switched in place.
- **EWMA and CUSUM charts:** target and σ given or estimated; signals table.
- **Multivariate control charts:** Hotelling T² of individual observations
  or subgroups (by an ID column or consecutive runs), the generalized
  variance |S| chart (Alt 1985) and the MEWMA chart (Lowry et al. 1992) of
  two or more correlated measurements. The first *m* points can be the
  reference sample: its points get beta-distributed (individuals) or F
  limits, and later points get the F limits for new observations. MEWMA
  limit given or found from an in-control average run length (up to 2000). Upper limit
  only; points outside it are drawn as a distinct shape and listed with
  each variable's contribution (T² minus T² without that variable) in a
  table and a chart. Points link to their rows for brushing.
- **Principal-components control chart:** a principal-components model
  (correlations or covariances) of a reference period, with the number of
  components set or chosen by element-wise cross-validation (smallest
  PRESS); T² on the retained scores with scaled-beta and F limits, and the
  squared prediction error (SPE, Q) with the Jackson–Mudholkar or scaled χ²
  limit; points above either limit listed with their largest contributor;
  T² and SPE contribution tables and plots; signalling rows against the
  reference mean in a parallel-coordinates plot; a likelihood-ratio
  change-point estimate of the mean with a permutation p-value; reference
  means and loadings. Brushable. *Options:* reference period, model,
  components and limits changed in place.
- **Environmental monitoring limits:** alert and action levels for colony
  counts (Poisson, negative binomial, zero-inflated Poisson and negative
  binomial), positive samples out of samples taken (binomial,
  beta-binomial) or continuous readings (fourteen families), fitted by
  maximum likelihood with the best family by AIC (or one chosen); levels at
  quantiles (0.975 / 0.9985), at the mean + 2/3 SD, typed in per group or
  read from columns; set for all locations, each location or each level of
  a column (a missing value as its own level); each location charted in time
  order against its levels with numbered early-warning rules (action and
  alert excursions, *n* alerts in *m* readings, a run of increases); summary
  and excursion tables by location; charts for all locations or only those
  with signals. Brushable.
- **Analysis of means:** each group's mean, proportion or Poisson rate
  against decision limits around the overall value (Ott 1967), with exact
  critical values from the equicorrelated multivariate t for equal or
  unequal group sizes (Nelson 1982); groups by a column, or each row its
  own group for counts. Two-way layouts (balanced, means): main-effect
  charts for both factors and an interaction chart when cells are
  replicated. Groups outside the limits are drawn as a distinct shape and
  listed; points link to their rows.
- **Process capability:** Cp, Cpk, Cpl, Cpu (within), Pp, Ppk, Ppl, Ppu
  (overall), Cpm; confidence intervals (within intervals with the degrees of
  freedom of the σ estimator); observed and expected nonconforming
  rates; histogram with specification limits and normal curve. Non-normal
  processes: a fitted distribution (any of the 14 families, or the best by
  AIC), a Johnson transformation or the sample's own percentiles give
  percentile-based Pp, Ppl, Ppu, Ppk (ISO 22514-2 / Clements), an
  equivalent Ppk from the expected nonconforming rate, expected rates under
  the fit, the fitted density over the histogram, a quantile plot, and the
  normal-theory indices for comparison. Or a Box–Cox power first.
  Bootstrap percentile intervals for the percentile-based indices (number
  of resamples and seed chosen; the model is refitted to every resample).
  By group of a column: a table of every group's N, mean, SD, Cp, Cpk, Pp,
  Ppk and % out of specification, and a Ppk by group chart with reference
  lines at 1 and 1.33 (sorted by label, ascending or descending). Optional
  probability plot with the specification limits; save an
  in-specification indicator (formula column, 1/0).
- **Capability summary page** (Quality ▸ Capability summary page…, or the
  "Summary page layout" option of a capability report): the indices and
  process summary, then the location and dispersion control charts from
  the chosen within-σ subgrouping, the last 25 subgroups (or values) against
  the specification limits, the histogram and a probability plot for the
  chosen model, on one page.
- **Between/within capability:** capability of batches or parallel streams
  given by a subgroup column (or consecutive rows of a fixed size): within
  σ (pooled SD or average range), between σ (moving range of subgroup means,
  or one-way variance components), between/within σ and overall σ; Cp/Cpk
  within, Cp/Cpk between/within and Pp/Ppk overall with intervals; variance
  components table; expected and observed nonconforming rates; values by
  subgroup with the subgroup means and limits; histogram. *Options:* limits
  and σ estimators edited in place.
- **Attribute capability:** binomial (percent defective and PPM with the
  exact Clopper–Pearson interval, process Z with its interval) or Poisson
  (defects per unit with Garwood's exact interval), from a count column and
  a size column or fixed size; the p, u or c chart as a stability check;
  cumulative estimate over the samples; rate against sample size; table of
  samples. *Options:* model and confidence level edited in place.
- **Acceptance sampling plan:** single and double attribute plans found
  from the producer's and consumer's risk points under a binomial,
  hypergeometric (lot size) or Poisson model; k-method variables plans with
  σ known or unknown (exact noncentral-t OC curve); OC, AOQ (with AOQL), ATI
  and, for double plans, ASN curves; comparison of entered single, double
  and variables plans. Plans are computed from the risks, not taken from
  published tables. *Options:* every setting edited in place.
- **Gauge R&R:** three study types. *Crossed:* ANOVA method (parts ×
  operators with interaction). *Nested:* destructive tests, parts within
  operators (operator, part(operator), repeatability ANOVA; parts may be
  numbered within each operator). *Expanded:* extra factor columns, each
  random or fixed and crossed or nested within parts or operators, parts
  optionally nested within operators, two-factor interactions, unbalanced
  data and missing cells, fitted by REML; variance components with standard
  errors and Satterthwaite intervals, gauge R&R totals (with intervals)
  from the random components (a random factor nested within parts, such as
  a location on the part, is within-part product variation: listed under
  part-to-part and counted with it, not in gauge R&R), F tests of fixed
  factors (with a note when missing cells leave only some of a term's
  effects testable). All report
  %contribution, %study variation, %tolerance and the number of distinct
  categories. Gauge run chart: each part's measurements in run order (an
  optional run-order column) by operator. *Options:* ANOVA table,
  measurements by part and by operator, the run chart, pooling the
  interaction when non-significant (crossed), interactions (expanded).
- **Gauge study worksheet:** see Design; the same dialog.
- **Bias and linearity:** bias of each reference part with a t test, the
  regression of individual biases on the reference value with its
  confidence band, linearity and %linearity, average bias and %bias
  against a process variation you enter (AIAG MSA method). *Options:*
  confidence level and process variation edited in place.
- **Gauge repeatability (Cg, Cgk):** repeated measurements of one master
  part: Cg, Cgk (K = 20%, 6s by default; both editable), %repeatability,
  bias with a t test, and a run chart with the reference and limits
  (Dietrich & Schulze). Optional hysteresis (mean difference between two
  directions of approach, Welch t test and interval), drift (least-squares
  slope over time or run order, with test, interval and total change) and a
  choice of summary statistics (N, mean, SD, SE, min, median, max, range,
  6 SD, CV).
- **Intraclass correlation:** the six Shrout & Fleiss (1979) forms with F
  intervals; the share of variance due to parts, Wheeler's monitor class
  (first to fourth), attenuation of process signals, the probable error and
  the effective measurement increment (Wheeler & Lyday). *Options:*
  confidence level and measurement increment.
- **Attribute agreement:** appraisers rating parts into categories (one row
  per rating: part, appraiser, rating, optional trial and standard
  columns; categories from the rows used, so excluded rows add none).
  Within each appraiser over repeat trials, each appraiser against the
  standard, between appraisers and all appraisers against the standard:
  percent of parts matched with exact Clopper–Pearson intervals, parts
  consistently misrated and parts with mixed ratings; Fleiss' (1971) kappa
  overall and per category with null standard errors and z tests; Cohen's
  (1960) kappa against the standard (every trial a pair) and for every pair
  of appraisers, with Fleiss–Cohen–Everitt standard errors and intervals;
  the counts of ratings by standard category; an agreement plot of percent
  matched with intervals per appraiser (within appraiser and against the
  standard, each with its own colour and marker). For ordered categories:
  linear and quadratic weighted kappas, Kendall's coefficient of concordance
  W (ties corrected, chi-square test) and Kendall's τ_b with the standard.
  *Options:* ordered categories and their order, confidence level.
- **Attribute gauge bias and repeatability:** a go/no-go gauge checked
  repeatedly at known reference values near a limit (counts per row, or one
  row per check with the result meaning accepted): adjusted probabilities of
  acceptance, the fitted normal gauge performance curve, bias, repeatability
  and the bias t test (AIAG MSA analytic method), with notes where the study
  departs from the recommended design. *Options:* the limit.
- **Pareto chart:** categories of a column ranked by the number of rows or
  by the sum of a frequency column, with the cumulative-percentage line
  (each point labelled), a table of counts, percentages and cumulative
  percentages, and a summary of the "vital few" that make up 80%. A grouping
  column gives side-by-side charts, each ranked on its own (Juran). Each bar
  carries its rows: clicking or box-selecting bars selects them, and
  selected rows show as a highlighted part of each bar. *Options:* pool
  categories below a chosen share into an "Other" bar (drawn lighter and
  last), bar heights as counts or as percentages of the total.
- **Cause-and-effect diagram:** a fishbone diagram from one to three columns
  (main branches, causes, sub-causes; each row one path, repeated paths
  merged) with the effect at its head (Ishikawa), and an outline table with
  the rows behind each cause. Clicking a branch, cause or sub-cause selects
  its rows; saved as PNG, SVG, PDF or EPS like any plot.
- **Multi-vari chart:** a response against one to four nested or crossed
  factors (outermost first): individual values at each cell (by default
  spread in row order within the cell and joined, a short run chart per
  cell; or stacked on one vertical), each factor's
  level means marked in its own colour and shape and joined within each
  level of the factor above, the outermost factor's levels as panes; a
  variation-by-factor table (spread of each factor's means within the factor
  above, within-cell range and pooled standard deviation; Seder's families of
  variation); a ranges view charting the range within each cell against
  3-sigma limits (cells beyond the upper limit flagged). Values and ranges
  are linked to their rows. *Options:* individual values, replicates in
  order or stacked, ranges view, cell summary table. Exported figures keep
  the pane headings inside the frame. Also a graph-canvas chart form.

## Reliability

Menu groups: Life data, Life data with predictors, Field and repair data,
Planning.

- **Survival curves** (Reliability ▸ Survival curves…): Kaplan–Meier curves
  with censoring and confidence bands (plain, log or log-log scale),
  product-limit table, log-rank test across groups and Peto's log-rank test
  for trend across ordered groups (numeric labels as scores, else 1 … k);
  a number-at-risk table under the curves aligned with the time ticks
  (optionally with censored counts per interval) and as a report table;
  quick Weibull and lognormal maximum-likelihood fits with AIC/BIC
  comparison. *Verified:* trend test against R `survival::survdiff`
  (covariance), numbers at risk against `summary(survfit, times)`.
- *Verified:* published Kaplan–Meier and maximum-likelihood results.
- **Life distributions** (Reliability ▸ Life distributions…): maximum
  likelihood with exact, right-, left- and interval-censored times (a time
  and censor column, or start and end columns; frequency and group columns)
  for exponential, Weibull, lognormal, log-logistic, gamma, normal, logistic,
  smallest and largest extreme value, and threshold Weibull, lognormal,
  log-logistic, exponential and gamma; model comparison (log-likelihood,
  AIC, BIC, censoring-adjusted Anderson–Darling), likelihood-ratio tests of
  nested forms; Turnbull's nonparametric estimate (Kaplan–Meier for
  right-censored data); for the chosen or best family a probability plot
  with a percentage axis and confidence limits, parameter estimates,
  percentiles and failure probabilities with Wald and likelihood-ratio
  intervals, survival and hazard plots; Weibull with a known shape for few
  or no failures (Nelson 1985). Samples: seal inspections, bearing
  endurance test. *Verified:* independent SciPy implementation.
- **Life–stress models** (Reliability ▸ Life–stress models…): accelerated
  life tests with a Weibull, lognormal, log-logistic or exponential life
  whose log-life location depends on one or two stresses through Arrhenius
  (°C or K; activation energy in eV), Eyring, inverse-power or exponential
  relationships (two stresses give Peck or, with their interaction, the
  generalized Eyring model); constant scale, or ln σ linear in the stresses
  with a likelihood-ratio test of a constant shape; right, left and interval
  censoring with frequencies; parameter estimates with Wald and
  likelihood-ratio limits, activation energy or power exponent, acceleration
  factors relative to use conditions, percentiles and failure probabilities
  at use, a life–stress plot with percentile lines, a probability plot per
  test level with the fitted lines, and likelihood-ratio tests of equal
  scales, of the relationship and of individual fits per level. Sample:
  Device-A accelerated life test. *Verified:* independent SciPy
  implementation, lifelines AFT fitters, Meeker & Escobar's and Nelson's
  published fits.
- **Fatigue (stress–life) curves** (Reliability ▸ Fatigue (stress–life)
  curves…): cycles to failure against stress amplitude, with run-outs
  (specimens stopped unbroken) right-censored and optional frequencies;
  Basquin (ln N linear in ln S), semi-log, quadratic log–log or
  endurance-limit (ln N = β₀ + β₁ ln(S − E), E by profile likelihood with a
  likelihood-ratio interval) curves with lognormal, Weibull or log-logistic
  scatter fitted by maximum likelihood, optionally with ln σ linear in
  ln S. Report: specimens, failures and run-outs per level; fit statistics;
  coefficients with Wald and likelihood-ratio limits and Basquin's
  S = A·N^b; fatigue strength at chosen lives; life quantiles at chosen
  stresses; S–N plot (log axes) with quantile curves such as B10 and the
  median and a lower confidence bound; standardized residuals against
  stress; probability plot of the residuals of all levels (run-outs as
  censored). Sample: Steel fatigue. *Verified:* an independent SciPy
  maximum-likelihood implementation and lifelines AFT fitters.
- **Degradation** (Reliability ▸ Degradation…): repeated measurements per
  unit with linear paths on log or square-root scales, asymptotic or power
  paths; pseudo failure times (units that never reach the threshold are
  censored) fitted by a life distribution, or by a life–stress model with a
  stress column; or random unit intercepts and slopes (REML) with the
  implied time-to-threshold distribution and seeded bootstrap bands;
  destructive degradation (one measurement per unit) as a regression of the
  transformed response with an Arrhenius or other stress on the rate (the
  activation energy of the rate). Path plot with the threshold, pseudo
  failure table, life fit and probability plot, percentiles and failure
  probabilities. Samples: laser degradation, adhesive bond strength.
  *Verified:* NumPy/SciPy, statsmodels MixedLM, Meeker & Escobar's laser
  data.
- **Proportional hazards** (Reliability ▸ Proportional hazards…): Cox
  partial-likelihood regression with Efron (default) or Breslow ties and
  optional strata; continuous and categorical predictors with interactions
  and powers (hierarchy kept); model summary with likelihood-ratio, Wald and
  score tests, Effect Pareto and likelihood-ratio effect tests, coefficients
  and hazard ratios with Wald and profile-likelihood limits; Grambsch–Therneau
  test of proportional hazards per coefficient and globally (Kaplan–Meier,
  rank, identity or log time) with scaled Schoenfeld residual plots;
  Breslow baseline survival, predicted survival curves by a predictor's
  levels or percentiles with pointwise bands and a table at chosen times,
  deviance residuals; Response Explorer (hazard ratio or survival at a
  time); saves linear predictor, hazard ratio, survival, martingale and
  deviance residuals. Sample: leukemia with covariates. *Verified:*
  statsmodels `PHReg`, lifelines `CoxPHFitter`, direct formulas.
- **Parametric survival regression** (Reliability ▸ Parametric survival
  regression…): accelerated failure time models (Weibull, lognormal,
  log-logistic, exponential) with exact, right-, left- and interval-censored
  times and frequencies; predictors with interactions and powers; model
  summary, comparison of the four distributions, Effect Pareto and
  likelihood-ratio effect tests, estimates with observed-information
  standard errors, Wald and profile limits, time ratios, predicted quantiles
  and failure probabilities with limits, fitted survival over Turnbull
  estimates, Cox–Snell residual check; Response Explorer for a predicted
  percentile; saves the predicted percentile and Cox–Snell residuals.
  Sample: capacitor life test. *Verified:* lifelines AFT fitters and an
  independent SciPy implementation.
- **Reliability growth** (Reliability ▸ Reliability growth…): Crow–AMSAA
  power-law process from cumulative failure times, time- or
  failure-terminated (an optional event column marks failures; the test end
  is the last time or a given value), with the unbiased shape estimate
  (default) or the maximum-likelihood one and exact chi-square limits for
  the shape; separate fits per phase (each phase starts where the previous
  one ended); estimates table (shape, scale, current intensity and MTBF with
  limits), cumulative failures against the fitted mean, intensity and MTBF
  plots with confidence bands and a line per phase, and the Cramér–von Mises
  goodness-of-fit test (simulated p-value). *Verified:* MIL-HDBK-189 and
  Crow (1974) formulas, independent computations.
- **Repairable systems** (Reliability ▸ Repairable systems…): Nelson's mean
  cumulative function from repair ages of several systems with their end of
  observation, robust (Lawless–Nadeau) or Poisson variance, normal or log
  limits; MCF plot and table; for two groups, the difference of their MCFs
  with limits; homogeneous Poisson process and power-law (multi-system
  maximum likelihood) fits with a likelihood-ratio test of a constant rate;
  Laplace and MIL-HDBK-189 trend tests per group. Sample: pump repairs.
- **Competing risks** (Reliability ▸ Competing risks…): units that fail from
  one of several causes or are still running (one cause column with a value
  meaning censored; optional group and frequency columns): Aalen–Johansen
  cumulative incidence per cause with delta-method standard errors and
  log(−log) limits, overlaid or stacked, with an all-causes curve; Gray's
  test of equal incidence across groups for each cause; Fine–Gray
  subdistribution hazards regression on continuous or nominal covariates
  (robust standard errors, hazard ratios with limits, Wald and
  likelihood-ratio tests, predicted incidence at chosen settings); a
  parametric life distribution per failure mode (other modes censored)
  combined into the series system's failure probability with delta-method
  limits. Sample: pump failure modes. *Verified:* lifelines Aalen–Johansen,
  independent NumPy computations of Gray's test and Fine–Gray, SciPy fits.
- **Warranty returns** (Reliability ▸ Warranty returns…): shipments and
  returns by period, as a triangular table (one row per shipment period,
  one return column per calendar period) or one row per count, become
  interval- and right-censored life data (ages in periods, an optional
  warranty length); life distributions are compared by AIC with a
  probability plot, parameters and percentiles; returns are forecast per
  future period from units still in service and planned shipments, with
  prediction limits that include parameter uncertainty, plus the cumulative
  total. Sample: monthly warranty returns. *Verified:* independent SciPy
  maximum likelihood and forecast.
- **Reliability test plans** (Reliability ▸ Test plans…): demonstration
  plans with a Weibull shape (units needed, or test time per unit, for zero
  or c allowed failures, with the probability of passing at a true
  reliability); estimation plans giving the units needed to estimate a
  Weibull or lognormal percentile within a precision factor under time
  censoring; accelerated life test plans (optimum two-level and 4:2:1
  compromise plans, or your own levels) with Arrhenius, inverse-power or
  linear stress, giving levels, allocations, expected failures and the
  variance of the use-condition percentile. *Verified:* binomial and
  Fisher-information formulas (Meeker & Escobar 1998), independent
  computations.

## Series and Signals

Every time series analysis takes one or more numeric series columns (one
report each) in table order (newest-first tables, whose time column runs
backwards, are read from the bottom up; any other order is refused), an optional date-time or numeric time column
that labels the time axis (dates on the calendar) and the forecasts' time
stamps, and the observations per seasonal cycle, which follow the time
column's spacing (monthly 12, quarterly 4, weekly 52, daily 7, hourly 24,
otherwise 1) unless set. Excluded or missing rows at either end are left
out; one inside the series is reported as a gap (the methods need equally
spaced, complete data). Reports follow exclusions and edits like any other
and reopen exactly from a project file; scripts run them through
`analyze` (`docs/scripting.md`).

### Explore

- **Series description** (Series and Signals ▸ Series description…): summary and
  time series plot (brushable points joined by a line); an optional Box–Cox
  transformation (λ given, or chosen by Guerrero's method) and regular and
  seasonal differencing, with the transformed series plotted; ACF and PACF
  needle plots with white-noise limits (or Bartlett limits on the ACF) and a
  table of values; Ljung–Box tests at multiples of the period; augmented
  Dickey–Fuller (no terms, constant, or constant and trend; lag by AIC) and
  KPSS level and trend tests with statistic, lag, p-value and a
  plain-language conclusion. *Options:* unit-root tests, Dickey–Fuller
  terms, Bartlett limits, and the **spectral density** (off by default):
  periodogram markers and the modified-Daniell smoothed spectrum (choice of
  smoothing) with confidence limits on a log scale, against frequency or
  period, rules at the seasonal frequency and its multiples, and a table of
  the largest peaks. Sample: monthly electricity demand. *Verified:* the
  engine's reference tests against R (`acf`, `pacf`, `Box.test`,
  `spec.pgram`, `tseries`/`urca` unit-root tests).
- **Decomposition** (Series and Signals ▸ Decomposition…): classical
  moving-average decomposition (additive or multiplicative) or STL (seasonal
  pattern the same every cycle or smoothed over a span; robust option);
  panels of the observed series, trend (with the observations), seasonal
  component and remainder on the same time axis; seasonal indices by month,
  quarter or position (classical); strengths of trend and seasonality
  (Wang, Smith & Hyndman 2006; on the log scale for a multiplicative
  decomposition). *Options:* save the trend, seasonal, remainder and
  seasonally adjusted series as columns. Sample: monthly electricity demand.
  *Verified:* R `decompose` and `stl`.
- **Trend models** (Series and Signals ▸ Trend models…): linear, quadratic,
  exponential-growth or logistic S-curve fits against time (t = 1 at the
  first observation), or single moving-average forecasts of a chosen span;
  a plot of the data with the fitted curve and forecasts (prediction limits
  for the linear, quadratic and exponential models), the fitted equation,
  coefficients with standard errors and their meaning (the logistic curve's
  ceiling and midpoint), accuracy (MAPE, MAD, MSD, RMSE, mean error) and a
  forecast table with calendar time stamps. *Options:* periods to forecast;
  save fitted values and residuals (the forecast table saves as a data
  table from its right-click menu). Sample: product adoption. *Verified:*
  R `lm`/`nls` fits and textbook accuracy formulas.
- **Cross-correlation** (Series and Signals ▸ Cross-correlation…): an input series
  and one or more output series; correlations of the input at time t with
  the output at t + k for k = −K…K as a needle plot with ±z/√n limits and a
  table, an explanation of the lag sign in plain words (a peak at a
  positive lag means the output follows the input) naming the largest
  correlation; optional prewhitening by an autoregression fitted to the
  input (order by AIC or fixed) with the filter listed. *Options:*
  prewhitening. Sample: advertising and sales. *Verified:* R `ccf` and
  prewhitening by `ar.yw`.

### Forecasting models

- **Forecasting models** — shared report sections: forecast plot
  (observations as brushable points, one-step fitted values continuing as
  the forecast with diamond markers after a rule at the forecast origin,
  widest limits dashed, narrower ones thin; calendar-aware forecast times),
  forecast table (time, forecast, std error, limits at each level),
  residuals over time, residual autocorrelation with ±1.96/√n limits,
  Ljung–Box test (2m or 10 lags, fitted parameters off the degrees of
  freedom), in-sample RMSE, MAE and MAPE; horizon and interval levels (80%
  and 95% by default) in the dialog; ⋯ Options ▸ **Save forecasts as data
  table** adds a table of time, actual, one-step fitted, forecast, std error
  and limits to the project.
- **Exponential smoothing** (Series and Signals ▸ Exponential smoothing…): ETS
  models in innovations state-space form fitted by maximum likelihood —
  automatic selection by AICc (optionally without multiplicative forms or
  damped trends), simple, Holt linear, damped trend, additive and
  multiplicative Holt–Winters, or any error/trend/seasonal combination;
  α, β, γ, φ can be held fixed. Report: model summary (label and name,
  log-likelihood, AIC, AICc, BIC, σ), model selection table, smoothing
  parameters (estimated or fixed), initial states, and the shared sections;
  multiplicative-seasonal limits from 5000 seeded simulated paths (with the
  mean of the paths). Sample: hotel occupancy.
- **ARIMA** (Series and Signals ▸ ARIMA…): seasonal ARIMA(p,d,q)(P,D,Q)ₘ by exact
  maximum likelihood with a mean or drift term, or the stepwise (or
  exhaustive) automatic search with differencing by KPSS and
  seasonal-strength tests and orders by AICc; optional Box–Cox
  transformation (given λ or Guerrero's), forecasts transformed back
  (median; limits beyond the transform's range shown as 0 or ∞ with a
  note). Report: model summary (σ², log-likelihood, AIC, AICc, BIC,
  smallest root modulus), model search table, coefficients with standard
  errors, t ratios and p-values, and the shared sections. Sample: reactor
  temperature.
  - **Input series (transfer functions):** other columns enter through
    rational distributed lags ω(B)Bᵇ/δ(B) (Box–Jenkins transfer-function–
    noise models) with the ARIMA noise chosen or searched; the delay can come
    from the prewhitened cross-correlation; rows after the series with the
    inputs filled in supply future inputs for the forecasts. Report adds the
    transfer functions with steady-state gains, each input's impulse
    response (fitted and prewhitened estimates with limits) and the residual
    cross-correlation check with the prewhitened input. Sample: kiln fuel
    feed.

### Signals and spectra

These work on curves measured point by point (sensor traces, spectra,
chromatograms) rather than on seasonal series: each takes a y column, an x
column (optional for smoothing) and an optional column holding one curve per
level; points are sorted by x and rows sharing an x averaged. Engine:
`stat-signal`. Every routine is also a formula function and a script
function (`docs/formulas.md`, `docs/scripting.md`).

- **Smoothing** (Series and Signals ▸ Smoothing…): Savitzky–Golay (window, degree,
  derivative order), centred or trailing moving average, exponential
  smoothing, running median and running percentile, Gaussian kernel. Report:
  summary per curve (points, spacing, residual SD) and raw points with the
  smoothed curve overlaid; save the smoothed values (or derivative) as a
  column; settings change in place. Samples: machine vibration, ECG-like
  signal.
- **Derivatives and integrals** (Series and Signals ▸ Derivatives and integrals…):
  derivative by central differences (any spacing) or Savitzky–Golay; running
  integral by the trapezoid or Simpson's rule; optional resampling onto an
  even grid by linear, not-a-knot or natural cubic spline, or PCHIP
  interpolation. Report: summary (total integral, largest and smallest slope
  and where), derivative and running-integral plots, resampled-values table
  (save as a data table); save the derivative and integral as columns.
  Sample: machine vibration (acceleration integrated to velocity).
- **Baseline correction, Peak finding and Peak fitting** (Series and Signals ▸
  Baseline correction…, Peak finding…, Peak fitting…; one report): baseline by
  arPLS, asymmetric least squares, SNIP (optionally on the log–log–square-root
  scale) or a known baseline column; peaks after optional Savitzky–Golay
  smoothing by height and prominence in multiples of a robust noise level and
  by minimum separation, with position, height, prominence, width at half
  prominence and area; optional fit of a sum of Gaussian, Lorentzian,
  pseudo-Voigt, Voigt, split Gaussian or split Lorentzian peaks plus none, a
  constant or a linear baseline (Levenberg–Marquardt with bounds), starting
  from the peaks found or typed centres, with a common width and free, equal
  or fixed spacing of centres. Report: summary, spectrum and baseline plot,
  peaks table and plot, and per curve the fit with its components drawn, fit
  statistics (SSE, residual SD, R², AICc, BIC, parameters on a bound), each
  peak's centre, height, FWHM and area with standard errors and share of
  area, all parameter estimates, residuals, and a comparison of 1, 2, … peaks
  by AICc and extra-sum-of-squares F tests; with many curves, the same peak
  model fitted to each and a parameter table by curve (save as a data
  table). Save the baseline, corrected and smoothed spectra, fitted values and
  residuals as columns; peak and fit settings change in place. Samples: Raman
  bands (peak fitting), ECG-like signal (R-wave peaks).

The Fourier, filter, correlation and wavelet reports (Series and Signals ▸ Signals
and spectra) take one or more numeric signal
columns (one report each) with an optional x column and group column. With
an x column, rows are sorted by it and the sampling rate is read from its
spacing; unevenly spaced x–y pairs are resampled by linear interpolation onto
an even grid (with a note). Without one, the rate is set in the dialog (1 per
row by default). A group column gives one signal per level. The dialog shows
the sampling rate and the Nyquist frequency. Each report saves its outputs as
columns from the options menu (filtered signal, envelope, denoised signal and
so on, aligned with the original rows), and its tables as data tables like
any report. Engine: `stat-signal`, checked against NumPy, SciPy, PyWavelets
and scikit-image. Sample: test bench signals (simulated microphone,
accelerometer and strain recordings in three runs).

- **Fourier spectrum** (Series and Signals ▸ Fourier spectrum…): FFT of any length
  (mixed radix, Bluestein for other lengths); amplitude, power (dB or linear)
  and optional phase spectra, one- or two-sided, density or spectrum scaling;
  windows (rectangular, Hann, Hamming, Blackman, flat-top, Kaiser, Tukey),
  detrending (none, mean, linear) and zero padding (to a multiple of the
  length or the next power of two); a table of dominant frequencies; Welch's
  averaged periodogram; a spectrogram (short-time FFT) drawn as a
  time–frequency map with a colour bar.
- **Digital filter** (Series and Signals ▸ Digital filter…): Butterworth, Chebyshev
  I and II and elliptic IIR filters in second-order sections, windowed-sinc
  FIR filters, FFT filters (ideal or smooth cut-off) and notch filters; low
  pass, high pass, band pass and band stop; applied forwards or forwards and
  backwards (zero phase). Report: the signal with the filtered signal,
  frequency response (gain in dB) and, for forward filtering, phase response,
  and the filter coefficients (sections or taps).
- **Convolution and correlation** (Series and Signals ▸ Convolution and
  correlation…): cross-correlation with a second signal and autocorrelation
  (coefficient, biased, unbiased or raw) with the lag of maximum per group;
  linear convolution with a kernel column; deconvolution by Wiener division or
  Richardson–Lucy iteration; envelope, instantaneous phase and instantaneous
  frequency by the Hilbert transform.
- **Wavelets** (Series and Signals ▸ Wavelets…): multilevel discrete wavelet
  transform (Haar, Daubechies db1–db10, symlets sym2–sym10, coiflets
  coif1–coif5; symmetric, periodic or zero extension) with a stacked
  decomposition plot and a table of energy per level; denoising by soft or
  hard thresholds (universal, SURE or a fixed value; noise from the finest
  level or each level), showing the denoised signal; an optional continuous
  wavelet transform (Morlet or Mexican hat) drawn as a scalogram.

The image reports (Series and Signals ▸ Signals and spectra ▸ Images) take an
image stored one row per pixel: x and y columns on an even grid (y
downwards) and one or more value columns (one report each), with an
optional group column for several images in one table. Excluded or missing
pixels are filled with the image mean (counted in the summary); up to
4096 × 4096 pixels. Images are drawn as pictures with a colour bar,
linked to their rows for brushing. Engine: `stat_signal::image2d`, checked
against NumPy, SciPy and scikit-image. Samples: machined surface scan
(simulated), blurred and shifted scene (simulated).

- **Image spectrum** (Series and Signals ▸ Image spectrum…): centred 2-D Fourier
  log-magnitude spectrum with the dominant frequencies marked, optional
  phase spectrum; window (none, separable or radial Hann), mean removal,
  padding (zeros to a fast size, or mirror); radially averaged power
  spectrum on log–log axes with a power-law fit (slope, standard error,
  exponent β, R²); radial profiles: straight cuts through the spectrum
  along typed directions (default 0° and 90°) or through each dominant
  frequency, or wedges of ±w° averaged, plotted together and listed in a
  table that can be saved as a data table; angular power by direction; a table of dominant spatial
  frequencies with period, direction, amplitude and share of power.
- **Image frequency filter** (Series and Signals ▸ Image frequency filter…): low-,
  high-, band-pass and band-reject masks (ideal, Butterworth of any order,
  Gaussian) and notch filters at typed frequencies or at the strongest
  ones found, as reject or pass notches, applied by inverse FFT with mirror
  or zero padding. Report: original, filtered and removed images, the
  spectrum with the notches marked, the mask, and the notch table; save the
  filtered image and the removed part as columns.
- **Image registration and deblurring** (Series and Signals ▸ Image registration
  and deblurring…): deblurring of a Gaussian blur by Wiener filtering,
  constrained least squares or Richardson–Lucy iterations (save the
  restored image as a column); the shift between two images by phase
  correlation with sub-pixel refinement, the aligned image and the
  differences before and after (save the aligned image); the 2-D
  autocorrelation.

## Visualize — graph canvas

- Drag columns onto shelves: axes, panel splits (across, down, wrap, pages),
  colour, size, shape, subgroups, frequency, weight; column pickers; a
  per-graph row filter.
- Drop zones on the panels: while a column is dragged, zones appear over
  the graph (bottom edge horizontal axis, left edge vertical axis, top edge
  split across, right edge split down, centre colour), the one under the
  pointer highlighted and named ("Color by Region"); letting go assigns the
  column, an axis drop taking the axis's first place.
- The row filter's form (shared with reports' filter panel): level lists,
  from / to sliders and Step through levels or a moving window (Previous,
  Next, Play / Pause, seconds per step); **Show beside the graph** moves it
  into a panel left of the graph.
- Axis scales: linear, log₁₀, ln, logit, square root, reciprocal. Date and
  time columns get calendar axes (ticks on seconds, minutes, hours, days,
  months or years, labelled as dates and times).
- Layers, combinable: jittered points; polynomial fits with confidence and
  prediction bands and movable equation / R² / p-value labels; smoothing
  splines and LOESS; box plots (notched) and violins; summary statistics with
  error bars; density ellipses, kernel-density contours (optionally shaded),
  hexagonal and square
  binning; formula curves; bars (side by side, stacked, 100%) with error bars;
  line and run charts (rows in table order get the four run-chart tests:
  clustering, mixing, trend, oscillation); category heat maps; histograms
  (automatic, count or width bins; overlaid or stacked subgroups; counts or
  density; selected share shaded); Wilkinson dot plots; probability plots
  against any of the 14 fitted distributions of Identify distribution, with
  an order-statistic band and percentage guides; point labels.
- Chart forms: panels with layers, treemap, parallel coordinates, multi-vari
  chart (the vertical column by up to four horizontal-axis factors, with each
  factor's means joined within the factor above), scatterplot matrix (every
  pair of the chosen continuous columns, one-variable layers such as
  histograms on the diagonal).
- Round and special chart forms (the round ones and the ternary plot drawn
  true to shape on screen and in exported files):
  - Pie and donut charts: slices by a category column, sized by row count or
    by the sum of the vertical column, labelled with level, share and value
    and marked with the level's marker shape (a donut writes the total in
    its centre); clicking a slice selects its rows, and slices holding
    selected rows are ringed and say how many.
  - Radar chart: one spoke per numeric column (each scaled from its minimum
    to its maximum, range written at the spoke's end), one polygon per row
    or per colour group's means; each group has its own colour, marker and
    line pattern; optional fill.
  - Polar chart: points (optionally joined in row order) at an angle
    (compass degrees, mathematical degrees or radians) and a radius.
  - Wind rose: rows counted by direction sector (8, 16 or 36), stacked by
    round-edged bins of a magnitude column (or by its levels), rings in
    percent or counts; clicking a sector piece selects its rows.
  - Ternary plot: three component columns, each row divided by its total,
    on an equilateral triangle with 10% gridlines and edge labels; points
    coloured and shaped by the usual shelves, box and click selection.
  - Waterfall chart: running total of a numeric column's sums by category
    in table order, with a total bar; increases and decreases differ in
    colour *and* hatching (decreases hatched), and each bar is labelled with
    its signed value (+120, −35); clicking a bar selects its rows.
  - Ridgeline plot: kernel densities (Gaussian, Silverman bandwidth) of a
    column, one ridge per group stacked down the page on one height scale,
    with a rug of rows and adjustable ridge height.
  - Vector field (quiver plot): an arrow per row from (x, y) along two
    component columns, scaled together (adjustable), with the scale written
    under the graph.
- Lollipop charts: the bars layer's "Lollipops (stem and dot)" layout.
- Markers: size, colour and shape per plot and per group (right-click a
  marker); key placement.
- Right axis: a Right axis shelf draws numeric columns against a second
  vertical axis with its own scale, ticks, title and spine; the key says which
  series uses which axis ("(right axis)", with dashed lines and a different or
  hollow marker, so colour is never the only cue). Right-axis points can be
  selected like any other; the right axis drags as a ruler (double-click
  re-fits), and its title and tick display are in Plot settings ▸ Right axis
  (or right-click it).
- Axis breaks: Plot settings leaves stretches [from, to] out of a
  continuous axis (either or both; Add a break for several), drawn with
  break marks on both spines (slashes or a zigzag); data inside a gap are
  hidden and counted in a note. Segments and marks gives each piece between
  breaks its own scale (linear, log₁₀, ln, square root, logit, reciprocal;
  ticks and labels from each piece, and a note naming the scales) and a
  share of the axis length in percent. Report scatter plots take the same
  breaks. Saved figures draw the same; breaks are kept in projects and
  graph files.
- Titles: graph title and axis titles typed in Plot settings, with math
  markup; Options ▸ "Read math markup in column names, levels and labels"
  applies it to column-name titles, key entries and text labels too.
- Export to PNG (300 ppi), SVG, PDF and EPS, with the grid rules shown on screen; summary table; save as a JSON pipeline that
  reopens on any dataset with the same column names.
- Annotations: right-click a panel ▸ Add text, Add arrow, Add line or Add
  rectangle at the point clicked. Text takes math markup (an arrow can carry
  text at its tail). Each is anchored to the data (moves with the axes and
  follows a change of scale) or to the plot frame (stays in place), chosen in
  its editor; drag to move, double-click to edit, right-click to edit,
  delete, bring forward or send backward. Click an annotation or a movable
  statistics label (fit equation, correlation) to select it, then nudge it
  with the arrow keys (1 point; Shift: 10); Escape clears the selection and
  Delete removes a selected annotation. Drawn in exported figures and kept
  with the graph (`axes.style.annotations`).
- Drawing order: Layers ▸ Drawing order lists the layers front to back with
  Bring forward / Send backward buttons (also on right-clicking a layer's
  toggle); the order is saved (`layer_order`), and layers turned on later
  take their usual place.
- Editable key: Edit key… (in the Chart bar, or right-click the key) lists
  every key entry with a check box to show or hide it, its text (rename;
  math markup as elsewhere) and buttons to move it up or down; Reset key
  undoes every change. Edits follow what each entry stands for (a layer's
  entry, or a column and level), so new levels still appear; exported
  figures show the edited key; saved as `axes.style.key`.
- Swarm points: a points-layer layout (Points ▸ … ▸ Swarm) that places
  markers on a category axis side by side without overlap, nearest the
  category's centre line first (deterministic; saved in the graph JSON).
- Significance brackets (layer for category axes): pairwise comparisons by
  Tukey–Kramer (default), Games–Howell, Holm-adjusted t or Dunnett against
  the first category; every compared pair or only the pairs ticked in the
  layer's settings; each bracket a chosen gap above the tallest point of
  the categories it spans, wider brackets arching over narrower ones;
  labelled with the adjusted p-value, stars or both, with star thresholds
  of the user's choice and optionally "ns" on pairs that are not
  significant; optionally only pairs below a chosen p; the procedure and
  star key in the graph's notes.
- Error bars (summary points and bars): colour, line width, cap width (or
  no caps) and direction (both ways, upper only, lower only), saved with
  the graph.
- Convex hulls (layer for scatter panels): the hull of each color or subgroup
  group, outlined in the group's color with its own line pattern, optionally
  shaded; click inside a hull to select its rows.
- Clustered heat map (chart form): numeric columns across, rows down, an
  optional row-label column; values standardized per column (optional);
  rows and columns reordered by hierarchical clustering (average, complete,
  single or Ward linkage) with dendrograms; a ramp key; values in
  cells (optional); click a cell to select its row.
- Estimation plot (chart form): every value of each group as a swarm with
  the group mean ± SD; two groups give a Gardner–Altman plot (the mean
  difference on a floating axis anchored at the reference mean), more give a
  Cumming plot (a lower panel of differences from the reference group).
  Intervals by the BCa bootstrap (5000 resamples, seeded, with the bootstrap
  distribution drawn) or Welch t; level and reference group chosen in the
  canvas; the differences and intervals in the notes and the summary table.
- Flow charts (chart forms): Sankey diagram of two or more categorical
  columns as stages, flows sized by rows or by the sum of the vertical
  column, node order chosen by barycentre sweeps to reduce crossings; chord
  diagram of flows between a "from" and a "to" column; network of the two
  ends of each edge with an optional numeric weight, laid out by a seeded
  Fruchterman–Reingold force-directed algorithm. Click a flow, ribbon, arc,
  node or edge to select its rows.
- Samples: Reading trial (invented), Parcel flows (invented), Regional
  climate (invented).
- Samples: Harbour weather (wind rose, vector field, radar chart,
  ridgelines, donut), Household budget (waterfall, donut by sum,
  lollipops), Soil texture (ternary plot); all invented.

## Visualize — 3-D scatter, contour and surface plots

- 3-D scatter plot of three numeric columns: colour (a ramp for a continuous
  column, colour and marker shape per level otherwise), marker size and
  shape columns; drop lines to the floor; drag to turn, view from above or
  from the side; box or click selection linked to every view; excluded rows
  hollow.
- Contour and surface plot of a response over two columns, made by Delaunay
  triangulation (linear on each triangle, blank outside the data), an
  interpolating thin-plate spline, or a smoothing thin-plate spline with λ
  chosen by generalized cross-validation (fit table with λ, effective df,
  GCV and residual SD, the last two over the distinct locations when rows
  at one location were averaged); grid divisions 8–120. Filled contour plot
  with a colour bar, labelled levels and selectable observations; turnable 3-D
  surface, filled or wireframe, with contours on the surface, a floor contour
  map and the data points, in the contour plot's colour scale.
- Contour maps (response surface, mixture ternary, nonlinear surface and
  contour-and-surface reports): bands between levels filled in a colour scale
  (sequential, heat, spectrum, diverging around a centre, grey), with a colour
  bar; Plot settings switch fill, level lines and level values, reverse the
  scale, set the opacity, the number of levels and a fixed range; "Lines only"
  gives level lines and values alone. Saved with the report in projects,
  dashboards and scripts (`contour_style`); exports draw the bands as real
  shapes (one even–odd outline per band in SVG, PDF and EPS).
- Both 3-D plots save as PNG, SVG, PDF and EPS at the current angle; both
  run from scripts (`scatter-3d`, `data-surface`) and are kept in projects
  with their angle and display options.
- Sample: Hillside survey (invented).

## Visualize — image plots

- Image plot (Visualize ▸ Image plot…) of a value column over x and y
  columns on an evenly spaced grid, drawn cell for cell (no smoothing) in a
  colour scale (sequential, heat, spectrum, diverging, grey; any of them
  reversed, grey from black to white by default) over the data range or a
  fixed range, with a colour bar; y downwards as in image files or
  upwards; equal scale on both axes. Hovering names the cell, its position
  and value; box or click selection selects the pixels' rows, shown as a
  tinted overlay and linked to every view; excluded rows are blank. A Grid
  table gives the size, spacing and value range.
- Exports embed the picture as a bitmap: PNG data in SVG, an image object
  (with transparency) in PDF, an inline image in EPS, pixel for pixel in PNG
  and on figure pages. Runs from scripts (`image-plot`) and is kept in
  projects.

## Visualize — figure page

- Several plots on one page, added from any plot's right-click menu (Add to
  figure page) or a graph canvas's options menu; report plots, graph
  canvases and Response Explorers redraw at their panel's size (text keeps
  its point size), other pictures shrink to fit.
- Page size in millimetres or inches, with journal-column (89, 120, 183 mm)
  and A4 / US Letter presets; 1–6 columns, panels spanning columns, space
  between panels, an optional page title; panel letters A, a, (A), (a) or
  none, with a letter size; plots' own titles kept or left out; reorder and
  remove panels; live preview.
- Saved as one PNG (72–1200 pixels per inch, recorded in the file), SVG, PDF
  or EPS; PDF and EPS optionally in CMYK colour; transparent background;
  sans-serif or serif text.

## Plots (everywhere)

- Light and dark themes; quiet grid (horizontal major rules by default);
  data-framing axis ranges (6% padding, zero baseline kept for non-negative
  data, except that scatter points at or just above zero get padding below;
  categorical axes leave half a category at each end). Date axes in reports
  and the graph canvas tick on calendar units and read as dates, with no zero
  baseline; sample and subgroup axes tick on whole numbers. Tick labels that
  would be cut off at the top or collide in the corner are left out.
- Labels inside plots keep clear of each other: survival end labels and
  run-rule numbers move apart, Pareto cumulative percentages that would
  collide are left out, reference lines are named beside their lines, and
  correspondence-map names move off other markers. Ternary plots are drawn
  true to shape without numeric axes. In dark mode the darkest group colours
  are lightened.
- Axes are draggable rulers: drag an end to rescale, the middle to shift; no
  scroll or pinch zoom. Reset view re-fits. One line above the reports says
  how to work the plots (each plot's ? repeats it).
- Lasso selection in report scatter plots (right-click ▸ Lasso selection):
  drag a freehand outline instead of a box.
- Zoom to selected points (a report plot's right-click menu) frames the
  selection; Reset view goes back.
- Hovering a point in a report plot shows its row number and its two values
  (the category on a categorical axis).
- Each plot's key names its lines, bands and markers for what they are
  (fitted line, reference line y = x, 4/n threshold, OC curve, limits of
  agreement...); crowded category labels turn instead of overlapping
  (Plot settings ▸ Category labels: Automatic — level, else slanted 45°,
  else upright — or fixed Horizontal, Slanted (45°), Vertical; thinned only
  when even upright names overlap; exported figures follow the setting).
- Box selection in the plot (lasso too on the graph canvas); click to select a
  point; selected rows ringed.
- **Plots with very many points:** above 20,000 points, report scatter and
  quantile plots, box-plot values beyond the whiskers and selection rings,
  and the graph canvas's point layers are drawn as one density picture at the
  screen's resolution instead of a marker per row (darker where points
  overlap, the selection in its own colour on top, excluded rows in theirs),
  with a note under the plot saying so. Box, lasso and click selection and
  hovering work as before, and exported files keep every point. The GPU is
  also asked for its largest buffers, so a frame with very much geometry no
  longer stops the program.
- Plot settings per plot: titles, ranges, major/minor ticks, tick origin, grid
  lines, number format, axis position, equal scale, key; on the graph canvas
  also the right axis; on scatter plots and the canvas, axis breaks
  (several per axis, a scale and share per piece, slash or zigzag marks). The settings
  (all but dragged ranges; ranges typed in Plot settings are kept) are kept
  with the plot in projects, dashboards, scripts and graph files
  (`plot_styles` in a report, `axes.style` in a graph), so reopened plots
  look as they were left.
- **Copy plot style / Paste plot style** (every plot's right-click menu and
  section heading; a graph canvas's ⋯ Options and plot menu): the copied
  style is kept for the session; the paste submenu names its source and has
  ticks for Axis ranges (off by default), Ticks, grid and spines, Font and
  size, Colours and markers (between canvases), Reference lines and Key, and
  pastes **Onto this plot**, **Onto every plot in this report** or **Onto
  every plot in this project**. Titles, breaks and annotations stay; pasted
  settings are saved with each plot's style.
- Figure output, in every plot's right-click menu (a graph canvas's ⋯
  Options): **Copy plot** puts the figure on the clipboard as PDF, PNG and
  SVG at once (macOS `com.adobe.pdf`, `public.png`, `public.svg-image`;
  Windows a bitmap, `PNG` and `image/svg+xml`; Linux `image/png`,
  `application/pdf`, `image/svg+xml`; falls back to the picture alone if the
  clipboard refuses the rest). **Save plot as…** opens a dialog with the
  format, a size on paper in millimetres or inches (journal-column widths,
  keep proportions), PNG resolution (36–1200 ppi, pixel size shown), a
  transparent background, the font and the text size, with a preview; text
  keeps its point size at any paper size. **Transparent background** and
  **Plot font** (Sans serif or Serif, text size 50–300%) are also menu items.
  Sans serif is Helvetica in PDF and EPS, Liberation Sans in PNG; Serif is
  Times, Liberation Serif in PNG; SVG names the font with its stand-ins. On
  screen the font and text size apply to the plot's titles and tick labels
  (numbers stay in the monospaced face); in figures the text size enlarges
  text, marks and lines together. The plot keeps these settings with its
  style (`export` in `plot_styles` and in a graph's `axes.style`), and the
  quick Save plot as PNG/SVG/PDF/EPS items and Copy plot use them. Pictures
  kept as drawn (3-D views, trees, diagrams, word clouds) keep them for the
  session and scale to the size on paper.
- Axis look (Plot settings ▸ each axis): **Ticks** — tick direction (inside,
  outside, across the axis line), length and thickness of major and minor
  marks (minor marks drawn whenever the axis has minor ticks), and your own
  tick labels: a label in math markup at any value replaces the number there
  or adds a tick (**Add a tick at…**; or double-click a tick label on the
  plot, or right-click the axis ▸ **Edit tick label…**, with *Use the
  number* to undo); **Spine** — offset of the axis line from the data area
  (marks, labels and the draggable axis ruler move with it) and **Extend to
  nearest ticks** (an automatic range widened to the major ticks just
  outside it; off by default). **Plot area**: a scale ratio at equal scale
  (one x unit as long as *r* y units) and a print scale for saved figures
  (1 x unit = *N* mm). Saved SVG, PDF, EPS and PNG figures of report plots
  and graph canvases draw these settings as the screen does (report figures
  also the user's tick intervals, minor ticks, number format, grid choice
  and axis sides).
- Reference lines and bands (Plot settings ▸ Reference lines…, the plot's
  right-click menu, or the canvas's Chart row): any number of horizontal or
  vertical lines, each with its value, label (optionally followed by the
  value), colour, width and dash pattern (solid, dashed, dotted, dash-dot),
  and shaded bands between two values with fill colour, opacity, edge lines
  and edge values, drawn behind the data. A value is typed or a statistic
  of the plotted values on that axis — mean, median, a quantile, mean ± k
  standard deviations, or an individuals chart's centre line and 3σ limits
  (x̄ ± 3·MR̄/d₂) — recomputed as data and row states change (excluded rows
  left out). On the graph canvas a statistic is of the whole graph, each
  panel or each subgroup (one line per subgroup in its colour). Every line
  and band has a key entry and a label on the plot; the view widens to show
  them; saved figures (SVG, PDF, EPS, PNG, figure page) draw them; they are
  saved with the plot's style.
- Math in labels: titles typed in Plot settings read a small markup —
  subscripts and superscripts (`x_1`, `x_{ij}`, `e^{-t}`), Greek letters by
  name (`\alpha`, `\Delta`), a few symbols (`\pm`, `\le`, `\infty`), stacked
  fractions (`\frac{a}{b}`, or `a/b` inside `$…$`), square roots
  (`\sqrt{x}`), `\sum` and `\prod`, and escapes (`\_`, `\^`,
  `\$`, `\\`); vertical titles too. Saved SVG, PDF, EPS and PNG figures set
  the same scripts and fractions. Text without markup characters draws
  exactly as before.
- Resizable plots: a strip under each plot sets its height, a strip on its
  right edge its width, the corner both; double-click restores the default.
  Never wider than the visible pane. Covers x–y plots, charts, survival
  plots, the graph canvas and the 3-D scatter and surface plots; a Response
  Explorer's panels share one grip that resizes them all together (kept per
  report; saved figures follow it). Diagrams sized by their text (heat maps,
  trees, cause-and-effect, Effect Pareto) scroll instead.
  Plots of one variable against another and 3-D plots open at most about
  760 points wide;
  observed-vs-predicted, probability (against y = x) and score plots open at
  equal scale. Dragging one axis limit of an equal-scale plot moves only
  that limit and rescales the other axis about its centre; on the graph
  canvas a drag fixes only the dragged axis. Exported figures take the
  proportions of the drawn plot frame.
- Grouped data use a colour-blind-safe palette paired with distinct marker
  shapes.
