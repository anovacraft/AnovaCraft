//! Multivariate outlier distances of each row, bound to the row state
//! ([`stat_multivariate::outliers`]).
//!
//! The rows complete in every chosen column and not excluded give the mean
//! and covariance; each of them gets its Mahalanobis distance, jackknifed
//! distance and Hotelling T². The report plots the chosen distance by row
//! number against its reference limit (rows above it drawn as a distinct
//! shape and listed), and every point keeps its dataset row for brushing.
//! All three distances can be saved as columns.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_multivariate::{DistanceLimits, OutlierDistances, outlier_distances};

use crate::format::format_number;
use crate::report::{
    Annotations, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use crate::report_columns::ReportColumn;

const DIGITS: usize = 5;

/// Which distance the report plots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutlierDistance {
    /// Mahalanobis distance from the mean of all rows.
    #[default]
    Mahalanobis,
    /// Mahalanobis distance from the mean and covariance of the other rows.
    Jackknife,
    /// Hotelling's T² (the squared Mahalanobis distance).
    T2,
}

impl OutlierDistance {
    /// Every distance, in menu order.
    pub const ALL: [Self; 3] = [Self::Mahalanobis, Self::Jackknife, Self::T2];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Mahalanobis => "Mahalanobis distance",
            Self::Jackknife => "Jackknifed distance",
            Self::T2 => "Hotelling T²",
        }
    }

    /// Stable key used in project files, scripts and saved column keys.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Mahalanobis => "mahalanobis",
            Self::Jackknife => "jackknife",
            Self::T2 => "t2",
        }
    }

    /// The distance with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.key() == key)
    }
}

/// Columns, plotted distance and α.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlierRequest {
    /// Continuous columns (one or more).
    pub columns: Vec<usize>,
    /// The distance plotted.
    pub distance: OutlierDistance,
    /// Upper-tail probability of the reference limit for each row.
    pub alpha: f64,
}

impl OutlierRequest {
    /// Default α of the reference limits.
    pub const DEFAULT_ALPHA: f64 = 0.05;

    /// Mahalanobis distances of `columns` with α = 0.05.
    #[must_use]
    pub fn new(columns: Vec<usize>) -> Self {
        Self { columns, distance: OutlierDistance::Mahalanobis, alpha: Self::DEFAULT_ALPHA }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.columns.clone()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.columns.iter_mut().for_each(|c| *c = f(*c));
    }

    /// The project-file form (`"analysis": "outlier-distances"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        crate::project::obj([
            ("analysis", crate::project::s("outlier-distances")),
            ("columns", crate::correlation_analysis::columns_json(&self.columns, table)),
            ("distance", crate::project::s(self.distance.key())),
            ("alpha", JsonValue::number(self.alpha)),
        ])
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, an unknown distance.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let d = Self::new(crate::correlation_analysis::json_columns(v, "columns", table)?);
        let distance = match v.get("distance").and_then(JsonValue::as_str) {
            None => d.distance,
            Some(key) => OutlierDistance::from_key(key)
                .ok_or_else(|| invalid(format!("outlier distances: unknown distance {key:?}")))?,
        };
        Ok(Self { distance, alpha: v.get("alpha").and_then(JsonValue::as_f64).unwrap_or(d.alpha), ..d })
    }
}

/// Computed outlier distances.
#[derive(Debug, Clone)]
pub struct OutlierAnalysis {
    request: OutlierRequest,
    names: Vec<String>,
    n_rows: usize,
    /// Dataset row of each analysed observation.
    rows: Vec<usize>,
    distances: OutlierDistances,
    limits: DistanceLimits,
    jackknife: Vec<f64>,
    n_excluded: usize,
    n_missing: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

impl OutlierAnalysis {
    /// Compute the distances.
    ///
    /// # Errors
    /// No columns, a missing or non-numeric column, an α outside (0, 1),
    /// and errors from [`outlier_distances`] (too few rows, a singular
    /// covariance matrix).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &OutlierRequest) -> Result<Self> {
        if request.columns.is_empty() {
            return Err(invalid("choose at least one column"));
        }
        let mut names = Vec::new();
        let mut values = Vec::new();
        for &i in &request.columns {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            values.push(c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?);
            names.push(c.name.clone());
        }
        let (mut n_excluded, mut n_missing) = (0, 0);
        let mut rows = Vec::new();
        for r in 0..table.n_rows() {
            if !values.iter().all(|v| v[r].is_finite()) {
                n_missing += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                rows.push(r);
            }
        }
        let data: Vec<Vec<f64>> = rows.iter().map(|&r| values.iter().map(|v| v[r]).collect()).collect();
        let distances = outlier_distances(&data)?;
        let limits = distances.limits(request.alpha)?;
        let jackknife = distances.jackknife();
        Ok(Self {
            request: request.clone(),
            names,
            n_rows: table.n_rows(),
            rows,
            distances,
            limits,
            jackknife,
            n_excluded,
            n_missing,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &OutlierRequest {
        &self.request
    }

    /// Report title, e.g. `"Outlier Distances: x, y"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Outlier Distances: {}", self.names.join(", "))
    }

    /// Values of `distance` for the analysed observations, in row order.
    #[must_use]
    pub fn values(&self, distance: OutlierDistance) -> Vec<f64> {
        match distance {
            OutlierDistance::Mahalanobis => self.distances.mahalanobis(),
            OutlierDistance::Jackknife => self.jackknife.clone(),
            OutlierDistance::T2 => self.distances.t2(),
        }
    }

    /// Reference limit of `distance`.
    #[must_use]
    pub fn limit(&self, distance: OutlierDistance) -> f64 {
        match distance {
            OutlierDistance::Mahalanobis => self.limits.mahalanobis,
            OutlierDistance::Jackknife => self.limits.jackknife,
            OutlierDistance::T2 => self.limits.t2,
        }
    }

    /// Dataset rows whose plotted distance is above its limit.
    #[must_use]
    pub fn rows_above(&self) -> Vec<usize> {
        let limit = self.limit(self.request.distance);
        self.values(self.request.distance)
            .iter()
            .zip(&self.rows)
            .filter(|(v, _)| **v > limit)
            .map(|(_, &r)| r)
            .collect()
    }

    /// A distance for every dataset row (NaN for rows not analysed).
    #[must_use]
    pub fn column(&self, distance: OutlierDistance) -> Vec<f64> {
        let mut out = vec![f64::NAN; self.n_rows];
        for (v, &r) in self.values(distance).into_iter().zip(&self.rows) {
            out[r] = v;
        }
        out
    }

    /// Columns the report can add: `mahalanobis`, `jackknife`, `t2`.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        OutlierDistance::ALL
            .into_iter()
            .map(|d| {
                ReportColumn::continuous(d.key(), &format!("Save {}", d.label()), d.label().to_owned(), self.column(d))
            })
            .collect()
    }

    /// Report sections: summary, distance plot, rows above the limit, mean
    /// and covariance.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![self.summary(), self.plot(), self.flagged(), self.estimates()]
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        push("Columns", self.names.join(", "));
        push("Rows analysed", self.rows.len().to_string());
        if self.n_excluded > 0 {
            push("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            push("Rows with a missing value", self.n_missing.to_string());
        }
        push("Upper-tail probability α (per row)", format_number(self.request.alpha, 4));
        push("Mahalanobis limit (√ scaled beta)", format_number(self.limits.mahalanobis, DIGITS));
        push("Jackknifed limit (√ scaled F)", format_number(self.limits.jackknife, DIGITS));
        push("Hotelling T² limit (scaled beta)", format_number(self.limits.t2, DIGITS));
        push(&format!("Rows above the {} limit", self.request.distance.label()), self.rows_above().len().to_string());
        section(SectionKind::OutlierSummary, "Summary", SectionContent::Table(t))
    }

    fn plot(&self) -> ReportSection {
        let d = self.request.distance;
        let limit = self.limit(d);
        let values = self.values(d);
        let mut points = Vec::with_capacity(values.len());
        let mut membership = Vec::with_capacity(values.len());
        for (v, &r) in values.iter().zip(&self.rows) {
            // An infinite jackknifed distance (singular leave-one-out
            // covariance) is drawn at twice the limit.
            let y = if v.is_finite() { *v } else { 2.0 * limit };
            points.push(RowPoint { row: r, x: (r + 1) as f64, y });
            membership.push(usize::from(*v > limit));
        }
        let right = self.n_rows as f64 + 0.5;
        let plot = PlotSpec {
            x_label: "Row".to_owned(),
            y_label: d.label().to_owned(),
            points,
            groups: Some(PointGroups {
                title: "Rows".to_owned(),
                labels: vec!["Within the limit".to_owned(), "Above the limit".to_owned()],
                membership,
                lines: Vec::new(),
            }),
            segments: vec![vec![[0.5, limit], [right, limit]]],
            annotations: Some(Annotations {
                groups: Vec::new(),
                items: Vec::new(),
                labels: vec![(right, limit, format!("limit {}", format_number(limit, 4)))],
            }),
            ..PlotSpec::default()
        };
        let title = format!("{} by Row", d.label());
        section(SectionKind::OutlierPlot, &title, SectionContent::Plot(Box::new(plot)))
    }

    fn flagged(&self) -> ReportSection {
        let title = "Rows Above the Limit";
        let limit = self.limit(self.request.distance);
        let chosen = self.values(self.request.distance);
        let (m, t2) = (self.distances.mahalanobis(), self.distances.t2());
        let mut t = TextTable::new(["Row", "Mahalanobis", "Jackknifed", "Hotelling T²"]);
        for (i, &r) in self.rows.iter().enumerate() {
            if chosen[i] > limit {
                t.push([
                    (r + 1).to_string(),
                    format_number(m[i], DIGITS),
                    format_number(self.jackknife[i], DIGITS),
                    format_number(t2[i], DIGITS),
                ]);
            }
        }
        if t.rows.is_empty() {
            let note = format!("None: every row's {} is within its limit.", self.request.distance.label());
            return section(SectionKind::OutlierRows, title, SectionContent::Note(note));
        }
        section(SectionKind::OutlierRows, title, SectionContent::Table(t))
    }

    fn estimates(&self) -> ReportSection {
        let mut header = vec!["Column".to_owned(), "Mean".to_owned()];
        header.extend(self.names.iter().map(|n| format!("Cov. with {n}")));
        let mut t = TextTable::new(header);
        for (j, name) in self.names.iter().enumerate() {
            let mut row = vec![name.clone(), format_number(self.distances.mean[j], DIGITS)];
            row.extend(self.distances.covariance[j].iter().map(|v| format_number(*v, DIGITS)));
            t.push(row);
        }
        section(SectionKind::OutlierEstimates, "Mean and Covariance", SectionContent::Table(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::flowers;

    fn request(t: &DataTable) -> OutlierRequest {
        OutlierRequest::new(
            ["Sepal length", "Sepal width", "Petal length", "Petal width"].iter().map(|c| t.find(c).unwrap()).collect(),
        )
    }

    #[test]
    fn distances_plot_and_saved_columns() {
        let t = flowers();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([5], true);
        let a = OutlierAnalysis::run(&t, &rows, &request(&t)).unwrap();
        // Σ T² = p (n − 1) for the analysed rows.
        let t2 = a.values(OutlierDistance::T2);
        assert_eq!(t2.len(), 89);
        assert!((t2.iter().sum::<f64>() - 4.0 * 88.0).abs() < 1e-9);
        // Mahalanobis² = T²; jackknifed distances are larger.
        let m = a.values(OutlierDistance::Mahalanobis);
        let j = a.values(OutlierDistance::Jackknife);
        for i in 0..89 {
            assert!((m[i] * m[i] - t2[i]).abs() < 1e-10);
            assert!(j[i] > m[i]);
        }
        let s = a.sections();
        let SectionContent::Plot(p) = &s[1].content else { panic!("plot") };
        assert_eq!(p.points.len(), 89);
        assert!(p.points.iter().all(|q| q.row != 5 && q.x == (q.row + 1) as f64));
        let g = p.groups.as_ref().unwrap();
        let above = g.membership.iter().filter(|&&k| k == 1).count();
        assert_eq!(above, a.rows_above().len());
        let saved = a.saveable_columns();
        let keys: Vec<&str> = saved.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["mahalanobis", "jackknife", "t2"]);
        assert!(saved[0].values[5].is_nan() && saved[0].values[0].is_finite());
        assert_eq!(saved[2].values.len(), 90);
    }

    #[test]
    fn plotted_distance_sets_the_flagged_rows() {
        let t = flowers();
        let rows = RowStateManager::new(t.n_rows());
        for d in OutlierDistance::ALL {
            let r = OutlierRequest { distance: d, alpha: 0.2, ..request(&t) };
            let a = OutlierAnalysis::run(&t, &rows, &r).unwrap();
            let limit = a.limit(d);
            let values = a.column(d);
            assert!(a.rows_above().iter().all(|&row| values[row] > limit));
            assert_eq!(OutlierDistance::from_key(d.key()), Some(d));
        }
        // Mahalanobis and T² flag the same rows (one is the square of the other).
        let m = OutlierAnalysis::run(&t, &rows, &OutlierRequest { alpha: 0.2, ..request(&t) }).unwrap();
        let sq = OutlierRequest { distance: OutlierDistance::T2, alpha: 0.2, ..request(&t) };
        let t2 = OutlierAnalysis::run(&t, &rows, &sq).unwrap();
        assert_eq!(m.rows_above(), t2.rows_above());
        assert!(OutlierAnalysis::run(&t, &rows, &OutlierRequest::new(vec![0])).is_err(), "text column");
        assert!(OutlierAnalysis::run(&t, &rows, &OutlierRequest { alpha: 1.5, ..request(&t) }).is_err());
    }
}
