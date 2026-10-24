//! Latent class analysis of several categorical columns, bound to the row
//! state ([`stat_multivariate::latent_class`]).
//!
//! Each chosen column is an item; its levels are the distinct non-empty
//! values on the analysed rows, in natural order. Rows that are excluded,
//! or that answer none of the items, are left out; a row with some items
//! missing is kept, and its missing items are skipped in the likelihood.
//! Every number of classes in the requested range is fitted from several
//! random starts and the smallest BIC chooses the model. The report shows
//! the model-selection table and plot, the chosen model's fit, class sizes,
//! item-response probabilities with standard errors, a profile plot of the
//! probabilities by class, and the average posterior probabilities by
//! assigned class. The most likely class and the posterior probabilities
//! can be saved as columns.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager, natural_order};
use stat_multivariate::{LatentClassFit, LatentClassOptions, LatentClassSelection, select_latent_classes};

use crate::format::format_number;
use crate::report::{
    Annotations, PlotPane, PlotSpec, PointGroups, Polyline, ReportSection, SectionContent, SectionKind, TextTable,
};
use crate::report_columns::ReportColumn;

/// Most levels an item may have (more suggests a continuous column).
pub const MAX_LEVELS: usize = 30;

/// Items, range of classes and starts of a latent class analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct LatentClassRequest {
    /// Categorical item columns (two or more).
    pub items: Vec<usize>,
    /// Smallest number of classes tried.
    pub min_classes: usize,
    /// Largest number of classes tried.
    pub max_classes: usize,
    /// Random EM starts per number of classes.
    pub starts: usize,
    /// Dirichlet smoothing added to every level count (0: maximum likelihood).
    pub smoothing: f64,
    /// Seed of the random starts.
    pub seed: u64,
}

impl LatentClassRequest {
    /// Default largest number of classes.
    pub const DEFAULT_MAX: usize = 5;

    /// 1 to 5 classes, 10 starts, no smoothing, seed 1.
    #[must_use]
    pub fn new(items: Vec<usize>) -> Self {
        Self { items, min_classes: 1, max_classes: Self::DEFAULT_MAX, starts: 10, smoothing: 0.0, seed: 1 }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.items.clone()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.items.iter_mut().for_each(|c| *c = f(*c));
    }

    /// The project-file form (`"analysis": "latent-class"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        use crate::project::{obj, s};
        obj([
            ("analysis", s("latent-class")),
            ("items", crate::correlation_analysis::columns_json(&self.items, table)),
            ("min_classes", JsonValue::Number(self.min_classes as f64)),
            ("max_classes", JsonValue::Number(self.max_classes as f64)),
            ("starts", JsonValue::Number(self.starts as f64)),
            ("smoothing", JsonValue::Number(self.smoothing)),
            ("seed", JsonValue::Number(self.seed as f64)),
        ])
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown item columns.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let d = Self::new(crate::correlation_analysis::json_columns(v, "items", table)?);
        let usize_or = |key: &str, default: usize| v.get(key).and_then(JsonValue::as_usize).unwrap_or(default);
        Ok(Self {
            min_classes: usize_or("min_classes", d.min_classes),
            max_classes: usize_or("max_classes", d.max_classes),
            starts: usize_or("starts", d.starts),
            smoothing: v.get("smoothing").and_then(JsonValue::as_f64).unwrap_or(d.smoothing),
            seed: v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(1),
            ..d
        })
    }
}

/// A computed latent class analysis.
#[derive(Debug, Clone)]
pub struct LatentClassAnalysis {
    request: LatentClassRequest,
    names: Vec<String>,
    /// Level labels of each item.
    levels: Vec<Vec<String>>,
    n_rows: usize,
    rows: Vec<usize>,
    selection: LatentClassSelection,
    n_excluded: usize,
    n_unanswered: usize,
    n_partial: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

fn percent(p: f64) -> String {
    format!("{:.1}%", 100.0 * p)
}

/// A value, or "—" when it is not available.
fn number_or_dash(x: f64, digits: usize) -> String {
    if x.is_finite() { format_number(x, digits) } else { "—".to_owned() }
}

impl LatentClassAnalysis {
    /// Fit every number of classes in the range and choose by BIC.
    ///
    /// # Errors
    /// Fewer than two items, a missing or continuous column, an item with
    /// fewer than two or more than [`MAX_LEVELS`] levels, an invalid range
    /// of classes, no starts, a negative smoothing, too few rows, or every
    /// fit failing.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &LatentClassRequest) -> Result<Self> {
        if request.items.len() < 2 {
            return Err(invalid("choose at least two items"));
        }
        if request.min_classes == 0 || request.min_classes > request.max_classes {
            return Err(invalid("the numbers of classes must run from at least 1 up to a larger or equal number"));
        }
        if request.starts == 0 {
            return Err(invalid("use at least one start"));
        }
        if !(request.smoothing >= 0.0 && request.smoothing.is_finite()) {
            return Err(invalid("the smoothing must be zero or positive"));
        }
        let mut names = Vec::new();
        let mut labels: Vec<Vec<String>> = Vec::new();
        for &i in &request.items {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            if c.level == MeasurementLevel::Continuous && c.as_numeric().is_some() {
                return Err(invalid(format!(
                    "latent class analysis uses categorical items; {:?} is continuous (set its measurement level to \
                     nominal or ordinal)",
                    c.name
                )));
            }
            names.push(c.name.clone());
            labels.push((0..table.n_rows()).map(|r| c.level_text(r).trim().to_owned()).collect());
        }
        let (mut n_excluded, mut n_unanswered) = (0, 0);
        let mut rows = Vec::new();
        for r in 0..table.n_rows() {
            if states.is_excluded(r) {
                n_excluded += 1;
            } else if labels.iter().all(|l| l[r].is_empty()) {
                n_unanswered += 1;
            } else {
                rows.push(r);
            }
        }
        let mut levels: Vec<Vec<String>> = Vec::new();
        for (j, l) in labels.iter().enumerate() {
            let mut lv: Vec<String> = rows.iter().map(|&r| l[r].clone()).filter(|v| !v.is_empty()).collect();
            lv.sort_by(|a, b| natural_order(a, b));
            lv.dedup();
            if lv.len() < 2 {
                return Err(invalid(format!("the item {:?} needs at least two levels", names[j])));
            }
            if lv.len() > MAX_LEVELS {
                return Err(invalid(format!(
                    "the item {:?} has {} levels; latent class analysis takes items with at most {MAX_LEVELS}",
                    names[j],
                    lv.len()
                )));
            }
            levels.push(lv);
        }
        if rows.len() < request.max_classes.max(2) {
            return Err(StatError::InsufficientData { required: request.max_classes.max(2), found: rows.len() });
        }
        let responses: Vec<Vec<Option<usize>>> = rows
            .iter()
            .map(|&r| labels.iter().zip(&levels).map(|(l, lv)| lv.iter().position(|v| *v == l[r])).collect::<Vec<_>>())
            .collect();
        let n_partial = responses.iter().filter(|row| row.iter().any(Option::is_none)).count();
        let counts: Vec<usize> = levels.iter().map(Vec::len).collect();
        let base = LatentClassOptions {
            starts: request.starts,
            smoothing: request.smoothing,
            seed: request.seed,
            ..LatentClassOptions::new(1)
        };
        let ks: Vec<usize> = (request.min_classes..=request.max_classes).collect();
        let selection = select_latent_classes(&responses, &counts, &ks, &base)?;
        Ok(Self {
            request: request.clone(),
            names,
            levels,
            n_rows: table.n_rows(),
            rows,
            selection,
            n_excluded,
            n_unanswered,
            n_partial,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &LatentClassRequest {
        &self.request
    }

    /// The chosen model.
    #[must_use]
    pub fn chosen(&self) -> &LatentClassFit {
        self.selection.chosen()
    }

    /// Every number of classes fitted.
    #[must_use]
    pub fn selection(&self) -> &LatentClassSelection {
        &self.selection
    }

    /// Level labels of each item, in model order.
    #[must_use]
    pub fn levels(&self) -> &[Vec<String>] {
        &self.levels
    }

    /// Dataset rows analysed, in order.
    #[must_use]
    pub fn rows(&self) -> &[usize] {
        &self.rows
    }

    /// Report title, e.g. `"Latent Class Analysis (3 classes): a, b, c"`.
    #[must_use]
    pub fn title(&self) -> String {
        let k = self.chosen().k();
        let items = if self.names.len() > 6 {
            format!("{}, … ({} items)", self.names[..3].join(", "), self.names.len())
        } else {
            self.names.join(", ")
        };
        format!(
            "Latent Class Analysis ({}): {items}",
            if k == 1 { "1 class".to_owned() } else { format!("{k} classes") }
        )
    }

    fn per_row(&self, values: impl Iterator<Item = f64>) -> Vec<f64> {
        let mut out = vec![f64::NAN; self.n_rows];
        for (v, &r) in values.zip(&self.rows) {
            out[r] = v;
        }
        out
    }

    /// Columns the report can add: `class` (most likely class, nominal) and
    /// `probability_1`, `probability_2`, … (posterior class probabilities).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let c = self.chosen();
        let mut class = ReportColumn::continuous(
            "class",
            "Save most likely class",
            "Latent class".to_owned(),
            self.per_row(c.classification().into_iter().map(|k| (k + 1) as f64)),
        );
        class.level = MeasurementLevel::Nominal;
        let mut out = vec![class];
        out.extend((0..c.k()).map(|k| {
            ReportColumn::continuous(
                &format!("probability_{}", k + 1),
                &format!("Save probability of class {}", k + 1),
                format!("Probability class {}", k + 1),
                self.per_row(c.posterior.iter().map(|h| h[k])),
            )
        }));
        out
    }

    /// Report sections: chosen model, model selection table and plot, class
    /// sizes, item-response probabilities, profile plot, classification.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![
            self.summary(),
            self.selection_table(),
            self.criteria_plot(),
            self.class_sizes(),
            self.probability_table(),
            self.profile_plot(),
            self.classification_table(),
        ]
    }

    fn summary(&self) -> ReportSection {
        let c = self.chosen();
        let g = &c.pattern_fit;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        push("Items", self.names.join(", "));
        push("Rows analysed", self.rows.len().to_string());
        if self.n_partial > 0 {
            push("Rows with some items missing (kept)", self.n_partial.to_string());
        }
        if self.n_unanswered > 0 {
            push("Rows answering no item (left out)", self.n_unanswered.to_string());
        }
        if self.n_excluded > 0 {
            push("Rows excluded", self.n_excluded.to_string());
        }
        push("Classes (chosen by BIC)", c.k().to_string());
        push("Log-likelihood", format_number(c.log_likelihood, 8));
        push("Parameters", c.n_parameters().to_string());
        push("AIC", format_number(c.aic(), 8));
        push("BIC (smaller is better)", format_number(c.bic(), 8));
        push("Sample-size-adjusted BIC", format_number(c.adjusted_bic(), 8));
        push("Entropy R²", format_number(c.entropy_r2(), 4));
        push(
            "Response patterns",
            format!(
                "{} observed among {} complete rows, of {} possible",
                g.observed_patterns,
                g.complete_rows,
                format_number(g.cells, 6)
            ),
        );
        let p = |x: f64| if x.is_finite() { format_number(x, 4) } else { "—".to_owned() };
        push(
            "Likelihood-ratio G²",
            format!("{} (df {}, p {})", format_number(g.g2, 6), format_number(g.df, 6), p(g.g2_p)),
        );
        push("Pearson X²", format!("{} (df {}, p {})", format_number(g.chi2, 6), format_number(g.df, 6), p(g.chi2_p)));
        push(
            "Starts",
            format!(
                "{} of {} reached the best log-likelihood (seed {})",
                c.starts_at_best, c.starts, self.request.seed
            ),
        );
        if c.smoothing > 0.0 {
            push("Smoothing", format!("{} added to every level count", format_number(c.smoothing, 4)));
        }
        if !c.converged {
            push("Note", "EM stopped at its iteration limit before converging".to_owned());
        }
        if g.cells > 5.0 * g.complete_rows as f64 {
            push(
                "Note",
                "Most response patterns are rare, so the G² and X² p-values are unreliable; compare models by BIC"
                    .to_owned(),
            );
        }
        section(SectionKind::LatentClassSummary, "Chosen Model", SectionContent::Table(t))
    }

    fn selection_table(&self) -> ReportSection {
        let mut t = TextTable::new([
            "Classes",
            "Log-likelihood",
            "Parameters",
            "AIC",
            "BIC",
            "Adjusted BIC",
            "Entropy R²",
            "G²",
            "df",
            "Starts at best",
        ]);
        for (i, cand) in self.selection.candidates.iter().enumerate() {
            match &cand.fit {
                Ok(f) => {
                    let bic = format_number(f.bic(), 7);
                    t.push([
                        cand.k.to_string(),
                        format_number(f.log_likelihood, 8),
                        f.n_parameters().to_string(),
                        format_number(f.aic(), 7),
                        if i == self.selection.best { format!("{bic} (chosen)") } else { bic },
                        format_number(f.adjusted_bic(), 7),
                        format_number(f.entropy_r2(), 4),
                        format_number(f.pattern_fit.g2, 6),
                        format_number(f.pattern_fit.df, 6),
                        format!("{} of {}", f.starts_at_best, f.starts),
                    ]);
                }
                Err(why) => {
                    let mut row = vec![cand.k.to_string(), format!("not fitted: {why}")];
                    row.extend(std::iter::repeat_n(String::new(), 8));
                    t.push(row);
                }
            }
        }
        section(
            SectionKind::LatentClassSelection,
            "Model Selection by Number of Classes (smaller criteria are better)",
            SectionContent::Table(t),
        )
    }

    fn criteria_plot(&self) -> ReportSection {
        let names = ["BIC", "AIC", "Adjusted BIC"];
        let value = |f: &LatentClassFit, m: usize| match m {
            0 => f.bic(),
            1 => f.aic(),
            _ => f.adjusted_bic(),
        };
        let lines: Vec<Polyline> = (0..3)
            .map(|m| {
                self.selection
                    .candidates
                    .iter()
                    .filter_map(|c| c.fit.as_ref().ok().map(|f| [c.k as f64, value(f, m)]))
                    .collect()
            })
            .collect();
        let mut items: Vec<(f64, f64, String, usize)> = Vec::new();
        for (m, line) in lines.iter().enumerate() {
            for &[x, y] in line {
                items.push((x, y, String::new(), m));
            }
        }
        let chosen = &self.selection.candidates[self.selection.best];
        if let Some(b) = chosen.bic() {
            items.push((chosen.k as f64, b, "chosen".to_owned(), 0));
        }
        let labels: Vec<String> = names.iter().map(|s| (*s).to_owned()).collect();
        let plot = PlotSpec {
            x_label: "Classes".to_owned(),
            y_label: "Criterion".to_owned(),
            groups: Some(PointGroups {
                title: "Criterion".to_owned(),
                labels: labels.clone(),
                membership: Vec::new(),
                lines,
            }),
            annotations: Some(Annotations { groups: labels, items, labels: Vec::new() }),
            x_ticks: Some(
                (self.request.min_classes..=self.request.max_classes).map(|k| (k as f64, k.to_string())).collect(),
            ),
            ..PlotSpec::default()
        };
        section(
            SectionKind::LatentClassCriteriaPlot,
            "Information Criteria by Number of Classes (smaller is better)",
            SectionContent::Plot(Box::new(plot)),
        )
    }

    fn class_sizes(&self) -> ReportSection {
        let c = self.chosen();
        let class = c.classification();
        let mut t = TextTable::new(["Class", "Size", "Std error", "Rows (most likely)"]);
        for k in 0..c.k() {
            t.push([
                (k + 1).to_string(),
                format_number(c.class_sizes[k], 4),
                number_or_dash(c.class_size_se[k], 3),
                class.iter().filter(|&&m| m == k).count().to_string(),
            ]);
        }
        section(SectionKind::LatentClassSizes, "Class Sizes (largest first)", SectionContent::Table(t))
    }

    fn probability_table(&self) -> ReportSection {
        let c = self.chosen();
        let mut header = vec!["Item".to_owned(), "Level".to_owned()];
        for k in 1..=c.k() {
            header.push(format!("Class {k}"));
            header.push(format!("Std error {k}"));
        }
        let mut t = TextTable::new(header);
        for (j, name) in self.names.iter().enumerate() {
            for (r, level) in self.levels[j].iter().enumerate() {
                let mut row = vec![if r == 0 { name.clone() } else { String::new() }, level.clone()];
                for k in 0..c.k() {
                    row.push(format_number(c.probabilities[j][k][r], 4));
                    row.push(number_or_dash(c.probability_se[j][k][r], 3));
                }
                t.push(row);
            }
        }
        section(
            SectionKind::LatentClassProbabilities,
            "Item Response Probabilities by Class (— : on the boundary, no standard error)",
            SectionContent::Table(t),
        )
    }

    fn profile_plot(&self) -> ReportSection {
        let c = self.chosen();
        let k = c.k();
        let width = 0.8 / k as f64;
        let mut panes = Vec::new();
        let mut ticks = Vec::new();
        let mut items = Vec::new();
        let mut segments = Vec::new();
        let mut offset = 0.0;
        for (j, name) in self.names.iter().enumerate() {
            let r_j = self.levels[j].len();
            panes.push(PlotPane { label: name.clone(), x_min: offset - 0.5, x_max: offset + r_j as f64 - 0.5 });
            for (r, level) in self.levels[j].iter().enumerate() {
                let centre = offset + r as f64;
                ticks.push((centre, level.clone()));
                for cls in 0..k {
                    let x = centre + (cls as f64 - (k as f64 - 1.0) / 2.0) * width;
                    let p = c.probabilities[j][cls][r];
                    segments.push(vec![[x, 0.0], [x, p]]);
                    items.push((x, p, String::new(), cls));
                }
            }
            offset += r_j as f64;
        }
        let groups: Vec<String> =
            (0..k).map(|cls| format!("Class {} ({})", cls + 1, percent(c.class_sizes[cls]))).collect();
        let plot = PlotSpec {
            x_label: "Item and level".to_owned(),
            y_label: "Probability of the level".to_owned(),
            segments,
            panes,
            x_ticks: Some(ticks),
            annotations: Some(Annotations { groups, items, labels: Vec::new() }),
            ..PlotSpec::default()
        };
        section(
            SectionKind::LatentClassProfilePlot,
            "Response Probability Profiles (one stem per class at each level)",
            SectionContent::Plot(Box::new(plot)),
        )
    }

    fn classification_table(&self) -> ReportSection {
        let c = self.chosen();
        let class = c.classification();
        let avg = c.average_posterior();
        let mut header = vec!["Most likely class".to_owned(), "Rows".to_owned()];
        header.extend((1..=c.k()).map(|k| format!("Mean probability class {k}")));
        let mut t = TextTable::new(header);
        for (a, row_avg) in avg.iter().enumerate() {
            let mut row = vec![(a + 1).to_string(), class.iter().filter(|&&m| m == a).count().to_string()];
            row.extend(row_avg.iter().map(|v| number_or_dash(*v, 4)));
            t.push(row);
        }
        section(
            SectionKind::LatentClassClassification,
            "Classification: Average Posterior Probabilities by Most Likely Class",
            SectionContent::Table(t),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> crate::sample::Sample {
        crate::sample::catalog()
            .into_iter()
            .find(|s| s.name.starts_with("Symptom checklist"))
            .map(|s| (s.build)())
            .unwrap()
    }

    fn items(t: &DataTable) -> Vec<usize> {
        crate::sample::SYMPTOM_ITEMS.iter().map(|n| t.find(n).unwrap()).collect()
    }

    #[test]
    fn chooses_three_classes_and_saves_columns() {
        let s = sample();
        let t = &s.table;
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([0], true);
        let req = LatentClassRequest { max_classes: 4, ..LatentClassRequest::new(items(t)) };
        let a = LatentClassAnalysis::run(t, &rows, &req).unwrap();
        assert_eq!(a.chosen().k(), 3);
        assert!(a.title().starts_with("Latent Class Analysis (3 classes): Fatigue, "), "{}", a.title());
        let s = a.sections();
        assert_eq!(s.len(), 7);
        let SectionContent::Table(sel) = &s[1].content else { panic!("table") };
        assert_eq!(sel.rows.len(), 4);
        assert!(sel.rows[2][4].ends_with("(chosen)"));
        let SectionContent::Table(probs) = &s[4].content else { panic!("table") };
        let n_levels: usize = a.levels().iter().map(Vec::len).sum();
        assert_eq!(probs.rows.len(), n_levels);
        let SectionContent::Plot(p) = &s[5].content else { panic!("plot") };
        assert_eq!(p.panes.len(), items(t).len());
        assert_eq!(p.segments.len(), 3 * n_levels);
        assert!(!a.rows().contains(&0));
        let saved = a.saveable_columns();
        let keys: Vec<&str> = saved.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["class", "probability_1", "probability_2", "probability_3"]);
        assert!(saved[0].values[0].is_nan());
        let r = a.rows()[3];
        let sum: f64 = (1..4).map(|k| saved[k].values[r]).sum();
        assert!((sum - 1.0).abs() < 1e-12);
        // Class 1 is the largest.
        let sizes = &a.chosen().class_sizes;
        assert!(sizes[0] >= sizes[1] && sizes[1] >= sizes[2]);
    }

    #[test]
    fn json_round_trip_and_errors() {
        let s = sample();
        let t = &s.table;
        let rows = RowStateManager::new(t.n_rows());
        let full = LatentClassRequest {
            min_classes: 2,
            max_classes: 3,
            starts: 4,
            smoothing: 0.5,
            seed: 9,
            ..LatentClassRequest::new(items(t))
        };
        let back = LatentClassRequest::from_json(&full.to_json(t), t).unwrap();
        assert_eq!(back, full);
        let a = LatentClassAnalysis::run(t, &rows, &full).unwrap();
        assert_eq!(a.selection().candidates.len(), 2);
        assert!(LatentClassAnalysis::run(t, &rows, &LatentClassRequest::new(vec![items(t)[0]])).is_err());
        let bad = LatentClassRequest { min_classes: 3, max_classes: 2, ..full.clone() };
        assert!(LatentClassAnalysis::run(t, &rows, &bad).is_err());
        // A continuous column is refused; the respondent id has too many levels.
        let age = t.find("Age").unwrap();
        let with_age = LatentClassRequest { items: vec![items(t)[0], age], ..full.clone() };
        assert!(LatentClassAnalysis::run(t, &rows, &with_age).unwrap_err().to_string().contains("continuous"));
        let id = t.find("Respondent").unwrap();
        let with_id = LatentClassRequest { items: vec![items(t)[0], id], ..full };
        assert!(LatentClassAnalysis::run(t, &rows, &with_id).unwrap_err().to_string().contains("levels"));
    }
}
