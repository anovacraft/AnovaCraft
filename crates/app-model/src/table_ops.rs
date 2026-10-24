//! Table operations as requests: what the Data menu's Rows, Reshape, Merge and
//! Summarize commands ask for, naming columns and tables by name, run against
//! open documents.
//!
//! Every operation except an in-place sort or duplicate removal builds a new
//! document ([`DerivedDocument`]) and leaves its sources untouched. Rows that
//! come from exactly one source row keep that row's selection, exclusion,
//! hiding and colour. The new document remembers how it was made
//! ([`TableOrigin`]), which project files store and scripts will replay.
//!
//! Sorting and removing duplicates in place are ordinary undoable edits
//! ([`sort_edit`], [`remove_duplicates_edit`]): undo restores the original
//! order or the removed rows with their row states.
//!
//! The computations are in [`stat_data::table_ops`], which documents the
//! methods and references.

use stat_core::Result;
use stat_core::descriptive::QuantileMethod;
use stat_data::table_ops::{
    self, JoinKind, Relationship, SampleSize, SortKey, Statistic, SummarizeOptions, duplicate_rows, sort_order,
};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_viz::FilterCondition;

use crate::editing::Edit;
use crate::project::{QUANTILE_METHODS, invalid, obj, s};

/// Which rows a subset starts from.
#[derive(Debug, Clone, PartialEq)]
pub enum SubsetRows {
    /// Every row.
    All,
    /// The selected rows.
    Selected,
    /// The rows not excluded.
    Included,
    /// Rows meeting every condition (levels of categorical columns, ranges
    /// of continuous ones).
    Matching(Vec<FilterCondition>),
}

/// A random sample drawn from the subset's rows.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleSpec {
    /// How many rows.
    pub size: SampleSize,
    /// Columns to stratify by (empty: a simple random sample).
    pub strata: Vec<String>,
    /// Seed, so the sample can be drawn again.
    pub seed: u64,
}

/// A table operation, with columns named as in its source tables.
#[derive(Debug, Clone, PartialEq)]
pub enum TableOperation {
    /// Sorted copy: `(column, descending)` keys.
    Sort {
        /// Sort keys, most significant first.
        keys: Vec<(String, bool)>,
    },
    /// Copy without rows that repeat an earlier row on `columns` (all when empty).
    RemoveDuplicates {
        /// Columns that define a duplicate.
        columns: Vec<String>,
    },
    /// Chosen rows and columns, optionally a random sample of those rows.
    Subset {
        /// Rows to start from.
        rows: SubsetRows,
        /// Columns to keep, in order.
        columns: Vec<String>,
        /// Draw a random sample of the rows.
        sample: Option<SampleSpec>,
    },
    /// This table's rows followed by the other tables' rows.
    CombineRows {
        /// Name of a column recording each row's table, if wanted.
        source_column: Option<String>,
    },
    /// Join with the other table on `(this, other)` key column pairs.
    Join {
        /// Key column pairs.
        keys: Vec<(String, String)>,
        /// Which rows to keep.
        kind: JoinKind,
        /// What to expect of the keys.
        relationship: Relationship,
    },
    /// Wide → long.
    Stack {
        /// Columns to stack.
        columns: Vec<String>,
        /// Name of the new label column.
        label: String,
        /// Name of the new value column.
        value: String,
    },
    /// Long → wide.
    Unstack {
        /// Column whose levels become columns.
        label: String,
        /// Columns whose values fill the new columns.
        values: Vec<String>,
        /// Columns identifying a result row.
        ids: Vec<String>,
    },
    /// Rows ↔ columns.
    Transpose {
        /// Columns to transpose.
        columns: Vec<String>,
        /// Column whose values name the new columns.
        label: Option<String>,
    },
    /// Statistics by group.
    Summarize {
        /// Grouping columns.
        groups: Vec<String>,
        /// Columns to summarize.
        columns: Vec<String>,
        /// Statistics for each column.
        statistics: Vec<Statistic>,
        /// Quantile definition.
        quantile_method: QuantileMethod,
        /// Use excluded rows too.
        include_excluded: bool,
    },
    /// Multiple imputation: the original rows and `m` completed copies,
    /// stacked with an imputation number (see
    /// [`crate::multiple_imputation::MiRun::stacked`]).
    MultipleImputation {
        /// Columns imputed.
        columns: Vec<String>,
        /// Model, number of imputations and per-column settings.
        settings: crate::multiple_imputation::MiSettings,
        /// Seed, so the same imputations are made again.
        seed: u64,
    },
    /// Distances between the included rows as a new table: an `Object`
    /// column and one column per object (see
    /// [`crate::distance_analysis::distance_table`]).
    DistanceMatrix {
        /// Columns the distances are measured on.
        columns: Vec<String>,
        /// The metric.
        metric: stat_multivariate::distance::DistanceMetric,
        /// Standardize numeric columns first (Euclidean and Manhattan).
        standardize: bool,
        /// Column whose values name the objects (row numbers otherwise).
        label: Option<String>,
    },
}

/// A table and its row states.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    /// The data.
    pub table: &'a DataTable,
    /// Its row states.
    pub rows: &'a RowStateManager,
}

/// How a document was made: the operation and the names of its source tables
/// (the primary table first).
#[derive(Debug, Clone, PartialEq)]
pub struct TableOrigin {
    /// The operation.
    pub operation: TableOperation,
    /// Source table names.
    pub sources: Vec<String>,
}

/// A new document made by an operation.
#[derive(Debug, Clone)]
pub struct DerivedDocument {
    /// The data.
    pub table: DataTable,
    /// Row states carried from source rows.
    pub rows: RowStateManager,
    /// Things to tell the user.
    pub notes: Vec<String>,
    /// How it was made.
    pub origin: TableOrigin,
}

fn column(table: &DataTable, name: &str) -> Result<usize> {
    table.find(name).ok_or_else(|| invalid(format!("{:?} has no column {name:?}", table.name())))
}

fn columns(table: &DataTable, names: &[String]) -> Result<Vec<usize>> {
    names.iter().map(|n| column(table, n)).collect()
}

fn sort_keys(table: &DataTable, keys: &[(String, bool)]) -> Result<Vec<SortKey>> {
    keys.iter().map(|(n, d)| Ok(SortKey { column: column(table, n)?, descending: *d })).collect()
}

/// An undoable edit that sorts `table` in place by `(column, descending)`
/// keys (stable; missing values last).
///
/// # Errors
/// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument) without keys or for an unknown column.
pub fn sort_edit(table: &DataTable, keys: &[(String, bool)]) -> Result<Edit> {
    Ok(Edit::ReorderRows { order: sort_order(table, &sort_keys(table, keys)?)? })
}

/// An undoable edit that deletes rows repeating an earlier row on `columns`
/// (all columns when empty), or `None` when there are none.
///
/// # Errors
/// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument) for an unknown column.
pub fn remove_duplicates_edit(table: &DataTable, columns_: &[String]) -> Result<Option<Edit>> {
    let rows = duplicate_rows(table, &columns(table, columns_)?)?;
    Ok((!rows.is_empty()).then_some(Edit::RemoveRows { rows }))
}

/// Label of the edit [`update_edit`] makes (shown as "Undo update from source").
pub const UPDATE_LABEL: &str = "update from source";

/// The edit that puts the contents of `fresh` (a derived table made again
/// from its current sources, for example by
/// [`ProjectFile::rerun_origin`](crate::project::ProjectFile::rerun_origin))
/// into `current`, as one undoable step labelled [`UPDATE_LABEL`]: every row
/// is replaced, same-named columns are refilled in place, new columns are
/// added at the end, columns the result no longer has are removed, and
/// formula columns added to the derived table are kept and recomputed (see
/// [`crate::external::refresh_edit`]).
///
/// # Errors
/// A removed column that a formula uses, or one listed in `in_use`
/// (columns that open reports read).
pub fn update_edit(current: &DataTable, fresh: &DataTable, in_use: &[usize]) -> Result<Edit> {
    match crate::external::refresh_edit(current, fresh, in_use)? {
        Edit::Batch { edits, .. } => Ok(Edit::Batch { label: UPDATE_LABEL.to_owned(), edits }),
        other => Ok(Edit::Batch { label: UPDATE_LABEL.to_owned(), edits: vec![other] }),
    }
}

/// `name`, or `name 2`, `name 3`, … — the first that `taken` does not
/// contain, so a new table never shares a name with an open one (operations
/// name their sources by table name).
#[must_use]
pub fn unique_name<'a>(name: &str, taken: impl IntoIterator<Item = &'a str> + Clone) -> String {
    let free = |n: &str| !taken.clone().into_iter().any(|t| t == n);
    if free(name) {
        return name.to_owned();
    }
    (2..).map(|i| format!("{name} {i}")).find(|n| free(n)).expect("some numbered name is free")
}

impl SubsetRows {
    /// The rows of `source` this choice names, ascending.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument) if a condition names an unknown column.
    pub fn rows(&self, source: Source<'_>) -> Result<Vec<usize>> {
        let n = source.table.n_rows();
        Ok(match self {
            Self::All => (0..n).collect(),
            Self::Selected => source.rows.selected_rows(),
            Self::Included => source.rows.included_rows(),
            Self::Matching(conditions) => {
                let mut keep = vec![true; n];
                for c in conditions {
                    column(source.table, c.column())?;
                    for (k, m) in keep.iter_mut().zip(c.matches(source.table)) {
                        *k &= m;
                    }
                }
                (0..n).filter(|&r| keep[r]).collect()
            }
        })
    }
}

impl TableOperation {
    /// How many other tables the operation needs besides the primary one:
    /// one for a join, any number (at least one) to combine rows.
    #[must_use]
    pub fn needs_others(&self) -> bool {
        matches!(self, Self::CombineRows { .. } | Self::Join { .. })
    }

    /// Short verb phrase for result names, e.g. `stacked`.
    #[must_use]
    pub fn past_tense(&self) -> &'static str {
        match self {
            Self::Sort { .. } => "sorted",
            Self::RemoveDuplicates { .. } => "without duplicates",
            Self::Subset { sample: Some(_), .. } => "sample",
            Self::Subset { .. } => "subset",
            Self::CombineRows { .. } => "combined",
            Self::Join { .. } => "joined",
            Self::Stack { .. } => "stacked",
            Self::Unstack { .. } => "unstacked",
            Self::Transpose { .. } => "transposed",
            Self::Summarize { .. } => "summary",
            Self::MultipleImputation { .. } => "imputed",
            Self::DistanceMatrix { .. } => "distances",
        }
    }

    /// A short name for the table the operation makes, read as a label under
    /// its source: `Summary by Fertilizer`, `Fertilizer A only`,
    /// `Stacked Before, After`, `Joined with Sites`. `others` are the other tables'
    /// names (for combining and joining). Names stay short because a table
    /// kept in its source's project is listed under that source; see
    /// [`unique_name`] to keep it distinct from open tables.
    #[must_use]
    pub fn default_name(&self, others: &[&str]) -> String {
        let list = |v: &[String]| v.join(", ");
        let name = match self {
            Self::Sort { keys } => {
                format!("Sorted by {}", keys.iter().map(|k| k.0.as_str()).collect::<Vec<_>>().join(", "))
            }
            Self::RemoveDuplicates { .. } => "Unique rows".to_owned(),
            Self::Subset { sample: Some(spec), .. } => match spec.size {
                stat_data::table_ops::SampleSize::Count(n) => format!("Random sample of {n}"),
                stat_data::table_ops::SampleSize::Fraction(f) => {
                    format!("Random sample of {}%", crate::format::format_number(f * 100.0, 4))
                }
            },
            Self::Subset { rows: SubsetRows::Selected, .. } => "Selected rows".to_owned(),
            Self::Subset { rows: SubsetRows::Included, .. } => "Included rows".to_owned(),
            Self::Subset { rows: SubsetRows::All, .. } => "Copy".to_owned(),
            Self::Subset { rows: SubsetRows::Matching(conditions), .. } => {
                let parts: Vec<String> = conditions
                    .iter()
                    .map(|c| match c {
                        FilterCondition::Levels { column, levels } => format!("{column} {}", levels.join(", ")),
                        FilterCondition::Range { column, min, max } => format!(
                            "{column} {} to {}",
                            crate::format::format_number(*min, 4),
                            crate::format::format_number(*max, 4)
                        ),
                    })
                    .collect();
                if parts.is_empty() { "Copy".to_owned() } else { format!("{} only", parts.join(", ")) }
            }
            Self::CombineRows { .. } if others.is_empty() => "Combined rows".to_owned(),
            Self::CombineRows { .. } => format!("Combined with {}", others.join(", ")),
            Self::Join { .. } => match others.first() {
                Some(other) => format!("Joined with {other}"),
                None => "Joined".to_owned(),
            },
            Self::Stack { columns, .. } => format!("Stacked {}", list(columns)),
            Self::Unstack { label, .. } => format!("Unstacked by {label}"),
            Self::Transpose { .. } => "Transposed".to_owned(),
            Self::Summarize { groups, .. } if groups.is_empty() => "Summary".to_owned(),
            Self::Summarize { groups, .. } => format!("Summary by {}", list(groups)),
            Self::MultipleImputation { settings, .. } => format!("{} imputations", settings.imputations),
            Self::DistanceMatrix { metric, .. } => format!("{} distances", metric.label()),
        };
        // Very long condition lists or column lists are cut at a word.
        const LIMIT: usize = 48;
        if name.chars().count() <= LIMIT {
            return name;
        }
        let cut: String = name.chars().take(LIMIT).collect();
        let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head).trim_end_matches(',');
        format!("{cut}…")
    }

    /// Run the operation on `primary` (and `others`, for combining and
    /// joining) and build the new document named `name`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument) for an unknown column, a missing other
    /// table, or any error of the underlying operation (see
    /// [`stat_data::table_ops`]).
    pub fn run(&self, primary: Source<'_>, others: &[Source<'_>], name: &str) -> Result<DerivedDocument> {
        let t = primary.table;
        if let Self::MultipleImputation { columns: c, settings, seed } = self {
            let run = crate::multiple_imputation::MiRun::run(t, primary.rows, &columns(t, c)?, settings, *seed)?;
            let (table, source_rows) = run.stacked(t, name)?;
            let states: Vec<stat_data::RowState> = source_rows.iter().map(|&r| primary.rows.state(r)).collect();
            return Ok(DerivedDocument {
                rows: RowStateManager::from_states(&states),
                table,
                notes: vec![format!(
                    "{} imputations of {} stacked under the original rows (Imputation 0). Reports on this table \
                     can pool their results across the imputations (Pool across imputations in a report's \
                     options).",
                    run.m(),
                    c.join(", ")
                )],
                origin: TableOrigin { operation: self.clone(), sources: vec![t.name().to_owned()] },
            });
        }
        if let Self::DistanceMatrix { columns: c, metric, standardize, label } = self {
            let spec = crate::distance_analysis::DistanceSpec { columns: columns(t, c)?, metric: *metric, standardize: *standardize };
            let label = label.as_deref().map(|l| column(t, l)).transpose()?;
            let (table, source_rows) = crate::distance_analysis::distance_table(t, primary.rows, &spec, label, name)?;
            let states: Vec<stat_data::RowState> = source_rows.iter().map(|&r| primary.rows.state(r)).collect();
            return Ok(DerivedDocument {
                rows: RowStateManager::from_states(&states),
                notes: vec![format!(
                    "{} distances between {} rows: row i and column i are the same object. Multivariate ▸ \
                     Principal coordinates, Dimension reduction and hierarchical Clustering read it as a distance \
                     matrix.",
                    metric.label(),
                    source_rows.len()
                )],
                table,
                origin: TableOrigin { operation: self.clone(), sources: vec![t.name().to_owned()] },
            });
        }
        let derived = match self {
            Self::Sort { keys } => table_ops::sorted(t, &sort_keys(t, keys)?, name)?,
            Self::RemoveDuplicates { columns: c } => table_ops::without_duplicates(t, &columns(t, c)?, name)?,
            Self::Subset { rows, columns: c, sample } => {
                let mut chosen = rows.rows(primary)?;
                if let Some(spec) = sample {
                    chosen = table_ops::sample_rows(t, &chosen, spec.size, &columns(t, &spec.strata)?, spec.seed)?;
                }
                table_ops::subset(t, &chosen, &columns(t, c)?, name)?
            }
            Self::CombineRows { source_column } => {
                if others.is_empty() {
                    return Err(invalid("choose at least one other table to combine with"));
                }
                let tables: Vec<&DataTable> = std::iter::once(t).chain(others.iter().map(|o| o.table)).collect();
                table_ops::combine_rows(&tables, source_column.as_deref(), name)?
            }
            Self::Join { keys, kind, relationship } => {
                let other = others.first().ok_or_else(|| invalid("choose a table to join with"))?.table;
                let pairs =
                    keys.iter().map(|(a, b)| Ok((column(t, a)?, column(other, b)?))).collect::<Result<Vec<_>>>()?;
                table_ops::join(t, other, &pairs, *kind, *relationship, name)?
            }
            Self::Stack { columns: c, label, value } => table_ops::stack(t, &columns(t, c)?, label, value, name)?,
            Self::Unstack { label, values, ids } => {
                table_ops::unstack(t, column(t, label)?, &columns(t, values)?, &columns(t, ids)?, name)?
            }
            Self::Transpose { columns: c, label } => {
                let label = label.as_deref().map(|l| column(t, l)).transpose()?;
                table_ops::transpose(t, &columns(t, c)?, label, name)?
            }
            Self::Summarize { groups, columns: c, statistics, quantile_method, include_excluded } => {
                let rows = if *include_excluded { (0..t.n_rows()).collect() } else { primary.rows.included_rows() };
                let options = SummarizeOptions {
                    groups: columns(t, groups)?,
                    columns: columns(t, c)?,
                    statistics: statistics.clone(),
                    quantile_method: *quantile_method,
                };
                table_ops::summarize(t, &rows, &options, name)?
            }
            Self::MultipleImputation { .. } | Self::DistanceMatrix { .. } => unreachable!("handled above"),
        };
        let sources: Vec<Source<'_>> = std::iter::once(primary).chain(others.iter().copied()).collect();
        let states: Vec<stat_data::RowState> = derived
            .origins
            .iter()
            .map(|o| o.and_then(|o| sources.get(o.source).map(|s| s.rows.state(o.row))).unwrap_or_default())
            .collect();
        Ok(DerivedDocument {
            rows: RowStateManager::from_states(&states),
            table: derived.table,
            notes: derived.notes,
            origin: TableOrigin {
                operation: self.clone(),
                sources: sources.iter().map(|s| s.table.name().to_owned()).collect(),
            },
        })
    }
}

// ----------------------------------------------------------------------------
// JSON (project files)
// ----------------------------------------------------------------------------

fn names(v: &[String]) -> JsonValue {
    JsonValue::Array(v.iter().map(|n| s(n)).collect())
}

fn read_names(v: &JsonValue, key: &str) -> Result<Vec<String>> {
    v.get(key)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| invalid(format!("origin: missing {key}")))?
        .iter()
        .map(|n| n.as_str().map(str::to_owned).ok_or_else(|| invalid(format!("origin: {key} must hold strings"))))
        .collect()
}

fn read_str(v: &JsonValue, key: &str) -> Result<String> {
    v.get(key).and_then(JsonValue::as_str).map(str::to_owned).ok_or_else(|| invalid(format!("origin: missing {key}")))
}

fn opt_str(v: &JsonValue, key: &str) -> Option<String> {
    v.get(key).and_then(JsonValue::as_str).map(str::to_owned)
}

fn join_kind_key(k: JoinKind) -> &'static str {
    match k {
        JoinKind::Inner => "inner",
        JoinKind::Left => "left",
        JoinKind::Right => "right",
        JoinKind::Full => "full",
    }
}

fn relationship_key(r: Relationship) -> &'static str {
    match r {
        Relationship::Any => "any",
        Relationship::ManyToOne => "many-to-one",
        Relationship::OneToOne => "one-to-one",
    }
}

fn statistic_to_json(st: Statistic) -> JsonValue {
    match st {
        Statistic::Quantile(p) => obj([("quantile", JsonValue::number(p))]),
        other => s(match other {
            Statistic::Count => "n",
            Statistic::Missing => "missing",
            Statistic::Mean => "mean",
            Statistic::StdDev => "sd",
            Statistic::Min => "min",
            Statistic::Max => "max",
            Statistic::Median => "median",
            _ => "sum",
        }),
    }
}

fn statistic_from_json(v: &JsonValue) -> Result<Statistic> {
    if let Some(p) = v.get("quantile").and_then(JsonValue::as_f64) {
        return Ok(Statistic::Quantile(p));
    }
    Ok(match v.as_str() {
        Some("n") => Statistic::Count,
        Some("missing") => Statistic::Missing,
        Some("mean") => Statistic::Mean,
        Some("sd") => Statistic::StdDev,
        Some("min") => Statistic::Min,
        Some("max") => Statistic::Max,
        Some("median") => Statistic::Median,
        Some("sum") => Statistic::Sum,
        _ => return Err(invalid("origin: unknown statistic")),
    })
}

/// A filter condition as JSON: `{"column": name, "levels": [text, …]}` or
/// `{"column": name, "min": x, "max": y}`. Shared with dashboard filters
/// ([`crate::dashboard`]).
pub(crate) fn condition_to_json(c: &FilterCondition) -> JsonValue {
    match c {
        FilterCondition::Levels { column, levels } => obj([("column", s(column)), ("levels", names(levels))]),
        FilterCondition::Range { column, min, max } => {
            obj([("column", s(column)), ("min", JsonValue::number(*min)), ("max", JsonValue::number(*max))])
        }
    }
}

/// Read a condition written by [`condition_to_json`]; `context` starts error
/// messages (for example `"origin"`).
pub(crate) fn condition_from_json(v: &JsonValue, context: &str) -> Result<FilterCondition> {
    let column = v
        .get("column")
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("{context}: missing column")))?;
    if let Some(levels) = v.get("levels") {
        let levels = levels
            .as_array()
            .ok_or_else(|| invalid(format!("{context}: levels must be a list")))?
            .iter()
            .map(|n| {
                n.as_str().map(str::to_owned).ok_or_else(|| invalid(format!("{context}: levels must hold strings")))
            })
            .collect::<Result<_>>()?;
        return Ok(FilterCondition::Levels { column, levels });
    }
    let num = |k: &str| v.get(k).and_then(JsonValue::as_f64).ok_or_else(|| invalid(format!("{context}: missing {k}")));
    Ok(FilterCondition::Range { column, min: num("min")?, max: num("max")? })
}

impl TableOrigin {
    /// JSON for a project file: `{"operation": key, "sources": [names], …options}`.
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let b = JsonValue::Bool;
        let pairs =
            |v: &[(String, String)]| JsonValue::Array(v.iter().map(|(a, c)| names(&[a.clone(), c.clone()])).collect());
        let (key, options): (&str, Vec<(&str, JsonValue)>) = match &self.operation {
            TableOperation::Sort { keys } => (
                "sort",
                vec![(
                    "keys",
                    JsonValue::Array(
                        keys.iter().map(|(c, d)| obj([("column", s(c)), ("descending", b(*d))])).collect(),
                    ),
                )],
            ),
            TableOperation::RemoveDuplicates { columns } => ("remove-duplicates", vec![("columns", names(columns))]),
            TableOperation::Subset { rows, columns, sample } => {
                let rows = match rows {
                    SubsetRows::All => s("all"),
                    SubsetRows::Selected => s("selected"),
                    SubsetRows::Included => s("included"),
                    SubsetRows::Matching(c) => {
                        obj([("matching", JsonValue::Array(c.iter().map(condition_to_json).collect()))])
                    }
                };
                let sample = sample.as_ref().map_or(JsonValue::Null, |sp| {
                    let size = match sp.size {
                        SampleSize::Count(n) => obj([("count", JsonValue::Number(n as f64))]),
                        SampleSize::Fraction(f) => obj([("fraction", JsonValue::number(f))]),
                    };
                    // Seeds are written as text: JSON numbers hold only 53 bits exactly.
                    obj([("size", size), ("strata", names(&sp.strata)), ("seed", s(&sp.seed.to_string()))])
                });
                ("subset", vec![("rows", rows), ("columns", names(columns)), ("sample", sample)])
            }
            TableOperation::CombineRows { source_column } => {
                ("combine-rows", vec![("source_column", source_column.as_deref().map_or(JsonValue::Null, s))])
            }
            TableOperation::Join { keys, kind, relationship } => (
                "join",
                vec![
                    ("keys", pairs(keys)),
                    ("kind", s(join_kind_key(*kind))),
                    ("relationship", s(relationship_key(*relationship))),
                ],
            ),
            TableOperation::Stack { columns, label, value } => {
                ("stack", vec![("columns", names(columns)), ("label", s(label)), ("value", s(value))])
            }
            TableOperation::Unstack { label, values, ids } => {
                ("unstack", vec![("label", s(label)), ("values", names(values)), ("ids", names(ids))])
            }
            TableOperation::Transpose { columns, label } => {
                ("transpose", vec![("columns", names(columns)), ("label", label.as_deref().map_or(JsonValue::Null, s))])
            }
            TableOperation::Summarize { groups, columns, statistics, quantile_method, include_excluded } => (
                "summarize",
                vec![
                    ("groups", names(groups)),
                    ("columns", names(columns)),
                    ("statistics", JsonValue::Array(statistics.iter().map(|st| statistic_to_json(*st)).collect())),
                    ("quantile_type", JsonValue::Number(f64::from(quantile_method.type_number()))),
                    ("include_excluded", b(*include_excluded)),
                ],
            ),
            TableOperation::MultipleImputation { columns, seed, .. } => {
                ("multiple-imputation", vec![("columns", names(columns)), ("seed", s(&seed.to_string()))])
            }
            TableOperation::DistanceMatrix { columns, metric, standardize, label } => (
                "distance-matrix",
                vec![
                    ("columns", names(columns)),
                    ("metric", s(metric.key())),
                    ("standardize", b(*standardize)),
                    ("label", label.as_deref().map_or(JsonValue::Null, s)),
                ],
            ),
        };
        let mut members = vec![("operation".to_owned(), s(key)), ("sources".to_owned(), names(&self.sources))];
        members.extend(options.into_iter().map(|(k, v)| (k.to_owned(), v)));
        if let TableOperation::MultipleImputation { settings, .. } = &self.operation {
            members.extend(settings.json_members());
        }
        JsonValue::Object(members)
    }

    /// Read what [`TableOrigin::to_json`] wrote.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`](stat_core::StatError::InvalidArgument) for an unknown operation or malformed options.
    pub fn from_json(v: &JsonValue) -> Result<Self> {
        let sources = read_names(v, "sources")?;
        let flag = |k: &str| v.get(k).and_then(JsonValue::as_bool).unwrap_or(false);
        let array =
            |k: &str| v.get(k).and_then(JsonValue::as_array).ok_or_else(|| invalid(format!("origin: missing {k}")));
        let operation = match read_str(v, "operation")?.as_str() {
            "sort" => TableOperation::Sort {
                keys: array("keys")?
                    .iter()
                    .map(|k| {
                        Ok((read_str(k, "column")?, k.get("descending").and_then(JsonValue::as_bool).unwrap_or(false)))
                    })
                    .collect::<Result<_>>()?,
            },
            "remove-duplicates" => TableOperation::RemoveDuplicates { columns: read_names(v, "columns")? },
            "subset" => {
                let rows = match v.get("rows") {
                    Some(JsonValue::String(r)) if r == "all" => SubsetRows::All,
                    Some(JsonValue::String(r)) if r == "selected" => SubsetRows::Selected,
                    Some(JsonValue::String(r)) if r == "included" => SubsetRows::Included,
                    Some(m) if m.get("matching").is_some() => SubsetRows::Matching(
                        m.get("matching")
                            .and_then(JsonValue::as_array)
                            .unwrap_or(&[])
                            .iter()
                            .map(|c| condition_from_json(c, "origin"))
                            .collect::<Result<_>>()?,
                    ),
                    _ => return Err(invalid("origin: unknown subset rows")),
                };
                let sample = match v.get("sample") {
                    None | Some(JsonValue::Null) => None,
                    Some(sp) => {
                        let size = sp.get("size").ok_or_else(|| invalid("origin: missing sample size"))?;
                        let size = if let Some(n) = size.get("count").and_then(JsonValue::as_usize) {
                            SampleSize::Count(n)
                        } else {
                            SampleSize::Fraction(
                                size.get("fraction")
                                    .and_then(JsonValue::as_f64)
                                    .ok_or_else(|| invalid("origin: bad sample size"))?,
                            )
                        };
                        let seed = read_str(sp, "seed")?.parse().map_err(|_| invalid("origin: bad seed"))?;
                        Some(SampleSpec { size, strata: read_names(sp, "strata")?, seed })
                    }
                };
                TableOperation::Subset { rows, columns: read_names(v, "columns")?, sample }
            }
            "combine-rows" => TableOperation::CombineRows { source_column: opt_str(v, "source_column") },
            "join" => TableOperation::Join {
                keys: array("keys")?
                    .iter()
                    .map(|p| match p.as_array() {
                        Some([a, c]) => Ok((
                            a.as_str().ok_or_else(|| invalid("origin: bad key"))?.to_owned(),
                            c.as_str().ok_or_else(|| invalid("origin: bad key"))?.to_owned(),
                        )),
                        _ => Err(invalid("origin: bad key pair")),
                    })
                    .collect::<Result<_>>()?,
                kind: match read_str(v, "kind")?.as_str() {
                    "inner" => JoinKind::Inner,
                    "left" => JoinKind::Left,
                    "right" => JoinKind::Right,
                    "full" => JoinKind::Full,
                    _ => return Err(invalid("origin: unknown join kind")),
                },
                relationship: match read_str(v, "relationship")?.as_str() {
                    "any" => Relationship::Any,
                    "many-to-one" => Relationship::ManyToOne,
                    "one-to-one" => Relationship::OneToOne,
                    _ => return Err(invalid("origin: unknown relationship")),
                },
            },
            "stack" => TableOperation::Stack {
                columns: read_names(v, "columns")?,
                label: read_str(v, "label")?,
                value: read_str(v, "value")?,
            },
            "unstack" => TableOperation::Unstack {
                label: read_str(v, "label")?,
                values: read_names(v, "values")?,
                ids: read_names(v, "ids")?,
            },
            "transpose" => TableOperation::Transpose { columns: read_names(v, "columns")?, label: opt_str(v, "label") },
            "summarize" => TableOperation::Summarize {
                groups: read_names(v, "groups")?,
                columns: read_names(v, "columns")?,
                statistics: array("statistics")?.iter().map(statistic_from_json).collect::<Result<_>>()?,
                quantile_method: v
                    .get("quantile_type")
                    .and_then(JsonValue::as_usize)
                    .and_then(|t| QUANTILE_METHODS.get(t.wrapping_sub(1)).copied())
                    .ok_or_else(|| invalid("origin: bad quantile type"))?,
                include_excluded: flag("include_excluded"),
            },
            "multiple-imputation" => TableOperation::MultipleImputation {
                columns: read_names(v, "columns")?,
                settings: crate::multiple_imputation::MiSettings::from_json(v)?,
                seed: read_str(v, "seed")?.parse().map_err(|_| invalid("origin: bad seed"))?,
            },
            "distance-matrix" => TableOperation::DistanceMatrix {
                columns: read_names(v, "columns")?,
                metric: {
                    let k = read_str(v, "metric")?;
                    stat_multivariate::distance::DistanceMetric::from_key(&k)
                        .ok_or_else(|| invalid(format!("origin: unknown metric {k:?}")))?
                },
                standardize: flag("standardize"),
                label: opt_str(v, "label"),
            },
            other => return Err(invalid(format!("origin: unknown operation {other:?}"))),
        };
        Ok(Self { operation, sources })
    }

    /// One line saying how the table was made, e.g.
    /// `Stacked from "Cars" (3 columns)`.
    #[must_use]
    pub fn describe(&self) -> String {
        let src = self.sources.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ");
        let count = |n: usize, what: &str| if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") };
        match &self.operation {
            TableOperation::Sort { keys } => {
                format!("Sorted copy of {src} by {}", keys.iter().map(|k| k.0.as_str()).collect::<Vec<_>>().join(", "))
            }
            TableOperation::RemoveDuplicates { .. } => format!("{src} without duplicate rows"),
            TableOperation::Subset { sample: Some(_), .. } => format!("Random sample of {src}"),
            TableOperation::Subset { .. } => format!("Subset of {src}"),
            TableOperation::CombineRows { .. } => format!("Rows of {src} combined"),
            TableOperation::Join { .. } => format!("{src} joined"),
            TableOperation::Stack { columns, .. } => format!("{src} with {} stacked", count(columns.len(), "column")),
            TableOperation::Unstack { label, .. } => format!("{src} unstacked by {label:?}"),
            TableOperation::Transpose { .. } => format!("{src} transposed"),
            TableOperation::Summarize { groups, .. } if groups.is_empty() => format!("Summary of {src}"),
            TableOperation::Summarize { groups, .. } => format!("Summary of {src} by {}", groups.join(", ")),
            TableOperation::MultipleImputation { columns, settings, .. } => {
                format!("{} imputations of {} in {src}, stacked", settings.imputations, count(columns.len(), "column"))
            }
            TableOperation::DistanceMatrix { columns, metric, .. } => {
                format!("{} distances between the rows of {src} ({})", metric.label(), count(columns.len(), "column"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editing::EditHistory;
    use crate::project::{Project, load_project, save_project};
    use stat_data::{Column, SelectionMode};

    fn sv(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    fn table() -> DataTable {
        DataTable::new(
            "Plants",
            vec![
                Column::text("site", sv(&["b", "a", "b", "a"])),
                Column::numeric("before", vec![3.0, 1.0, 2.0, 1.0]),
                Column::numeric("after", vec![4.0, 2.0, 5.0, 3.0]),
            ],
        )
        .unwrap()
    }

    fn states() -> RowStateManager {
        let mut rows = RowStateManager::new(4);
        rows.set_excluded([0], true);
        rows.select([2], SelectionMode::Replace);
        rows
    }

    fn every_operation() -> Vec<TableOperation> {
        vec![
            TableOperation::Sort { keys: vec![("site".into(), false), ("before".into(), true)] },
            TableOperation::RemoveDuplicates { columns: vec![] },
            TableOperation::Subset {
                rows: SubsetRows::Matching(vec![FilterCondition::Range {
                    column: "before".into(),
                    min: 1.5,
                    max: 9.0,
                }]),
                columns: sv(&["after", "site"]),
                sample: None,
            },
            TableOperation::Subset {
                rows: SubsetRows::All,
                columns: sv(&["site"]),
                sample: Some(SampleSpec { size: SampleSize::Fraction(0.5), strata: sv(&["site"]), seed: u64::MAX - 3 }),
            },
            TableOperation::CombineRows { source_column: Some("Source".into()) },
            TableOperation::Join {
                keys: vec![("site".into(), "site".into())],
                kind: JoinKind::Left,
                relationship: Relationship::ManyToOne,
            },
            TableOperation::Stack { columns: sv(&["before", "after"]), label: "time".into(), value: "height".into() },
            TableOperation::Unstack { label: "site".into(), values: sv(&["before"]), ids: sv(&["after"]) },
            TableOperation::Transpose { columns: sv(&["before", "after"]), label: None },
            TableOperation::Summarize {
                groups: sv(&["site"]),
                columns: sv(&["after"]),
                statistics: vec![Statistic::Mean, Statistic::Quantile(0.9), Statistic::Count],
                quantile_method: QuantileMethod::Linear,
                include_excluded: false,
            },
        ]
    }

    fn other() -> DataTable {
        DataTable::new(
            "Sites",
            vec![Column::text("site", sv(&["a", "b"])), Column::text("soil", sv(&["clay", "sand"]))],
        )
        .unwrap()
    }

    #[test]
    fn every_operation_runs_and_round_trips_through_json() {
        let (t, rows, o) = (table(), states(), other());
        let o_rows = RowStateManager::new(2);
        for op in every_operation() {
            let others = if op.needs_others() { vec![Source { table: &o, rows: &o_rows }] } else { vec![] };
            let doc = op.run(Source { table: &t, rows: &rows }, &others, "out").unwrap();
            assert_eq!(doc.table.n_rows(), doc.rows.len(), "{op:?}");
            let back = TableOrigin::from_json(&doc.origin.to_json()).unwrap();
            assert_eq!(back, doc.origin, "{op:?}");
            assert!(!doc.origin.describe().is_empty());
        }
    }

    #[test]
    fn row_states_follow_single_source_rows() {
        let (t, rows) = (table(), states());
        let sort = TableOperation::Sort { keys: vec![("before".into(), false)] };
        let doc = sort.run(Source { table: &t, rows: &rows }, &[], "s").unwrap();
        // Order: rows 1, 3, 2, 0 → row 0 (excluded) is last, row 2 (selected) third.
        assert!(doc.rows.is_excluded(3) && doc.rows.is_selected(2));
        let stack = TableOperation::Stack { columns: sv(&["before", "after"]), label: "l".into(), value: "v".into() };
        let doc = stack.run(Source { table: &t, rows: &rows }, &[], "s").unwrap();
        assert_eq!(doc.rows.counts().excluded, 0, "stacked rows are new rows");
    }

    #[test]
    fn subsets_choose_rows() {
        let (t, rows) = (table(), states());
        let src = Source { table: &t, rows: &rows };
        assert_eq!(SubsetRows::Selected.rows(src).unwrap(), vec![2]);
        assert_eq!(SubsetRows::Included.rows(src).unwrap(), vec![1, 2, 3]);
        let levels = SubsetRows::Matching(vec![FilterCondition::Levels { column: "site".into(), levels: sv(&["a"]) }]);
        assert_eq!(levels.rows(src).unwrap(), vec![1, 3]);
        let unknown = SubsetRows::Matching(vec![FilterCondition::Levels { column: "nope".into(), levels: vec![] }]);
        assert!(unknown.rows(src).is_err());
        let summary = TableOperation::Summarize {
            groups: vec![],
            columns: sv(&["before"]),
            statistics: vec![Statistic::Count],
            quantile_method: QuantileMethod::default(),
            include_excluded: false,
        };
        let doc = summary.run(src, &[], "s").unwrap();
        assert_eq!(doc.table.column(1).unwrap().cell_text(0), "3", "excluded row left out");
    }

    #[test]
    fn in_place_sort_and_duplicate_removal_undo() {
        let mut t = table();
        let mut rows = states();
        let mut h = EditHistory::default();
        h.apply(sort_edit(&t, &[("before".into(), false)]).unwrap(), &mut t, &mut rows).unwrap();
        assert_eq!(t.column(1).unwrap().as_numeric().unwrap(), &[1.0, 1.0, 2.0, 3.0]);
        assert!(rows.is_excluded(3));
        let dup = remove_duplicates_edit(&t, &sv(&["site", "before"])).unwrap().unwrap();
        h.apply(dup, &mut t, &mut rows).unwrap();
        assert_eq!(t.n_rows(), 3);
        assert!(remove_duplicates_edit(&t, &[]).unwrap().is_none());
        assert!(remove_duplicates_edit(&t, &sv(&["site", "before"])).unwrap().is_none());
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(t, table());
        assert!(rows.is_excluded(0) && rows.is_selected(2));
        assert!(sort_edit(&t, &[("nope".into(), false)]).is_err());
    }

    #[test]
    fn projects_keep_the_origin() {
        let (t, rows) = (table(), states());
        let op = TableOperation::Transpose { columns: sv(&["before"]), label: Some("site".into()) };
        let doc = op.run(Source { table: &t, rows: &rows }, &[], "Plants, transposed").unwrap();
        let project = Project {
            external: None,
            table: doc.table,
            rows: doc.rows,
            reports: vec![],
            origin: Some(doc.origin.clone()),
            scripts: Vec::new(),
        };
        let loaded = load_project(&save_project(&project)).unwrap();
        assert_eq!(loaded.origin, Some(doc.origin));
        assert_eq!(loaded.table, project.table);
        let plain = Project { origin: None, ..loaded };
        assert!(!save_project(&plain).contains("\"origin\""));
    }

    #[test]
    fn default_names_are_short_labels() {
        let names: Vec<String> = every_operation().iter().map(|op| op.default_name(&["Sites"])).collect();
        assert_eq!(
            names,
            [
                "Sorted by site, before",
                "Unique rows",
                "before 1.5 to 9 only",
                "Random sample of 50%",
                "Combined with Sites",
                "Joined with Sites",
                "Stacked before, after",
                "Unstacked by site",
                "Transposed",
                "Summary by site",
            ]
        );
        let one_level = TableOperation::Subset {
            rows: SubsetRows::Matching(vec![FilterCondition::Levels {
                column: "Fertilizer".into(),
                levels: vec!["A".into()],
            }]),
            columns: vec![],
            sample: None,
        };
        assert_eq!(one_level.default_name(&[]), "Fertilizer A only");
        let long = TableOperation::Summarize {
            groups: (0..20).map(|i| format!("group{i}")).collect(),
            columns: vec![],
            statistics: vec![],
            quantile_method: QuantileMethod::default(),
            include_excluded: false,
        };
        let name = long.default_name(&[]);
        assert!(name.chars().count() <= 49 && name.ends_with('…'), "{name}");
    }

    #[test]
    fn unique_names_count_up() {
        let taken = ["Summary", "Summary 2", "Other"];
        assert_eq!(unique_name("Summary", taken.iter().copied()), "Summary 3");
        assert_eq!(unique_name("Fresh", taken.iter().copied()), "Fresh");
    }

    #[test]
    fn update_edit_is_one_undoable_step() {
        let (t, rows) = (table(), states());
        let op = summarize_by_site();
        let mut derived = op.run(Source { table: &t, rows: &rows }, &[], "Summary by site").unwrap();
        let before = derived.table.clone();
        let mut edited = t.clone();
        let mut edited_rows = rows.clone();
        let mut history = EditHistory::default();
        history.apply(Edit::set_cell_text(&edited, 1, 2, "20").unwrap(), &mut edited, &mut edited_rows).unwrap();
        let fresh = op.run(Source { table: &edited, rows: &edited_rows }, &[], "Summary by site").unwrap();
        let edit = update_edit(&derived.table, &fresh.table, &[]).unwrap();
        assert_eq!(edit.label(), UPDATE_LABEL);
        let mut history = EditHistory::default();
        history.apply(edit, &mut derived.table, &mut derived.rows).unwrap();
        assert_eq!(derived.table.columns(), fresh.table.columns());
        history.undo(&mut derived.table, &mut derived.rows).unwrap().unwrap();
        assert_eq!(derived.table.columns(), before.columns(), "undo puts the old values back");
    }

    fn summarize_by_site() -> TableOperation {
        TableOperation::Summarize {
            groups: sv(&["site"]),
            columns: sv(&["after"]),
            statistics: vec![Statistic::Mean],
            quantile_method: QuantileMethod::default(),
            include_excluded: false,
        }
    }
}
