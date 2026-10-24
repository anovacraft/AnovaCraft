//! Dashboards: reports and notes from a project's tables arranged on a grid,
//! with local filters shared by every tile (roadmap §10).
//!
//! A [`Dashboard`] belongs to a [`ProjectFile`] and uses only that project's
//! tables. It holds:
//!
//! * **tiles** ([`DashboardTile`]) placed on a grid of `grid_columns` columns
//!   and as many rows as needed; each tile covers a rectangle of whole cells
//!   ([`GridArea`]) and tiles never overlap. A tile is either a report — its
//!   own copy of a report request ([`SavedReport`], the same request a table's
//!   reports and [`run_report`] use) bound to one table by [`TableId`] — or a
//!   heading or text note ([`TileContent::Text`]);
//! * **local filters**: at most one [`FilterCondition`] per column name.
//!   Every filter applies to every report tile whose table has a column of
//!   that name (all conditions must hold); a table without the column is not
//!   filtered by it. Filters never change a table's own row states: each
//!   tile runs on a copy of its table's [`RowStateManager`] in which the rows
//!   outside the filters are also excluded and hidden ([`Dashboard::filter_rows`]).
//!   Row indices stay those of the table, so selecting rows in a tile is
//!   linked, through the table's own row states, with every other view of the
//!   same table, and only of that table;
//! * **shared column pickers** ([`SharedSwitcher`]): at most one per column
//!   name. A switcher names a column, the columns it can show instead, and
//!   the one it shows now. Every report tile that reads the switcher's
//!   column runs with the chosen column in its place, in every role the
//!   request uses it ([`Dashboard::switched_report`]). A tile is left as it
//!   is when its table lacks the chosen column, or when its request already
//!   reads the chosen column (the two would collide). Tiles keep their own
//!   requests unchanged, so choosing the switcher's own column again shows
//!   them as they were.
//!
//! [`compute_dashboard`] runs every tile. Changes go through
//! [`DashboardEdit`]s, each of which returns its inverse when applied, so a
//! [`DashboardHistory`] can undo and redo them. Dashboards are saved in the
//! version-3 project file (see [`crate::project`] for the JSON form).

use std::collections::HashMap;

use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_viz::FilterCondition;

use crate::project::{
    ProjectFile, SavedReport, TableId, array, checked_report_to_json, invalid, member, obj, report_from_json, s,
};
use crate::report_run::{ComputedReport, run_report};
use crate::table_ops::{condition_from_json, condition_to_json};

/// Grid width of a new dashboard, in columns.
pub const DEFAULT_GRID_COLUMNS: u32 = 12;
/// Widest grid allowed, in columns.
pub const MAX_GRID_COLUMNS: u32 = 48;
/// Rows a tile may reach down to (its `row + height`): a bound that keeps
/// saved files sane, far beyond any practical dashboard.
pub const MAX_GRID_ROWS: u32 = 10_000;
/// Most edits kept for undo by a [`DashboardHistory`].
pub const DASHBOARD_HISTORY_LIMIT: usize = 500;

/// A dashboard's identifier within a project: stable across saves and
/// independent of the dashboard's (editable) name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DashboardId(pub String);

impl DashboardId {
    /// The identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DashboardId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for DashboardId {
    fn from(id: &str) -> Self {
        Self(id.to_owned())
    }
}

/// A tile's identifier within its dashboard (a positive number, stable
/// across saves and moves).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId(pub u32);

impl std::fmt::Display for TileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tile {}", self.0)
    }
}

/// The rectangle of grid cells a tile covers: its top-left cell (`column`,
/// `row`, counted from 0) and its span (`width` columns by `height` rows,
/// each at least 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridArea {
    /// Leftmost column (0-based).
    pub column: u32,
    /// Top row (0-based).
    pub row: u32,
    /// Columns spanned (at least 1).
    pub width: u32,
    /// Rows spanned (at least 1).
    pub height: u32,
}

impl GridArea {
    /// The area with top-left cell (`column`, `row`) spanning `width` by
    /// `height` cells.
    #[must_use]
    pub fn new(column: u32, row: u32, width: u32, height: u32) -> Self {
        Self { column, row, width, height }
    }

    /// `true` when the two areas share at least one cell.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        let span = |a: u32, n: u32| u64::from(a) + u64::from(n);
        u64::from(self.column) < span(other.column, other.width)
            && u64::from(other.column) < span(self.column, self.width)
            && u64::from(self.row) < span(other.row, other.height)
            && u64::from(other.row) < span(self.row, self.height)
    }

    /// The row just below the area (`row + height`).
    #[must_use]
    pub fn bottom(&self) -> u64 {
        u64::from(self.row) + u64::from(self.height)
    }
}

/// How a text tile's text is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextStyle {
    /// A heading.
    Heading,
    /// Body text (the default).
    #[default]
    Body,
}

impl TextStyle {
    fn key(self) -> &'static str {
        match self {
            Self::Heading => "heading",
            Self::Body => "body",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "heading" => Self::Heading,
            "body" => Self::Body,
            _ => return None,
        })
    }
}

/// What a tile shows.
#[derive(Debug, Clone, PartialEq)]
pub enum TileContent {
    /// A report: the tile's own copy of a report request, run on one of the
    /// project's tables with the dashboard's filters applied.
    Report {
        /// The table the request is run on. Column indices in `report`
        /// refer to this table's columns (graphs name them).
        table: TableId,
        /// The request and its display options (boxed: requests are large).
        report: Box<SavedReport>,
    },
    /// A heading or a note.
    Text {
        /// The text (may span several lines).
        text: String,
        /// How it is shown.
        style: TextStyle,
    },
}

impl TileContent {
    /// The table a report tile is bound to (`None` for text).
    #[must_use]
    pub fn table(&self) -> Option<&TableId> {
        match self {
            Self::Report { table, .. } => Some(table),
            Self::Text { .. } => None,
        }
    }
}

/// One tile of a dashboard.
#[derive(Debug, Clone, PartialEq)]
pub struct DashboardTile {
    /// Identifier, unique within the dashboard.
    pub id: TileId,
    /// Where it sits on the grid.
    pub area: GridArea,
    /// What it shows.
    pub content: TileContent,
}

/// A column picker shared by every tile of a dashboard (see the
/// [module documentation](self)): tiles that read `column` run with
/// `current` in its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedSwitcher {
    /// The column the tiles' requests read and the switcher replaces.
    pub column: String,
    /// Columns (by name) the switcher offers, in order; `column` is one of
    /// them. Names need not belong to every table: a tile is switched only
    /// when its table has the chosen column.
    pub choices: Vec<String>,
    /// The column shown now (one of `choices`). Equal to `column`, nothing
    /// is switched.
    pub current: String,
}

impl SharedSwitcher {
    /// A switcher on `column` offering `choices` (with `column` put first
    /// when missing; repeats dropped), showing `column`.
    #[must_use]
    pub fn new(column: impl Into<String>, choices: impl IntoIterator<Item = String>) -> Self {
        let column = column.into();
        let mut list = vec![column.clone()];
        for c in choices {
            if !list.contains(&c) {
                list.push(c);
            }
        }
        Self { current: column.clone(), column, choices: list }
    }

    /// `true` when it shows a column other than its own.
    #[must_use]
    pub fn is_switched(&self) -> bool {
        self.current != self.column
    }

    /// The same switcher showing `current`.
    #[must_use]
    pub fn showing(&self, current: impl Into<String>) -> Self {
        Self { current: current.into(), ..self.clone() }
    }

    /// `"Humidity → Temperature"` when switched, else the column's name.
    #[must_use]
    pub fn describe(&self) -> String {
        if self.is_switched() { format!("{} → {}", self.column, self.current) } else { self.column.clone() }
    }

    fn check(&self) -> Result<()> {
        if self.column.is_empty() {
            return Err(invalid("a column picker needs a column"));
        }
        if !self.choices.contains(&self.column) {
            return Err(invalid(format!("the picker on {:?} must offer that column", self.column)));
        }
        if !self.choices.contains(&self.current) {
            return Err(invalid(format!("the picker on {:?} shows a column it does not offer", self.column)));
        }
        if self.choices.iter().enumerate().any(|(i, c)| self.choices[..i].contains(c)) {
            return Err(invalid(format!("the picker on {:?} offers a column twice", self.column)));
        }
        Ok(())
    }
}

/// A tile's request as the dashboard's switchers make it
/// ([`Dashboard::switched_report`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchedReport {
    /// The request the tile runs (its own when nothing is switched).
    pub report: SavedReport,
    /// Switchers that changed it, as (switcher column, column shown), in
    /// the order applied.
    pub applied: Vec<(String, String)>,
    /// Switchers on a column the request reads that could not change it:
    /// the table lacks the chosen column, or the request already reads it.
    pub skipped: Vec<String>,
}

/// `report` (a request on `table`) reading column `to` wherever it reads
/// `from`; `None` when it does not read `from`, already reads `to`, or
/// `table` lacks either. The report's own row filter and switcher are kept
/// as they are. A generalized linear model whose response changes goes back
/// to its default event level.
#[must_use]
pub fn substitute_column(report: &SavedReport, table: &DataTable, from: &str, to: &str) -> Option<SavedReport> {
    let (f, t) = (table.find(from)?, table.find(to)?);
    if f == t {
        return None;
    }
    let (mut inner, scope) = report.clone().split_scope();
    if matches!(inner, SavedReport::Graph { .. }) {
        let names = inner.column_names();
        if !names.contains(&from) || names.contains(&to) {
            return None;
        }
        inner.rename_column(from, to);
    } else {
        let columns = inner.columns();
        if !columns.contains(&f) || columns.contains(&t) {
            return None;
        }
        inner = inner.map_columns(|c| if c == f { t } else { c });
        if let SavedReport::Glm { request, .. } = &mut inner
            && request.response == t
        {
            request.event = None;
        }
    }
    Some(inner.with_scope(scope))
}

/// A dashboard: tiles on a grid, local filters and shared column pickers
/// (see the [module documentation](self)).
#[derive(Debug, Clone, PartialEq)]
pub struct Dashboard {
    /// Identifier, unique within the project.
    pub id: DashboardId,
    /// Name shown to the user.
    pub name: String,
    /// Grid width in columns (1 to [`MAX_GRID_COLUMNS`]).
    pub grid_columns: u32,
    /// Tiles, in reading order (also the order they are computed in).
    pub tiles: Vec<DashboardTile>,
    /// Local filters, at most one per column name, in the order added.
    pub filters: Vec<FilterCondition>,
    /// Shared column pickers, at most one per column, in the order added
    /// (also the order they are applied in).
    pub switchers: Vec<SharedSwitcher>,
}

/// The dashboard's filters as they apply to one table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableFilter {
    /// Whether each row passes every filter that applies (`true` for every
    /// row when none applies).
    pub mask: Vec<bool>,
    /// Columns whose filters apply (the table has the column).
    pub applied: Vec<String>,
    /// Columns whose filters do not apply because the table lacks them.
    pub skipped: Vec<String>,
}

impl TableFilter {
    /// Rows passing the filters.
    #[must_use]
    pub fn rows_kept(&self) -> usize {
        self.mask.iter().filter(|m| **m).count()
    }

    /// `true` when at least one filter applies to the table.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.applied.is_empty()
    }
}

impl Dashboard {
    /// An empty dashboard with the default grid width
    /// ([`DEFAULT_GRID_COLUMNS`]).
    #[must_use]
    pub fn new(id: DashboardId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            grid_columns: DEFAULT_GRID_COLUMNS,
            tiles: Vec::new(),
            filters: Vec::new(),
            switchers: Vec::new(),
        }
    }

    /// The tile with id `id`.
    #[must_use]
    pub fn tile(&self, id: TileId) -> Option<&DashboardTile> {
        self.tiles.iter().find(|t| t.id == id)
    }

    fn tile_index(&self, id: TileId) -> Result<usize> {
        self.tiles.iter().position(|t| t.id == id).ok_or_else(|| invalid(format!("the dashboard has no {id}")))
    }

    /// An id no tile uses yet: one more than the largest in use (1 for an
    /// empty dashboard), so a removed tile's id is not reused while later
    /// tiles remain.
    #[must_use]
    pub fn unused_tile_id(&self) -> TileId {
        TileId(self.tiles.iter().map(|t| t.id.0).max().unwrap_or(0).saturating_add(1))
    }

    /// The filter on column `column`.
    #[must_use]
    pub fn filter(&self, column: &str) -> Option<&FilterCondition> {
        self.filters.iter().find(|f| f.column() == column)
    }

    /// The shared switcher on column `column`.
    #[must_use]
    pub fn switcher(&self, column: &str) -> Option<&SharedSwitcher> {
        self.switchers.iter().find(|s| s.column == column)
    }

    /// `report` (a tile's request on `table`) as the switchers make it: each
    /// switcher showing another column, in order, replaces its column with
    /// the one shown ([`substitute_column`]) when the request reads it.
    #[must_use]
    pub fn switched_report(&self, report: &SavedReport, table: &DataTable) -> SwitchedReport {
        let mut out = SwitchedReport { report: report.clone(), applied: Vec::new(), skipped: Vec::new() };
        for s in self.switchers.iter().filter(|s| s.is_switched()) {
            match substitute_column(&out.report, table, &s.column, &s.current) {
                Some(r) => {
                    out.report = r;
                    out.applied.push((s.column.clone(), s.current.clone()));
                }
                None if reads_column(&out.report, table, &s.column) => out.skipped.push(s.column.clone()),
                None => {}
            }
        }
        out
    }

    /// The tile request to store for `shown`, a request changed in a tile
    /// that the switchers in `applied` (as [`SwitchedReport::applied`]
    /// lists them) had switched: each switch undone, last first, so the tile
    /// keeps reading the switchers' own columns.
    #[must_use]
    pub fn unswitched_report(shown: &SavedReport, table: &DataTable, applied: &[(String, String)]) -> SavedReport {
        applied
            .iter()
            .rev()
            .fold(shown.clone(), |r, (column, current)| substitute_column(&r, table, current, column).unwrap_or(r))
    }

    /// The distinct tables the report tiles use, in tile order.
    #[must_use]
    pub fn tables(&self) -> Vec<&TableId> {
        let mut out: Vec<&TableId> = Vec::new();
        for t in self.tiles.iter().filter_map(|t| t.content.table()) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
        out
    }

    /// The report requests of tiles bound to `table`, for example to tell
    /// which of its columns the dashboard uses ([`SavedReport::columns`],
    /// [`SavedReport::column_names`]) before a column is deleted.
    pub fn reports_on<'a>(&'a self, table: &'a TableId) -> impl Iterator<Item = &'a SavedReport> + 'a {
        self.tiles.iter().filter_map(move |t| match &t.content {
            TileContent::Report { table: t, report } if t == table => Some(&**report),
            _ => None,
        })
    }

    /// Follow a column of `table` renamed from `old` to `new` in the report
    /// tiles bound to it (graphs name their columns; other requests use
    /// positions). Filters are keyed by name across tables and are left
    /// as they are.
    pub fn rename_column(&mut self, table: &TableId, old: &str, new: &str) {
        for t in &mut self.tiles {
            if let TileContent::Report { table: t, report } = &mut t.content
                && t == table
            {
                report.rename_column(old, new);
            }
        }
    }

    /// Check that `area` has a non-zero span, fits in the grid and overlaps
    /// no tile other than `ignore`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument)
    /// naming the problem.
    pub fn check_area(&self, area: GridArea, ignore: Option<TileId>) -> Result<()> {
        self.check_area_among(area, self.tiles.iter().filter(|t| Some(t.id) != ignore))
    }

    /// [`Dashboard::check_area`] against the tiles `others` only.
    fn check_area_among<'a>(&self, area: GridArea, mut others: impl Iterator<Item = &'a DashboardTile>) -> Result<()> {
        if area.width == 0 || area.height == 0 {
            return Err(invalid("a tile spans at least one column and one row"));
        }
        if u64::from(area.column) + u64::from(area.width) > u64::from(self.grid_columns) {
            return Err(invalid(format!("the tile does not fit in the dashboard's {} columns", self.grid_columns)));
        }
        if area.bottom() > u64::from(MAX_GRID_ROWS) {
            return Err(invalid(format!("a tile must end within the first {MAX_GRID_ROWS} rows")));
        }
        if let Some(other) = others.find(|t| t.area.overlaps(&area)) {
            return Err(invalid(format!("the tile would overlap {}", other.id)));
        }
        Ok(())
    }

    /// The first free area of `width` by `height` cells, scanning rows from
    /// the top and columns from the left (`width` is limited to the grid
    /// width, and both spans are at least 1). Below the last tile there is
    /// always room.
    #[must_use]
    pub fn free_area(&self, width: u32, height: u32) -> GridArea {
        let width = width.clamp(1, self.grid_columns.max(1));
        let height = height.max(1);
        let last = self.tiles.iter().map(|t| t.area.bottom()).max().unwrap_or(0);
        let last = u32::try_from(last).unwrap_or(MAX_GRID_ROWS);
        for row in 0..=last {
            for column in 0..=self.grid_columns.saturating_sub(width) {
                let area = GridArea::new(column, row, width, height);
                if !self.tiles.iter().any(|t| t.area.overlaps(&area)) {
                    return area;
                }
            }
        }
        GridArea::new(0, last, width, height)
    }

    /// The edit that adds a tile showing `content` at the end of the reading
    /// order, with a new id, in the first free area of `width` by `height`
    /// cells ([`Dashboard::free_area`]).
    #[must_use]
    pub fn add_tile_edit(&self, content: TileContent, width: u32, height: u32) -> DashboardEdit {
        let tile = DashboardTile { id: self.unused_tile_id(), area: self.free_area(width, height), content };
        DashboardEdit::InsertTile { index: self.tiles.len(), tile }
    }

    /// The dashboard's filters as they apply to `table`: a filter on a
    /// column the table has applies (rows must pass every one, as
    /// [`FilterCondition::matches`] tests them); a filter on a column it
    /// lacks is skipped.
    #[must_use]
    pub fn table_filter(&self, table: &DataTable) -> TableFilter {
        let mut mask = vec![true; table.n_rows()];
        let (mut applied, mut skipped) = (Vec::new(), Vec::new());
        for f in &self.filters {
            if table.find(f.column()).is_none() {
                skipped.push(f.column().to_owned());
                continue;
            }
            applied.push(f.column().to_owned());
            for (m, pass) in mask.iter_mut().zip(f.matches(table)) {
                *m &= pass;
            }
        }
        TableFilter { mask, applied, skipped }
    }

    /// A copy of `rows` (the row states of `table`) with every row outside
    /// the dashboard's filters also excluded and hidden, so analyses leave
    /// it out and plots do not draw it; selection and colours are kept.
    /// `rows` itself is not changed. Returns the copy and the filter.
    #[must_use]
    pub fn filter_rows(&self, table: &DataTable, rows: &RowStateManager) -> (RowStateManager, TableFilter) {
        let filter = self.table_filter(table);
        let mut out = rows.clone();
        let outside: Vec<usize> = filter.mask.iter().enumerate().filter(|(_, m)| !**m).map(|(r, _)| r).collect();
        if !outside.is_empty() {
            out.set_excluded(outside.iter().copied(), true);
            out.set_hidden(outside, true);
        }
        (out, filter)
    }

    /// Check ids, grid width, tile areas, filters and switchers.
    fn validate(&self) -> Result<()> {
        let context = format!("dashboards {:?}", self.id.0);
        if self.id.0.is_empty() {
            return Err(invalid("dashboards: a dashboard id must not be empty"));
        }
        if !(1..=MAX_GRID_COLUMNS).contains(&self.grid_columns) {
            return Err(invalid(format!("{context}: grid_columns must be 1 to {MAX_GRID_COLUMNS}")));
        }
        for (i, t) in self.tiles.iter().enumerate() {
            if t.id.0 == 0 {
                return Err(invalid(format!("{context}: tile ids are positive numbers")));
            }
            if self.tiles[..i].iter().any(|u| u.id == t.id) {
                return Err(invalid(format!("{context}: the id {} is used twice", t.id.0)));
            }
            self.check_area_among(t.area, self.tiles[..i].iter())
                .map_err(|e| invalid(format!("{context}, {}: {e}", t.id)))?;
        }
        for (i, f) in self.filters.iter().enumerate() {
            check_condition(f).map_err(|e| invalid(format!("{context}: {e}")))?;
            if self.filters[..i].iter().any(|g| g.column() == f.column()) {
                return Err(invalid(format!("{context}: two filters on {:?}", f.column())));
            }
        }
        for (i, s) in self.switchers.iter().enumerate() {
            s.check().map_err(|e| invalid(format!("{context}: {e}")))?;
            if self.switchers[..i].iter().any(|t| t.column == s.column) {
                return Err(invalid(format!("{context}: two column pickers on {:?}", s.column)));
            }
        }
        Ok(())
    }
}

/// Whether `report` (a request on `table`) reads column `name`.
fn reads_column(report: &SavedReport, table: &DataTable, name: &str) -> bool {
    let inner = report.unscoped();
    inner.column_names().contains(&name) || table.find(name).is_some_and(|c| inner.columns().contains(&c))
}

/// A condition must have finite limits (so it is written and read back
/// exactly).
fn check_condition(c: &FilterCondition) -> Result<()> {
    match c {
        FilterCondition::Range { min, max, .. } if !min.is_finite() || !max.is_finite() => {
            Err(invalid(format!("the filter on {:?} needs finite limits", c.column())))
        }
        _ => Ok(()),
    }
}

// ----------------------------------------------------------------------------
// Edits
// ----------------------------------------------------------------------------

/// One reversible change to a dashboard. [`DashboardEdit::apply`] returns
/// the edit that undoes it.
#[derive(Debug, Clone, PartialEq)]
pub enum DashboardEdit {
    /// Insert `tile` at `index` of the reading order (at most the number of
    /// tiles). Its id must be unused and its area free
    /// ([`Dashboard::add_tile_edit`] makes one).
    InsertTile {
        /// Position in [`Dashboard::tiles`].
        index: usize,
        /// The tile.
        tile: DashboardTile,
    },
    /// Remove a tile.
    RemoveTile {
        /// Which tile.
        tile: TileId,
    },
    /// Move a tile's top-left cell, keeping its span.
    MoveTile {
        /// Which tile.
        tile: TileId,
        /// New leftmost column.
        column: u32,
        /// New top row.
        row: u32,
    },
    /// Change a tile's span, keeping its top-left cell.
    ResizeTile {
        /// Which tile.
        tile: TileId,
        /// Columns spanned.
        width: u32,
        /// Rows spanned.
        height: u32,
    },
    /// Replace what a tile shows (for example a report's display options).
    SetTileContent {
        /// Which tile.
        tile: TileId,
        /// The new content.
        content: TileContent,
    },
    /// Set the filter on `condition`'s column: replaces the filter on that
    /// column in place, or adds it at the end.
    SetFilter {
        /// The condition.
        condition: FilterCondition,
    },
    /// Insert a filter at `index` (its column must not have one yet).
    InsertFilter {
        /// Position in [`Dashboard::filters`].
        index: usize,
        /// The condition.
        condition: FilterCondition,
    },
    /// Remove the filter on a column.
    RemoveFilter {
        /// The column name.
        column: String,
    },
    /// Set the switcher on `switcher`'s column (for example to show another
    /// column): replaces the switcher on that column in place, or adds it at
    /// the end.
    SetSwitcher {
        /// The switcher.
        switcher: SharedSwitcher,
    },
    /// Insert a switcher at `index` (its column must not have one yet).
    InsertSwitcher {
        /// Position in [`Dashboard::switchers`].
        index: usize,
        /// The switcher.
        switcher: SharedSwitcher,
    },
    /// Remove the switcher on a column.
    RemoveSwitcher {
        /// The column name.
        column: String,
    },
    /// Rename the dashboard.
    Rename {
        /// The new name.
        name: String,
    },
}

impl DashboardEdit {
    /// A short description for undo menus, e.g. `"Move tile"`.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::InsertTile { .. } => "Add tile".to_owned(),
            Self::RemoveTile { .. } => "Remove tile".to_owned(),
            Self::MoveTile { .. } => "Move tile".to_owned(),
            Self::ResizeTile { .. } => "Resize tile".to_owned(),
            Self::SetTileContent { .. } => "Change tile".to_owned(),
            Self::SetFilter { condition } => format!("Filter {}", condition.column()),
            Self::InsertFilter { condition, .. } => format!("Add filter on {}", condition.column()),
            Self::RemoveFilter { column } => format!("Remove filter on {column}"),
            Self::SetSwitcher { switcher } => format!("Switch {}", switcher.column),
            Self::InsertSwitcher { switcher, .. } => format!("Add picker on {}", switcher.column),
            Self::RemoveSwitcher { column } => format!("Remove picker on {column}"),
            Self::Rename { .. } => "Rename dashboard".to_owned(),
        }
    }

    /// Apply the edit to `dashboard` and return its inverse.
    ///
    /// Tile tables are not checked here (a dashboard does not know its
    /// project's tables); [`ProjectFile::edit_dashboard`] checks them.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument)
    /// for an unknown tile or filter column, a used tile id, an area that
    /// has no span, leaves the grid or overlaps another tile, an index past
    /// the end, a second filter or switcher on one column, a range filter
    /// with an infinite or missing limit, or a switcher that does not offer
    /// its own column or the one it shows. On error the dashboard is
    /// unchanged.
    pub fn apply(self, dashboard: &mut Dashboard) -> Result<Self> {
        match self {
            Self::InsertTile { index, tile } => {
                if index > dashboard.tiles.len() {
                    return Err(invalid("the tile position is past the end"));
                }
                if tile.id.0 == 0 || dashboard.tile(tile.id).is_some() {
                    return Err(invalid(format!("{} is already used", tile.id)));
                }
                dashboard.check_area(tile.area, None)?;
                let id = tile.id;
                dashboard.tiles.insert(index, tile);
                Ok(Self::RemoveTile { tile: id })
            }
            Self::RemoveTile { tile } => {
                let index = dashboard.tile_index(tile)?;
                let tile = dashboard.tiles.remove(index);
                Ok(Self::InsertTile { index, tile })
            }
            Self::MoveTile { tile, column, row } => {
                let index = dashboard.tile_index(tile)?;
                let old = dashboard.tiles[index].area;
                let area = GridArea { column, row, ..old };
                dashboard.check_area(area, Some(tile))?;
                dashboard.tiles[index].area = area;
                Ok(Self::MoveTile { tile, column: old.column, row: old.row })
            }
            Self::ResizeTile { tile, width, height } => {
                let index = dashboard.tile_index(tile)?;
                let old = dashboard.tiles[index].area;
                let area = GridArea { width, height, ..old };
                dashboard.check_area(area, Some(tile))?;
                dashboard.tiles[index].area = area;
                Ok(Self::ResizeTile { tile, width: old.width, height: old.height })
            }
            Self::SetTileContent { tile, content } => {
                let index = dashboard.tile_index(tile)?;
                let old = std::mem::replace(&mut dashboard.tiles[index].content, content);
                Ok(Self::SetTileContent { tile, content: old })
            }
            Self::SetFilter { condition } => {
                check_condition(&condition)?;
                match dashboard.filters.iter().position(|f| f.column() == condition.column()) {
                    Some(i) => {
                        let old = std::mem::replace(&mut dashboard.filters[i], condition);
                        Ok(Self::SetFilter { condition: old })
                    }
                    None => {
                        let column = condition.column().to_owned();
                        dashboard.filters.push(condition);
                        Ok(Self::RemoveFilter { column })
                    }
                }
            }
            Self::InsertFilter { index, condition } => {
                check_condition(&condition)?;
                if index > dashboard.filters.len() {
                    return Err(invalid("the filter position is past the end"));
                }
                if dashboard.filter(condition.column()).is_some() {
                    return Err(invalid(format!("{:?} already has a filter", condition.column())));
                }
                let column = condition.column().to_owned();
                dashboard.filters.insert(index, condition);
                Ok(Self::RemoveFilter { column })
            }
            Self::RemoveFilter { column } => {
                let index = dashboard
                    .filters
                    .iter()
                    .position(|f| f.column() == column)
                    .ok_or_else(|| invalid(format!("there is no filter on {column:?}")))?;
                let condition = dashboard.filters.remove(index);
                Ok(Self::InsertFilter { index, condition })
            }
            Self::SetSwitcher { switcher } => {
                switcher.check()?;
                match dashboard.switchers.iter().position(|s| s.column == switcher.column) {
                    Some(i) => {
                        Ok(Self::SetSwitcher { switcher: std::mem::replace(&mut dashboard.switchers[i], switcher) })
                    }
                    None => {
                        let column = switcher.column.clone();
                        dashboard.switchers.push(switcher);
                        Ok(Self::RemoveSwitcher { column })
                    }
                }
            }
            Self::InsertSwitcher { index, switcher } => {
                switcher.check()?;
                if index > dashboard.switchers.len() {
                    return Err(invalid("the picker position is past the end"));
                }
                if dashboard.switcher(&switcher.column).is_some() {
                    return Err(invalid(format!("{:?} already has a column picker", switcher.column)));
                }
                let column = switcher.column.clone();
                dashboard.switchers.insert(index, switcher);
                Ok(Self::RemoveSwitcher { column })
            }
            Self::RemoveSwitcher { column } => {
                let index = dashboard
                    .switchers
                    .iter()
                    .position(|s| s.column == column)
                    .ok_or_else(|| invalid(format!("there is no column picker on {column:?}")))?;
                let switcher = dashboard.switchers.remove(index);
                Ok(Self::InsertSwitcher { index, switcher })
            }
            Self::Rename { name } => Ok(Self::Rename { name: std::mem::replace(&mut dashboard.name, name) }),
        }
    }
}

/// Undo and redo stacks of [`DashboardEdit`]s for one dashboard (at most
/// [`DASHBOARD_HISTORY_LIMIT`] undo steps; applying a new edit clears redo).
#[derive(Debug, Clone, Default)]
pub struct DashboardHistory {
    undo: Vec<(String, DashboardEdit)>,
    redo: Vec<(String, DashboardEdit)>,
}

impl DashboardHistory {
    /// Apply `edit` to `dashboard` and remember its inverse.
    ///
    /// # Errors
    /// As [`DashboardEdit::apply`]; nothing is recorded then.
    pub fn apply(&mut self, edit: DashboardEdit, dashboard: &mut Dashboard) -> Result<()> {
        let label = edit.label();
        let inverse = edit.apply(dashboard)?;
        self.undo.push((label, inverse));
        if self.undo.len() > DASHBOARD_HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(())
    }

    /// Label of the edit [`DashboardHistory::undo`] would undo.
    #[must_use]
    pub fn next_undo(&self) -> Option<&str> {
        self.undo.last().map(|(l, _)| l.as_str())
    }

    /// Label of the edit [`DashboardHistory::redo`] would redo.
    #[must_use]
    pub fn next_redo(&self) -> Option<&str> {
        self.redo.last().map(|(l, _)| l.as_str())
    }

    /// Undo the latest edit; `None` when there is nothing to undo.
    pub fn undo(&mut self, dashboard: &mut Dashboard) -> Option<Result<()>> {
        let (label, edit) = self.undo.pop()?;
        Some(match edit.clone().apply(dashboard) {
            Ok(inverse) => {
                self.redo.push((label, inverse));
                Ok(())
            }
            Err(e) => {
                self.undo.push((label, edit));
                Err(e)
            }
        })
    }

    /// Redo the latest undone edit; `None` when there is nothing to redo.
    pub fn redo(&mut self, dashboard: &mut Dashboard) -> Option<Result<()>> {
        let (label, edit) = self.redo.pop()?;
        Some(match edit.clone().apply(dashboard) {
            Ok(inverse) => {
                self.undo.push((label, inverse));
                Ok(())
            }
            Err(e) => {
                self.redo.push((label, edit));
                Err(e)
            }
        })
    }

    /// Forget every step.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

// ----------------------------------------------------------------------------
// Project-level operations
// ----------------------------------------------------------------------------

impl ProjectFile {
    /// The dashboard with id `id`.
    #[must_use]
    pub fn dashboard(&self, id: &DashboardId) -> Option<&Dashboard> {
        self.dashboards.iter().find(|d| &d.id == id)
    }

    /// The dashboard with id `id`, to change directly (bypassing the checks
    /// of [`ProjectFile::edit_dashboard`]).
    pub fn dashboard_mut(&mut self, id: &DashboardId) -> Option<&mut Dashboard> {
        self.dashboards.iter_mut().find(|d| &d.id == id)
    }

    /// An id no dashboard uses yet: `dashboard-1`, `dashboard-2`, ….
    #[must_use]
    pub fn unused_dashboard_id(&self) -> DashboardId {
        (1..)
            .map(|i| DashboardId(format!("dashboard-{i}")))
            .find(|id| self.dashboard(id).is_none())
            .expect("there are fewer dashboards than ids")
    }

    /// Add an empty dashboard named `name` at the end and return its id.
    pub fn add_dashboard(&mut self, name: impl Into<String>) -> DashboardId {
        let id = self.unused_dashboard_id();
        self.dashboards.push(Dashboard::new(id.clone(), name));
        id
    }

    /// Remove dashboard `id`, returning its position and the dashboard so
    /// that [`ProjectFile::insert_dashboard`] can put it back (undo).
    pub fn remove_dashboard(&mut self, id: &DashboardId) -> Option<(usize, Dashboard)> {
        let index = self.dashboards.iter().position(|d| &d.id == id)?;
        Some((index, self.dashboards.remove(index)))
    }

    /// Insert `dashboard` at `index` (clamped to the end).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument)
    /// when its id is already used, or a tile names a table the project
    /// does not have, or the dashboard is malformed (see
    /// [`DashboardEdit::apply`] for the rules).
    pub fn insert_dashboard(&mut self, index: usize, dashboard: Dashboard) -> Result<()> {
        if self.dashboard(&dashboard.id).is_some() {
            return Err(invalid(format!("the dashboard id {:?} is already used", dashboard.id.0)));
        }
        dashboard.validate()?;
        for t in dashboard.tables() {
            self.check_table(t)?;
        }
        let index = index.min(self.dashboards.len());
        self.dashboards.insert(index, dashboard);
        Ok(())
    }

    fn check_table(&self, id: &TableId) -> Result<()> {
        self.table(id).map(|_| ()).ok_or_else(|| invalid(format!("the project has no table {id}")))
    }

    /// Apply `edit` to dashboard `id` and return its inverse, after checking
    /// that a tile it adds or changes is bound to one of the project's
    /// tables (a dashboard uses only its project's tables).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument)
    /// for an unknown dashboard or table, or anything
    /// [`DashboardEdit::apply`] rejects. On error nothing changes.
    pub fn edit_dashboard(&mut self, id: &DashboardId, edit: DashboardEdit) -> Result<DashboardEdit> {
        let content = match &edit {
            DashboardEdit::InsertTile { tile, .. } => Some(&tile.content),
            DashboardEdit::SetTileContent { content, .. } => Some(content),
            _ => None,
        };
        if let Some(table) = content.and_then(TileContent::table) {
            self.check_table(table)?;
        }
        let dashboard = self.dashboard_mut(id).ok_or_else(|| invalid(format!("the project has no dashboard {id}")))?;
        edit.apply(dashboard)
    }

    /// The tiles, as `(dashboard, tile)` ids, bound to table `table`: the
    /// application asks before removing a table they use.
    #[must_use]
    pub fn tiles_using_table(&self, table: &TableId) -> Vec<(DashboardId, TileId)> {
        self.dashboards
            .iter()
            .flat_map(|d| d.tiles.iter().filter(|t| t.content.table() == Some(table)).map(|t| (d.id.clone(), t.id)))
            .collect()
    }
}

// ----------------------------------------------------------------------------
// Computing tiles
// ----------------------------------------------------------------------------

/// A table's row states as a dashboard's tiles see them.
#[derive(Debug, Clone)]
pub struct FilteredTable {
    /// The table.
    pub table: TableId,
    /// Its row states with the rows outside the filters excluded and
    /// hidden ([`Dashboard::filter_rows`]); draw graph tiles with these.
    pub rows: RowStateManager,
    /// How the filters apply to it.
    pub filter: TableFilter,
}

/// What a tile shows once computed.
#[derive(Debug, Clone, PartialEq)]
pub enum TileOutput {
    /// A heading or note, as stored.
    Text {
        /// The text.
        text: String,
        /// How it is shown.
        style: TextStyle,
    },
    /// A report run on its table's filtered rows.
    Report {
        /// The table it ran on.
        table: TableId,
        /// The report's title and sections, or why it could not run (for
        /// example too few rows left by the filters, or a table the project
        /// no longer has).
        report: Result<Box<ComputedReport>>,
        /// Switchers that changed the request, as (switcher column, column
        /// shown); see [`SwitchedReport`].
        switched: Vec<(String, String)>,
    },
}

/// One computed tile.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputedTile {
    /// The tile's id.
    pub id: TileId,
    /// Where it sits.
    pub area: GridArea,
    /// What it shows.
    pub output: TileOutput,
}

/// Everything a dashboard shows: each table's filtered row states and each
/// tile's output, in tile order.
#[derive(Debug, Clone)]
pub struct DashboardResults {
    /// One entry per table the report tiles use (that the project has), in
    /// first-use order.
    pub tables: Vec<FilteredTable>,
    /// One entry per tile, in [`Dashboard::tiles`] order.
    pub tiles: Vec<ComputedTile>,
}

impl DashboardResults {
    /// The filtered row states of `table`.
    #[must_use]
    pub fn table(&self, table: &TableId) -> Option<&FilteredTable> {
        self.tables.iter().find(|t| &t.table == table)
    }

    /// The computed tile `id`.
    #[must_use]
    pub fn tile(&self, id: TileId) -> Option<&ComputedTile> {
        self.tiles.iter().find(|t| t.id == id)
    }
}

/// The filtered row states of `table` for `dashboard`, or `None` when the
/// project has no such table.
#[must_use]
pub fn filtered_table(file: &ProjectFile, dashboard: &Dashboard, table: &TableId) -> Option<FilteredTable> {
    let t = file.table(table)?;
    let (rows, filter) = dashboard.filter_rows(&t.table, &t.rows);
    Some(FilteredTable { table: table.clone(), rows, filter })
}

/// Compute one tile with `rows` as its table's filtered row states.
fn compute_with(
    file: &ProjectFile,
    dashboard: &Dashboard,
    tile: &DashboardTile,
    rows: Option<&RowStateManager>,
) -> ComputedTile {
    let output = match &tile.content {
        TileContent::Text { text, style } => TileOutput::Text { text: text.clone(), style: *style },
        TileContent::Report { table, report } => {
            let (result, switched) = match (file.table(table), rows) {
                (Some(t), Some(rows)) => {
                    let s = dashboard.switched_report(report, &t.table);
                    (run_report(&s.report, &t.table, rows).map(Box::new), s.applied)
                }
                _ => (Err(invalid(format!("the project has no table {table}"))), Vec::new()),
            };
            TileOutput::Report { table: table.clone(), report: result, switched }
        }
    };
    ComputedTile { id: tile.id, area: tile.area, output }
}

/// Run tile `id` of `dashboard` on its table's filtered rows (see
/// [`compute_dashboard`]); `None` when the dashboard has no such tile.
#[must_use]
pub fn compute_tile(file: &ProjectFile, dashboard: &Dashboard, id: TileId) -> Option<ComputedTile> {
    let tile = dashboard.tile(id)?;
    let filtered = tile.content.table().and_then(|t| filtered_table(file, dashboard, t));
    Some(compute_with(file, dashboard, tile, filtered.as_ref().map(|f| &f.rows)))
}

/// Run every tile of `dashboard`: each report tile's request, as the
/// switchers make it ([`Dashboard::switched_report`]), through
/// [`run_report`] on its table with the rows outside the dashboard's
/// filters excluded and hidden ([`Dashboard::filter_rows`]); text tiles as
/// stored. Each table's filter is evaluated once. Deterministic, like every
/// report: the same project and dashboard give the same results.
#[must_use]
pub fn compute_dashboard(file: &ProjectFile, dashboard: &Dashboard) -> DashboardResults {
    let tables: Vec<FilteredTable> =
        dashboard.tables().into_iter().filter_map(|t| filtered_table(file, dashboard, t)).collect();
    let by_id: HashMap<&TableId, &RowStateManager> = tables.iter().map(|t| (&t.table, &t.rows)).collect();
    let tiles = dashboard
        .tiles
        .iter()
        .map(|tile| compute_with(file, dashboard, tile, tile.content.table().and_then(|t| by_id.get(t).copied())))
        .collect();
    DashboardResults { tables, tiles }
}

// ----------------------------------------------------------------------------
// JSON
// ----------------------------------------------------------------------------

fn number(x: u32) -> JsonValue {
    JsonValue::Number(f64::from(x))
}

/// The `"dashboards"` array of a version-3 project file. Tiles bound to a
/// table not in `file` are left out (their requests name columns through
/// that table).
pub(crate) fn dashboards_json(file: &ProjectFile) -> JsonValue {
    let dashboards = file
        .dashboards
        .iter()
        .map(|d| {
            let filters = d.filters.iter().map(condition_to_json).collect();
            let tiles = d
                .tiles
                .iter()
                .filter_map(|t| {
                    let mut m = vec![
                        ("id".to_owned(), number(t.id.0)),
                        ("column".to_owned(), number(t.area.column)),
                        ("row".to_owned(), number(t.area.row)),
                        ("width".to_owned(), number(t.area.width)),
                        ("height".to_owned(), number(t.area.height)),
                    ];
                    match &t.content {
                        TileContent::Report { table, report } => {
                            let data = &file.table(table)?.table;
                            m.push(("table".to_owned(), s(table.as_str())));
                            m.push(("report".to_owned(), checked_report_to_json(report, data).ok()?));
                        }
                        TileContent::Text { text, style } => {
                            m.push(("text".to_owned(), s(text)));
                            m.push(("style".to_owned(), s(style.key())));
                        }
                    }
                    Some(JsonValue::Object(m))
                })
                .collect();
            let mut members = vec![
                ("id", s(d.id.as_str())),
                ("name", s(&d.name)),
                ("grid_columns", number(d.grid_columns)),
                ("filters", JsonValue::Array(filters)),
            ];
            // Written only when there are any, so older files save unchanged.
            if !d.switchers.is_empty() {
                let list = d
                    .switchers
                    .iter()
                    .map(|w| {
                        obj([
                            ("column", s(&w.column)),
                            ("choices", JsonValue::Array(w.choices.iter().map(|c| s(c)).collect())),
                            ("current", s(&w.current)),
                        ])
                    })
                    .collect();
                members.push(("switchers", JsonValue::Array(list)));
            }
            members.push(("tiles", JsonValue::Array(tiles)));
            JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
        })
        .collect();
    JsonValue::Array(dashboards)
}

fn read_u32(v: &JsonValue, key: &str, context: &str) -> Result<u32> {
    member(v, key)?
        .as_usize()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| invalid(format!("{context}: {key:?} must be a whole number")))
}

fn optional_list<'a>(v: &'a JsonValue, key: &str) -> Result<&'a [JsonValue]> {
    match v.get(key) {
        None | Some(JsonValue::Null) => Ok(&[]),
        Some(_) => array(v, key),
    }
}

/// Read the `"dashboards"` member of a version-3 document into `file`
/// (whose tables are already read), checking every dashboard. A report tile
/// whose request cannot be read (for example one naming a missing column) is
/// left out; the returned notes name each such tile and why.
pub(crate) fn read_dashboards(doc: &JsonValue, file: &mut ProjectFile) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    for d in optional_list(doc, "dashboards")? {
        let text = |key: &str| {
            member(d, key)?
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("dashboards: {key:?} must be text")))
        };
        let id = DashboardId(text("id")?);
        let context = format!("dashboards {:?}", id.0);
        let grid_columns = match d.get("grid_columns") {
            None | Some(JsonValue::Null) => DEFAULT_GRID_COLUMNS,
            Some(_) => read_u32(d, "grid_columns", &context)?,
        };
        let filters = optional_list(d, "filters")?
            .iter()
            .map(|f| condition_from_json(f, &format!("{context} filters")))
            .collect::<Result<_>>()?;
        let mut switchers = Vec::new();
        for w in optional_list(d, "switchers")? {
            let bad = || invalid(format!("{context} switchers: each needs a \"column\", \"choices\" and \"current\""));
            let name = |key: &str| w.get(key).and_then(JsonValue::as_str).map(str::to_owned).ok_or_else(bad);
            let choices = w
                .get("choices")
                .and_then(JsonValue::as_array)
                .ok_or_else(bad)?
                .iter()
                .map(|c| c.as_str().map(str::to_owned).ok_or_else(bad))
                .collect::<Result<Vec<_>>>()?;
            switchers.push(SharedSwitcher { column: name("column")?, choices, current: name("current")? });
        }
        let mut tiles = Vec::new();
        for t in optional_list(d, "tiles")? {
            let tile_context = format!("{context} tiles");
            let tile_id = TileId(read_u32(t, "id", &tile_context)?);
            let area = GridArea {
                column: read_u32(t, "column", &tile_context)?,
                row: read_u32(t, "row", &tile_context)?,
                width: read_u32(t, "width", &tile_context)?,
                height: read_u32(t, "height", &tile_context)?,
            };
            let content = if let Some(report) = t.get("report") {
                let table = t
                    .get("table")
                    .and_then(JsonValue::as_str)
                    .map(TableId::from)
                    .ok_or_else(|| invalid(format!("{tile_context}: a report tile needs a \"table\" id")))?;
                let data = &file
                    .table(&table)
                    .ok_or_else(|| invalid(format!("{tile_context}: the project has no table {table}")))?
                    .table;
                match report_from_json(report, data) {
                    Ok(report) => TileContent::Report { report: Box::new(report), table },
                    Err(e) => {
                        // Leave the tile out; the rest of the dashboard opens.
                        notes.push(format!(
                            "Dashboard {}: tile {} could not be restored and was left out: {e}",
                            id.0, tile_id.0
                        ));
                        continue;
                    }
                }
            } else {
                let text = member(t, "text")?
                    .as_str()
                    .ok_or_else(|| invalid(format!("{tile_context}: \"text\" must be text")))?
                    .to_owned();
                let style = match t.get("style") {
                    None | Some(JsonValue::Null) => TextStyle::default(),
                    Some(v) => v
                        .as_str()
                        .and_then(TextStyle::from_key)
                        .ok_or_else(|| invalid(format!("{tile_context}: \"style\" must be \"heading\" or \"body\"")))?,
                };
                TileContent::Text { text, style }
            };
            tiles.push(DashboardTile { id: tile_id, area, content });
        }
        let dashboard = Dashboard { id, name: text("name")?, grid_columns, tiles, filters, switchers };
        let end = file.dashboards.len();
        file.insert_dashboard(end, dashboard)?;
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{ProjectLayout, load_project_file, save_project, save_project_file};
    use crate::{DistributionOptions, DistributionRequest, LinearModelSpec, ReportOptions, SectionContent};
    use stat_core::descriptive::QuantileMethod;
    use stat_data::{Column, SelectionMode};

    fn sv(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    /// Plants: site (text), x, y (8 rows, row 7 excluded).
    fn plants() -> DataTable {
        DataTable::new(
            "Plants",
            vec![
                Column::text("site", sv(&["a", "b", "a", "b", "a", "b", "a", "b"])),
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]),
                Column::numeric("y", vec![2.1, 3.9, 6.2, 8.1, 9.8, 12.2, 14.1, 15.7]),
            ],
        )
        .unwrap()
    }

    /// Weather: no "site" column; x and temperature.
    fn weather() -> DataTable {
        DataTable::new(
            "Weather",
            vec![
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0, 5.0]),
                Column::numeric("temp", vec![10.0, 12.0, 11.0, 15.0, 14.0]),
            ],
        )
        .unwrap()
    }

    fn distribution(column: usize) -> SavedReport {
        SavedReport::Distribution {
            request: DistributionRequest { column, quantile_method: QuantileMethod::default(), censoring: None },
            options: DistributionOptions::default(),
        }
    }

    fn linear() -> SavedReport {
        SavedReport::Linear {
            spec: LinearModelSpec { response: 2, predictors: vec![1], effects: Vec::new(), splines: Vec::new() },
            options: ReportOptions::default(),
        }
    }

    /// A project with two tables and one dashboard holding a heading, a
    /// linear model and a distribution on Plants, and a distribution on
    /// Weather; filtered to site "a".
    fn project() -> (ProjectFile, DashboardId, [TableId; 2]) {
        let mut file = ProjectFile::default();
        let mut rows = RowStateManager::new(8);
        rows.set_excluded([7], true);
        rows.select([0, 3], SelectionMode::Replace);
        let p = file.add_table(plants(), rows);
        let w = file.add_table(weather(), RowStateManager::new(5));
        let d = file.add_dashboard("Overview");
        let tiles = [
            (TileContent::Text { text: "Growth".into(), style: TextStyle::Heading }, 12, 1),
            (TileContent::Report { table: p.clone(), report: Box::new(linear()) }, 6, 4),
            (TileContent::Report { table: p.clone(), report: Box::new(distribution(2)) }, 6, 4),
            (TileContent::Report { table: w.clone(), report: Box::new(distribution(1)) }, 6, 4),
        ];
        for (content, width, height) in tiles {
            let edit = file.dashboard(&d).unwrap().add_tile_edit(content, width, height);
            file.edit_dashboard(&d, edit).unwrap();
        }
        let filter = FilterCondition::Levels { column: "site".into(), levels: sv(&["a"]) };
        file.edit_dashboard(&d, DashboardEdit::SetFilter { condition: filter }).unwrap();
        (file, d, [p, w])
    }

    #[test]
    fn tiles_are_placed_in_the_first_free_area() {
        let (file, d, _) = project();
        let areas: Vec<GridArea> = file.dashboard(&d).unwrap().tiles.iter().map(|t| t.area).collect();
        assert_eq!(
            areas,
            [
                GridArea::new(0, 0, 12, 1),
                GridArea::new(0, 1, 6, 4),
                GridArea::new(6, 1, 6, 4),
                GridArea::new(0, 5, 6, 4)
            ]
        );
        let ids: Vec<u32> = file.dashboard(&d).unwrap().tiles.iter().map(|t| t.id.0).collect();
        assert_eq!(ids, [1, 2, 3, 4]);
        // A too-wide request is clamped to the grid.
        assert_eq!(file.dashboard(&d).unwrap().free_area(40, 2), GridArea::new(0, 9, 12, 2));
    }

    #[test]
    fn dashboards_round_trip() {
        let (mut file, d, [p, _]) = project();
        let range = FilterCondition::Range { column: "x".into(), min: 1.5, max: 6.25 };
        file.edit_dashboard(&d, DashboardEdit::SetFilter { condition: range }).unwrap();
        let second = file.add_dashboard("Empty");
        file.dashboard_mut(&second).unwrap().grid_columns = 8;
        file.dashboard_mut(&d).unwrap().tiles.push(DashboardTile {
            id: TileId(9),
            area: GridArea::new(6, 5, 6, 2),
            content: TileContent::Text { text: "Line one\nline \"two\"".into(), style: TextStyle::Body },
        });
        let text = save_project_file(&file);
        assert!(text.contains("\"dashboards\"") && text.contains("\"version\": 3"), "{text}");
        let back = load_project_file(&text).unwrap();
        assert_eq!(back.dashboards, file.dashboards);
        assert_eq!(save_project_file(&back), text, "saving again is stable");
        // The tiles are bound to the same tables and compute the same.
        assert_eq!(back.tiles_using_table(&p).len(), 2);
        let a = compute_dashboard(&file, file.dashboard(&d).unwrap());
        let b = compute_dashboard(&back, back.dashboard(&d).unwrap());
        assert_eq!(a.tiles, b.tiles);
    }

    /// The dashboard shown and dashboards in windows of their own are saved
    /// with the layout; entries naming a dashboard the file lacks are dropped.
    #[test]
    fn dashboard_layout_round_trips() {
        use crate::project::DashboardWindow;
        let (mut file, d, _) = project();
        file.layout.active_dashboard = Some(d.clone());
        file.layout.dashboard_windows =
            vec![DashboardWindow { dashboard: d.clone(), position: Some([40.0, 60.5]), size: Some([900.0, 700.0]) }];
        let text = save_project_file(&file);
        assert!(
            text.contains("\"active_dashboard\": \"dashboard-1\"") && text.contains("\"dashboard_windows\""),
            "{text}"
        );
        let back = load_project_file(&text).unwrap();
        assert_eq!(back.layout, file.layout);
        assert_eq!(save_project_file(&back), text, "saving again is stable");
        let unknown = text.replace("\"dashboard\": \"dashboard-1\"", "\"dashboard\": \"dashboard-9\"");
        let unknown = unknown.replace("\"active_dashboard\": \"dashboard-1\"", "\"active_dashboard\": \"dashboard-9\"");
        let back = load_project_file(&unknown).unwrap();
        assert!(back.layout.active_dashboard.is_none() && back.layout.dashboard_windows.is_empty());
    }

    #[test]
    fn a_project_without_dashboards_saves_as_before() {
        let sample = crate::project::load_project(include_str!("../../../examples/data/plant_growth.acraft")).unwrap();
        let file = ProjectFile::from(sample.clone());
        assert!(file.dashboards.is_empty());
        assert_eq!(save_project_file(&file), save_project(&sample), "one table: the old form, byte for byte");
        let (mut two, d, _) = project();
        two.remove_dashboard(&d).unwrap();
        let text = save_project_file(&two);
        assert!(!text.contains("dashboards"), "{text}");
        assert!(load_project_file(&text).unwrap().dashboards.is_empty());
        // A dashboard forces the version-3 form even for one table.
        let mut one = ProjectFile::from(sample);
        one.add_dashboard("D");
        assert!(one.layout == ProjectLayout::default());
        let text = save_project_file(&one);
        assert!(text.contains("\"version\": 3"));
        assert_eq!(load_project_file(&text).unwrap().dashboards.len(), 1);
    }

    #[test]
    fn a_filter_skips_tables_without_its_column() {
        let (file, d, [p, w]) = project();
        let dash = file.dashboard(&d).unwrap();
        let results = compute_dashboard(&file, dash);
        let plants = results.table(&p).unwrap();
        assert_eq!(plants.filter.mask, [true, false, true, false, true, false, true, false]);
        assert_eq!(plants.filter.applied, ["site"]);
        assert!(plants.filter.skipped.is_empty());
        let weather = results.table(&w).unwrap();
        assert!(weather.filter.mask.iter().all(|m| *m), "Weather has no site column");
        assert!(!weather.filter.is_active());
        assert_eq!(weather.filter.skipped, ["site"]);
        assert_eq!(weather.filter.rows_kept(), 5);
        // The weather tile is the report on all of its rows.
        let tile = results.tile(TileId(4)).unwrap();
        let wt = file.table(&w).unwrap();
        let TileOutput::Report { report, .. } = &tile.output else { panic!() };
        assert_eq!(**report.as_ref().unwrap(), run_report(&distribution(1), &wt.table, &wt.rows).unwrap());
        // A second filter on a column both tables have applies to both.
        let mut file = file;
        let range = FilterCondition::Range { column: "x".into(), min: 2.0, max: 5.0 };
        file.edit_dashboard(&d, DashboardEdit::SetFilter { condition: range }).unwrap();
        let results = compute_dashboard(&file, file.dashboard(&d).unwrap());
        assert_eq!(results.table(&p).unwrap().filter.mask, [false, false, true, false, true, false, false, false]);
        assert_eq!(results.table(&w).unwrap().filter.mask, [false, true, true, true, true]);
        assert_eq!(results.table(&w).unwrap().filter.applied, ["x"]);
    }

    #[test]
    fn tiles_equal_the_report_on_the_filtered_rows() {
        let (file, d, [p, _]) = project();
        let results = compute_dashboard(&file, file.dashboard(&d).unwrap());
        let source = file.table(&p).unwrap();
        // The table's own row states are untouched.
        assert_eq!(source.rows.included_rows(), [0, 1, 2, 3, 4, 5, 6]);
        // Equal to running the report with the filtered-out rows excluded.
        let mut by_hand = source.rows.clone();
        by_hand.set_excluded([1, 3, 5], true);
        by_hand.set_hidden([1, 3, 5, 7], true);
        let filtered = &results.table(&p).unwrap().rows;
        assert_eq!(filtered.included_rows(), [0, 2, 4, 6]);
        assert_eq!(filtered.visible_rows(), [0, 2, 4, 6]);
        assert_eq!(filtered.selected_rows(), [0, 3], "selection is kept for linked brushing");
        for id in [TileId(2), TileId(3)] {
            let TileOutput::Report { report, table, .. } = &results.tile(id).unwrap().output else { panic!() };
            assert_eq!(table, &p);
            let saved = &report.as_ref().unwrap().saved;
            assert_eq!(**report.as_ref().unwrap(), run_report(saved, &source.table, &by_hand).unwrap());
            // And its tables equal those of the report on a table holding
            // only the matching rows.
            let mut subset = source.table.clone();
            subset.remove_rows(&[1, 3, 5, 7]).unwrap();
            let alone = run_report(saved, &subset, &RowStateManager::new(4)).unwrap();
            // (Apart from the count of excluded rows, which the filtered
            // rows add to.)
            let tables = |r: &ComputedReport| -> Vec<crate::TextTable> {
                r.sections
                    .iter()
                    .filter_map(|s| match &s.content {
                        SectionContent::Table(t) => {
                            let mut t = t.clone();
                            t.rows.retain(|row| row.first().is_none_or(|c| c != "Rows excluded" && c != "Excluded"));
                            Some(t)
                        }
                        _ => None,
                    })
                    .collect()
            };
            assert!(!tables(&alone).is_empty());
            assert_eq!(tables(report.as_ref().unwrap()), tables(&alone), "{id}");
        }
        // compute_tile agrees with compute_dashboard; text tiles pass through.
        let dash = file.dashboard(&d).unwrap();
        assert_eq!(compute_tile(&file, dash, TileId(2)).as_ref(), results.tile(TileId(2)));
        assert_eq!(
            results.tile(TileId(1)).unwrap().output,
            TileOutput::Text { text: "Growth".into(), style: TextStyle::Heading }
        );
        assert!(compute_tile(&file, dash, TileId(77)).is_none());
        // A filter that leaves too few rows makes that tile an error only.
        let mut file = file;
        let none = FilterCondition::Levels { column: "site".into(), levels: Vec::new() };
        file.edit_dashboard(&d, DashboardEdit::SetFilter { condition: none }).unwrap();
        let results = compute_dashboard(&file, file.dashboard(&d).unwrap());
        let TileOutput::Report { report, .. } = &results.tile(TileId(2)).unwrap().output else { panic!() };
        assert!(report.is_err());
        let TileOutput::Report { report, .. } = &results.tile(TileId(4)).unwrap().output else { panic!() };
        assert!(report.is_ok(), "Weather is not filtered by site");
    }

    #[test]
    fn edits_undo_and_redo() {
        let (mut file, d, [p, _]) = project();
        let before = file.dashboard(&d).unwrap().clone();
        let mut history = DashboardHistory::default();
        let dash = file.dashboard_mut(&d).unwrap();
        let edits = vec![
            DashboardEdit::MoveTile { tile: TileId(4), column: 6, row: 5 },
            DashboardEdit::ResizeTile { tile: TileId(4), width: 6, height: 2 },
            DashboardEdit::RemoveTile { tile: TileId(2) },
            DashboardEdit::SetTileContent {
                tile: TileId(1),
                content: TileContent::Text { text: "Renamed".into(), style: TextStyle::Body },
            },
            DashboardEdit::SetFilter {
                condition: FilterCondition::Levels { column: "site".into(), levels: sv(&["b"]) },
            },
            DashboardEdit::SetFilter { condition: FilterCondition::Range { column: "x".into(), min: 0.0, max: 3.0 } },
            DashboardEdit::RemoveFilter { column: "site".into() },
            DashboardEdit::Rename { name: "Later".into() },
        ];
        let n = edits.len();
        for e in edits {
            history.apply(e, dash).unwrap();
        }
        let after = dash.clone();
        assert_eq!(after.tile(TileId(4)).unwrap().area, GridArea::new(6, 5, 6, 2));
        assert!(after.tile(TileId(2)).is_none());
        assert_eq!(after.filters, [FilterCondition::Range { column: "x".into(), min: 0.0, max: 3.0 }]);
        assert_eq!(after.name, "Later");
        assert_eq!(history.next_undo(), Some("Rename dashboard"));
        for _ in 0..n {
            history.undo(dash).unwrap().unwrap();
        }
        assert!(history.undo(dash).is_none());
        assert_eq!(*dash, before, "every edit undone");
        for _ in 0..n {
            history.redo(dash).unwrap().unwrap();
        }
        assert_eq!(*dash, after, "every edit redone");
        assert!(history.next_redo().is_none());
        // A new id after removal does not reuse the largest.
        assert_eq!(dash.unused_tile_id(), TileId(5));
        // Remove and put back a whole dashboard.
        let (index, removed) = file.remove_dashboard(&d).unwrap();
        file.insert_dashboard(index, removed.clone()).unwrap();
        assert!(file.insert_dashboard(0, removed).is_err(), "id already used");
        let _ = p;
    }

    #[test]
    fn invalid_edits_change_nothing() {
        let (mut file, d, [p, _]) = project();
        let before = file.dashboard(&d).unwrap().clone();
        let bad = [
            DashboardEdit::MoveTile { tile: TileId(4), column: 3, row: 2 }, // overlaps tiles 2 and 3
            DashboardEdit::MoveTile { tile: TileId(4), column: 7, row: 5 }, // leaves the grid
            DashboardEdit::ResizeTile { tile: TileId(2), width: 0, height: 4 },
            DashboardEdit::ResizeTile { tile: TileId(2), width: 6, height: MAX_GRID_ROWS },
            DashboardEdit::RemoveTile { tile: TileId(42) },
            DashboardEdit::RemoveFilter { column: "nope".into() },
            DashboardEdit::InsertFilter {
                index: 0,
                condition: FilterCondition::Levels { column: "site".into(), levels: Vec::new() },
            },
            DashboardEdit::SetFilter {
                condition: FilterCondition::Range { column: "x".into(), min: f64::NEG_INFINITY, max: 1.0 },
            },
            DashboardEdit::InsertTile {
                index: 0,
                tile: DashboardTile {
                    id: TileId(2),
                    area: GridArea::new(0, 20, 1, 1),
                    content: TileContent::Text { text: String::new(), style: TextStyle::Body },
                },
            },
            DashboardEdit::InsertTile {
                index: 9,
                tile: DashboardTile {
                    id: TileId(20),
                    area: GridArea::new(0, 20, 1, 1),
                    content: TileContent::Text { text: String::new(), style: TextStyle::Body },
                },
            },
        ];
        for e in bad {
            assert!(file.edit_dashboard(&d, e.clone()).is_err(), "{e:?}");
            assert_eq!(file.dashboard(&d).unwrap(), &before, "{e:?}");
        }
        // Tiles must use the project's own tables.
        let foreign = file.dashboard(&d).unwrap().add_tile_edit(
            TileContent::Report { table: TableId::from("elsewhere"), report: Box::new(distribution(0)) },
            4,
            4,
        );
        assert!(file.edit_dashboard(&d, foreign).unwrap_err().to_string().contains("elsewhere"));
        assert!(file.edit_dashboard(&DashboardId::from("x"), DashboardEdit::Rename { name: "n".into() }).is_err());
        let _ = p;
    }

    #[test]
    fn malformed_dashboards_are_rejected() {
        let (file, _, _) = project();
        let good = save_project_file(&file);
        let bad = |from: &str, to: &str| {
            assert!(good.contains(from), "{from}");
            load_project_file(&good.replacen(from, to, 1))
        };
        assert!(load_project_file(&good).is_ok());
        assert!(bad("\"table\": \"table-1\"", "\"table\": \"table-9\"").unwrap_err().to_string().contains("table-9"));
        assert!(bad("\"id\": 2,", "\"id\": 1,").unwrap_err().to_string().contains("twice"));
        assert!(bad("\"id\": 2,", "\"id\": 0,").is_err());
        assert!(bad("\"width\": 6", "\"width\": 7").unwrap_err().to_string().contains("overlap"));
        assert!(bad("\"width\": 12", "\"width\": 13").is_err());
        assert!(bad("\"height\": 1", "\"height\": -1").is_err());
        assert!(bad("\"grid_columns\": 12", "\"grid_columns\": 0").is_err());
        assert!(bad("\"style\": \"heading\"", "\"style\": \"title\"").is_err());
        assert!(bad("\"id\": \"dashboard-1\"", "\"id\": \"\"").is_err());
        assert!(bad("\"levels\": [", "\"levels\": 3, \"x\": [").is_err());
        let twice = good.replacen("\"filters\": [", "\"filters\": [{\"column\": \"site\", \"levels\": []}, ", 1);
        assert!(load_project_file(&twice).unwrap_err().to_string().contains("two filters"));
        // Optional members may be left out.
        let lean =
            good.replacen("\"grid_columns\": 12,", "", 1).replacen("\"style\": \"heading\"", "\"style\": null", 1);
        let back = load_project_file(&lean).unwrap();
        assert_eq!(back.dashboards[0].grid_columns, DEFAULT_GRID_COLUMNS);
        assert_eq!(
            back.dashboards[0].tiles[0].content,
            TileContent::Text { text: "Growth".into(), style: TextStyle::Body }
        );
    }

    /// The report tile `id` of `results`, unwrapped, and the switches made to it.
    fn report_tile(results: &DashboardResults, id: u32) -> (&ComputedReport, &[(String, String)]) {
        let TileOutput::Report { report, switched, .. } = &results.tile(TileId(id)).unwrap().output else { panic!() };
        (report.as_ref().unwrap(), switched)
    }

    #[test]
    fn a_shared_switcher_changes_the_column_in_every_tile_that_reads_it() {
        let (mut file, d, [p, w]) = project();
        let base = compute_dashboard(&file, file.dashboard(&d).unwrap());
        // Showing its own column, a switcher changes nothing.
        let on_y = SharedSwitcher::new("y", sv(&["x", "temp"]));
        assert_eq!(on_y.choices, sv(&["y", "x", "temp"]));
        file.edit_dashboard(&d, DashboardEdit::SetSwitcher { switcher: on_y.clone() }).unwrap();
        let same = compute_dashboard(&file, file.dashboard(&d).unwrap());
        assert_eq!(same.tiles, base.tiles);

        file.edit_dashboard(&d, DashboardEdit::SetSwitcher { switcher: on_y.showing("x") }).unwrap();
        let dash = file.dashboard(&d).unwrap();
        let results = compute_dashboard(&file, dash);
        let plants = file.table(&p).unwrap();
        let filtered = &results.table(&p).unwrap().rows;
        // Tile 3, the distribution of y, now shows x.
        let (r3, switched) = report_tile(&results, 3);
        assert_eq!(switched, [("y".to_owned(), "x".to_owned())]);
        assert_eq!(*r3, run_report(&distribution(1), &plants.table, filtered).unwrap());
        // Tile 2 (y by x) already reads x: left as it is, and said so.
        let (r2, switched) = report_tile(&results, 2);
        assert!(switched.is_empty());
        assert_eq!(r2, report_tile(&base, 2).0);
        let TileContent::Report { report, .. } = &dash.tile(TileId(2)).unwrap().content else { panic!() };
        assert_eq!(dash.switched_report(report, &plants.table).skipped, sv(&["y"]));
        // Weather has no y: untouched, not skipped.
        let (r4, switched) = report_tile(&results, 4);
        assert!(switched.is_empty() && r4 == report_tile(&base, 4).0);
        let TileContent::Report { report, .. } = &dash.tile(TileId(4)).unwrap().content else { panic!() };
        assert!(dash.switched_report(report, &file.table(&w).unwrap().table).skipped.is_empty());
        // The tiles' own requests are unchanged.
        assert_eq!(dash.tiles, project().0.dashboard(&d).unwrap().tiles);

        // A switcher whose choice only one table has switches that table's tiles.
        let on_temp = SharedSwitcher::new("temp", sv(&["x"])).showing("x");
        file.edit_dashboard(&d, DashboardEdit::SetSwitcher { switcher: on_temp }).unwrap();
        let results = compute_dashboard(&file, file.dashboard(&d).unwrap());
        let weather = file.table(&w).unwrap();
        let (r4, switched) = report_tile(&results, 4);
        assert_eq!(switched, [("temp".to_owned(), "x".to_owned())]);
        assert_eq!(*r4, run_report(&distribution(0), &weather.table, &results.table(&w).unwrap().rows).unwrap());
        // Undo puts the switchers back as they were, one step each.
        let mut history = DashboardHistory::default();
        let dash = file.dashboard_mut(&d).unwrap();
        history.apply(DashboardEdit::RemoveSwitcher { column: "y".into() }, dash).unwrap();
        assert_eq!(dash.switchers.len(), 1);
        history.undo(dash).unwrap().unwrap();
        assert_eq!(dash.switchers[0], on_y.showing("x"), "back in its place");
    }

    #[test]
    fn switched_requests_map_back_to_the_tile_request() {
        let t = plants();
        let dash = {
            let mut d = Dashboard::new(DashboardId::from("d"), "D");
            d.switchers.push(SharedSwitcher::new("y", sv(&["x"])).showing("x"));
            d
        };
        // A graph names its columns: every use of y becomes x.
        let graph = SavedReport::Graph {
            spec: Box::new(crate::GraphSpec {
                encoding: stat_viz::EncodingPlan {
                    x: vec!["site".into()],
                    y: vec!["y".into()],
                    color: Some("y".into()),
                    ..Default::default()
                },
                ..Default::default()
            }),
        };
        let s = dash.switched_report(&graph, &t);
        let SavedReport::Graph { spec } = &s.report else { panic!() };
        assert_eq!((spec.encoding.y.clone(), spec.encoding.color.clone()), (sv(&["x"]), Some("x".to_owned())));
        assert_eq!(Dashboard::unswitched_report(&s.report, &t, &s.applied), graph);
        // A report's own filter is not switched; its request is.
        let scoped = distribution(2).with_scope(crate::ReportScope {
            filter: vec![FilterCondition::Range { column: "y".into(), min: 0.0, max: 9.0 }],
            ..crate::ReportScope::default()
        });
        let s = dash.switched_report(&scoped, &t);
        assert_eq!(s.report.unscoped(), &distribution(1));
        assert_eq!(s.report.scope(), scoped.scope());
        assert_eq!(Dashboard::unswitched_report(&s.report, &t, &s.applied), scoped);
        // Nothing to substitute: missing columns, or the same column.
        assert!(substitute_column(&distribution(2), &t, "y", "nope").is_none());
        assert!(substitute_column(&distribution(2), &t, "x", "y").is_none(), "does not read x");
        assert!(substitute_column(&distribution(2), &t, "y", "y").is_none());
    }

    #[test]
    fn switchers_round_trip_and_bad_ones_are_rejected() {
        let (mut file, d, _) = project();
        let before = save_project_file(&file);
        assert!(!before.contains("switchers"), "no pickers: the file is as before");
        let s = SharedSwitcher::new("y", sv(&["x"])).showing("x");
        file.edit_dashboard(&d, DashboardEdit::SetSwitcher { switcher: s.clone() }).unwrap();
        let text = save_project_file(&file);
        assert!(text.contains("\"switchers\"") && text.contains("\"current\": \"x\""), "{text}");
        let back = load_project_file(&text).unwrap();
        assert_eq!(back.dashboards, file.dashboards);
        assert_eq!(save_project_file(&back), text, "saving again is stable");
        let bad = |from: &str, to: &str| {
            assert!(text.contains(from), "{from}");
            load_project_file(&text.replacen(from, to, 1)).unwrap_err().to_string()
        };
        assert!(bad("\"current\": \"x\"", "\"current\": \"z\"").contains("does not offer"));
        assert!(bad("\"current\": \"x\"", "\"current\": 3").contains("switchers"));
        // Invalid edits change nothing.
        let dash = file.dashboard(&d).unwrap().clone();
        for e in [
            DashboardEdit::SetSwitcher { switcher: SharedSwitcher { current: "q".into(), ..s.clone() } },
            DashboardEdit::SetSwitcher { switcher: SharedSwitcher { choices: sv(&["x", "x", "y"]), ..s.clone() } },
            DashboardEdit::InsertSwitcher { index: 0, switcher: s.clone() },
            DashboardEdit::InsertSwitcher { index: 5, switcher: SharedSwitcher::new("x", Vec::new()) },
            DashboardEdit::RemoveSwitcher { column: "nope".into() },
        ] {
            assert!(file.edit_dashboard(&d, e.clone()).is_err(), "{e:?}");
            assert_eq!(file.dashboard(&d).unwrap(), &dash, "{e:?}");
        }
        assert_eq!(DashboardEdit::SetSwitcher { switcher: s }.label(), "Switch y");
    }

    #[test]
    fn renaming_a_column_follows_graph_tiles_and_lists_reports() {
        let (mut file, d, [p, w]) = project();
        let dash = file.dashboard_mut(&d).unwrap();
        assert_eq!(dash.reports_on(&p).count(), 2);
        assert_eq!(dash.reports_on(&w).count(), 1);
        assert_eq!(dash.tables(), [&p, &w]);
        dash.rename_column(&p, "x", "dose"); // no graph tiles: nothing to follow
        assert_eq!(dash.reports_on(&p).flat_map(SavedReport::columns).collect::<Vec<_>>(), [2, 1, 2]);
    }
}
