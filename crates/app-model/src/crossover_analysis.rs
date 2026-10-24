//! Two-period, two-treatment crossover trials
//! ([`stat_inference::crossover`]): treatment, period and carryover effects,
//! the analysis of variance, and average equivalence of a test treatment to
//! a reference by two one-sided tests, on the difference or the ratio
//! (log) scale.
//!
//! The data are stacked: one row per subject and period, with columns for
//! the response, the subject, the period and the treatment received. The
//! period and treatment columns must each have exactly two values. A
//! subject's sequence is the order in which it received the treatments.
//! Subjects without exactly one usable row in each period, or that received
//! the same treatment twice, are left out and counted in the report.

use crate::compare::{equivalence_table, ratio_scale, section};
use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, obj, s};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_inference::crossover::{CrossoverAnalysis, CrossoverEffect, CrossoverSubject, crossover_equivalence};
use stat_linear::natural_order;
use std::collections::HashMap;

const DIGITS: usize = 5;

/// Settings of a crossover report.
#[derive(Debug, Clone, PartialEq)]
pub struct CrossoverRequest {
    /// Response column (numeric).
    pub response: usize,
    /// Subject identifier column.
    pub subject: usize,
    /// Period column (two values; the first in natural order is period 1).
    pub period: usize,
    /// Treatment column (two values).
    pub treatment: usize,
    /// The reference treatment's value; `None`: the first in natural order.
    pub reference: Option<String>,
    /// Compare the ratio of geometric means (log scale) instead of the
    /// difference of means.
    pub ratio: bool,
    /// Equivalence bounds: differences, or ratios (e.g. 0.8 and 1.25) when
    /// `ratio` is set.
    pub bounds: (f64, f64),
    /// Level `α` of each one-sided test (0.05 gives a 90% interval).
    pub alpha: f64,
}

impl CrossoverRequest {
    /// Average bioequivalence defaults: ratio scale, bounds 0.80–1.25, α = 0.05.
    #[must_use]
    pub fn new(response: usize, subject: usize, period: usize, treatment: usize) -> Self {
        Self { response, subject, period, treatment, reference: None, ratio: true, bounds: (0.8, 1.25), alpha: 0.05 }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        vec![self.response, self.subject, self.period, self.treatment]
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        Self {
            response: f(self.response),
            subject: f(self.subject),
            period: f(self.period),
            treatment: f(self.treatment),
            ..self.clone()
        }
    }

    /// Project-file form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        obj([
            ("analysis", s("crossover")),
            ("response", col(self.response)),
            ("subject", col(self.subject)),
            ("period", col(self.period)),
            ("treatment", col(self.treatment)),
            ("reference", self.reference.as_deref().map_or(JsonValue::Null, s)),
            ("scale", s(if self.ratio { "ratio" } else { "difference" })),
            ("bounds", JsonValue::Array(vec![JsonValue::number(self.bounds.0), JsonValue::number(self.bounds.1)])),
            ("alpha", JsonValue::number(self.alpha)),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing columns, an unknown scale or malformed bounds.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let col = |key: &str| -> Result<usize> {
            let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let ratio = match v.get("scale").and_then(JsonValue::as_str) {
            None | Some("ratio") => true,
            Some("difference") => false,
            Some(other) => return Err(invalid(format!("crossover: unknown scale {other:?}"))),
        };
        let bounds = match v.get("bounds").and_then(JsonValue::as_array) {
            None => {
                if ratio {
                    (0.8, 1.25)
                } else {
                    return Err(invalid("crossover: equivalence bounds are required on the difference scale"));
                }
            }
            Some([a, b]) => (
                a.as_f64().ok_or_else(|| invalid("crossover: bounds must be numbers"))?,
                b.as_f64().ok_or_else(|| invalid("crossover: bounds must be numbers"))?,
            ),
            Some(_) => return Err(invalid("crossover: bounds must be two numbers")),
        };
        Ok(Self {
            response: col("response")?,
            subject: col("subject")?,
            period: col("period")?,
            treatment: col("treatment")?,
            reference: v.get("reference").and_then(JsonValue::as_str).map(str::to_owned),
            ratio,
            bounds,
            alpha: v.get("alpha").and_then(JsonValue::as_f64).unwrap_or(0.05),
        })
    }
}

/// A computed crossover report.
#[derive(Debug, Clone)]
pub struct CrossoverReport {
    request: CrossoverRequest,
    response_name: String,
    /// Period labels in order.
    periods: [String; 2],
    /// Reference and test treatment labels.
    treatments: [String; 2],
    /// Subjects as used: label and the analysis input.
    subjects: Vec<(String, CrossoverSubject)>,
    /// Subjects left out (incomplete or the same treatment twice).
    dropped: usize,
    /// The analysis.
    pub analysis: CrossoverAnalysis,
}

/// The two distinct values of a column among `rows`, in natural order.
fn two_values(table: &DataTable, column: usize, rows: &[usize], what: &str) -> Result<[String; 2]> {
    let c = table.column(column).ok_or_else(|| invalid("no such column"))?;
    let mut values: Vec<String> = Vec::new();
    for &r in rows {
        let v = c.level_text(r);
        if !values.contains(&v) {
            values.push(v);
        }
    }
    values.sort_by(|a, b| natural_order(a, b));
    match <[String; 2]>::try_from(values) {
        Ok(v) => Ok(v),
        Err(v) => {
            Err(invalid(format!("the {what} column {} must have exactly two values (found {})", c.name, v.len())))
        }
    }
}

impl CrossoverReport {
    /// Pair each subject's two periods and run the analysis.
    ///
    /// # Errors
    /// Invalid or repeated columns, a period or treatment column without
    /// exactly two values, an unknown reference, or the engine's errors
    /// (too few subjects, non-positive responses on the ratio scale).
    pub fn run(table: &DataTable, rows: &RowStateManager, request: &CrossoverRequest) -> Result<Self> {
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("the response, subject, period and treatment columns must all differ"));
        }
        let response = table.column(request.response).ok_or_else(|| invalid("no such column"))?;
        let y = response.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", response.name)))?;
        let text = |c: usize, r: usize| table.column(c).map(|c| c.level_text(r)).unwrap_or_default();
        let used: Vec<usize> = (0..table.n_rows())
            .filter(|&r| {
                !rows.is_excluded(r)
                    && !y[r].is_nan()
                    && [request.subject, request.period, request.treatment].iter().all(|&c| !text(c, r).is_empty())
            })
            .collect();
        let periods = two_values(table, request.period, &used, "period")?;
        let found = two_values(table, request.treatment, &used, "treatment")?;
        let treatments = match &request.reference {
            None => found,
            Some(r) if *r == found[0] => found,
            Some(r) if *r == found[1] => [found[1].clone(), found[0].clone()],
            Some(r) => return Err(invalid(format!("the reference treatment {r:?} does not occur in the data"))),
        };
        // Subject → rows in period 1 and 2 (treatment is test?, value).
        let mut order: Vec<String> = Vec::new();
        let mut by_subject: HashMap<String, [Vec<(bool, f64)>; 2]> = HashMap::new();
        for &r in &used {
            let subject = text(request.subject, r);
            let p = usize::from(text(request.period, r) == periods[1]);
            let is_test = text(request.treatment, r) == treatments[1];
            let entry = by_subject.entry(subject.clone()).or_insert_with(|| {
                order.push(subject);
                [Vec::new(), Vec::new()]
            });
            entry[p].push((is_test, y[r]));
        }
        order.sort_by(|a, b| natural_order(a, b));
        let mut subjects = Vec::new();
        let mut dropped = 0;
        for name in order {
            match &by_subject[&name] {
                [a, b] if a.len() == 1 && b.len() == 1 && a[0].0 != b[0].0 => {
                    subjects.push((name, CrossoverSubject { test_first: a[0].0, period1: a[0].1, period2: b[0].1 }))
                }
                _ => dropped += 1,
            }
        }
        let input: Vec<CrossoverSubject> = subjects.iter().map(|(_, s)| *s).collect();
        let analysis = crossover_equivalence(&input, request.bounds, request.alpha, request.ratio)?;
        Ok(Self {
            request: request.clone(),
            response_name: response.name.clone(),
            periods,
            treatments,
            subjects,
            dropped,
            analysis,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &CrossoverRequest {
        &self.request
    }

    /// The reference and test treatments' values.
    #[must_use]
    pub fn treatments(&self) -> &[String; 2] {
        &self.treatments
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Crossover Equivalence: {} ({} vs {})", self.response_name, self.treatments[1], self.treatments[0])
    }

    /// Sequence label: the treatments in the order received.
    fn sequence(&self, test_first: bool) -> String {
        let [r, t] = &self.treatments;
        if test_first { format!("{t} → {r}") } else { format!("{r} → {t}") }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let a = &self.analysis;
        let mut out = Vec::new();
        // Summary per sequence.
        let mut summary = TextTable::new([
            "Sequence".to_owned(),
            "Subjects".to_owned(),
            format!("Mean, {}", self.periods[0]),
            format!("Mean, {}", self.periods[1]),
        ]);
        for test_first in [false, true] {
            let group: Vec<&CrossoverSubject> =
                self.subjects.iter().map(|(_, s)| s).filter(|s| s.test_first == test_first).collect();
            let n = group.len() as f64;
            let m1 = group.iter().map(|s| s.period1).sum::<f64>() / n;
            let m2 = group.iter().map(|s| s.period2).sum::<f64>() / n;
            summary.push([
                self.sequence(test_first),
                group.len().to_string(),
                format_number(m1, DIGITS),
                format_number(m2, DIGITS),
            ]);
        }
        let dropped = if self.dropped > 0 {
            format!(
                "; {} subjects without one {} and one {} row left out",
                self.dropped, self.treatments[0], self.treatments[1]
            )
        } else {
            String::new()
        };
        out.push(section(
            SectionKind::CrossoverSummary,
            format!("Sequences (reference: {}; test: {}){dropped}", self.treatments[0], self.treatments[1]),
            SectionContent::Table(summary),
        ));
        // Effects.
        let scale = if a.log_scale { " (log scale)" } else { "" };
        let df = (a.sizes.0 + a.sizes.1 - 2) as f64;
        let mut effects = TextTable::new(["Effect", "Estimate", "Std error", "t", "DF", "p-value"]);
        let row = |name: String, e: &CrossoverEffect| {
            [
                name,
                format_number(e.estimate, DIGITS),
                format_number(e.std_error, DIGITS),
                format_number(e.t, 4),
                format_number(df, 4),
                format_p_value(e.p_value),
            ]
        };
        effects.push(row(format!("Treatment ({} − {})", self.treatments[1], self.treatments[0]), &a.treatment));
        effects.push(row(format!("Period ({} − {})", self.periods[1], self.periods[0]), &a.period));
        effects.push(row(
            format!("Carryover ({} − {} subject totals)", self.sequence(true), self.sequence(false)),
            &a.carryover,
        ));
        out.push(section(
            SectionKind::CrossoverEffects,
            format!("Treatment, Period and Carryover Effects{scale}"),
            SectionContent::Table(effects),
        ));
        // ANOVA.
        let mut anova = TextTable::new(["Source", "DF", "Sum of squares", "Mean square", "F", "p-value"]);
        for r in &a.anova {
            let blank_test = r.f.is_nan();
            anova.push([
                r.source.to_owned(),
                format_number(r.df, 4),
                format_number(r.sum_sq, DIGITS),
                if r.mean_sq.is_nan() { String::new() } else { format_number(r.mean_sq, DIGITS) },
                if blank_test { String::new() } else { format_number(r.f, 4) },
                if blank_test { String::new() } else { format_p_value(r.p_value) },
            ]);
        }
        out.push(section(
            SectionKind::CrossoverAnova,
            format!("Analysis of Variance{scale} (carryover tested against subjects within sequences)"),
            SectionContent::Table(anova),
        ));
        // Equivalence.
        let (title, test) = if a.log_scale {
            let cv = if a.within_cv.is_finite() {
                format!("; within-subject CV {}%", format_number(100.0 * a.within_cv, 4))
            } else {
                String::new()
            };
            (
                format!(
                    "Average Equivalence: ratio {} / {} of geometric means (least-squares geometric means {} and {}; \
                     TOST at α = {}; estimate and interval as ratios{cv})",
                    self.treatments[1],
                    self.treatments[0],
                    format_number(a.means.1.exp(), DIGITS),
                    format_number(a.means.0.exp(), DIGITS),
                    format_number(a.equivalence.alpha, 4),
                ),
                ratio_scale(&a.equivalence),
            )
        } else {
            (
                format!(
                    "Average Equivalence: difference {} − {} (least-squares means {} and {}; TOST at α = {})",
                    self.treatments[1],
                    self.treatments[0],
                    format_number(a.means.1, DIGITS),
                    format_number(a.means.0, DIGITS),
                    format_number(a.equivalence.alpha, 4),
                ),
                a.equivalence.clone(),
            )
        };
        out.push(section(
            SectionKind::CrossoverEquivalence,
            title,
            SectionContent::Table(equivalence_table(None, vec![(None, Ok(test))])),
        ));
        // Subject profiles.
        let mut items = Vec::new();
        let mut segments = Vec::new();
        for (_, s) in &self.subjects {
            let g = usize::from(s.test_first);
            items.push((1.0, s.period1, String::new(), g));
            items.push((2.0, s.period2, String::new(), g));
            segments.push(vec![[1.0, s.period1], [2.0, s.period2]]);
        }
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: "Period".to_owned(),
            y_label: self.response_name.clone(),
            points: Vec::new(),
            line: None,
            band: None,
            groups: None,
            x_ticks: Some(vec![(1.0, self.periods[0].clone()), (2.0, self.periods[1].clone())]),
            panes: Vec::new(),
            annotations: Some(Annotations {
                groups: vec![self.sequence(false), self.sequence(true)],
                items,
                labels: Vec::new(),
            }),
            segments,
            ..PlotSpec::default()
        };
        out.push(section(
            SectionKind::CrossoverPlot,
            "Subject Profiles (one line per subject; marker: sequence)",
            SectionContent::Plot(Box::new(spec)),
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table_of(s: &[ReportSection], kind: SectionKind) -> &TextTable {
        match &s.iter().find(|x| x.kind == kind).unwrap().content {
            SectionContent::Table(t) => t,
            _ => panic!(),
        }
    }

    /// A small crossover in stacked form; the effects themselves are checked
    /// against a least-squares reference in `stat_inference::crossover`, and
    /// here against the same engine called directly.
    fn trial() -> DataTable {
        // subject, sequence (true: test first), period 1, period 2
        let data = [
            ("1", false, 74.7, 81.0),
            ("2", false, 96.4, 89.2),
            ("3", false, 101.9, 97.5),
            ("4", false, 79.6, 82.2),
            ("5", true, 85.0, 78.2),
            ("6", true, 89.5, 94.1),
            ("7", true, 75.3, 80.6),
            ("8", true, 102.3, 91.8),
            // An incomplete subject, left out.
            ("9", true, 90.0, f64::NAN),
        ];
        let (mut subj, mut per, mut trt, mut y) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (s, tf, a, b) in data {
            for (p, v) in [(1, a), (2, b)] {
                subj.push(s.to_owned());
                per.push(format!("P{p}"));
                trt.push(if (p == 1) == tf { "T".to_owned() } else { "R".to_owned() });
                y.push(v);
            }
        }
        DataTable::new(
            "trial",
            vec![
                Column::text("Subject", subj),
                Column::text("Period", per),
                Column::text("Formulation", trt),
                Column::numeric("AUC", y),
            ],
        )
        .unwrap()
    }

    #[test]
    fn stacked_rows_pair_into_subjects() {
        let t = trial();
        let req = CrossoverRequest::new(3, 0, 1, 2);
        let a = CrossoverReport::run(&t, &RowStateManager::new(t.n_rows()), &req).unwrap();
        assert_eq!(a.dropped, 1);
        assert_eq!(a.analysis.sizes, (4, 4));
        let direct: Vec<CrossoverSubject> = a.subjects.iter().map(|(_, s)| *s).collect();
        let engine = crossover_equivalence(&direct, (0.8, 1.25), 0.05, true).unwrap();
        assert_eq!(engine.treatment, a.analysis.treatment);
        assert_eq!(engine.equivalence, a.analysis.equivalence);
        assert!(!a.subjects[0].1.test_first && a.subjects[4].1.test_first);
        let s = a.sections();
        assert_eq!(table_of(&s, SectionKind::CrossoverSummary).rows[0][0], "R → T");
        assert_eq!(table_of(&s, SectionKind::CrossoverEffects).rows.len(), 3);
        let eq = table_of(&s, SectionKind::CrossoverEquivalence);
        assert_eq!(eq.rows[0][1], "0.8");
        assert!(s.iter().any(|x| x.kind == SectionKind::CrossoverPlot));
        // Swapping the reference inverts the ratio.
        let swapped = CrossoverRequest { reference: Some("T".into()), ..req.clone() };
        let b = CrossoverReport::run(&t, &RowStateManager::new(t.n_rows()), &swapped).unwrap();
        assert!((b.analysis.treatment.estimate + a.analysis.treatment.estimate).abs() < 1e-12);
        // Round trip.
        assert_eq!(CrossoverRequest::from_json(&req.to_json(&t), &t).unwrap(), req);
        let diff = CrossoverRequest { ratio: false, bounds: (-5.0, 5.0), ..req };
        assert_eq!(CrossoverRequest::from_json(&diff.to_json(&t), &t).unwrap(), diff);
        assert!(CrossoverReport::run(&t, &RowStateManager::new(t.n_rows()), &diff).is_ok());
    }

    #[test]
    fn rejects_bad_layouts() {
        let t = trial();
        let rows = RowStateManager::new(t.n_rows());
        assert!(CrossoverReport::run(&t, &rows, &CrossoverRequest::new(3, 0, 0, 2)).is_err());
        // Subject as the period column has nine values.
        assert!(CrossoverReport::run(&t, &rows, &CrossoverRequest::new(3, 1, 0, 2)).is_err());
        let unknown = CrossoverRequest { reference: Some("X".into()), ..CrossoverRequest::new(3, 0, 1, 2) };
        assert!(CrossoverReport::run(&t, &rows, &unknown).is_err());
    }
}
