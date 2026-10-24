//! Project templates: a project's reports, graph canvases, dashboards,
//! scripts and plot styles without its data, to build again on new data.
//!
//! "Save project as template…" writes [`template_text`]; "New project from
//! template…" reads it ([`ProjectTemplate::parse`]), maps each template
//! table's columns onto a new table ([`ColumnMapping`], as for
//! "Duplicate on another table…" in [`crate::report_transfer`]) and builds
//! the project ([`ProjectTemplate::apply`]). Several data files can each take
//! the template of a one-table project ([`merge_projects`] puts them in one
//! project).
//!
//! The file is the project file with every table emptied:
//!
//! ```text
//! {"format": "statistical-engine-template", "version": 1,
//!  "tables": [{"name": "…", "needs": ["Load", "Strain", …]}, …],
//!  "project": {…a project file whose columns hold no values and whose
//!              row states are empty…}}
//! ```
//!
//! `needs` lists, per table and in the order of the project's tables, the
//! data columns the reports, dashboards and formula columns read; the
//! table's columns in `project` are those columns (their names, kinds and
//! measurement levels, to show when mapping) and the formula columns they
//! use, which are computed again on the new data. Derived tables (made by a
//! table operation) are left out, as are row states and the window layout.

use std::collections::BTreeSet;

use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager, parse_json};

use crate::dashboard::DashboardId;
use crate::formula;
use crate::project::{
    ProjectFile, ProjectLayout, SavedReport, TableId, column_from_json, column_to_json, invalid, load_project_file_noting,
    save_project_file,
};
use crate::report_transfer::{ColumnMapping, ColumnNeed, rename_all, stage};

/// The `format` member of a template file.
pub const TEMPLATE_FORMAT: &str = "statistical-engine-template";

/// The file extension of templates.
pub const TEMPLATE_EXTENSION: &str = "acrafttemplate";

/// The template version this build writes.
const TEMPLATE_VERSION: usize = 1;

/// Every report of table `t` of `file`, its own and its dashboard tiles'.
fn table_reports(file: &ProjectFile, t: usize) -> Vec<&SavedReport> {
    let id = &file.tables[t].id;
    let tiles = file.dashboards.iter().flat_map(|d| &d.tiles).filter_map(|tile| match &tile.content {
        crate::dashboard::TileContent::Report { table, report } if table == id => Some(report.as_ref()),
        _ => None,
    });
    file.tables[t].reports.iter().chain(tiles).collect()
}

/// The columns of table `t` that its reports, dashboard tiles, dashboard
/// filters and switchers read, with the columns their formula columns
/// read, by position.
fn used_columns(file: &ProjectFile, t: usize) -> BTreeSet<usize> {
    let table = &file.tables[t].table;
    let mut used = BTreeSet::new();
    for report in table_reports(file, t) {
        used.extend(report.columns().into_iter().filter(|&c| c < table.n_cols()));
        used.extend(report.column_names().iter().filter_map(|n| table.find(n)));
    }
    for d in &file.dashboards {
        let names = d.filters.iter().map(|f| f.column()).chain(d.switchers.iter().flat_map(|s| s.choices.iter().map(String::as_str)));
        used.extend(names.filter_map(|n| table.find(n)));
    }
    // Formula columns read other columns.
    let mut todo: Vec<usize> = used.iter().copied().collect();
    while let Some(c) = todo.pop() {
        if let Some(f) = table.columns()[c].formula.as_deref() {
            for r in formula::references(table, f) {
                if used.insert(r) {
                    todo.push(r);
                }
            }
        }
    }
    used
}

/// The table objects of a project document: the document itself for a
/// one-table (version 1 or 2) file, else each member of `tables`.
fn table_objects_mut(doc: &mut JsonValue) -> Vec<&mut JsonValue> {
    if !matches!(doc.get("tables"), Some(JsonValue::Array(_))) {
        return vec![doc];
    }
    match doc {
        JsonValue::Object(members) => members
            .iter_mut()
            .filter(|(k, _)| k == "tables")
            .flat_map(|(_, v)| match v {
                JsonValue::Array(tables) => tables.iter_mut().collect(),
                _ => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The table objects of a project document (see [`table_objects_mut`]).
fn table_objects(doc: &JsonValue) -> Vec<&JsonValue> {
    match doc.get("tables") {
        Some(JsonValue::Array(tables)) => tables.iter().collect(),
        _ => vec![doc],
    }
}

/// Set member `key` of object `v` (added at the end when missing).
fn set_member(v: &mut JsonValue, key: &str, value: JsonValue) {
    if let JsonValue::Object(members) = v {
        match members.iter_mut().find(|(k, _)| k == key) {
            Some((_, m)) => *m = value,
            None => members.push((key.to_owned(), value)),
        }
    }
}

/// Remove member `key` of object `v`.
fn remove_member(v: &mut JsonValue, key: &str) {
    if let JsonValue::Object(members) = v {
        members.retain(|(k, _)| k != key);
    }
}

/// Row states with nothing set.
fn empty_rows() -> JsonValue {
    let empty = || JsonValue::Array(Vec::new());
    JsonValue::Object(vec![
        ("excluded".into(), empty()),
        ("hidden".into(), empty()),
        ("selected".into(), empty()),
        ("colors".into(), empty()),
    ])
}

/// The template of project `file` (see the [module documentation](self)),
/// with a note for each table left out.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] when every table of the
/// project is derived from another (there is nothing to build on).
pub fn template_text(file: &ProjectFile) -> Result<(String, Vec<String>)> {
    let mut f = file.clone();
    let mut notes = Vec::new();
    f.tables.retain(|t| {
        let keep = t.lineage.is_none();
        if !keep {
            notes.push(format!("{} is made from another table, so the template leaves it out", t.table.name()));
        }
        keep
    });
    if f.tables.is_empty() {
        return Err(invalid("the project has no table of its own to make a template from"));
    }
    f.layout = ProjectLayout::default();
    for t in &mut f.tables {
        t.rows = RowStateManager::new(t.table.n_rows());
        t.external = None;
    }
    let used: Vec<BTreeSet<usize>> = (0..f.tables.len()).map(|t| used_columns(&f, t)).collect();
    let mut doc = parse_json(&save_project_file(&f))?;
    let mut list = Vec::new();
    for ((obj, t), used) in table_objects_mut(&mut doc).into_iter().zip(&f.tables).zip(&used) {
        let mut columns = Vec::new();
        let mut needs = Vec::new();
        for &c in used {
            let column = &t.table.columns()[c];
            let mut json = column_to_json(column);
            if column.formula.is_some() {
                remove_member(&mut json, "values");
            } else {
                set_member(&mut json, "values", JsonValue::Array(Vec::new()));
                needs.push(JsonValue::String(column.name.clone()));
            }
            columns.push(json);
        }
        let name = JsonValue::String(t.table.name().to_owned());
        set_member(obj, "table", JsonValue::Object(vec![("name".into(), name.clone()), ("columns".into(), JsonValue::Array(columns))]));
        set_member(obj, "rows", empty_rows());
        list.push(JsonValue::Object(vec![("name".into(), name), ("needs".into(), JsonValue::Array(needs))]));
    }
    let text = JsonValue::Object(vec![
        ("format".into(), JsonValue::String(TEMPLATE_FORMAT.into())),
        ("version".into(), JsonValue::number(TEMPLATE_VERSION as f64)),
        ("tables".into(), JsonValue::Array(list)),
        ("project".into(), doc),
    ])
    .to_pretty();
    Ok((text, notes))
}

/// One table of a template.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateTable {
    /// The table's name in the project the template was made from.
    pub name: String,
    /// The data columns it needs, as they were.
    pub needs: Vec<ColumnNeed>,
    /// Its formula columns, by name, computed again on new data.
    pub formulas: Vec<String>,
}

/// A template read from its file (see the [module documentation](self)).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectTemplate {
    /// Its tables, in order.
    pub tables: Vec<TemplateTable>,
    /// The emptied project.
    doc: JsonValue,
}

impl ProjectTemplate {
    /// Read a template file.
    ///
    /// # Errors
    /// [`stat_core::StatError::InvalidArgument`] for invalid JSON, a file
    /// that is not a template, a newer version, or malformed tables.
    pub fn parse(text: &str) -> Result<Self> {
        let v = parse_json(text)?;
        if v.get("format").and_then(JsonValue::as_str) != Some(TEMPLATE_FORMAT) {
            return Err(invalid("this file is not a project template"));
        }
        let version = v.get("version").and_then(JsonValue::as_usize).ok_or_else(|| invalid("template: missing version"))?;
        if version > TEMPLATE_VERSION {
            return Err(invalid(format!("template version {version} is newer than this application supports")));
        }
        let doc = v.get("project").cloned().ok_or_else(|| invalid("template: missing project"))?;
        let Some(JsonValue::Array(list)) = v.get("tables") else { return Err(invalid("template: missing tables")) };
        let objects = table_objects(&doc);
        if objects.len() != list.len() {
            return Err(invalid("template: the table list does not match the project"));
        }
        let mut tables = Vec::new();
        for (entry, obj) in list.iter().zip(objects) {
            let name = entry.get("name").and_then(JsonValue::as_str).unwrap_or_default().to_owned();
            let columns = obj
                .get("table")
                .and_then(|t| t.get("columns"))
                .and_then(JsonValue::as_array)
                .ok_or_else(|| invalid("template: a table has no columns"))?
                .iter()
                .map(column_from_json)
                .collect::<Result<Vec<_>>>()?;
            let needed: Vec<&str> = entry
                .get("needs")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| invalid("template: a table has no needs"))?
                .iter()
                .map(|n| n.as_str().ok_or_else(|| invalid("template: needs must be column names")))
                .collect::<Result<_>>()?;
            let needs = needed
                .iter()
                .map(|n| {
                    columns
                        .iter()
                        .find(|c| c.name == *n)
                        .map(ColumnNeed::of)
                        .ok_or_else(|| invalid(format!("template: no column {n:?}")))
                })
                .collect::<Result<_>>()?;
            let formulas = columns.iter().filter(|c| c.formula.is_some()).map(|c| c.name.clone()).collect();
            tables.push(TemplateTable { name, needs, formulas });
        }
        Ok(Self { tables, doc })
    }

    /// The project built on `data` (one table per template table, in
    /// order), each template table's needs met as `mappings` says, with
    /// notes on what could not be built (reports that do not run on the
    /// new columns, formula columns that fail).
    ///
    /// # Errors
    /// [`stat_core::StatError::InvalidArgument`] when the numbers of tables
    /// or mappings differ from the template's, a mapping is incomplete, or
    /// the project cannot be read (see
    /// [`load_project_file_noting`]).
    pub fn apply(&self, data: &[DataTable], mappings: &[ColumnMapping]) -> Result<(ProjectFile, Vec<String>)> {
        if data.len() != self.tables.len() || mappings.len() != self.tables.len() {
            return Err(invalid(format!("the template needs {} table(s)", self.tables.len())));
        }
        let mut doc = self.doc.clone();
        let mut backs = Vec::new();
        for (((obj, table), mapping), t) in table_objects_mut(&mut doc).into_iter().zip(data).zip(mappings).zip(&self.tables) {
            let pairs = mapping.pairs(table)?;
            let staged = stage(table, &pairs, &t.formulas)?;
            let saved_columns = obj.get("table").and_then(|t| t.get("columns")).and_then(JsonValue::as_array).map(<[JsonValue]>::to_vec).unwrap_or_default();
            let mut columns: Vec<JsonValue> = staged.table.columns().iter().map(column_to_json).collect();
            // The template's formula columns, computed on the new data.
            columns.extend(saved_columns.into_iter().filter(|c| c.get("formula").is_some_and(|f| !matches!(f, JsonValue::Null))));
            let name = JsonValue::String(table.name().to_owned());
            set_member(obj, "table", JsonValue::Object(vec![("name".into(), name), ("columns".into(), JsonValue::Array(columns))]));
            set_member(obj, "rows", empty_rows());
            backs.push(staged.back);
        }
        let (mut file, notes) = load_project_file_noting(&doc.to_pretty())?;
        for (t, back) in backs.iter().enumerate() {
            rename_in_table(&mut file, t, back);
        }
        Ok((file, notes))
    }
}

/// Follow the renames `back` (old, new) in table `t` of `file`: the column
/// names, its formulas, its reports and dashboard tiles, and dashboard
/// filters and switchers when the project has only this table.
fn rename_in_table(file: &mut ProjectFile, t: usize, back: &[(String, String)]) {
    let only = file.tables.len() == 1;
    let id = file.tables[t].id.clone();
    let ProjectFile { tables, dashboards, .. } = file;
    let pt = &mut tables[t];
    rename_all(back, |old, new| {
        if let Some(i) = pt.table.find(old) {
            // Through placeholders, so the new name is free.
            let _ = pt.table.rename_column(i, new);
        }
        for j in 0..pt.table.n_cols() {
            if let Some(f) = pt.table.columns()[j].formula.as_deref() {
                let rewritten = formula::renamed(f, old, new);
                if rewritten != f {
                    let _ = pt.table.set_formula(j, Some(rewritten));
                }
            }
        }
        for r in &mut pt.reports {
            r.rename_column(old, new);
        }
        for d in dashboards.iter_mut() {
            d.rename_column(&id, old, new);
            if only {
                let mut scope = crate::ReportScope { filter: std::mem::take(&mut d.filters), ..Default::default() };
                scope.rename(old, new);
                d.filters = scope.filter;
                for s in &mut d.switchers {
                    for name in std::iter::once(&mut s.column).chain(&mut s.choices).chain(std::iter::once(&mut s.current)) {
                        if name == old {
                            new.clone_into(name);
                        }
                    }
                }
            }
        }
    });
}

/// One project holding every table of `files` (each usually one table built
/// from the same template), with table and dashboard identifiers made
/// unique and each dashboard named after its table when there are several.
/// Scripts, notes and notebooks come from the first file.
#[must_use]
pub fn merge_projects(files: Vec<ProjectFile>) -> ProjectFile {
    let several = files.len() > 1;
    let mut out = ProjectFile::default();
    for (k, f) in files.into_iter().enumerate() {
        let suffix = |s: &str| if k == 0 { s.to_owned() } else { format!("{s}-{}", k + 1) };
        let renamed: Vec<(TableId, TableId)> =
            f.tables.iter().map(|t| (t.id.clone(), TableId(suffix(t.id.as_str())))).collect();
        let first_name = f.tables.first().map(|t| t.table.name().to_owned()).unwrap_or_default();
        for mut t in f.tables {
            t.id = TableId(suffix(t.id.as_str()));
            out.tables.push(t);
        }
        for mut d in f.dashboards {
            d.id = DashboardId(suffix(&d.id.0));
            if several {
                d.name = format!("{} — {first_name}", d.name);
            }
            for tile in &mut d.tiles {
                if let crate::dashboard::TileContent::Report { table, .. } = &mut tile.content
                    && let Some((_, new)) = renamed.iter().find(|(old, _)| old == table)
                {
                    *table = new.clone();
                }
            }
            out.dashboards.push(d);
        }
        if k == 0 {
            out.scripts = f.scripts;
            out.notes = f.notes;
            out.notebooks = f.notebooks;
            out.name = f.name;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// The reports of every table of `file` as their project form names
    /// them.
    fn report_text(file: &ProjectFile) -> Vec<String> {
        file.tables
            .iter()
            .flat_map(|t| t.reports.iter().map(|r| crate::project::report_to_json(r, &t.table).to_pretty()))
            .collect()
    }

    fn mappings(template: &ProjectTemplate, data: &[DataTable]) -> Vec<ColumnMapping> {
        template.tables.iter().zip(data).map(|(t, d)| ColumnMapping::new(t.needs.clone(), d)).collect()
    }

    #[test]
    fn sample_projects_rebuild_on_their_own_data() {
        for info in crate::sample::project_samples() {
            let file = (info.build)();
            let (text, _) = template_text(&file).unwrap();
            assert!(!text.contains("\"values\": [\n"), "{}: the template holds no data", info.name);
            let template = ProjectTemplate::parse(&text).unwrap();
            let own: Vec<DataTable> =
                file.tables.iter().filter(|t| t.lineage.is_none()).map(|t| t.table.clone()).collect();
            let maps = mappings(&template, &own);
            assert!(maps.iter().all(ColumnMapping::is_exact), "{}", info.name);
            let (built, notes) = template.apply(&own, &maps).unwrap();
            assert!(notes.is_empty(), "{}: {notes:?}", info.name);
            let original = ProjectFile {
                tables: file.tables.iter().filter(|t| t.lineage.is_none()).cloned().collect(),
                ..file.clone()
            };
            assert_eq!(report_text(&built), report_text(&original), "{}", info.name);
            assert_eq!(built.dashboards.len(), file.dashboards.len(), "{}", info.name);
            assert_eq!(built.scripts, file.scripts, "{}", info.name);
        }
    }

    /// A one-table project: a linear model of Strain on Load, a formula
    /// column Ratio = Strain / Load, and a distribution of Ratio.
    fn tensile(names: [&str; 3], loads: Vec<f64>) -> ProjectFile {
        let strain: Vec<f64> = loads.iter().map(|l| 0.01 * l + 0.002).collect();
        let mut ratio = Column::numeric("Ratio", vec![0.0; loads.len()]);
        ratio.formula = Some(format!("{} / {}", names[1], names[0]));
        let mut table = DataTable::new(
            "Batch",
            vec![
                Column::numeric(names[0], loads.clone()),
                Column::numeric(names[1], strain),
                Column::numeric(names[2], vec![1.0; loads.len()]),
                ratio,
            ],
        )
        .unwrap();
        formula::refresh_all(&mut table);
        let linear = SavedReport::Linear {
            spec: crate::LinearModelSpec { response: 1, predictors: vec![0], effects: Vec::new(), splines: Vec::new() },
            options: crate::ReportOptions::default(),
        };
        let mut spec = stat_viz::GraphSpec::default();
        spec.encoding.assign(stat_viz::Role::X, names[0]);
        spec.encoding.assign(stat_viz::Role::Y, "Ratio");
        let mut style = crate::plot_style::PlotStyle::default();
        style.axes.y.grid = stat_viz::axis_settings::GridLines::None;
        spec.axes.style = Some(style);
        let graph = SavedReport::Graph { spec: Box::new(spec) };
        let rows = RowStateManager::new(table.n_rows());
        ProjectFile {
            tables: vec![crate::project::ProjectTable {
                id: TableId("table-1".into()),
                table,
                rows,
                reports: vec![linear, graph],
                external: None,
                lineage: None,
            }],
            ..ProjectFile::default()
        }
    }

    #[test]
    fn a_template_builds_on_new_names_with_its_formula_column() {
        let (text, _) = template_text(&tensile(["Load", "Strain", "Spare"], vec![1.0, 2.0, 3.0, 5.0])).unwrap();
        let template = ProjectTemplate::parse(&text).unwrap();
        let t = &template.tables[0];
        // Spare is read by nothing; Ratio is computed.
        assert_eq!(t.needs.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["Load", "Strain"]);
        assert_eq!(t.formulas, ["Ratio"]);

        let new = DataTable::new(
            "Batch 2",
            vec![
                Column::numeric("Strain", vec![9.0, 8.0, 7.0, 6.0]),
                Column::numeric("Force", vec![2.0, 4.0, 6.0, 9.0]),
                Column::numeric("Elongation", vec![0.03, 0.05, 0.07, 0.1]),
            ],
        )
        .unwrap();
        let mut m = ColumnMapping::new(t.needs.clone(), &new);
        // Strain matches by name but should be Elongation here; Load is Force.
        assert_eq!(m.chosen, vec![None, Some("Strain".into())]);
        m.chosen = vec![Some("Force".into()), Some("Elongation".into())];
        let (built, notes) = template.apply(std::slice::from_ref(&new), &[m]).unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        let b = &built.tables[0];
        let names: Vec<&str> = b.table.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Strain", "Force", "Elongation", "Ratio"]);
        assert_eq!(b.table.name(), "Batch 2");
        assert_eq!(b.table.columns()[3].formula.as_deref(), Some("Elongation / Force"));
        let ratio = b.table.columns()[3].as_numeric().unwrap();
        assert!((ratio[0] - 0.015).abs() < 1e-12);
        let SavedReport::Linear { spec, .. } = &b.reports[0] else { panic!("a linear model") };
        assert_eq!((spec.response, spec.predictors.clone()), (2, vec![1]));
        let SavedReport::Graph { spec } = &b.reports[1] else { panic!("a graph") };
        assert_eq!(spec.encoding.get(stat_viz::Role::X), vec!["Force"]);
        assert_eq!(spec.encoding.get(stat_viz::Role::Y), vec!["Ratio"]);
        assert!(spec.axes.style.is_some(), "the plot style comes along");
    }

    #[test]
    fn folders_merge_into_one_project() {
        let (text, _) = template_text(&tensile(["Load", "Strain", "Spare"], vec![1.0, 2.0, 3.0])).unwrap();
        let template = ProjectTemplate::parse(&text).unwrap();
        let files: Vec<ProjectFile> = (0..3)
            .map(|k| {
                let mut d = tensile(["Load", "Strain", "Spare"], vec![1.0 + f64::from(k), 2.0, 4.0]).tables.remove(0).table;
                d.set_name(format!("Batch {k}"));
                let maps = mappings(&template, std::slice::from_ref(&d));
                template.apply(&[d], &maps).unwrap().0
            })
            .collect();
        let merged = merge_projects(files);
        let ids: Vec<&str> = merged.tables.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["table-1", "table-1-2", "table-1-3"]);
        assert!(merged.tables.iter().all(|t| t.reports.len() == 2));
        // It saves and opens again.
        let again = crate::project::load_project_file(&save_project_file(&merged)).unwrap();
        assert_eq!(again.tables.len(), 3);
    }

    #[test]
    fn templates_refuse_other_files_and_derived_only_projects() {
        assert!(ProjectTemplate::parse("{\"format\": \"something\"}").is_err());
        let newer = format!("{{\"format\": \"{TEMPLATE_FORMAT}\", \"version\": 99}}");
        assert!(ProjectTemplate::parse(&newer).unwrap_err().to_string().contains("newer"));
    }
}
