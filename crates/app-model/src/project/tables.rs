//! Project files holding several tables, with derived-table lineage and a
//! window layout (format version 3; see the [parent module](super) for the
//! JSON form).
//!
//! A [`ProjectFile`] is a list of [`ProjectTable`]s, each with a stable
//! [`TableId`], plus the project's scripts and an optional
//! [`ProjectLayout`]. A table made by a table operation keeps a
//! [`TableLineage`]: the operation ([`TableOrigin`]), the ids of its source
//! tables within the project, and a [`DataFingerprint`] of each source as it
//! was when the table was made. Comparing those fingerprints with the
//! sources' current data ([`ProjectFile::origin_is_stale`]) tells whether a
//! source changed since; [`ProjectFile::rerun_origin`] re-runs the stored
//! operation on the current sources through [`TableOperation::run`].
//!
//! [`save_project_file`] writes a project with exactly one table, an empty
//! layout and no dashboards in the single-table form of [`save_project`] (byte for byte), so
//! builds that predate version 3 still open it; anything else is written in
//! the version-3 form. [`load_project_file`] reads both.

use std::collections::HashMap;

use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager, parse_json};

use super::{
    LoadedTable, PROJECT_FORMAT, PROJECT_VERSION, Project, ProjectScript, SavedReport, array, invalid, member, obj,
    read_scripts, read_table_members, s, save_project, scripts_json, table_members,
};
use crate::dashboard::{Dashboard, DashboardId};
use crate::external::ExternalSource;
use crate::table_ops::{DerivedDocument, Source, SubsetRows, TableOperation, TableOrigin};

/// Version written for project files in the several-table form. Version 3
/// keeps everything versions 1 and 2 hold (date-time columns included) and
/// adds `tables`, `source_tables`, `dashboards` and `layout`.
pub const PROJECT_FILE_VERSION: usize = 3;

/// A table's identifier within a project file: stable across saves and
/// independent of the table's (editable) name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TableId(pub String);

impl TableId {
    /// The identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TableId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for TableId {
    fn from(id: &str) -> Self {
        Self(id.to_owned())
    }
}

/// A 64-bit FNV-1a hash of a source table's contents, with the row states
/// the operation reads (see [`DataFingerprint::of_source`]).
///
/// FNV-1a (Fowler, Noll and Vo; IETF draft `draft-eastlake-fnv`) is not a
/// cryptographic hash: it detects edits, not tampering. It is computed the
/// same way on every platform and in every build, so a fingerprint saved in
/// a file stays comparable. Written in files as 16 lowercase hexadecimal
/// digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DataFingerprint(pub u64);

/// FNV-1a over byte slices fed in order.
struct Fnv(u64);

impl Fnv {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn len(&mut self, n: usize) {
        self.bytes(&(n as u64).to_le_bytes());
    }

    fn text(&mut self, t: &str) {
        self.len(t.len());
        self.bytes(t.as_bytes());
    }

    fn numbers(&mut self, v: &[f64]) {
        self.len(v.len());
        for &x in v {
            // Every NaN is the same missing value.
            let bits = if x.is_nan() { f64::NAN.to_bits() } else { x.to_bits() };
            self.bytes(&bits.to_le_bytes());
        }
    }
}

impl DataFingerprint {
    /// Fingerprint of `source` as `operation` reads it when `source` is the
    /// operation's primary table (`primary`) or one of its other tables.
    ///
    /// Covers the table's columns in order: each column's name, measurement
    /// level, formula, storage type (with a date-time column's display
    /// settings) and values, bit for bit (every missing number counts as the
    /// same). The table's name is left out, so renaming a source does not
    /// make a derived table stale. Row states count only where the
    /// operation's choice of rows depends on them: exclusion for a summary
    /// that leaves excluded rows out and for a subset of the included rows,
    /// selection for a subset of the selected rows (all on the primary
    /// table). Selection or colours merely carried over to derived rows do
    /// not count.
    #[must_use]
    pub fn of_source(operation: &TableOperation, source: Source<'_>, primary: bool) -> Self {
        let mut h = Fnv(Fnv::OFFSET);
        let table = source.table;
        h.len(table.n_cols());
        for c in table.columns() {
            h.text(&c.name);
            h.text(super::level_name(c.level));
            match &c.formula {
                Some(f) => {
                    h.bytes(&[1]);
                    h.text(f);
                }
                None => h.bytes(&[0]),
            }
            match &c.data {
                stat_data::ColumnData::Numeric(v) => {
                    h.bytes(b"n");
                    h.numbers(v);
                }
                stat_data::ColumnData::DateTime { values, display } => {
                    h.bytes(b"d");
                    h.text(display.format.key());
                    h.bytes(&[display.fraction_digits]);
                    h.bytes(&display.offset_minutes.to_le_bytes());
                    h.numbers(values);
                }
                stat_data::ColumnData::Text(v) => {
                    h.bytes(b"t");
                    h.len(v.len());
                    for t in v {
                        h.text(t);
                    }
                }
            }
        }
        let rows = |h: &mut Fnv, tag: u8, list: Vec<usize>| {
            h.bytes(&[tag]);
            h.len(source.rows.len());
            h.len(list.len());
            for r in list {
                h.len(r);
            }
        };
        if primary {
            match operation {
                TableOperation::Summarize { include_excluded: false, .. }
                | TableOperation::Subset { rows: SubsetRows::Included, .. } => {
                    rows(&mut h, b'x', source.rows.included_rows());
                }
                TableOperation::Subset { rows: SubsetRows::Selected, .. } => {
                    rows(&mut h, b's', source.rows.selected_rows());
                }
                _ => {}
            }
        }
        Self(h.0)
    }

    fn to_json(self) -> JsonValue {
        s(&format!("{:016x}", self.0))
    }

    fn from_json(v: &JsonValue) -> Result<Self> {
        v.as_str()
            .filter(|t| t.len() == 16)
            .and_then(|t| u64::from_str_radix(t, 16).ok())
            .map(Self)
            .ok_or_else(|| invalid("source_tables: a fingerprint must be 16 hexadecimal digits"))
    }
}

/// One source of a derived table, as it was when the table was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSnapshot {
    /// The source table's id within the project.
    pub table: TableId,
    /// Its fingerprint then ([`DataFingerprint::of_source`]).
    pub fingerprint: DataFingerprint,
}

/// How a table in a project was made: the operation and, for each of its
/// sources, where that source is in the project and what it held.
#[derive(Debug, Clone, PartialEq)]
pub struct TableLineage {
    /// The operation and its sources' names (primary table first).
    pub origin: TableOrigin,
    /// One entry per name in `origin.sources`, in the same order: the
    /// source's id and fingerprint, or `None` when that source is not in the
    /// project (it was never saved with it, or the file predates version 3).
    pub sources: Vec<Option<SourceSnapshot>>,
}

impl TableLineage {
    /// A lineage without project links (every source unknown), as an older
    /// one-table file records it.
    #[must_use]
    pub fn unlinked(origin: TableOrigin) -> Self {
        let sources = vec![None; origin.sources.len()];
        Self { origin, sources }
    }

    /// Record `origin` with its sources as they are now: `sources[i]` is the
    /// id and current contents of the `i`-th source (primary first), or
    /// `None` when that source is not part of the project.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument)
    /// when `sources` does not have one entry per source of `origin`.
    pub fn record(origin: TableOrigin, sources: &[Option<(&TableId, Source<'_>)>]) -> Result<Self> {
        if sources.len() != origin.sources.len() {
            return Err(invalid(format!(
                "the operation has {} source tables but {} were given",
                origin.sources.len(),
                sources.len()
            )));
        }
        let snapshots = sources
            .iter()
            .enumerate()
            .map(|(i, s)| {
                s.map(|(id, source)| SourceSnapshot {
                    table: id.clone(),
                    fingerprint: DataFingerprint::of_source(&origin.operation, source, i == 0),
                })
            })
            .collect();
        Ok(Self { origin, sources: snapshots })
    }
}

/// One table in a project file, with everything kept alongside it.
#[derive(Debug, Clone)]
pub struct ProjectTable {
    /// Stable identifier within the project.
    pub id: TableId,
    /// The data.
    pub table: DataTable,
    /// Selection, exclusion, hiding and colours.
    pub rows: RowStateManager,
    /// Analyses, in display order.
    pub reports: Vec<SavedReport>,
    /// The database or web source the table was read from, when it was
    /// (without credentials; see [`crate::external`]).
    pub external: Option<ExternalSource>,
    /// How the table was made, when a table operation made it.
    pub lineage: Option<TableLineage>,
}

impl ProjectTable {
    /// The table as a [`Source`] for table operations.
    #[must_use]
    pub fn source(&self) -> Source<'_> {
        Source { table: &self.table, rows: &self.rows }
    }

    /// This table alone as a one-table [`Project`] carrying `scripts`
    /// (source links are dropped; the operation is kept).
    #[must_use]
    pub fn to_project(&self, scripts: Vec<ProjectScript>) -> Project {
        Project {
            table: self.table.clone(),
            rows: self.rows.clone(),
            reports: self.reports.clone(),
            origin: self.lineage.as_ref().map(|l| l.origin.clone()),
            external: self.external.clone(),
            scripts,
        }
    }
}

/// How a table's document is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// The data grid.
    Data,
    /// The reports.
    Results,
    /// Data and reports side by side.
    SideBySide,
}

impl ViewMode {
    fn key(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Results => "results",
            Self::SideBySide => "side-by-side",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "data" => Self::Data,
            "results" => Self::Results,
            "side-by-side" => Self::SideBySide,
            _ => return None,
        })
    }
}

/// A table's view mode in the layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableView {
    /// The table.
    pub table: TableId,
    /// How it is shown.
    pub mode: ViewMode,
}

/// What a popped-out window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowContent {
    /// The table's data grid.
    Data,
    /// All of the table's reports.
    Results,
    /// One report, by its index in the table's reports.
    Report(usize),
}

/// A window popped out of the main window.
#[derive(Debug, Clone, PartialEq)]
pub struct PoppedOutWindow {
    /// The table whose content it shows.
    pub table: TableId,
    /// What it shows.
    pub content: WindowContent,
    /// Outer position of its top-left corner on screen, in points.
    pub position: Option<[f32; 2]>,
    /// Outer size, in points.
    pub size: Option<[f32; 2]>,
}

/// A dashboard shown in a window of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct DashboardWindow {
    /// The dashboard (see [`crate::dashboard`]).
    pub dashboard: DashboardId,
    /// Outer position of its top-left corner on screen, in points.
    pub position: Option<[f32; 2]>,
    /// Outer size, in points.
    pub size: Option<[f32; 2]>,
}

/// Plain data describing how the application had the project laid out.
/// Every part is optional; an empty layout means "use the defaults".
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProjectLayout {
    /// The table shown in the main window.
    pub active_table: Option<TableId>,
    /// View modes of tables that do not use the default.
    pub views: Vec<TableView>,
    /// Popped-out windows.
    pub windows: Vec<PoppedOutWindow>,
    /// The dashboard shown in the main window (in place of a table).
    pub active_dashboard: Option<DashboardId>,
    /// Dashboards shown in windows of their own.
    pub dashboard_windows: Vec<DashboardWindow>,
}

impl ProjectLayout {
    /// `true` when nothing is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.active_table.is_none()
            && self.views.is_empty()
            && self.windows.is_empty()
            && self.active_dashboard.is_none()
            && self.dashboard_windows.is_empty()
    }
}

/// A project: several tables, their scripts, dashboards and layout.
#[derive(Debug, Clone, Default)]
pub struct ProjectFile {
    /// The tables, in tab order.
    pub tables: Vec<ProjectTable>,
    /// Scripts kept with the project.
    pub scripts: Vec<ProjectScript>,
    /// How the windows were laid out.
    pub layout: ProjectLayout,
    /// Dashboards built from the project's tables, in tab order (see
    /// [`crate::dashboard`]).
    pub dashboards: Vec<Dashboard>,
    /// Free-text notes kept with the project (empty: none).
    pub notes: String,
    /// Script notebooks kept with the project (see [`crate::notebook`]).
    pub notebooks: Vec<crate::notebook::Notebook>,
    /// The name the user gave the project, shown instead of its file's name
    /// (`None`: the file's name). Optional on read, so older files open.
    pub name: Option<String>,
    /// Folders the user made in the project's tree (see [`super::folders`]).
    /// Optional on read, so older files open.
    pub folders: Vec<super::ProjectFolder>,
}

/// The result of [`ProjectFile::rerun_origin`].
#[derive(Debug, Clone)]
pub struct Rerun {
    /// The table made again from the current sources.
    pub document: DerivedDocument,
    /// Its lineage with the sources' current fingerprints.
    pub lineage: TableLineage,
}

impl From<Project> for ProjectFile {
    /// A one-table project file; the table gets the id `table-1`.
    fn from(project: Project) -> Self {
        let Project { table, rows, reports, origin, external, scripts } = project;
        let table = ProjectTable {
            id: TableId::from("table-1"),
            table,
            rows,
            reports,
            external,
            lineage: origin.map(TableLineage::unlinked),
        };
        Self { tables: vec![table], scripts, ..Self::default() }
    }
}

impl ProjectFile {
    /// The table with id `id`.
    #[must_use]
    pub fn table(&self, id: &TableId) -> Option<&ProjectTable> {
        self.tables.iter().find(|t| &t.id == id)
    }

    /// The table with id `id`, to change.
    pub fn table_mut(&mut self, id: &TableId) -> Option<&mut ProjectTable> {
        self.tables.iter_mut().find(|t| &t.id == id)
    }

    /// An id no table uses yet: `table-1`, `table-2`, ….
    #[must_use]
    pub fn unused_table_id(&self) -> TableId {
        (1..)
            .map(|i| TableId(format!("table-{i}")))
            .find(|id| self.table(id).is_none())
            .expect("there are fewer tables than ids")
    }

    /// Add `table` (with no lineage) under a new id and return the id.
    pub fn add_table(&mut self, table: DataTable, rows: RowStateManager) -> TableId {
        let id = self.unused_table_id();
        self.tables.push(ProjectTable {
            id: id.clone(),
            table,
            rows,
            reports: Vec::new(),
            external: None,
            lineage: None,
        });
        id
    }

    /// Add a table made by an operation from the project's tables `sources`
    /// (one id per source of `document.origin`, primary first, `None` for a
    /// source outside the project), recording the sources' current
    /// fingerprints, and return its id.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument)
    /// for an unknown id or a wrong number of sources.
    pub fn add_derived(&mut self, document: DerivedDocument, sources: &[Option<TableId>]) -> Result<TableId> {
        let lineage = self.record_lineage(document.origin, sources)?;
        let id = self.unused_table_id();
        self.tables.push(ProjectTable {
            id: id.clone(),
            table: document.table,
            rows: document.rows,
            reports: Vec::new(),
            external: None,
            lineage: Some(lineage),
        });
        Ok(id)
    }

    fn record_lineage(&self, origin: TableOrigin, sources: &[Option<TableId>]) -> Result<TableLineage> {
        let resolved = sources
            .iter()
            .map(|id| {
                id.as_ref()
                    .map(|id| {
                        let t = self.table(id).ok_or_else(|| invalid(format!("the project has no table {id}")))?;
                        Ok((id, t.source()))
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        TableLineage::record(origin, &resolved)
    }

    /// The ids of `id`'s sources whose current contents differ from what
    /// they held when `id` was made (by [`DataFingerprint::of_source`]).
    /// Empty when `id` is not a derived table, or when its sources are not
    /// in the project (nothing can be compared).
    #[must_use]
    pub fn stale_sources(&self, id: &TableId) -> Vec<TableId> {
        let Some(lineage) = self.table(id).and_then(|t| t.lineage.as_ref()) else {
            return Vec::new();
        };
        lineage
            .sources
            .iter()
            .enumerate()
            .filter_map(|(i, snap)| {
                let snap = snap.as_ref()?;
                let source = self.table(&snap.table)?;
                let now = DataFingerprint::of_source(&lineage.origin.operation, source.source(), i == 0);
                (now != snap.fingerprint).then(|| snap.table.clone())
            })
            .collect()
    }

    /// `true` when a source of table `id` changed since `id` was made (see
    /// [`ProjectFile::stale_sources`]).
    #[must_use]
    pub fn origin_is_stale(&self, id: &TableId) -> bool {
        !self.stale_sources(id).is_empty()
    }

    /// Run table `id`'s stored operation again on its sources as they are
    /// now, through [`TableOperation::run`], and return the new table (named
    /// like the current one) with an updated lineage. The project is not
    /// changed; the caller decides whether to replace the table.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument)
    /// when `id` is unknown or not a derived table, when a source is not in
    /// the project, or for any error of the operation (for example a column
    /// it names was renamed or deleted).
    pub fn rerun_origin(&self, id: &TableId) -> Result<Rerun> {
        let target = self.table(id).ok_or_else(|| invalid(format!("the project has no table {id}")))?;
        let lineage = target
            .lineage
            .as_ref()
            .ok_or_else(|| invalid(format!("{:?} was not made by a table operation", target.table.name())))?;
        let ids = lineage
            .sources
            .iter()
            .zip(&lineage.origin.sources)
            .map(|(snap, name)| {
                let snap =
                    snap.as_ref().ok_or_else(|| invalid(format!("the source table {name:?} is not in the project")))?;
                self.table(&snap.table)
                    .map(|_| Some(snap.table.clone()))
                    .ok_or_else(|| invalid(format!("the source table {name:?} is not in the project")))
            })
            .collect::<Result<Vec<_>>>()?;
        let sources: Vec<Source<'_>> =
            ids.iter().flatten().filter_map(|id| self.table(id)).map(ProjectTable::source).collect();
        let (primary, others) =
            sources.split_first().ok_or_else(|| invalid("the operation records no source table"))?;
        let document = lineage.origin.operation.run(*primary, others, target.table.name())?;
        let lineage = self.record_lineage(document.origin.clone(), &ids)?;
        Ok(Rerun { document, lineage })
    }

    /// Check that ids are unique and non-empty, that lineage links name other
    /// tables of the project and match their operation's source count.
    fn validate(&self) -> Result<()> {
        if self.tables.is_empty() {
            return Err(invalid("a project holds at least one table"));
        }
        for (i, t) in self.tables.iter().enumerate() {
            if t.id.0.is_empty() {
                return Err(invalid("tables: a table id must not be empty"));
            }
            if self.tables[..i].iter().any(|u| u.id == t.id) {
                return Err(invalid(format!("tables: the id {:?} is used twice", t.id.0)));
            }
        }
        for t in &self.tables {
            let Some(lineage) = &t.lineage else { continue };
            if lineage.sources.len() != lineage.origin.sources.len() {
                return Err(invalid(format!("tables {:?}: source_tables must have one entry per source", t.id.0)));
            }
            for snap in lineage.sources.iter().flatten() {
                if snap.table == t.id || self.table(&snap.table).is_none() {
                    return Err(invalid(format!(
                        "tables {:?}: source_tables names {:?}, which is not another table of the project",
                        t.id.0, snap.table.0
                    )));
                }
            }
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// JSON
// ----------------------------------------------------------------------------

fn point_json(p: [f32; 2]) -> JsonValue {
    JsonValue::Array(p.iter().map(|&x| JsonValue::number(f64::from(x))).collect())
}

fn read_point(v: Option<&JsonValue>, key: &str) -> Result<Option<[f32; 2]>> {
    match v {
        None | Some(JsonValue::Null) => Ok(None),
        Some(v) => match v.as_array() {
            // Points are stored from f32, so narrowing back is exact.
            #[allow(clippy::cast_possible_truncation)]
            Some([JsonValue::Number(x), JsonValue::Number(y)]) if x.is_finite() && y.is_finite() => {
                Ok(Some([*x as f32, *y as f32]))
            }
            _ => Err(invalid(format!("layout.windows: {key:?} must be two numbers"))),
        },
    }
}

fn read_id(v: &JsonValue, key: &str, context: &str) -> Result<TableId> {
    v.get(key)
        .and_then(JsonValue::as_str)
        .map(TableId::from)
        .ok_or_else(|| invalid(format!("{context}: {key:?} must be a table id")))
}

impl ProjectLayout {
    fn to_json(&self) -> JsonValue {
        let mut members = Vec::new();
        if let Some(active) = &self.active_table {
            members.push(("active_table".to_owned(), s(active.as_str())));
        }
        if !self.views.is_empty() {
            let views =
                self.views.iter().map(|v| obj([("table", s(v.table.as_str())), ("mode", s(v.mode.key()))])).collect();
            members.push(("views".to_owned(), JsonValue::Array(views)));
        }
        if !self.windows.is_empty() {
            let windows = self
                .windows
                .iter()
                .map(|w| {
                    let content = match w.content {
                        WindowContent::Data => s("data"),
                        WindowContent::Results => s("results"),
                        WindowContent::Report(i) => obj([("report", JsonValue::Number(i as f64))]),
                    };
                    let mut m = vec![("table".to_owned(), s(w.table.as_str())), ("view".to_owned(), content)];
                    if let Some(p) = w.position {
                        m.push(("position".to_owned(), point_json(p)));
                    }
                    if let Some(sz) = w.size {
                        m.push(("size".to_owned(), point_json(sz)));
                    }
                    JsonValue::Object(m)
                })
                .collect();
            members.push(("windows".to_owned(), JsonValue::Array(windows)));
        }
        if let Some(d) = &self.active_dashboard {
            members.push(("active_dashboard".to_owned(), s(d.as_str())));
        }
        if !self.dashboard_windows.is_empty() {
            let windows = self
                .dashboard_windows
                .iter()
                .map(|w| {
                    let mut m = vec![("dashboard".to_owned(), s(w.dashboard.as_str()))];
                    if let Some(p) = w.position {
                        m.push(("position".to_owned(), point_json(p)));
                    }
                    if let Some(sz) = w.size {
                        m.push(("size".to_owned(), point_json(sz)));
                    }
                    JsonValue::Object(m)
                })
                .collect();
            members.push(("dashboard_windows".to_owned(), JsonValue::Array(windows)));
        }
        JsonValue::Object(members)
    }

    /// Read a layout, dropping entries that name a table not in `file`, a
    /// report it does not have, or a dashboard it does not have. `skipped`
    /// lists, per table, the file positions of reports left out on reading
    /// (ascending): windows of later reports follow them to their new
    /// positions, and windows of the left-out reports are dropped.
    fn from_json(v: &JsonValue, file: &ProjectFile, skipped: &HashMap<TableId, Vec<usize>>) -> Result<Self> {
        if v.as_object().is_none() {
            return Err(invalid("layout must be an object"));
        }
        let known = |id: &TableId| file.table(id).is_some();
        let active_table = match v.get("active_table") {
            None | Some(JsonValue::Null) => None,
            Some(JsonValue::String(id)) => Some(TableId(id.clone())).filter(known),
            Some(_) => return Err(invalid("layout: \"active_table\" must be a table id")),
        };
        let list = |key: &str| match v.get(key) {
            None | Some(JsonValue::Null) => Ok(&[][..]),
            Some(_) => array(v, key),
        };
        let mut views = Vec::new();
        for e in list("views")? {
            let table = read_id(e, "table", "layout.views")?;
            let mode =
                e.get("mode").and_then(JsonValue::as_str).and_then(ViewMode::from_key).ok_or_else(|| {
                    invalid("layout.views: \"mode\" must be \"data\", \"results\" or \"side-by-side\"")
                })?;
            if known(&table) {
                views.push(TableView { table, mode });
            }
        }
        let mut windows = Vec::new();
        for e in list("windows")? {
            let table = read_id(e, "table", "layout.windows")?;
            let content = match e.get("view") {
                Some(JsonValue::String(k)) if k == "data" => WindowContent::Data,
                Some(JsonValue::String(k)) if k == "results" => WindowContent::Results,
                Some(r) if r.get("report").is_some() => {
                    let i = r
                        .get("report")
                        .and_then(JsonValue::as_usize)
                        .ok_or_else(|| invalid("layout.windows: \"report\" must be a report index"))?;
                    let left_out = skipped.get(&table).map_or(&[][..], Vec::as_slice);
                    if left_out.binary_search(&i).is_ok() {
                        continue;
                    }
                    WindowContent::Report(i - left_out.iter().filter(|&&k| k < i).count())
                }
                _ => {
                    return Err(invalid(
                        "layout.windows: \"view\" must be \"data\", \"results\" or {\"report\": index}",
                    ));
                }
            };
            let window = PoppedOutWindow {
                table,
                content,
                position: read_point(e.get("position"), "position")?,
                size: read_point(e.get("size"), "size")?,
            };
            let fits = file.table(&window.table).is_some_and(|t| match window.content {
                WindowContent::Report(i) => i < t.reports.len(),
                _ => true,
            });
            if fits {
                windows.push(window);
            }
        }
        let known_dashboard = |id: &DashboardId| file.dashboard(id).is_some();
        let active_dashboard = match v.get("active_dashboard") {
            None | Some(JsonValue::Null) => None,
            Some(JsonValue::String(id)) => Some(DashboardId(id.clone())).filter(known_dashboard),
            Some(_) => return Err(invalid("layout: \"active_dashboard\" must be a dashboard id")),
        };
        let mut dashboard_windows = Vec::new();
        for e in list("dashboard_windows")? {
            let dashboard = e
                .get("dashboard")
                .and_then(JsonValue::as_str)
                .map(DashboardId::from)
                .ok_or_else(|| invalid("layout.dashboard_windows: \"dashboard\" must be a dashboard id"))?;
            let window = DashboardWindow {
                dashboard,
                position: read_point(e.get("position"), "position")?,
                size: read_point(e.get("size"), "size")?,
            };
            if known_dashboard(&window.dashboard) {
                dashboard_windows.push(window);
            }
        }
        Ok(Self { active_table, views, windows, active_dashboard, dashboard_windows })
    }
}

/// Serialize a project file to JSON text.
///
/// A project with exactly one table, an empty layout, no dashboards, no
/// notebooks, folders or notes and no name is written exactly as [`save_project`] writes that table as a
/// [`Project`] (version 1 or 2), so older builds open it. Otherwise the
/// version-3 form is written, with the notes (when there are any) as the
/// string member `notes`. Source links that name a table not in the project are left out,
/// as are dashboard tiles bound to such a table.
#[must_use]
pub fn save_project_file(file: &ProjectFile) -> String {
    if file.tables.len() == 1
        && file.layout.is_empty()
        && file.dashboards.is_empty()
        && file.notes.is_empty()
        && file.notebooks.is_empty()
        && file.name.is_none()
        && file.folders.is_empty()
    {
        return save_project(&file.tables[0].to_project(file.scripts.clone()));
    }
    let tables = file
        .tables
        .iter()
        .map(|t| {
            let origin = t.lineage.as_ref().map(|l| &l.origin);
            let mut members = vec![("id".to_owned(), s(t.id.as_str()))];
            members.extend(table_members(&t.table, &t.rows, &t.reports, origin, t.external.as_ref()));
            if let Some(lineage) = &t.lineage {
                let links = lineage
                    .sources
                    .iter()
                    .map(|snap| match snap {
                        Some(snap) if snap.table != t.id && file.table(&snap.table).is_some() => {
                            obj([("id", s(snap.table.as_str())), ("fingerprint", snap.fingerprint.to_json())])
                        }
                        _ => JsonValue::Null,
                    })
                    .collect();
                members.push(("source_tables".to_owned(), JsonValue::Array(links)));
            }
            JsonValue::Object(members)
        })
        .collect();
    let mut members = vec![
        ("format".to_owned(), s(PROJECT_FORMAT)),
        ("version".to_owned(), JsonValue::Number(PROJECT_FILE_VERSION as f64)),
        ("tables".to_owned(), JsonValue::Array(tables)),
    ];
    if !file.scripts.is_empty() {
        members.push(("scripts".to_owned(), scripts_json(&file.scripts)));
    }
    if let Some(name) = &file.name {
        members.push(("name".to_owned(), s(name)));
    }
    if !file.notes.is_empty() {
        members.push(("notes".to_owned(), s(&file.notes)));
    }
    if !file.notebooks.is_empty() {
        members.push(("notebooks".to_owned(), crate::notebook::notebooks_json(&file.notebooks)));
    }
    if !file.folders.is_empty() {
        members.push(("folders".to_owned(), super::folders::folders_json(&file.folders)));
    }
    if !file.dashboards.is_empty() {
        members.push(("dashboards".to_owned(), crate::dashboard::dashboards_json(file)));
    }
    if !file.layout.is_empty() {
        members.push(("layout".to_owned(), file.layout.to_json()));
    }
    JsonValue::Object(members).to_pretty()
}

/// Parse a project file in either form: a version 1 or 2 single-table
/// project (which becomes a one-table [`ProjectFile`] whose table has the id
/// `table-1`), or a version-3 file written by [`save_project_file`].
///
/// # Errors
/// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument)
/// describing the first problem: invalid JSON, a file that is not a project,
/// a newer format version, no tables, a missing, empty or repeated table id,
/// malformed table, row, layout or dashboard data (an unreadable report or
/// dashboard tile is left out instead; see [`load_project_file_noting`]), notes that are
/// not text, a malformed
/// fingerprint, `source_tables` naming a table the project does not have, or
/// a dashboard that breaks its rules (see [`crate::dashboard`]).
pub fn load_project_file(text: &str) -> Result<ProjectFile> {
    load_project_file_noting(text).map(|(file, _)| file)
}

/// [`load_project_file`], also returning notes on what could not be
/// restored: reports and dashboard tiles that cannot be read are left out,
/// and formula columns on a cycle or that no longer compile keep their saved
/// values, each with a note naming it and why (see
/// [`load_project_noting`](super::load_project_noting)).
///
/// # Errors
/// As for [`load_project_file`], except that unreadable reports and tiles
/// are notes, not errors.
pub fn load_project_file_noting(text: &str) -> Result<(ProjectFile, Vec<String>)> {
    let doc = parse_json(text)?;
    if doc.get("format").and_then(JsonValue::as_str) != Some(PROJECT_FORMAT) {
        return Err(invalid("this file is not a statistical-engine project"));
    }
    let version = doc.get("version").and_then(JsonValue::as_usize).ok_or_else(|| invalid("missing version"))?;
    if version > PROJECT_FILE_VERSION {
        return Err(invalid(format!(
            "project version {version} is newer than this application supports ({PROJECT_FILE_VERSION})"
        )));
    }
    if version <= PROJECT_VERSION {
        return super::load_project_noting(text).map(|(project, notes)| (ProjectFile::from(project), notes));
    }
    from_version_3(&doc)
}

/// Read the body of a version-3 document (already checked to be one), with
/// notes on what could not be restored (reports and dashboard tiles left
/// out, formula columns that could not be computed).
pub(super) fn from_version_3(doc: &JsonValue) -> Result<(ProjectFile, Vec<String>)> {
    let mut tables = Vec::new();
    let mut notes_out = Vec::new();
    let mut skipped = HashMap::new();
    for t in array(doc, "tables")? {
        let id = read_id(t, "id", "tables")?;
        let LoadedTable { table, rows, reports, origin, external, notes, skipped_reports } = read_table_members(t)?;
        notes_out.extend(notes);
        if !skipped_reports.is_empty() {
            skipped.insert(id.clone(), skipped_reports);
        }
        let lineage = match origin {
            None => None,
            Some(origin) => {
                let sources = match t.get("source_tables") {
                    None | Some(JsonValue::Null) => vec![None; origin.sources.len()],
                    Some(_) => array(t, "source_tables")?
                        .iter()
                        .map(|l| match l {
                            JsonValue::Null => Ok(None),
                            l => Ok(Some(SourceSnapshot {
                                table: read_id(l, "id", "source_tables")?,
                                fingerprint: DataFingerprint::from_json(member(l, "fingerprint")?)?,
                            })),
                        })
                        .collect::<Result<_>>()?,
                };
                Some(TableLineage { origin, sources })
            }
        };
        tables.push(ProjectTable { id, table, rows, reports, external, lineage });
    }
    let notes = match doc.get("notes") {
        None | Some(JsonValue::Null) => String::new(),
        Some(v) => v.as_str().ok_or_else(|| invalid("project notes must be text"))?.to_owned(),
    };
    let name = match doc.get("name") {
        None | Some(JsonValue::Null) => None,
        Some(v) => Some(v.as_str().ok_or_else(|| invalid("the project name must be text"))?.to_owned()),
    };
    let notebooks = crate::notebook::read_notebooks(doc)?;
    let mut file =
        ProjectFile { tables, scripts: read_scripts(doc)?, notes, notebooks, name, ..ProjectFile::default() };
    file.validate()?;
    notes_out.extend(crate::dashboard::read_dashboards(doc, &mut file)?);
    file.layout = match doc.get("layout") {
        None | Some(JsonValue::Null) => ProjectLayout::default(),
        Some(v) => ProjectLayout::from_json(v, &file, &skipped)?,
    };
    file.folders = super::folders::read_folders(doc, &file, &skipped)?;
    Ok((file, notes_out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::load_project;
    use stat_core::descriptive::QuantileMethod;
    use stat_data::table_ops::Statistic;
    use stat_data::{Column, ColumnData, SelectionMode};

    fn sv(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    fn plants() -> DataTable {
        DataTable::new(
            "Plants",
            vec![
                Column::text("site", sv(&["b", "a", "b", "a"])),
                Column::numeric("before", vec![3.0, 1.0, 2.0, f64::NAN]),
                Column::numeric("after", vec![4.0, 2.0, 5.0, 3.0]),
            ],
        )
        .unwrap()
    }

    fn sites() -> DataTable {
        DataTable::new(
            "Sites",
            vec![Column::text("site", sv(&["a", "b"])), Column::text("soil", sv(&["clay", "sand"]))],
        )
        .unwrap()
    }

    fn summarize() -> TableOperation {
        TableOperation::Summarize {
            groups: sv(&["site"]),
            columns: sv(&["after"]),
            statistics: vec![Statistic::Mean, Statistic::Count],
            quantile_method: QuantileMethod::default(),
            include_excluded: false,
        }
    }

    /// Plants (with an excluded row), Sites, and Plants summarized, stacked
    /// and joined with Sites.
    fn project() -> (ProjectFile, [TableId; 5]) {
        let mut file = ProjectFile::default();
        let mut rows = RowStateManager::new(4);
        rows.set_excluded([1], true);
        rows.select([2], SelectionMode::Replace);
        let p = file.add_table(plants(), rows);
        let s_ = file.add_table(sites(), RowStateManager::new(2));
        let derive = |file: &mut ProjectFile, op: TableOperation, others: &[TableId], name: &str| {
            let doc = {
                let primary = file.table(&p).unwrap().source();
                let others: Vec<Source<'_>> = others.iter().map(|o| file.table(o).unwrap().source()).collect();
                op.run(primary, &others, name).unwrap()
            };
            let ids: Vec<Option<TableId>> = std::iter::once(&p).chain(others).cloned().map(Some).collect();
            file.add_derived(doc, &ids).unwrap()
        };
        let sum = derive(&mut file, summarize(), &[], "Plants summary");
        let stack =
            TableOperation::Stack { columns: sv(&["before", "after"]), label: "time".into(), value: "h".into() };
        let st = derive(&mut file, stack, &[], "Plants stacked");
        let join = TableOperation::Join {
            keys: vec![("site".into(), "site".into())],
            kind: stat_data::table_ops::JoinKind::Left,
            relationship: stat_data::table_ops::Relationship::ManyToOne,
        };
        let j = derive(&mut file, join, std::slice::from_ref(&s_), "Plants joined");
        (file, [p, s_, sum, st, j])
    }

    fn assert_same(a: &ProjectFile, b: &ProjectFile) {
        assert_eq!(a.tables.len(), b.tables.len());
        for (x, y) in a.tables.iter().zip(&b.tables) {
            assert_eq!(x.id, y.id);
            assert_eq!(x.table, y.table);
            assert_eq!(x.rows.len(), y.rows.len());
            for r in 0..x.rows.len() {
                assert_eq!(x.rows.state(r), y.rows.state(r));
            }
            assert_eq!(x.reports, y.reports);
            assert_eq!(x.external, y.external);
            assert_eq!(x.lineage, y.lineage);
        }
        assert_eq!(a.scripts, b.scripts);
        assert_eq!(a.layout, b.layout);
    }

    #[test]
    fn several_tables_round_trip_with_lineage_scripts_and_layout() {
        let (mut file, [p, s_, sum, _st, j]) = project();
        file.tables[0].reports.push(SavedReport::Distribution {
            request: crate::DistributionRequest { column: 2, quantile_method: QuantileMethod::default(), censoring: None },
            options: crate::DistributionOptions::default(),
        });
        file.scripts.push(ProjectScript { name: "Hello".into(), source: "print(1);".into(), run_on_open: true });
        file.layout = ProjectLayout {
            active_table: Some(sum.clone()),
            views: vec![
                TableView { table: p.clone(), mode: ViewMode::SideBySide },
                TableView { table: s_.clone(), mode: ViewMode::Results },
            ],
            windows: vec![
                PoppedOutWindow {
                    table: p.clone(),
                    content: WindowContent::Report(0),
                    position: Some([10.5, -20.0]),
                    size: Some([640.0, 480.25]),
                },
                PoppedOutWindow { table: j.clone(), content: WindowContent::Data, position: None, size: None },
                PoppedOutWindow {
                    table: s_.clone(),
                    content: WindowContent::Results,
                    position: Some([0.1, 0.2]),
                    size: None,
                },
            ],
            ..ProjectLayout::default()
        };
        let text = save_project_file(&file);
        assert!(text.contains("\"version\": 3"), "{text}");
        let back = load_project_file(&text).unwrap();
        assert_same(&file, &back);
        assert_eq!(save_project_file(&back), text, "saving again is stable");
        let lineage = back.table(&j).unwrap().lineage.as_ref().unwrap();
        assert_eq!(lineage.sources.len(), 2);
        assert_eq!(lineage.sources[1].as_ref().unwrap().table, s_);
        assert!(!back.origin_is_stale(&j));
        // The single-table reader explains instead of failing obscurely.
        assert!(load_project(&text).unwrap_err().to_string().contains("5 tables"));
    }

    #[test]
    fn one_table_without_layout_is_written_in_the_old_form() {
        let sample = load_project(include_str!("../../../../examples/data/plant_growth.acraft")).unwrap();
        let old = save_project(&sample);
        let file = ProjectFile::from(sample.clone());
        assert_eq!(save_project_file(&file), old, "byte for byte");
        // And it reads back as the same one-table project.
        let back = load_project_file(&old).unwrap();
        assert_eq!(back.tables.len(), 1);
        assert_eq!(back.tables[0].id, TableId::from("table-1"));
        assert_eq!(back.tables[0].table, sample.table);
        assert_eq!(back.tables[0].reports, sample.reports);
        // With a derived origin (no project links) and scripts too.
        let (full, ids) = project();
        let mut alone = full.table(&ids[2]).unwrap().clone();
        alone.id = TableId::from("table-1");
        let scripts = vec![ProjectScript { name: "s".into(), source: "1".into(), run_on_open: false }];
        let one = ProjectFile { tables: vec![alone.clone()], scripts: scripts.clone(), ..ProjectFile::default() };
        let text = save_project_file(&one);
        assert_eq!(text, save_project(&alone.to_project(scripts)));
        assert!(text.contains("\"version\": 1") && !text.contains("source_tables"));
        let back = load_project_file(&text).unwrap();
        let lineage = back.tables[0].lineage.as_ref().unwrap();
        assert_eq!(lineage.origin, alone.lineage.as_ref().unwrap().origin);
        assert!(lineage.sources.iter().all(Option::is_none));
        // A layout forces the new form even for one table.
        let laid = ProjectFile {
            layout: ProjectLayout { active_table: Some(TableId::from("table-1")), ..ProjectLayout::default() },
            ..one
        };
        assert!(save_project_file(&laid).contains("\"version\": 3"));
        // A one-table version-3 file still opens with the single-table reader.
        let single = load_project(&save_project_file(&laid)).unwrap();
        assert_eq!(single.table, alone.table);
    }

    #[test]
    fn notebooks_are_saved_with_the_project() {
        let sample = load_project(include_str!("../../../../examples/data/plant_growth.acraft")).unwrap();
        let mut file = ProjectFile::from(sample);
        let mut notebook = crate::notebook::Notebook::new("Growth checks");
        notebook.cells[1].outputs.push(crate::notebook::CellOutput::Printed("ok".into()));
        file.notebooks.push(notebook);
        let text = save_project_file(&file);
        assert!(text.contains("\"version\": 3") && text.contains("\"notebooks\""));
        assert_eq!(load_project_file(&text).unwrap().notebooks, file.notebooks);
        file.notebooks.clear();
        let text = save_project_file(&file);
        assert!(!text.contains("\"notebooks\"") && !text.contains("\"version\": 3"));
        assert!(load_project_file(&text).unwrap().notebooks.is_empty());
    }

    /// A name given to the project is saved (in the several-table form, even
    /// for one table) and read back; files without one read as unnamed.
    #[test]
    fn a_given_name_is_saved_with_the_project() {
        let sample = load_project(include_str!("../../../../examples/data/plant_growth.acraft")).unwrap();
        let mut file = ProjectFile::from(sample);
        file.name = Some("Pump trial \"B\"".to_owned());
        let text = save_project_file(&file);
        assert!(text.contains("\"version\": 3") && text.contains("\"name\": \"Pump trial"));
        assert_eq!(load_project_file(&text).unwrap().name, file.name);
        file.name = None;
        let text = save_project_file(&file);
        assert_eq!(load_project_file(&text).unwrap().name, None);
    }

    #[test]
    fn notes_are_saved_with_the_project() {
        let sample = load_project(include_str!("../../../../examples/data/plant_growth.acraft")).unwrap();
        let mut file = ProjectFile::from(sample);
        file.notes = "Second batch after the pump was replaced.\nCheck run 14 \"twice\".".to_owned();
        // Notes need the version-3 form, even for one table.
        let text = save_project_file(&file);
        assert!(text.contains("\"version\": 3") && text.contains("\"notes\""));
        assert_eq!(load_project_file(&text).unwrap().notes, file.notes);
        // No notes: nothing written, and files without the member read as none.
        file.notes.clear();
        let text = save_project_file(&file);
        assert!(!text.contains("\"notes\""));
        assert_eq!(load_project_file(&text).unwrap().notes, "");
        // Notes that are not text are refused.
        let mut doc = parse_json(&save_project_file(&ProjectFile { notes: "x".into(), ..file })).unwrap();
        if let JsonValue::Object(members) = &mut doc {
            for (k, v) in members.iter_mut() {
                if k == "notes" {
                    *v = JsonValue::Number(3.0);
                }
            }
        }
        assert!(load_project_file(&doc.to_pretty()).is_err());
    }

    #[test]
    fn every_sample_project_loads() {
        for text in [
            include_str!("../../../../examples/data/plant_growth.acraft"),
            include_str!("../../../../examples/data/customer_survey.acraft"),
            include_str!("../../../../examples/data/plant_growth_outliers_excluded.acraft"),
            include_str!("../../../../examples/data/leukemia_remission.acraft"),
        ] {
            let old = load_project(text).unwrap();
            let file = load_project_file(text).unwrap();
            assert_eq!(file.tables.len(), 1);
            assert_eq!(file.tables[0].table, old.table);
            assert_eq!(file.tables[0].reports, old.reports);
            assert_eq!(save_project_file(&file), save_project(&old));
        }
    }

    #[test]
    fn edits_to_a_source_make_its_derived_tables_stale() {
        let (mut file, [p, s_, sum, st, j]) = project();
        for id in [&p, &s_, &sum, &st, &j] {
            assert!(!file.origin_is_stale(id), "{id}");
        }
        // Renaming the source table does not count.
        let t = &mut file.table_mut(&p).unwrap().table;
        *t = DataTable::new("Renamed", t.columns().to_vec()).unwrap();
        assert!(!file.origin_is_stale(&sum));
        // Excluding a row matters to the summary only.
        file.table_mut(&p).unwrap().rows.set_excluded([0], true);
        assert!(file.origin_is_stale(&sum));
        assert!(!file.origin_is_stale(&st) && !file.origin_is_stale(&j));
        // Selecting rows matters to none of them.
        file.table_mut(&p).unwrap().rows.select([0, 1], SelectionMode::Replace);
        assert!(!file.origin_is_stale(&st));
        // Editing a cell of the other table makes the join stale.
        file.table_mut(&s_).unwrap().table.set_value(0, 1, stat_data::CellValue::Text("loam".into())).unwrap();
        assert_eq!(file.stale_sources(&j), vec![s_.clone()]);
        // Editing a value of the primary table makes all three stale, and
        // survives a save and reload.
        file.table_mut(&p).unwrap().table.set_value(3, 1, stat_data::CellValue::Number(7.0)).unwrap();
        let back = load_project_file(&save_project_file(&file)).unwrap();
        for id in [&sum, &st, &j] {
            assert!(back.origin_is_stale(id), "{id}");
        }
        assert_eq!(back.stale_sources(&j), vec![p.clone(), s_.clone()]);
    }

    #[test]
    fn rerun_reproduces_summarize_stack_and_join_results() {
        let (mut file, [p, s_, sum, st, j]) = project();
        for id in [&sum, &st, &j] {
            let rerun = file.rerun_origin(id).unwrap();
            let current = file.table(id).unwrap();
            assert_eq!(rerun.document.table, current.table, "{id}");
            assert_eq!(&rerun.lineage, current.lineage.as_ref().unwrap());
        }
        // After an edit, the re-run sees the new value and is no longer stale.
        file.table_mut(&p).unwrap().table.set_value(0, 2, stat_data::CellValue::Number(10.0)).unwrap();
        file.table_mut(&s_).unwrap().table.set_value(1, 1, stat_data::CellValue::Text("silt".into())).unwrap();
        for id in [&sum, &st, &j] {
            assert!(file.origin_is_stale(id));
            let rerun = file.rerun_origin(id).unwrap();
            assert_ne!(rerun.document.table, file.table(id).unwrap().table, "{id}");
            let target = file.table_mut(id).unwrap();
            target.table = rerun.document.table;
            target.rows = rerun.document.rows;
            target.lineage = Some(rerun.lineage);
            assert!(!file.origin_is_stale(id), "{id}");
        }
        // Group "b" (rows 0 and 2) now averages (10 + 5) / 2.
        let summary = &file.table(&sum).unwrap().table;
        let mean = summary.columns().iter().find_map(|c| match &c.data {
            ColumnData::Numeric(v) if c.name.contains("Mean") || c.name.contains("mean") => Some(v.clone()),
            _ => None,
        });
        assert!(mean.unwrap().contains(&7.5), "{summary:?}");
        let joined = &file.table(&j).unwrap().table;
        assert!(
            (0..joined.n_rows()).any(|r| joined.column(joined.find("soil").unwrap()).unwrap().cell_text(r) == "silt")
        );
        // Errors: not derived, unknown, source gone, column gone.
        assert!(file.rerun_origin(&p).is_err());
        assert!(file.rerun_origin(&TableId::from("nope")).is_err());
        let mut gone = file.clone();
        gone.tables.retain(|t| t.id != s_);
        assert!(gone.rerun_origin(&j).unwrap_err().to_string().contains("not in the project"));
        assert!(!gone.origin_is_stale(&j), "nothing to compare");
        let mut renamed = file.clone();
        renamed.table_mut(&p).unwrap().table.rename_column(2, "later").unwrap();
        assert!(renamed.rerun_origin(&sum).is_err());
    }

    #[test]
    fn malformed_files_are_rejected() {
        let (file, _) = project();
        let good = save_project_file(&file);
        let bad = |from: &str, to: &str| {
            assert!(good.contains(from), "{from}");
            load_project_file(&good.replacen(from, to, 1))
        };
        assert!(load_project_file("{").is_err());
        assert!(load_project_file(r#"{"format":"other","version":3}"#).is_err());
        assert!(
            load_project_file(r#"{"format":"statistical-engine-project","version":4,"tables":[]}"#)
                .unwrap_err()
                .to_string()
                .contains("newer")
        );
        assert!(load_project_file(r#"{"format":"statistical-engine-project","version":3,"tables":[]}"#).is_err());
        assert!(load_project_file(r#"{"format":"statistical-engine-project","version":3}"#).is_err());
        // Duplicate, missing and empty ids.
        assert!(bad("\"id\": \"table-2\"", "\"id\": \"table-1\"").unwrap_err().to_string().contains("twice"));
        assert!(bad("\"id\": \"table-2\"", "\"id\": \"\"").is_err());
        assert!(bad("\"id\": \"table-2\"", "\"name\": \"x\"").is_err());
        // Links to unknown tables, bad fingerprints, wrong source counts.
        let link = "\"source_tables\": [\n        {\n          \"id\": \"table-1\"";
        assert!(bad(link, &link.replace("table-1", "table-9")).unwrap_err().to_string().contains("table-9"));
        assert!(bad("\"fingerprint\": \"", "\"fingerprint\": \"zz").is_err());
        assert!(bad("\"source_tables\": [", "\"source_tables\": [null, ").is_err());
        // Layout values of the wrong kind.
        let mut laid = file.clone();
        laid.layout.views.push(TableView { table: TableId::from("table-1"), mode: ViewMode::Data });
        laid.layout.windows.push(PoppedOutWindow {
            table: TableId::from("table-1"),
            content: WindowContent::Data,
            position: Some([1.0, 2.0]),
            size: None,
        });
        let text = save_project_file(&laid);
        assert!(load_project_file(&text.replace("\"mode\": \"data\"", "\"mode\": \"grid\"")).is_err());
        assert!(load_project_file(&text.replace("\"view\": \"data\"", "\"view\": 3")).is_err());
        assert!(load_project_file(&text.replace("\"position\": [", "\"position\": [\"a\", ")).is_err());
        // Entries naming tables or reports that are not there are dropped.
        let dangling = text.replace("\"view\": \"data\"", "\"view\": {\"report\": 5}");
        let back = load_project_file(&dangling).unwrap();
        assert!(back.layout.windows.is_empty() && back.layout.views.len() == 1);
    }
}
