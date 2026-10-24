//! Folders the user makes in a project's tree, to group its tables,
//! reports (graph canvases included), dashboards and notebooks.
//!
//! A folder has a name, folders of its own and a set of items. Each item is
//! in at most one folder; items in no folder stay at the top of the project.
//! Folders change nothing about the items themselves: a report in a folder
//! still belongs to its table and re-runs with it.
//!
//! Folders are saved in the version-3 form as the optional member
//! `"folders"`:
//!
//! ```text
//! "folders": [
//!   { "name": "…",
//!     "items": [ {"table": "table-1"}
//!              | {"table": "table-1", "report": index}   (index in that table's "reports")
//!              | {"dashboard": "dashboard-1"}
//!              | {"notebook": "…"}, … ],                 (optional)
//!     "folders": [ … ] },                                (optional; folders inside)
//!   …
//! ]
//! ```
//!
//! On reading, items that name a table, report, dashboard or notebook the
//! project does not have are left out, as is a second mention of an item
//! already placed; report indices follow reports left out on reading.

use std::collections::{HashMap, HashSet};

use stat_core::Result;
use stat_data::JsonValue;

use super::tables::{ProjectFile, TableId};
use super::{array, invalid, obj, s};
use crate::dashboard::DashboardId;

/// Deepest nesting of folders read from a file (deeper ones are an error,
/// so a malformed file cannot exhaust the stack).
pub const MAX_FOLDER_DEPTH: usize = 32;

/// Something a folder can hold.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FolderItem {
    /// A table, listed with its reports.
    Table(TableId),
    /// Report `index` (its place in the table's report list) of `table`.
    Report {
        /// The table the report belongs to.
        table: TableId,
        /// Position in that table's reports.
        index: usize,
    },
    /// A dashboard.
    Dashboard(DashboardId),
    /// A script notebook, by its name (unique in the project).
    Notebook(String),
}

/// A folder of a project's tree.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectFolder {
    /// The name shown in the tree (need not be unique).
    pub name: String,
    /// Folders inside this one, in order.
    pub folders: Vec<ProjectFolder>,
    /// What the folder holds, in order.
    pub items: Vec<FolderItem>,
}

impl ProjectFolder {
    /// An empty folder called `name`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Self::default() }
    }

    /// Every item in this folder and the folders inside it.
    pub fn all_items(&self) -> Box<dyn Iterator<Item = &FolderItem> + '_> {
        Box::new(self.items.iter().chain(self.folders.iter().flat_map(ProjectFolder::all_items)))
    }
}

impl FolderItem {
    fn to_json(&self) -> JsonValue {
        match self {
            Self::Table(t) => obj([("table", s(t.as_str()))]),
            Self::Report { table, index } => {
                obj([("table", s(table.as_str())), ("report", JsonValue::number(*index as f64))])
            }
            Self::Dashboard(d) => obj([("dashboard", s(d.as_str()))]),
            Self::Notebook(name) => obj([("notebook", s(name))]),
        }
    }

    fn from_json(v: &JsonValue) -> Result<Self> {
        let text = |key: &str| v.get(key).and_then(JsonValue::as_str).map(str::to_owned);
        if let Some(table) = text("table") {
            return match v.get("report") {
                None | Some(JsonValue::Null) => Ok(Self::Table(TableId(table))),
                Some(r) => {
                    let index = r.as_usize().ok_or_else(|| invalid("folders: \"report\" must be a report index"))?;
                    Ok(Self::Report { table: TableId(table), index })
                }
            };
        }
        if let Some(d) = text("dashboard") {
            return Ok(Self::Dashboard(DashboardId(d)));
        }
        if let Some(name) = text("notebook") {
            return Ok(Self::Notebook(name));
        }
        Err(invalid(
            "folders: each item names a \"table\" (with an optional \"report\"), a \"dashboard\" or a \"notebook\"",
        ))
    }

    /// This item as read, against `file`: `None` when it names something the
    /// project does not have (or a report left out on reading); a report
    /// index moved down past reports left out before it.
    fn resolve(self, file: &ProjectFile, skipped: &HashMap<TableId, Vec<usize>>) -> Option<Self> {
        match self {
            Self::Table(t) => file.table(&t).is_some().then_some(Self::Table(t)),
            Self::Report { table, index } => {
                let left_out = skipped.get(&table).map_or(&[][..], Vec::as_slice);
                if left_out.binary_search(&index).is_ok() {
                    return None;
                }
                let index = index - left_out.iter().filter(|&&k| k < index).count();
                file.table(&table).is_some_and(|t| index < t.reports.len()).then_some(Self::Report { table, index })
            }
            Self::Dashboard(d) => file.dashboard(&d).is_some().then_some(Self::Dashboard(d)),
            Self::Notebook(n) => file.notebooks.iter().any(|b| b.name == n).then_some(Self::Notebook(n)),
        }
    }
}

/// `folders` as the JSON array of the `"folders"` member.
pub(super) fn folders_json(folders: &[ProjectFolder]) -> JsonValue {
    JsonValue::Array(
        folders
            .iter()
            .map(|f| {
                let mut members = vec![("name".to_owned(), s(&f.name))];
                if !f.items.is_empty() {
                    members.push((
                        "items".to_owned(),
                        JsonValue::Array(f.items.iter().map(FolderItem::to_json).collect()),
                    ));
                }
                if !f.folders.is_empty() {
                    members.push(("folders".to_owned(), folders_json(&f.folders)));
                }
                JsonValue::Object(members)
            })
            .collect(),
    )
}

/// The `"folders"` member of `doc` (none when it is missing), keeping only
/// items `file` has, each once.
///
/// # Errors
/// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument) for
/// a member that is not an array of folders, a folder without a text name,
/// an item that names nothing, or folders nested more than
/// [`MAX_FOLDER_DEPTH`] deep.
pub(super) fn read_folders(
    doc: &JsonValue,
    file: &ProjectFile,
    skipped: &HashMap<TableId, Vec<usize>>,
) -> Result<Vec<ProjectFolder>> {
    let mut placed = HashSet::new();
    match doc.get("folders") {
        None | Some(JsonValue::Null) => Ok(Vec::new()),
        Some(_) => read_list(doc, file, skipped, &mut placed, 1),
    }
}

fn read_list(
    v: &JsonValue,
    file: &ProjectFile,
    skipped: &HashMap<TableId, Vec<usize>>,
    placed: &mut HashSet<FolderItem>,
    depth: usize,
) -> Result<Vec<ProjectFolder>> {
    if depth > MAX_FOLDER_DEPTH {
        return Err(invalid(format!("folders: nested more than {MAX_FOLDER_DEPTH} deep")));
    }
    let mut out = Vec::new();
    for f in array(v, "folders")? {
        let name = f
            .get("name")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| invalid("folders: each folder needs a \"name\" (text)"))?
            .to_owned();
        let mut items = Vec::new();
        if !matches!(f.get("items"), None | Some(JsonValue::Null)) {
            for item in array(f, "items")? {
                if let Some(item) = FolderItem::from_json(item)?.resolve(file, skipped) {
                    if placed.insert(item.clone()) {
                        items.push(item);
                    }
                }
            }
        }
        let folders = match f.get("folders") {
            None | Some(JsonValue::Null) => Vec::new(),
            Some(_) => read_list(f, file, skipped, placed, depth + 1)?,
        };
        out.push(ProjectFolder { name, folders, items });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{load_project_file, load_project_file_noting, save_project_file};
    use crate::{Dashboard, SavedReport};
    use stat_data::{Column, DataTable, RowStateManager};

    fn file() -> ProjectFile {
        let table = |name: &str| {
            DataTable::new(
                name,
                vec![Column::numeric("x", vec![1.0, 2.0, 3.0]), Column::numeric("y", vec![2.0, 4.0, 7.0])],
            )
            .unwrap()
        };
        let mut file = ProjectFile::default();
        let a = file.add_table(table("A"), RowStateManager::new(3));
        file.add_table(table("B"), RowStateManager::new(3));
        let report = |column: usize| SavedReport::Distribution {
            request: crate::DistributionRequest {
                column,
                quantile_method: stat_core::descriptive::QuantileMethod::default(),
                censoring: None,
            },
            options: crate::DistributionOptions::default(),
        };
        file.table_mut(&a).unwrap().reports = vec![report(0), report(1)];
        file.dashboards.push(Dashboard::new(DashboardId::from("dashboard-1"), "Overview"));
        file.notebooks.push(crate::notebook::Notebook::new("Checks"));
        file
    }

    #[test]
    fn folders_round_trip_through_the_project_file() {
        let mut file = file();
        let mut inner = ProjectFolder::new("Plots");
        inner.items.push(FolderItem::Report { table: TableId::from("table-1"), index: 1 });
        let mut outer = ProjectFolder::new("Week 1");
        outer.items = vec![
            FolderItem::Table(TableId::from("table-2")),
            FolderItem::Dashboard(DashboardId::from("dashboard-1")),
            FolderItem::Notebook("Checks".to_owned()),
        ];
        outer.folders.push(inner);
        file.folders = vec![outer, ProjectFolder::new("Empty")];
        let text = save_project_file(&file);
        assert!(text.contains("\"folders\""), "{text}");
        let back = load_project_file(&text).unwrap();
        assert_eq!(back.folders, file.folders);
        assert_eq!(save_project_file(&back), text);
    }

    #[test]
    fn a_one_table_project_with_a_folder_is_written_in_the_several_table_form() {
        let mut file = ProjectFile::default();
        file.add_table(DataTable::new("A", vec![Column::numeric("x", vec![1.0])]).unwrap(), RowStateManager::new(1));
        assert!(!save_project_file(&file).contains("\"tables\""));
        file.folders.push(ProjectFolder::new("Raw data"));
        let text = save_project_file(&file);
        assert!(text.contains("\"tables\"") && text.contains("Raw data"), "{text}");
        assert_eq!(load_project_file(&text).unwrap().folders, file.folders);
    }

    #[test]
    fn files_without_folders_still_open_and_unknown_items_are_left_out() {
        let file = file();
        let text = save_project_file(&file);
        assert!(!text.contains("\"folders\""));
        assert!(load_project_file(&text).unwrap().folders.is_empty());
        // Add folders by hand naming things the project lacks, and one item twice.
        let mut doc = stat_data::parse_json(&text).unwrap();
        let extra = stat_data::parse_json(
            r#"[{"name": "F", "items": [{"table": "table-9"}, {"table": "table-1", "report": 5},
                 {"dashboard": "nope"}, {"notebook": "Other"}, {"table": "table-1"}],
                 "folders": [{"name": "G", "items": [{"table": "table-1"}, {"table": "table-1", "report": 0}]}]}]"#,
        )
        .unwrap();
        if let JsonValue::Object(members) = &mut doc {
            members.push(("folders".to_owned(), extra));
        }
        let back = load_project_file(&doc.to_pretty()).unwrap();
        assert_eq!(back.folders.len(), 1);
        assert_eq!(back.folders[0].items, vec![FolderItem::Table(TableId::from("table-1"))]);
        assert_eq!(
            back.folders[0].folders[0].items,
            vec![FolderItem::Report { table: TableId::from("table-1"), index: 0 }]
        );
    }

    #[test]
    fn malformed_folders_are_errors() {
        let text = save_project_file(&file());
        for bad in [
            r#"{"name": "x"}"#,
            r#"[{"items": []}]"#,
            r#"[{"name": 3}]"#,
            r#"[{"name": "x", "items": [{"colour": "red"}]}]"#,
            r#"[{"name": "x", "items": [{"table": "table-1", "report": -1}]}]"#,
        ] {
            let mut doc = stat_data::parse_json(&text).unwrap();
            if let JsonValue::Object(members) = &mut doc {
                members.push(("folders".to_owned(), stat_data::parse_json(bad).unwrap()));
            }
            assert!(load_project_file(&doc.to_pretty()).is_err(), "{bad}");
        }
        // Too deep.
        let mut nested = r#"{"name": "leaf"}"#.to_owned();
        for _ in 0..MAX_FOLDER_DEPTH {
            nested = format!(r#"{{"name": "f", "folders": [{nested}]}}"#);
        }
        let mut doc = stat_data::parse_json(&text).unwrap();
        if let JsonValue::Object(members) = &mut doc {
            members.push(("folders".to_owned(), stat_data::parse_json(&format!("[{nested}]")).unwrap()));
        }
        assert!(load_project_file_noting(&doc.to_pretty()).is_err());
    }
}
