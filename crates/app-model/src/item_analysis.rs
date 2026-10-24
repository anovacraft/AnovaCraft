//! Item analysis of a multi-item scale, bound to the row state
//! ([`stat_multivariate::reliability`]).
//!
//! Respondents (rows) who answered every item and are not excluded take
//! part. Items marked as reverse-scored are flipped first, `x′ = low + high
//! − x`, where `low` and `high` are the ends of the response scale: given in
//! the request, or else the smallest and largest answers to any of the
//! items. The report gives Cronbach's alpha (raw, with Feldt's interval,
//! and standardized), the statistics of each item (mean, SD, item–total and
//! item–rest correlations, alpha if the item is deleted) and a heat map of
//! the inter-item correlations. The total score (after reversal) can be
//! saved as a column.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_multivariate::{ItemAnalysis, item_analysis};

use crate::format::format_number;
use crate::heat_map::HeatMapSpec;
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;

const DIGITS: usize = 4;

/// Items, reverse-scored items, scale ends and interval level.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemAnalysisRequest {
    /// Item columns (two or more).
    pub items: Vec<usize>,
    /// Items (a subset of `items`) to reverse-score.
    pub reversed: Vec<usize>,
    /// Ends of the response scale `(low, high)` used for reversing; `None`
    /// takes the smallest and largest answers.
    pub scale: Option<(f64, f64)>,
    /// Confidence level of Feldt's interval.
    pub level: f64,
}

impl ItemAnalysisRequest {
    /// Default confidence level.
    pub const DEFAULT_LEVEL: f64 = 0.95;

    /// An analysis of `items` with none reversed.
    #[must_use]
    pub fn new(items: Vec<usize>) -> Self {
        Self { items, reversed: Vec::new(), scale: None, level: Self::DEFAULT_LEVEL }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.items.clone()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.items.iter_mut().for_each(|c| *c = f(*c));
        self.reversed.iter_mut().for_each(|c| *c = f(*c));
    }

    /// The project-file form (`"analysis": "item-analysis"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let scale = self.scale.map_or(JsonValue::Null, |(low, high)| {
            JsonValue::Array(vec![JsonValue::number(low), JsonValue::number(high)])
        });
        crate::project::obj([
            ("analysis", crate::project::s("item-analysis")),
            ("items", crate::correlation_analysis::columns_json(&self.items, table)),
            ("reversed", crate::correlation_analysis::columns_json(&self.reversed, table)),
            ("scale", scale),
            ("level", JsonValue::number(self.level)),
        ])
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, a scale that is not two numbers.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let d = Self::new(crate::correlation_analysis::json_columns(v, "items", table)?);
        let scale = match v.get("scale") {
            None | Some(JsonValue::Null) => None,
            Some(x) => match x.as_array() {
                Some([low, high]) => Some((
                    low.as_f64().ok_or_else(|| invalid("item analysis: scale ends must be numbers"))?,
                    high.as_f64().ok_or_else(|| invalid("item analysis: scale ends must be numbers"))?,
                )),
                _ => return Err(invalid("item analysis: scale must be [low, high]")),
            },
        };
        Ok(Self {
            reversed: crate::correlation_analysis::json_columns(v, "reversed", table)?,
            scale,
            level: v.get("level").and_then(JsonValue::as_f64).unwrap_or(d.level),
            ..d
        })
    }
}

/// A computed item analysis.
#[derive(Debug, Clone)]
pub struct ItemAnalysisReport {
    request: ItemAnalysisRequest,
    names: Vec<String>,
    /// Whether each item was reversed.
    flipped: Vec<bool>,
    /// The scale ends used for reversing (when any item is reversed).
    scale: Option<(f64, f64)>,
    n_rows: usize,
    rows: Vec<usize>,
    /// Total score of each analysed row (after reversal).
    totals: Vec<f64>,
    analysis: ItemAnalysis,
    interval: (f64, f64),
    n_excluded: usize,
    n_missing: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

impl ItemAnalysisReport {
    /// Run the item analysis.
    ///
    /// # Errors
    /// Fewer than two items, a missing or non-numeric column, a reversed
    /// column that is not an item, scale ends that are not increasing, and
    /// errors from [`item_analysis`] (too few respondents, an item answered
    /// the same way by everyone).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ItemAnalysisRequest) -> Result<Self> {
        if request.items.len() < 2 {
            return Err(invalid("choose at least two item columns"));
        }
        if let Some(c) = request.reversed.iter().find(|c| !request.items.contains(c)) {
            return Err(invalid(format!("reversed column {c} is not one of the items")));
        }
        if let Some((low, high)) = request.scale {
            if !(low.is_finite() && high.is_finite() && low < high) {
                return Err(invalid("the scale's low end must be below its high end"));
            }
        }
        let mut names = Vec::new();
        let mut values = Vec::new();
        for &i in &request.items {
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
        let flipped: Vec<bool> = request.items.iter().map(|c| request.reversed.contains(c)).collect();
        let scale = if flipped.iter().any(|&f| f) {
            Some(request.scale.unwrap_or_else(|| {
                rows.iter()
                    .flat_map(|&r| values.iter().map(move |v| v[r]))
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)))
            }))
        } else {
            None
        };
        let data: Vec<Vec<f64>> = rows
            .iter()
            .map(|&r| {
                values
                    .iter()
                    .zip(&flipped)
                    .map(|(v, &f)| match (f, scale) {
                        (true, Some((low, high))) => low + high - v[r],
                        _ => v[r],
                    })
                    .collect()
            })
            .collect();
        let analysis = item_analysis(&data)?;
        let interval = analysis.feldt_interval(request.level)?;
        let totals = data.iter().map(|row| row.iter().sum()).collect();
        Ok(Self {
            request: request.clone(),
            names,
            flipped,
            scale,
            n_rows: table.n_rows(),
            rows,
            totals,
            analysis,
            interval,
            n_excluded,
            n_missing,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &ItemAnalysisRequest {
        &self.request
    }

    /// The computed statistics.
    #[must_use]
    pub fn analysis(&self) -> &ItemAnalysis {
        &self.analysis
    }

    /// Feldt's interval for alpha.
    #[must_use]
    pub fn interval(&self) -> (f64, f64) {
        self.interval
    }

    /// Report title, e.g. `"Item Analysis: q1, q2, q3"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Item Analysis: {}", self.names.join(", "))
    }

    /// Columns the report can add: `total` (the scale score, after reversal).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let mut values = vec![f64::NAN; self.n_rows];
        for (&r, &t) in self.rows.iter().zip(&self.totals) {
            values[r] = t;
        }
        vec![ReportColumn::continuous("total", "Save total score", "Total score".to_owned(), values)]
    }

    fn item_name(&self, j: usize) -> String {
        if self.flipped[j] { format!("{} (reversed)", self.names[j]) } else { self.names[j].clone() }
    }

    /// Report sections: reliability summary, item statistics, inter-item
    /// correlation heat map.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![self.summary(), self.items(), self.heat_map()]
    }

    fn summary(&self) -> ReportSection {
        let a = &self.analysis;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        push("Items", a.k.to_string());
        push("Respondents (rows with every item answered)", a.n.to_string());
        if self.n_excluded > 0 {
            push("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            push("Rows with an unanswered item", self.n_missing.to_string());
        }
        if let Some((low, high)) = self.scale {
            let reversed: Vec<&str> =
                self.names.iter().zip(&self.flipped).filter(|(_, f)| **f).map(|(n, _)| n.as_str()).collect();
            push("Reverse-scored items", reversed.join(", "));
            push(
                "Reversal",
                format!(
                    "{} − x (scale {} to {})",
                    format_number(low + high, 6),
                    format_number(low, 6),
                    format_number(high, 6)
                ),
            );
        }
        push("Cronbach's alpha", format_number(a.alpha, DIGITS));
        let level = format_number(self.request.level * 100.0, 4);
        push(
            &format!("{level}% interval (Feldt)"),
            format!("{} to {}", format_number(self.interval.0, DIGITS), format_number(self.interval.1, DIGITS)),
        );
        push("Standardized alpha", format_number(a.standardized_alpha, DIGITS));
        push("Mean inter-item correlation", format_number(a.mean_inter_item_correlation, DIGITS));
        push("Total score mean", format_number(a.total_mean, 6));
        push("Total score SD", format_number(a.total_std_dev, 6));
        section(SectionKind::ReliabilitySummary, "Reliability", SectionContent::Table(t))
    }

    fn items(&self) -> ReportSection {
        let mut t = TextTable::new([
            "Item",
            "Mean",
            "Std Dev",
            "Item–total r",
            "Item–rest r",
            "Alpha if deleted",
            "Std. alpha if deleted",
        ]);
        for (j, s) in self.analysis.items.iter().enumerate() {
            t.push([
                self.item_name(j),
                format_number(s.mean, 5),
                format_number(s.std_dev, 5),
                format_number(s.item_total, DIGITS),
                format_number(s.item_rest, DIGITS),
                format_number(s.alpha_if_deleted, DIGITS),
                format_number(s.standardized_alpha_if_deleted, DIGITS),
            ]);
        }
        section(SectionKind::ItemStatistics, "Item Statistics", SectionContent::Table(t))
    }

    fn heat_map(&self) -> ReportSection {
        let names: Vec<String> = (0..self.names.len()).map(|j| self.item_name(j)).collect();
        let values = self.analysis.correlations.clone();
        let text = values.iter().map(|row| row.iter().map(|v| format!("{v:.2}").replace('-', "−")).collect()).collect();
        let spec =
            HeatMapSpec { rows: names.clone(), columns: names, values, text, measure: "r".to_owned(), limit: 1.0 };
        section(SectionKind::InterItemCorrelations, "Inter-Item Correlations", SectionContent::HeatMap(spec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::{COURSE_ITEMS, course_evaluation};

    fn items(t: &DataTable) -> Vec<usize> {
        COURSE_ITEMS.iter().map(|(n, _)| t.find(n).unwrap()).collect()
    }

    fn reversed(t: &DataTable) -> Vec<usize> {
        COURSE_ITEMS.iter().filter(|(_, neg)| *neg).map(|(n, _)| t.find(n).unwrap()).collect()
    }

    #[test]
    fn reversal_matches_flipping_by_hand() {
        let t = course_evaluation();
        let rows = RowStateManager::new(t.n_rows());
        let request = ItemAnalysisRequest {
            reversed: reversed(&t),
            scale: Some((1.0, 5.0)),
            ..ItemAnalysisRequest::new(items(&t))
        };
        let a = ItemAnalysisReport::run(&t, &rows, &request).unwrap();
        assert_eq!(a.analysis().n, 116, "four rows have a blank answer");
        // The same analysis of the data flipped by hand (6 − x).
        let data: Vec<Vec<f64>> = (0..t.n_rows())
            .filter_map(|r| {
                let row: Vec<f64> = COURSE_ITEMS
                    .iter()
                    .map(|(n, neg)| {
                        let x = t.column(t.find(n).unwrap()).unwrap().as_numeric().unwrap()[r];
                        if *neg { 6.0 - x } else { x }
                    })
                    .collect();
                row.iter().all(|x| x.is_finite()).then_some(row)
            })
            .collect();
        let by_hand = item_analysis(&data).unwrap();
        assert!((a.analysis().alpha - by_hand.alpha).abs() < 1e-14);
        assert!(a.analysis().alpha > 0.8, "{}", a.analysis().alpha);
        assert!(a.analysis().items.iter().all(|i| i.item_rest > 0.3));
        let (lo, hi) = a.interval();
        assert!(lo < a.analysis().alpha && a.analysis().alpha < hi);
        // Without reversal the negatively worded items correlate negatively.
        let plain = ItemAnalysisReport::run(&t, &rows, &ItemAnalysisRequest::new(items(&t))).unwrap();
        assert!(plain.analysis().alpha < a.analysis().alpha);
        assert!(plain.analysis().items[2].item_rest < 0.0);
        // The observed range (1–5 here) is the default scale.
        let observed =
            ItemAnalysisReport::run(&t, &rows, &ItemAnalysisRequest { scale: None, ..request.clone() }).unwrap();
        assert_eq!(observed.scale, Some((1.0, 5.0)));
        // Sections and the saved total.
        let s = a.sections();
        assert_eq!(s.len(), 3);
        let SectionContent::Table(stats) = &s[1].content else { panic!() };
        assert_eq!(stats.rows[2][0], "Hard to follow (reversed)");
        let SectionContent::HeatMap(h) = &s[2].content else { panic!() };
        assert_eq!(h.values.len(), 6);
        let total = &a.saveable_columns()[0];
        assert!(total.values[7].is_nan());
        assert!((total.values[0] - data[0].iter().sum::<f64>()).abs() < 1e-12);
    }

    #[test]
    fn errors() {
        let t = course_evaluation();
        let rows = RowStateManager::new(t.n_rows());
        let it = items(&t);
        assert!(ItemAnalysisReport::run(&t, &rows, &ItemAnalysisRequest::new(vec![it[0]])).is_err());
        let stray = ItemAnalysisRequest { reversed: vec![1], ..ItemAnalysisRequest::new(it.clone()) };
        assert!(ItemAnalysisReport::run(&t, &rows, &stray).is_err(), "reversed column not an item");
        let bad_scale =
            ItemAnalysisRequest { reversed: vec![it[2]], scale: Some((5.0, 1.0)), ..ItemAnalysisRequest::new(it) };
        assert!(ItemAnalysisReport::run(&t, &rows, &bad_scale).is_err());
        assert!(ItemAnalysisReport::run(&t, &rows, &ItemAnalysisRequest::new(vec![1, 2])).is_err(), "text column");
    }
}
