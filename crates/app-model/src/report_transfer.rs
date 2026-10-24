//! Running a report on another table.
//!
//! "Duplicate on another table…" copies a report (or graph canvas) with its
//! display options and plot styles onto a different table. Reports refer to
//! columns by position or by name, so the copy is made through the
//! project-file form, which names every column: the report is written
//! against its own table ([`checked_report_to_json`]) and read back against
//! the target table with the target's columns renamed, for the moment, to
//! the names they stand in for ([`stage`]); name references are then turned
//! back into the target's own names ([`rename_back`]). Positions never
//! change, so the result refers to the target table exactly.
//!
//! Which target column stands in for which source column is a
//! [`ColumnMapping`]: columns with the same name match by themselves, a
//! name differing only in case or spacing is offered, and the rest are
//! chosen by the user. Project templates ([`crate::project_template`])
//! use the same mapping.

use stat_core::Result;
use stat_data::{Column, ColumnKind, DataTable, MeasurementLevel};

use crate::project::{SavedReport, checked_report_to_json, invalid, report_from_json};

/// The kind of values a column holds, as a mapping describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// Numbers.
    Numeric,
    /// Text.
    Text,
    /// Dates, times or durations.
    DateTime,
}

impl ValueKind {
    /// The kind of `column`'s values.
    #[must_use]
    pub fn of(column: &Column) -> Self {
        match column.kind() {
            ColumnKind::Numeric => Self::Numeric,
            ColumnKind::Text => Self::Text,
            ColumnKind::DateTime(_) => Self::DateTime,
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Numeric => "numeric",
            Self::Text => "text",
            Self::DateTime => "date-time",
        }
    }
}

/// A column a report or template reads, as it was on its own table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnNeed {
    /// The column's name on its own table.
    pub name: String,
    /// Its kind of values.
    pub kind: ValueKind,
    /// Its measurement level.
    pub level: MeasurementLevel,
}

impl ColumnNeed {
    /// The need of `column`.
    #[must_use]
    pub fn of(column: &Column) -> Self {
        Self { name: column.name.clone(), kind: ValueKind::of(column), level: column.level }
    }
}

/// Every column `report` reads on `table`, in table order.
#[must_use]
pub fn report_needs(report: &SavedReport, table: &DataTable) -> Vec<ColumnNeed> {
    let mut used = report.columns();
    used.extend(report.column_names().iter().filter_map(|n| table.find(n)));
    used.sort_unstable();
    used.dedup();
    used.iter().filter_map(|&i| table.column(i)).map(ColumnNeed::of).collect()
}

/// Which column of a target table stands in for each needed column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMapping {
    /// The columns needed, as they were on their own table.
    pub needs: Vec<ColumnNeed>,
    /// The target column chosen for each need (`None`: not chosen yet).
    pub chosen: Vec<Option<String>>,
}

/// Lower case without spaces, underscores or dots, for matching names that
/// differ only in how they are written.
fn loose(name: &str) -> String {
    name.chars().filter(|c| !c.is_whitespace() && *c != '_' && *c != '.').flat_map(char::to_lowercase).collect()
}

impl ColumnMapping {
    /// A mapping of `needs` onto `target`: each need takes the target column
    /// of the same name, else the one whose name differs only in case,
    /// spaces, underscores or dots, else none.
    #[must_use]
    pub fn new(needs: Vec<ColumnNeed>, target: &DataTable) -> Self {
        let chosen = needs
            .iter()
            .map(|n| {
                target.find(&n.name).map(|_| n.name.clone()).or_else(|| {
                    let want = loose(&n.name);
                    target.columns().iter().find(|c| loose(&c.name) == want).map(|c| c.name.clone())
                })
            })
            .collect();
        Self { needs, chosen }
    }

    /// `true` when every need is met by the target column of the same name,
    /// so there is nothing to ask.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.needs.iter().zip(&self.chosen).all(|(n, c)| c.as_deref() == Some(n.name.as_str()))
    }

    /// The pairs (needed name, target name), checked against `target`.
    ///
    /// # Errors
    /// [`stat_core::StatError::InvalidArgument`] naming the first need
    /// without a column, a chosen column `target` lacks, or a target column
    /// chosen for two needs.
    pub fn pairs(&self, target: &DataTable) -> Result<Vec<(String, String)>> {
        let mut out: Vec<(String, String)> = Vec::with_capacity(self.needs.len());
        for (need, chosen) in self.needs.iter().zip(&self.chosen) {
            let Some(chosen) = chosen else {
                return Err(invalid(format!("choose a column of {} for {:?}", target.name(), need.name)));
            };
            if target.find(chosen).is_none() {
                return Err(invalid(format!("{} has no column {chosen:?}", target.name())));
            }
            if let Some((other, _)) = out.iter().find(|(_, c)| c == chosen) {
                return Err(invalid(format!("{chosen:?} is chosen for both {other:?} and {:?}", need.name)));
            }
            out.push((need.name.clone(), chosen.clone()));
        }
        Ok(out)
    }

    /// A note for each pair whose kinds of values differ (the report may
    /// then not run on the target).
    #[must_use]
    pub fn kind_warnings(&self, target: &DataTable) -> Vec<String> {
        self.needs
            .iter()
            .zip(&self.chosen)
            .filter_map(|(need, chosen)| {
                let column = target.column(target.find(chosen.as_deref()?)?)?;
                let kind = ValueKind::of(column);
                (kind != need.kind).then(|| {
                    format!("{:?} is {} but {:?} was {}", column.name, kind.label(), need.name, need.kind.label())
                })
            })
            .collect()
    }
}

/// A target table whose columns carry, for the moment, the names of the
/// columns they stand in for, and the renames that undo it.
#[derive(Debug, Clone)]
pub(crate) struct Staged {
    /// The target table, renamed.
    pub(crate) table: DataTable,
    /// (staged name, the target's own name) for each renamed column.
    pub(crate) back: Vec<(String, String)>,
}

/// A name for the moment while columns swap names (a plain identifier, so
/// formulas can refer to it).
fn placeholder(i: usize) -> String {
    format!("__staged_column_{i}__")
}

/// `target` with the column chosen for each (needed name, target name) pair
/// renamed to the needed name. Other columns keep their names unless one is
/// a needed name or one of `reserved`, in which case they get a free one.
pub(crate) fn stage(target: &DataTable, pairs: &[(String, String)], reserved: &[String]) -> Result<Staged> {
    let mut table = target.clone();
    let n = table.n_cols();
    let mut names: Vec<Option<String>> = vec![None; n];
    for (need, chosen) in pairs {
        let i = target.find(chosen).ok_or_else(|| invalid(format!("{} has no column {chosen:?}", target.name())))?;
        names[i] = Some(need.clone());
    }
    let taken = |name: &str, names: &[Option<String>]| {
        names.iter().flatten().any(|m| m == name) || reserved.iter().any(|r| r == name)
    };
    for i in 0..n {
        if names[i].is_some() {
            continue;
        }
        let own = target.columns()[i].name.clone();
        let mut name = own.clone();
        let mut k = 2;
        while taken(&name, &names) || (name != own && target.find(&name).is_some()) {
            name = format!("{own} ({k})");
            k += 1;
        }
        names[i] = Some(name);
    }
    for i in 0..n {
        table.rename_column(i, &placeholder(i))?;
    }
    let mut back = Vec::new();
    for (i, name) in names.into_iter().enumerate() {
        let name = name.expect("every column named");
        table.rename_column(i, &name)?;
        let own = &target.columns()[i].name;
        if &name != own {
            back.push((name, own.clone()));
        }
    }
    Ok(Staged { table, back })
}

/// Apply the renames `back` (old, new) to whatever `rename` renames, through
/// placeholders so that one rename never catches another's result.
pub(crate) fn rename_all(back: &[(String, String)], mut rename: impl FnMut(&str, &str)) {
    for (i, (old, _)) in back.iter().enumerate() {
        rename(old, &placeholder(i));
    }
    for (i, (_, new)) in back.iter().enumerate() {
        rename(&placeholder(i), new);
    }
}

/// `report`, written against `source`, read against `target` with each
/// (source name, target name) pair of `pairs` standing in for each other.
/// The display options, row filter, column picker and plot styles come
/// along.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] when the report refers to a
/// column `source` lacks, a pair names a column the tables lack, or the
/// report cannot be read on the target (for example a column of the wrong
/// kind).
pub fn duplicate_report(
    report: &SavedReport,
    source: &DataTable,
    target: &DataTable,
    pairs: &[(String, String)],
) -> Result<SavedReport> {
    let json = checked_report_to_json(report, source)?;
    let staged = stage(target, pairs, &[])?;
    let mut copy = report_from_json(&json, &staged.table)?;
    rename_all(&staged.back, |old, new| copy.rename_column(old, new));
    Ok(copy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::report_to_json;
    use stat_viz::{GraphSpec, Role};

    fn table(name: &str, cols: &[&str]) -> DataTable {
        let columns =
            cols.iter().enumerate().map(|(i, c)| Column::numeric(*c, vec![1.0 + i as f64, 2.0, 4.0, 3.0])).collect();
        DataTable::new(name, columns).unwrap()
    }

    #[test]
    fn mapping_matches_names_then_loose_names() {
        let source = table("A", &["Load", "Strain rate", "Batch"]);
        let target = table("B", &["strain_rate", "Batch", "Force"]);
        let needs = source.columns().iter().map(ColumnNeed::of).collect();
        let mut m = ColumnMapping::new(needs, &target);
        assert_eq!(m.chosen, vec![None, Some("strain_rate".into()), Some("Batch".into())]);
        assert!(!m.is_exact());
        assert!(m.pairs(&target).unwrap_err().to_string().contains("\"Load\""));
        m.chosen[0] = Some("Batch".into());
        assert!(m.pairs(&target).unwrap_err().to_string().contains("both"));
        m.chosen[0] = Some("Force".into());
        let pairs = m.pairs(&target).unwrap();
        assert_eq!(pairs[0], ("Load".into(), "Force".into()));
        assert!(m.kind_warnings(&target).is_empty());
    }

    #[test]
    fn staging_swaps_names_and_renames_back() {
        // The target has a column named like a source column it does not
        // stand in for: it moves aside while staged.
        let target = table("B", &["Force", "Load", "Batch"]);
        let pairs = vec![("Load".to_owned(), "Force".to_owned()), ("Strain".to_owned(), "Load".to_owned())];
        let staged = stage(&target, &pairs, &[]).unwrap();
        let names: Vec<&str> = staged.table.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Load", "Strain", "Batch"]);
        let mut current: Vec<String> = names.iter().map(|s| (*s).to_owned()).collect();
        rename_all(&staged.back, |old, new| {
            for c in &mut current {
                if c == old {
                    *c = new.to_owned();
                }
            }
        });
        assert_eq!(current, ["Force", "Load", "Batch"]);
    }

    #[test]
    fn a_linear_model_moves_to_mapped_columns() {
        let source = table("A", &["Load", "Strain"]);
        let target = table("B", &["Id", "Force", "Rate"]);
        let report = SavedReport::Linear {
            spec: crate::LinearModelSpec {
                response: 1,
                predictors: vec![0],
                effects: Vec::new(),
                splines: Vec::new(),
            },
            options: crate::ReportOptions::default(),
        };
        let needs = report_needs(&report, &source);
        assert_eq!(needs.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["Load", "Strain"]);
        let pairs = vec![("Load".to_owned(), "Rate".to_owned()), ("Strain".to_owned(), "Force".to_owned())];
        let copy = duplicate_report(&report, &source, &target, &pairs).unwrap();
        let SavedReport::Linear { spec, .. } = &copy else { panic!("a linear model") };
        assert_eq!((spec.response, spec.predictors.clone()), (1, vec![2]));
        // Its project form names the target's columns.
        let json = report_to_json(&copy, &target).to_pretty();
        assert!(json.contains("\"Force\"") && json.contains("\"Rate\""));
    }

    #[test]
    fn a_graph_keeps_its_style_on_renamed_columns() {
        let source = table("A", &["Load", "Strain"]);
        let target = table("B", &["Strain", "Force"]);
        let mut spec = GraphSpec::default();
        spec.encoding.assign(Role::X, "Load");
        spec.encoding.assign(Role::Y, "Strain");
        let mut style = crate::plot_style::PlotStyle::default();
        style.axes.x.grid = stat_viz::axis_settings::GridLines::None;
        spec.axes.style = Some(style.clone());
        let report = SavedReport::Graph { spec: Box::new(spec) };
        let needs = report_needs(&report, &source);
        assert_eq!(needs.len(), 2);
        // Load → Strain and Strain → Force: a swap that only placeholders get right.
        let pairs = vec![("Load".to_owned(), "Strain".to_owned()), ("Strain".to_owned(), "Force".to_owned())];
        let copy = duplicate_report(&report, &source, &target, &pairs).unwrap();
        let SavedReport::Graph { spec } = &copy else { panic!("a graph") };
        assert_eq!(spec.encoding.get(Role::X), vec!["Strain"]);
        assert_eq!(spec.encoding.get(Role::Y), vec!["Force"]);
        assert_eq!(spec.axes.style, Some(style));
    }
}
