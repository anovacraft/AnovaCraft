//! Distances between rows and principal coordinates analysis, bound to the
//! row state ([`stat_multivariate::distance`], [`stat_multivariate::pcoa`]).
//!
//! **Distances** are measured between the included rows from chosen
//! columns with a metric: Euclidean and Manhattan (numeric columns,
//! optionally standardized to unit variance), Gower's coefficient for mixed
//! types (continuous columns range-scaled, ordinal columns by their level
//! ranks in natural order, nominal columns as match/mismatch; missing values
//! allowed pair by pair), Jaccard on presence/absence (nonzero = present),
//! Bray–Curtis on non-negative abundances, and Hamming and simple matching
//! on the values or levels. Rows with a missing value are left out except
//! for Gower's coefficient. The distances feed
//!
//! * the **Distance matrix** table operation (Data ▸ Reshape), which makes a
//!   new table with one row and one column per object, the layout the
//!   dimension-reduction and principal-coordinates reports read as a
//!   "distance matrix";
//! * **principal coordinates analysis** (PCoA; Gower 1966): the eigenvalues
//!   of Gower's centred matrix with each one's share and the broken-stick
//!   expectation, an ordination plot of the first two axes (points keep
//!   their dataset rows for brushing), correlations of numeric columns with
//!   the axes, and saved coordinates; negative eigenvalues of non-Euclidean
//!   distances are reported and can be removed by the Lingoes or Cailliez
//!   correction (Legendre & Legendre 2012, §9.3.4);
//! * hierarchical clustering with any metric or a distance matrix
//!   ([`crate::ClusterAnalysis`]).

use crate::format::format_number;
use crate::project::{invalid as json_invalid, member, s};
use crate::report::{PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_multivariate::distance::{DistanceColumn, DistanceMatrix, DistanceMetric, VariableKind, distance_matrix};
use stat_multivariate::pcoa::{NegativeCorrection, PrincipalCoordinates, principal_coordinates};

const DIGITS: usize = 6;
/// Largest number of objects (the full matrix is kept).
pub const DISTANCE_LIMIT: usize = 3000;
/// Most coordinate axes reported and saved.
const MAX_AXES: usize = 10;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

/// Columns and metric of a distance computation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistanceSpec {
    /// The variables.
    pub columns: Vec<usize>,
    /// The metric.
    pub metric: DistanceMetric,
    /// Scale numeric columns to unit standard deviation first (Euclidean
    /// and Manhattan only).
    pub standardize: bool,
}

/// Distances between the objects (rows) used.
#[derive(Debug, Clone)]
pub struct Measured {
    /// Dataset rows of the objects, in order.
    pub rows: Vec<usize>,
    /// The distances.
    pub matrix: DistanceMatrix,
    /// Rows left out because they are excluded.
    pub n_excluded: usize,
    /// Rows left out for missing values.
    pub n_missing: usize,
}

/// Level codes of a column's cells (natural order), NaN for empty cells.
fn level_codes(table: &DataTable, c: usize, rows: &[usize]) -> Vec<f64> {
    let col = table.column(c).expect("checked column");
    let texts: Vec<String> = rows.iter().map(|&r| col.level_text(r)).collect();
    let mut levels: Vec<&String> = texts.iter().filter(|t| !t.is_empty()).collect();
    levels.sort_by(|a, b| stat_data::natural_order(a, b));
    levels.dedup();
    texts
        .iter()
        .map(
            |t| {
                if t.is_empty() { f64::NAN } else { levels.iter().position(|l| *l == t).map_or(f64::NAN, |i| i as f64) }
            },
        )
        .collect()
}

/// Measure the distances of `spec` between the included rows.
///
/// # Errors
/// No columns, a missing column, a text column for a metric that needs
/// numbers, a constant column when standardizing, fewer than two usable
/// rows, more than [`DISTANCE_LIMIT`] rows, or the metric's own errors
/// (negative values for Bray–Curtis, a pair with nothing to compare).
pub fn measure(table: &DataTable, states: &RowStateManager, spec: &DistanceSpec) -> Result<Measured> {
    if spec.columns.is_empty() {
        return Err(invalid("choose at least one column"));
    }
    let gower = spec.metric == DistanceMetric::Gower;
    let codes_ok =
        matches!(spec.metric, DistanceMetric::Gower | DistanceMetric::Hamming | DistanceMetric::SimpleMatching);
    for &c in &spec.columns {
        let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
        if col.as_numeric().is_none() && !codes_ok {
            return Err(invalid(format!(
                "{} is not numeric; the {} distance needs numbers (Gower, Hamming and simple matching accept \
                 categories)",
                col.name,
                spec.metric.label()
            )));
        }
    }
    let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
    let n_excluded = table.n_rows() - included.len();
    let missing = |c: usize, r: usize| {
        let col = table.column(c).expect("checked column");
        match col.as_numeric() {
            Some(v) => !v[r].is_finite(),
            None => col.level_text(r).is_empty(),
        }
    };
    let rows: Vec<usize> = included
        .iter()
        .copied()
        .filter(|&r| {
            if gower {
                spec.columns.iter().any(|&c| !missing(c, r))
            } else {
                spec.columns.iter().all(|&c| !missing(c, r))
            }
        })
        .collect();
    let n_missing = included.len() - rows.len();
    if rows.len() < 2 {
        return Err(StatError::InsufficientData { required: 2, found: rows.len() });
    }
    if rows.len() > DISTANCE_LIMIT {
        return Err(invalid(format!("at most {DISTANCE_LIMIT} rows; there are {}", rows.len())));
    }
    let mut columns = Vec::with_capacity(spec.columns.len());
    for &c in &spec.columns {
        let col = table.column(c).expect("checked column");
        let categorical = col.as_numeric().is_none() || col.level != MeasurementLevel::Continuous;
        let (mut values, kind) = if categorical && codes_ok {
            let kind =
                if col.level == MeasurementLevel::Ordinal { VariableKind::Ordinal } else { VariableKind::Nominal };
            (level_codes(table, c, &rows), kind)
        } else {
            let v = col.as_numeric().expect("checked numeric");
            (rows.iter().map(|&r| v[r]).collect::<Vec<f64>>(), VariableKind::Numeric)
        };
        if spec.standardize && matches!(spec.metric, DistanceMetric::Euclidean | DistanceMetric::Manhattan) {
            let n = values.len() as f64;
            let m = values.iter().sum::<f64>() / n;
            let sd = (values.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0)).sqrt();
            if !(sd > 0.0 && sd.is_finite()) {
                return Err(invalid(format!("column {:?} is constant, so it cannot be standardized", col.name)));
            }
            for x in &mut values {
                *x = (*x - m) / sd;
            }
        }
        columns.push(DistanceColumn { values, kind });
    }
    let matrix = distance_matrix(&columns, spec.metric)?;
    Ok(Measured { rows, matrix, n_excluded, n_missing })
}

/// Read a distance matrix stored as `columns` (row `i` and column `i` are
/// the same object; the table has one row per column). Excluded rows and
/// their columns are left out.
///
/// # Errors
/// No columns, a size mismatch, a non-numeric column or a missing value,
/// fewer than two objects, or a matrix that is not symmetric with a zero
/// diagonal.
pub fn matrix_from_columns(table: &DataTable, states: &RowStateManager, columns: &[usize]) -> Result<Measured> {
    let n = columns.len();
    if n == 0 {
        return Err(invalid("choose the distance matrix columns"));
    }
    if table.n_rows() != n {
        return Err(invalid(format!(
            "a distance matrix of {n} columns needs {n} rows (one per object), but the table has {}",
            table.n_rows()
        )));
    }
    let mut values = Vec::with_capacity(n);
    for &c in columns {
        let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
        values.push(col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", col.name)))?);
    }
    let rows: Vec<usize> = (0..n).filter(|&r| !states.is_excluded(r)).collect();
    if rows.len() < 2 {
        return Err(StatError::InsufficientData { required: 2, found: rows.len() });
    }
    let mut flat = Vec::with_capacity(rows.len() * rows.len());
    for &r in &rows {
        for &c in &rows {
            let v = values[c][r];
            if !v.is_finite() {
                return Err(invalid(format!("the distance matrix has no value in row {}, column {}", r + 1, c + 1)));
            }
            flat.push(v);
        }
    }
    let matrix = DistanceMatrix::new(rows.len(), flat)?;
    Ok(Measured { n_excluded: n - rows.len(), n_missing: 0, rows, matrix })
}

/// The new table of a distance matrix: an object label column (the values
/// of `label`, or the row numbers) and one distance column per object,
/// named by the labels.
///
/// # Errors
/// As [`measure`].
pub fn distance_table(
    table: &DataTable,
    states: &RowStateManager,
    spec: &DistanceSpec,
    label: Option<usize>,
    name: &str,
) -> Result<(DataTable, Vec<usize>)> {
    let m = measure(table, states, spec)?;
    let mut labels: Vec<String> = m
        .rows
        .iter()
        .map(|&r| {
            label
                .and_then(|c| table.column(c))
                .map(|c| c.level_text(r))
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| format!("Row {}", r + 1))
        })
        .collect();
    // Column names must be unique.
    let mut seen: Vec<String> = Vec::new();
    for l in &mut labels {
        let base = l.clone();
        let mut k = 2;
        while seen.contains(l) || *l == "Object" {
            *l = format!("{base} ({k})");
            k += 1;
        }
        seen.push(l.clone());
    }
    let n = m.rows.len();
    let mut cols = vec![stat_data::Column::text("Object", labels.clone())];
    for (j, l) in labels.iter().enumerate() {
        cols.push(stat_data::Column::numeric(l, (0..n).map(|i| m.matrix.get(i, j)).collect()));
    }
    let t = DataTable::new(name, cols).map_err(|e| invalid(e.to_string()))?;
    Ok((t, m.rows))
}

/// Where a principal coordinates analysis gets its distances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PcoaInput {
    /// Distances measured from variables.
    Variables(DistanceSpec),
    /// A distance matrix stored as columns.
    Matrix(Vec<usize>),
}

/// Settings of a principal coordinates analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcoaRequest {
    /// The distances.
    pub input: PcoaInput,
    /// Correction for negative eigenvalues.
    pub correction: NegativeCorrection,
    /// Column that colours and marks the points (`None`: plain).
    pub color: Option<usize>,
}

impl PcoaRequest {
    /// Gower distances of `columns`, no correction.
    #[must_use]
    pub fn new(columns: Vec<usize>) -> Self {
        Self {
            input: PcoaInput::Variables(DistanceSpec { columns, metric: DistanceMetric::Gower, standardize: false }),
            correction: NegativeCorrection::None,
            color: None,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = match &self.input {
            PcoaInput::Variables(s) => s.columns.clone(),
            PcoaInput::Matrix(c) => c.clone(),
        };
        out.extend(self.color);
        out
    }

    /// Renumbers the request's columns with `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        let columns = match &mut self.input {
            PcoaInput::Variables(s) => &mut s.columns,
            PcoaInput::Matrix(c) => c,
        };
        columns.iter_mut().for_each(|c| *c = f(*c));
        self.color = self.color.map(f);
    }

    /// Project-file form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let cols = |v: &[usize]| JsonValue::Array(v.iter().map(|&c| col(c)).collect());
        let mut m: Vec<(String, JsonValue)> = vec![("analysis".into(), s("principal-coordinates"))];
        match &self.input {
            PcoaInput::Variables(spec) => {
                m.push(("columns".into(), cols(&spec.columns)));
                m.push(("metric".into(), s(spec.metric.key())));
                m.push(("standardize".into(), JsonValue::Bool(spec.standardize)));
            }
            PcoaInput::Matrix(c) => {
                m.push(("columns".into(), cols(c)));
                m.push(("distances".into(), JsonValue::Bool(true)));
            }
        }
        m.push(("correction".into(), s(self.correction.key())));
        if let Some(c) = self.color {
            m.push(("color".into(), col(c)));
        }
        JsonValue::Object(m)
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing columns or unknown settings.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find = |name: &str| -> Result<usize> {
            table.find(name).ok_or_else(|| json_invalid(format!("report refers to a missing column {name:?}")))
        };
        let columns = member(v, "columns")?
            .as_array()
            .ok_or_else(|| json_invalid("columns must be a list"))?
            .iter()
            .map(|c| c.as_str().ok_or_else(|| json_invalid("columns: expected names")).and_then(find))
            .collect::<Result<Vec<_>>>()?;
        let input = if v.get("distances").and_then(JsonValue::as_bool).unwrap_or(false) {
            PcoaInput::Matrix(columns)
        } else {
            let metric = match v.get("metric").and_then(JsonValue::as_str) {
                None => DistanceMetric::Gower,
                Some(k) => DistanceMetric::from_key(k).ok_or_else(|| json_invalid(format!("unknown metric {k:?}")))?,
            };
            PcoaInput::Variables(DistanceSpec {
                columns,
                metric,
                standardize: v.get("standardize").and_then(JsonValue::as_bool).unwrap_or(false),
            })
        };
        let correction = match v.get("correction").and_then(JsonValue::as_str) {
            None => NegativeCorrection::None,
            Some(k) => {
                NegativeCorrection::from_key(k).ok_or_else(|| json_invalid(format!("unknown correction {k:?}")))?
            }
        };
        let color = match v.get("color").and_then(JsonValue::as_str) {
            None => None,
            Some(n) => Some(find(n)?),
        };
        Ok(Self { input, correction, color })
    }
}

/// A computed principal coordinates analysis.
#[derive(Debug, Clone)]
pub struct PcoaAnalysis {
    request: PcoaRequest,
    n_rows: usize,
    measured: Measured,
    pcoa: PrincipalCoordinates,
    /// Numeric variables and their values for the objects (variables input).
    variables: Vec<(String, Vec<f64>)>,
    /// Colour groups: title, labels and each object's group.
    groups: Option<(String, Vec<String>, Vec<usize>)>,
    source: String,
}

impl PcoaAnalysis {
    /// Measure the distances and compute the coordinates.
    ///
    /// # Errors
    /// As [`measure`] or [`matrix_from_columns`], fewer than three objects,
    /// or distances that are all zero.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &PcoaRequest) -> Result<Self> {
        let (measured, source) = match &request.input {
            PcoaInput::Variables(spec) => {
                let names: Vec<String> =
                    spec.columns.iter().filter_map(|&c| table.column(c)).map(|c| c.name.clone()).collect();
                let standardized = if spec.standardize
                    && matches!(spec.metric, DistanceMetric::Euclidean | DistanceMetric::Manhattan)
                {
                    ", standardized"
                } else {
                    ""
                };
                (
                    measure(table, states, spec)?,
                    format!("{} distances{standardized} of {}", spec.metric.label(), names.join(", ")),
                )
            }
            PcoaInput::Matrix(c) => (matrix_from_columns(table, states, c)?, "a distance matrix".to_owned()),
        };
        if measured.rows.len() < 3 {
            return Err(StatError::InsufficientData { required: 3, found: measured.rows.len() });
        }
        let pcoa = principal_coordinates(&measured.matrix, request.correction, MAX_AXES)?;
        let variables = match &request.input {
            PcoaInput::Variables(spec) => spec
                .columns
                .iter()
                .filter_map(|&c| table.column(c))
                .filter(|c| c.level == MeasurementLevel::Continuous)
                .filter_map(|c| c.as_numeric().map(|v| (c.name.clone(), measured.rows.iter().map(|&r| v[r]).collect())))
                .collect(),
            PcoaInput::Matrix(_) => Vec::new(),
        };
        let groups = request.color.and_then(|c| table.column(c)).map(|col| {
            let texts: Vec<String> = measured.rows.iter().map(|&r| col.level_text(r)).collect();
            let mut labels: Vec<String> = texts.clone();
            labels.sort_by(|a, b| stat_data::natural_order(a, b));
            labels.dedup();
            let member = texts.iter().map(|t| labels.iter().position(|l| l == t).unwrap_or(0)).collect();
            (col.name.clone(), labels, member)
        });
        Ok(Self { request: request.clone(), n_rows: table.n_rows(), measured, pcoa, variables, groups, source })
    }

    /// The coordinates solution.
    #[must_use]
    pub fn coordinates(&self) -> &PrincipalCoordinates {
        &self.pcoa
    }

    /// Dataset rows of the objects.
    #[must_use]
    pub fn rows(&self) -> &[usize] {
        &self.measured.rows
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Principal Coordinates: {}", self.source)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.eigenvalues()];
        if self.pcoa.coordinates.first().is_some_and(|r| r.len() >= 2) {
            out.push(self.ordination());
        }
        if !self.variables.is_empty() {
            out.push(self.correlations());
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(["Quantity", "Value"]);
        t.push(["Objects (rows)".to_owned(), self.measured.rows.len().to_string()]);
        if self.measured.n_excluded > 0 {
            t.push(["Excluded rows".to_owned(), self.measured.n_excluded.to_string()]);
        }
        if self.measured.n_missing > 0 {
            t.push(["Rows left out for missing values".to_owned(), self.measured.n_missing.to_string()]);
        }
        t.push(["Distances".to_owned(), self.source.clone()]);
        let negative = self.pcoa.raw_eigenvalues.iter().filter(|v| **v < -1e-10 * self.scale()).count();
        t.push(["Negative eigenvalues (before any correction)".to_owned(), negative.to_string()]);
        t.push(["Correction".to_owned(), self.request.correction.label().to_owned()]);
        if let Some(c) = self.pcoa.correction_constant {
            t.push(["Correction constant".to_owned(), format_number(c, DIGITS)]);
        }
        t.push(["Axes with positive eigenvalues".to_owned(), self.pcoa.positive_axes().to_string()]);
        section(SectionKind::PcoaSummary, "Summary", SectionContent::Table(t))
    }

    fn scale(&self) -> f64 {
        self.pcoa.raw_eigenvalues.iter().fold(0.0_f64, |m, v| m.max(v.abs())).max(1e-300)
    }

    fn eigenvalues(&self) -> ReportSection {
        let mut t = TextTable::new(["Axis", "Eigenvalue", "Share (%)", "Cumulative (%)", "Broken stick (%)"]);
        let pct = |v: f64| format_number(100.0 * v, 4);
        let tiny = 1e-10 * self.scale();
        for (i, &e) in self.pcoa.eigenvalues.iter().enumerate() {
            if e.abs() <= tiny {
                continue;
            }
            t.push([
                format!("PCo {}", i + 1),
                format_number(e, DIGITS),
                if e > 0.0 { pct(self.pcoa.relative[i]) } else { "—".to_owned() },
                if e > 0.0 { pct(self.pcoa.cumulative[i]) } else { "—".to_owned() },
                self.pcoa.broken_stick.get(i).map_or_else(|| "—".to_owned(), |b| pct(*b)),
            ]);
        }
        section(
            SectionKind::PcoaEigenvalues,
            "Eigenvalues (shares of the positive eigenvalues' sum; axes above their broken-stick share stand out \
             from random partitioning)",
            SectionContent::Table(t),
        )
    }

    fn ordination(&self) -> ReportSection {
        let points: Vec<RowPoint> = self
            .measured
            .rows
            .iter()
            .zip(&self.pcoa.coordinates)
            .map(|(&row, c)| RowPoint { row, x: c[0], y: c[1] })
            .collect();
        let share = |i: usize| format_number(100.0 * self.pcoa.relative[i], 3);
        let groups = self.groups.as_ref().map(|(title, labels, member)| PointGroups {
            title: title.clone(),
            labels: labels.clone(),
            membership: member.clone(),
            lines: vec![Vec::new(); labels.len()],
        });
        let spec = PlotSpec {
            x_label: format!("PCo 1 ({}%)", share(0)),
            y_label: format!("PCo 2 ({}%)", share(1)),
            points,
            groups,
            point_name: Some("Objects".to_owned()),
            ..PlotSpec::default()
        };
        section(
            SectionKind::PcoaPlot,
            "Ordination (first two principal coordinates)",
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn correlations(&self) -> ReportSection {
        let axes = self.pcoa.coordinates.first().map_or(0, |r| r.len()).min(4);
        let mut heads = vec!["Column".to_owned()];
        heads.extend((1..=axes).map(|i| format!("PCo {i}")));
        let mut t = TextTable::new(heads);
        for (name, values) in &self.variables {
            let mut row = vec![name.clone()];
            for a in 0..axes {
                let pairs: Vec<(f64, f64)> = values
                    .iter()
                    .zip(&self.pcoa.coordinates)
                    .filter(|(v, _)| v.is_finite())
                    .map(|(v, c)| (*v, c[a]))
                    .collect();
                row.push(format_number(pearson(&pairs), 4));
            }
            t.push(row);
        }
        section(
            SectionKind::PcoaCorrelations,
            "Correlations of Continuous Columns with the Axes (Pearson, rows with a value)",
            SectionContent::Table(t),
        )
    }

    /// Columns a report can add to the table: the first two coordinates
    /// (or as many as there are), NaN for rows not used.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let axes = self.pcoa.coordinates.first().map_or(0, |r| r.len()).min(3);
        (0..axes)
            .map(|a| {
                let mut values = vec![f64::NAN; self.n_rows];
                for (&r, c) in self.measured.rows.iter().zip(&self.pcoa.coordinates) {
                    if r < self.n_rows {
                        values[r] = c[a];
                    }
                }
                ReportColumn::continuous(
                    &format!("pco{}", a + 1),
                    &format!("Save principal coordinate {}", a + 1),
                    format!("PCo {}", a + 1),
                    values,
                )
            })
            .collect()
    }
}

/// Pearson correlation of pairs (NaN when undefined).
fn pearson(pairs: &[(f64, f64)]) -> f64 {
    let n = pairs.len() as f64;
    if pairs.len() < 3 {
        return f64::NAN;
    }
    let (mx, my) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in pairs {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx) * (x - mx);
        syy += (y - my) * (y - my);
    }
    sxy / (sxx * syy).sqrt()
}

#[cfg(test)]
mod tests;
