//! Attribute measurement studies: agreement of appraisers who rate parts
//! into categories ([`stat_quality::attribute_agreement`]) and the bias and
//! repeatability of a go/no-go gauge from acceptance counts
//! ([`stat_quality::attribute_gauge`]).
//!
//! # Data layout
//! The agreement study reads one row per rating (long format): a part
//! column, an appraiser column, the rating, and optionally a trial column
//! (otherwise an appraiser's ratings of a part are trials in row order) and a
//! standard column holding each part's known category (the same on every
//! row of the part; empty cells are ignored). Every appraiser must rate every
//! part equally often.
//!
//! The gauge study reads a reference value per row with either an
//! acceptance count and a number of checks per row, or one row per check
//! with a result column, a check counting as accepted when its result is the
//! chosen label.
//!
//! Both reports run through [`crate::ComparisonReport`]; plotted markers are
//! summaries, not dataset rows.

use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, obj, s};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_linear::natural_order;
use stat_quality::{
    AcceptancePoint, AgreementOptions, AgreementRatings, AttributeAgreement, AttributeGaugeStudy, Concordance,
    FleissKappa, KappaEstimate, KendallTau, LimitSide, PercentAgreement, VersusStandard, attribute_agreement,
    attribute_gauge_study,
};

const DIGITS: usize = 4;

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn column_name(table: &DataTable, i: usize) -> JsonValue {
    s(table.column(i).map_or("", |c| c.name.as_str()))
}

fn find_column(v: &JsonValue, table: &DataTable, key: &str) -> Result<usize> {
    let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
    table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
}

fn optional_column(v: &JsonValue, table: &DataTable, key: &str) -> Result<Option<usize>> {
    match v.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(_) => find_column(v, table, key).map(Some),
    }
}

fn name_of(table: &DataTable, i: usize) -> String {
    table.column(i).map_or_else(String::new, |c| c.name.clone())
}

fn pct(v: f64) -> String {
    if v.is_finite() { format!("{v:.1}") } else { "—".to_owned() }
}

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

fn level_label(confidence: f64) -> String {
    format!("{}%", format_number(100.0 * confidence, 4))
}

// ---------------------------------------------------------------------------
// Agreement study
// ---------------------------------------------------------------------------

/// An attribute agreement study of ratings in long format.
#[derive(Debug, Clone, PartialEq)]
pub struct AgreementRequest {
    /// Part (item) column.
    pub part: usize,
    /// Appraiser column.
    pub appraiser: usize,
    /// Rating column (any type; its values are the categories).
    pub rating: usize,
    /// Trial column (`None`: an appraiser's ratings of a part in row order).
    pub trial: Option<usize>,
    /// Standard (known category of each part) column.
    pub standard: Option<usize>,
    /// Confidence level of the intervals.
    pub confidence: f64,
    /// The categories are ordered: add weighted kappas, Kendall's W and τ_b.
    pub ordinal: bool,
    /// Category order (empty: natural order of the labels, numbers by value).
    pub order: Vec<String>,
}

impl AgreementRequest {
    /// A request with 95% intervals, ordinal when the rating column is
    /// ordinal or continuous.
    #[must_use]
    pub fn new(table: &DataTable, part: usize, appraiser: usize, rating: usize) -> Self {
        let ordinal = table
            .column(rating)
            .is_some_and(|c| matches!(c.level, MeasurementLevel::Ordinal | MeasurementLevel::Continuous));
        Self { part, appraiser, rating, trial: None, standard: None, confidence: 0.95, ordinal, order: Vec::new() }
    }

    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.part, self.appraiser, self.rating];
        c.extend(self.trial);
        c.extend(self.standard);
        c
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.part = f(self.part);
        self.appraiser = f(self.appraiser);
        self.rating = f(self.rating);
        self.trial = self.trial.map(&f);
        self.standard = self.standard.map(&f);
        self
    }

    /// The project-file form (`"analysis": "attribute-agreement"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let opt = |c: Option<usize>| c.map_or(JsonValue::Null, |i| column_name(table, i));
        obj([
            ("analysis", s("attribute-agreement")),
            ("part", column_name(table, self.part)),
            ("appraiser", column_name(table, self.appraiser)),
            ("rating", column_name(table, self.rating)),
            ("trial", opt(self.trial)),
            ("standard", opt(self.standard)),
            ("confidence", JsonValue::number(self.confidence)),
            ("ordinal", JsonValue::Bool(self.ordinal)),
            ("order", JsonValue::Array(self.order.iter().map(|l| s(l)).collect())),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing or unknown columns.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let part = find_column(v, table, "part")?;
        let appraiser = find_column(v, table, "appraiser")?;
        let rating = find_column(v, table, "rating")?;
        let mut r = Self::new(table, part, appraiser, rating);
        r.trial = optional_column(v, table, "trial")?;
        r.standard = optional_column(v, table, "standard")?;
        if let Some(c) = v.get("confidence").and_then(JsonValue::as_f64) {
            r.confidence = c;
        }
        if let Some(o) = v.get("ordinal").and_then(JsonValue::as_bool) {
            r.ordinal = o;
        }
        if let Some(order) = v.get("order").and_then(JsonValue::as_array) {
            r.order = order.iter().filter_map(|l| l.as_str().map(str::to_owned)).collect();
        }
        Ok(r)
    }
}

/// The categories of the rating (and standard) columns in the order used:
/// `order` first, then any other labels in natural order.
///
/// # Errors
/// A missing column.
pub fn agreement_categories(table: &DataTable, request: &AgreementRequest) -> Result<Vec<String>> {
    let mut labels: Vec<String> = Vec::new();
    for c in std::iter::once(request.rating).chain(request.standard) {
        let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
        for r in 0..table.n_rows() {
            let t = col.level_text(r);
            if !t.is_empty() && !labels.contains(&t) {
                labels.push(t);
            }
        }
    }
    labels.sort_by(|a, b| natural_order(a, b));
    let mut ordered: Vec<String> = request.order.iter().filter(|l| labels.contains(l)).cloned().collect();
    ordered.extend(labels.into_iter().filter(|l| !request.order.contains(l)));
    Ok(ordered)
}

fn level_index(levels: &mut Vec<String>, label: String) -> usize {
    levels.iter().position(|l| *l == label).unwrap_or_else(|| {
        levels.push(label);
        levels.len() - 1
    })
}

/// A computed attribute agreement report.
#[derive(Debug, Clone)]
pub struct AgreementAnalysis {
    request: AgreementRequest,
    rating_name: String,
    data: AgreementRatings,
    study: AttributeAgreement,
    n_excluded: usize,
}

impl AgreementAnalysis {
    /// Run the study on the non-excluded rows with a part, appraiser and
    /// rating.
    ///
    /// # Errors
    /// Repeated columns, an unbalanced study, two standards for one part, a
    /// part without a standard, a trial given twice, or any error of
    /// [`attribute_agreement`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &AgreementRequest) -> Result<Self> {
        let cols = request.columns();
        if (0..cols.len()).any(|i| cols[i + 1..].contains(&cols[i])) {
            return Err(invalid("part, appraiser, rating, trial and standard must be different columns"));
        }
        let col = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let (part_col, app_col, rating_col) = (col(request.part)?, col(request.appraiser)?, col(request.rating)?);
        let trial_col = request.trial.map(col).transpose()?;
        let std_col = request.standard.map(col).transpose()?;
        // Categories of the ratings (and standards) of the included rows
        // used, in the request's order: a label seen only in excluded or
        // incomplete rows adds no empty category (which would change the
        // weighted kappas of ordered categories).
        let used_row = |r: usize| {
            !states.is_excluded(r)
                && !part_col.level_text(r).is_empty()
                && !app_col.level_text(r).is_empty()
                && !rating_col.level_text(r).is_empty()
        };
        let mut seen: Vec<String> = Vec::new();
        for r in (0..table.n_rows()).filter(|&r| used_row(r)) {
            for t in std::iter::once(rating_col.level_text(r)).chain(std_col.map(|c| c.level_text(r))) {
                if !t.is_empty() && !seen.contains(&t) {
                    seen.push(t);
                }
            }
        }
        let categories: Vec<String> =
            agreement_categories(table, request)?.into_iter().filter(|c| seen.contains(c)).collect();
        let category = |t: &str| categories.iter().position(|c| c == t).expect("listed above");
        let (mut parts, mut appraisers) = (Vec::new(), Vec::new());
        // (part, appraiser) → [(trial label, row, category)]
        let mut cells: Vec<Vec<Vec<(String, usize, usize)>>> = Vec::new();
        let mut standard: Vec<Option<usize>> = Vec::new();
        let mut n_excluded = 0;
        for r in 0..table.n_rows() {
            let (p, a, v) = (part_col.level_text(r), app_col.level_text(r), rating_col.level_text(r));
            if p.is_empty() || a.is_empty() || v.is_empty() {
                continue;
            }
            if states.is_excluded(r) {
                n_excluded += 1;
                continue;
            }
            let pi = level_index(&mut parts, p);
            let ai = level_index(&mut appraisers, a);
            if pi == cells.len() {
                cells.push(Vec::new());
                standard.push(None);
            }
            while cells[pi].len() <= ai {
                cells[pi].push(Vec::new());
            }
            let trial = trial_col.map(|c| c.level_text(r)).unwrap_or_default();
            cells[pi][ai].push((trial, r, category(&v)));
            if let Some(sc) = std_col {
                let st = sc.level_text(r);
                if !st.is_empty() {
                    let si = category(&st);
                    match standard[pi] {
                        Some(prev) if prev != si => {
                            return Err(invalid(format!(
                                "part {} has two standard values, {} and {}",
                                parts[pi], categories[prev], categories[si]
                            )));
                        }
                        _ => standard[pi] = Some(si),
                    }
                }
            }
        }
        if parts.len() < 2 {
            return Err(invalid("the study needs at least two parts with ratings"));
        }
        let k = appraisers.len();
        let mut ratings = vec![Vec::with_capacity(parts.len()); k];
        let mut trials: Option<usize> = None;
        for (pi, row) in cells.iter_mut().enumerate() {
            for (ai, slot) in ratings.iter_mut().enumerate() {
                let mut cell = row.get(ai).cloned().unwrap_or_default();
                if trial_col.is_some() {
                    cell.sort_by(|x, y| natural_order(&x.0, &y.0));
                    if let Some(w) = cell.windows(2).find(|w| w[0].0 == w[1].0) {
                        return Err(invalid(format!(
                            "{} rated part {} twice on trial {}",
                            appraisers[ai], parts[pi], w[0].0
                        )));
                    }
                }
                let m = *trials.get_or_insert(cell.len());
                if cell.len() != m {
                    return Err(invalid(format!(
                        "the study must be balanced: {} rated part {} {} times but other parts {} times \
                         (excluded rows and empty cells count against this)",
                        appraisers[ai],
                        parts[pi],
                        cell.len(),
                        m
                    )));
                }
                slot.push(cell.iter().map(|x| x.2).collect());
            }
        }
        let standard = if std_col.is_some() {
            let mut out = Vec::with_capacity(parts.len());
            for (pi, sv) in standard.iter().enumerate() {
                out.push(sv.ok_or_else(|| invalid(format!("part {} has no standard value", parts[pi])))?);
            }
            Some(out)
        } else {
            None
        };
        let data = AgreementRatings { categories, appraisers, parts, ratings, standard };
        let study =
            attribute_agreement(&data, &AgreementOptions { confidence: request.confidence, ordinal: request.ordinal })?;
        Ok(Self { request: request.clone(), rating_name: name_of(table, request.rating), data, study, n_excluded })
    }

    /// The request this analysis answers.
    #[must_use]
    pub fn request(&self) -> &AgreementRequest {
        &self.request
    }

    /// The computed study.
    #[must_use]
    pub fn study(&self) -> &AttributeAgreement {
        &self.study
    }

    /// The ratings as analysed.
    #[must_use]
    pub fn ratings(&self) -> &AgreementRatings {
        &self.data
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Attribute Agreement: {}", self.rating_name)
    }

    fn percent_columns(&self, first: &str) -> Vec<String> {
        let l = level_label(self.request.confidence);
        vec![
            first.to_owned(),
            "Parts".to_owned(),
            "Matched".to_owned(),
            "Agreement (%)".to_owned(),
            format!("Lower {l}"),
            format!("Upper {l}"),
        ]
    }

    fn percent_cells(a: &PercentAgreement) -> Vec<String> {
        vec![a.inspected.to_string(), a.matched.to_string(), pct(a.percent), pct(a.lower), pct(a.upper)]
    }

    fn fleiss_cells(k: &FleissKappa) -> Vec<String> {
        let o = &k.overall;
        vec![num(o.kappa), num(o.se0), num(o.z), format_p_value(o.p_value)]
    }

    fn concordance_cells(c: Option<&Concordance>) -> Vec<String> {
        c.map_or_else(
            || vec!["—".to_owned(); 4],
            |c| vec![num(c.w), num(c.chi_square), num(c.df), format_p_value(c.p_value)],
        )
    }

    fn cohen_cells(k: &KappaEstimate) -> Vec<String> {
        vec![num(k.kappa), num(k.se), num(k.lower), num(k.upper), num(k.z), format_p_value(k.p_value)]
    }

    fn cohen_columns(&self) -> Vec<String> {
        let l = level_label(self.request.confidence);
        ["Kappa", "SE", &format!("Kappa lower {l}"), &format!("Kappa upper {l}"), "z", "p-value"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    }

    fn tau_cells(t: Option<&KendallTau>) -> Vec<String> {
        t.map_or_else(|| vec!["—".to_owned(); 2], |t| vec![num(t.tau_b), format_p_value(t.p_value)])
    }

    fn versus_row(&self, label: &str, v: &VersusStandard) -> Vec<String> {
        let mut row = vec![label.to_owned()];
        row.extend(Self::percent_cells(&v.agreement));
        row.push(v.consistently_misrated.to_string());
        row.push(v.mixed.to_string());
        row.extend(Self::cohen_cells(&v.kappa));
        row
    }

    fn ordinal_row(label: &str, v: &VersusStandard) -> Option<Vec<String>> {
        let (lin, quad) = v.weighted.as_ref()?;
        let mut row = vec![label.to_owned()];
        for k in [lin, quad] {
            row.extend([num(k.kappa), num(k.se), format_p_value(k.p_value)]);
        }
        row.extend(Self::tau_cells(v.tau.as_ref()));
        Some(row)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let st = &self.study;
        let d = &self.data;
        let names = &d.appraisers;
        let mut out = Vec::new();

        let mut summary = TextTable::new(["Item", "Value"]);
        summary.push(["Parts".to_owned(), d.parts.len().to_string()]);
        summary.push(["Appraisers".to_owned(), names.len().to_string()]);
        summary.push(["Trials per appraiser and part".to_owned(), st.trials.to_string()]);
        summary.push([
            if self.request.ordinal { "Categories (ordered)" } else { "Categories" }.to_owned(),
            d.categories.join(", "),
        ]);
        summary.push(["Standard".to_owned(), self.request.standard.map_or("none".to_owned(), |_| "known".to_owned())]);
        if self.n_excluded > 0 {
            summary.push(["Rows excluded".to_owned(), self.n_excluded.to_string()]);
        }
        out.push(section(SectionKind::AgreementSummary, "Study Summary", SectionContent::Table(summary)));

        let mut by_category =
            TextTable::new(["Comparison", "Category", "Kappa", "SE under no agreement", "z", "p-value"]);
        let mut push_categories = |label: &str, ks: &[KappaEstimate]| {
            for (c, k) in d.categories.iter().zip(ks) {
                by_category.push([
                    label.to_owned(),
                    c.clone(),
                    num(k.kappa),
                    num(k.se0),
                    num(k.z),
                    format_p_value(k.p_value),
                ]);
            }
        };

        if !st.within.is_empty() {
            let mut cols = self.percent_columns("Appraiser");
            cols.extend(["Fleiss kappa", "SE", "z", "p-value"].map(str::to_owned));
            if self.request.ordinal {
                cols.extend(["Kendall's W", "Chi-square", "DF", "W p-value"].map(str::to_owned));
            }
            let mut t = TextTable::new(cols);
            for (name, w) in names.iter().zip(&st.within) {
                let mut row = vec![name.clone()];
                row.extend(Self::percent_cells(&w.agreement));
                row.extend(Self::fleiss_cells(&w.kappa));
                if self.request.ordinal {
                    row.extend(Self::concordance_cells(w.concordance.as_ref()));
                }
                t.push(row);
                push_categories(&format!("Within {name}"), &w.kappa.by_category);
            }
            out.push(section(
                SectionKind::AgreementWithin,
                "Within Each Appraiser (repeat trials)",
                SectionContent::Table(t),
            ));
        }

        if !st.versus_standard.is_empty() {
            let mut cols = self.percent_columns("Appraiser");
            cols.extend(["Consistently misrated", "Mixed ratings"].map(str::to_owned));
            cols.extend(self.cohen_columns());
            let mut t = TextTable::new(cols.clone());
            for (name, v) in names.iter().zip(&st.versus_standard) {
                t.push(self.versus_row(name, v));
                push_categories(&format!("{name} vs standard"), &v.by_category);
            }
            out.push(section(
                SectionKind::AgreementVersusStandard,
                "Each Appraiser Against the Standard",
                SectionContent::Table(t),
            ));
        }

        if let Some(b) = &st.between {
            let mut cols = self.percent_columns("Comparison");
            cols.extend(["Fleiss kappa", "SE", "z", "p-value"].map(str::to_owned));
            if self.request.ordinal {
                cols.extend(["Kendall's W", "Chi-square", "DF", "W p-value"].map(str::to_owned));
            }
            let mut t = TextTable::new(cols);
            let mut row = vec!["All appraisers".to_owned()];
            row.extend(Self::percent_cells(&b.agreement));
            row.extend(Self::fleiss_cells(&b.kappa));
            if self.request.ordinal {
                row.extend(Self::concordance_cells(b.concordance.as_ref()));
            }
            t.push(row);
            push_categories("Between appraisers", &b.kappa.by_category);
            out.push(section(SectionKind::AgreementBetween, "Between Appraisers", SectionContent::Table(t)));

            let mut cols = vec!["First appraiser".to_owned(), "Second appraiser".to_owned()];
            cols.extend(self.cohen_columns());
            let mut t = TextTable::new(cols);
            for p in &b.pairwise {
                let mut row = vec![names[p.first].clone(), names[p.second].clone()];
                row.extend(Self::cohen_cells(&p.kappa));
                t.push(row);
            }
            out.push(section(
                SectionKind::AgreementPairs,
                "Pairs of Appraisers (Cohen's kappa, ratings paired by trial)",
                SectionContent::Table(t),
            ));
        }

        if let Some(v) = &st.all_versus_standard {
            let mut cols = self.percent_columns("Comparison");
            cols.extend(["Consistently misrated", "Mixed ratings"].map(str::to_owned));
            cols.extend(self.cohen_columns());
            let mut t = TextTable::new(cols);
            t.push(self.versus_row("All appraisers", v));
            push_categories("All vs standard", &v.by_category);
            out.push(section(
                SectionKind::AgreementAllVersusStandard,
                "All Appraisers Against the Standard",
                SectionContent::Table(t),
            ));
        }

        if self.request.ordinal && !st.versus_standard.is_empty() {
            let mut t = TextTable::new([
                "Comparison",
                "Linear weighted kappa",
                "Linear SE",
                "Linear p-value",
                "Quadratic weighted kappa",
                "Quadratic SE",
                "Quadratic p-value",
                "Kendall's tau-b",
                "tau-b p-value",
            ]);
            for (name, v) in names.iter().zip(&st.versus_standard) {
                if let Some(r) = Self::ordinal_row(name, v) {
                    t.push(r);
                }
            }
            if let Some(v) = &st.all_versus_standard {
                if let Some(r) = Self::ordinal_row("All appraisers", v) {
                    t.push(r);
                }
            }
            out.push(section(
                SectionKind::AgreementOrdinal,
                "Ordered Categories Against the Standard",
                SectionContent::Table(t),
            ));
        }

        if !by_category.rows.is_empty() {
            out.push(section(
                SectionKind::AgreementByCategory,
                "Kappa by Category",
                SectionContent::Table(by_category),
            ));
        }

        if let Some(v) = st.all_versus_standard.as_ref().or(st.versus_standard.first()) {
            let mut cols = vec!["Standard \\ Rating".to_owned()];
            cols.extend(d.categories.iter().cloned());
            cols.push("Total".to_owned());
            let mut t = TextTable::new(cols);
            for (c, row) in d.categories.iter().zip(&v.table) {
                let mut r = vec![c.clone()];
                r.extend(row.iter().map(|x| format_number(*x, 8)));
                r.push(format_number(row.iter().sum(), 8));
                t.push(r);
            }
            out.push(section(
                SectionKind::AgreementConfusion,
                "Ratings Against the Standard (every rating counted)",
                SectionContent::Table(t),
            ));
        }

        if let Some(plot) = self.plot() {
            out.push(section(
                SectionKind::AgreementPlot,
                "Agreement by Appraiser",
                SectionContent::Plot(Box::new(plot)),
            ));
        }

        let mut note = format!(
            "A part matches when all of the ratings compared are the same (and equal the standard, for comparisons \
             with the standard); intervals for the percentage of matched parts are exact (Clopper–Pearson) at \
             {}. Fleiss' kappa treats each trial (and, between appraisers, each appraiser's trial) as a rating of \
             the part; its standard errors are those under no agreement beyond chance (Fleiss, Nee & Landis 1979). \
             Cohen's kappa against the standard counts every trial as a pair (rating, standard), with the \
             large-sample standard error and interval of Fleiss, Cohen & Everitt (1969); z tests use the standard \
             error under no agreement. A consistently misrated part got the same wrong rating every time; a part \
             with mixed ratings got different ones.",
            level_label(self.request.confidence)
        );
        if self.request.ordinal {
            note.push_str(
                " With ordered categories, weighted kappas give partial credit to near misses (linear: 1 − |i − j| \
                 / (c − 1); quadratic: 1 − (i − j)² / (c − 1)²), Kendall's W measures the concordance of the \
                 ratings' rankings of the parts (ties corrected; χ² = J(n − 1)W on n − 1 df) and Kendall's τ_b \
                 correlates the ratings with the standard.",
            );
        }
        note.push_str(" Kappa is shown as — when every rating falls in one category.");
        out.push(section(SectionKind::AgreementNote, "About These Measures", SectionContent::Note(note)));
        out
    }

    fn plot(&self) -> Option<PlotSpec> {
        let st = &self.study;
        if st.within.is_empty() && st.versus_standard.is_empty() {
            return None;
        }
        let mut groups = Vec::new();
        let mut items = Vec::new();
        let mut segments = Vec::new();
        let series: [(&str, Vec<&PercentAgreement>); 2] = [
            ("Within appraiser", st.within.iter().map(|w| &w.agreement).collect()),
            ("Appraiser vs standard", st.versus_standard.iter().map(|v| &v.agreement).collect()),
        ];
        let shown = series.iter().filter(|(_, v)| !v.is_empty()).count();
        for (label, values) in series.into_iter().filter(|(_, v)| !v.is_empty()) {
            let g = groups.len();
            let offset = if shown == 2 { if g == 0 { -0.12 } else { 0.12 } } else { 0.0 };
            for (i, a) in values.iter().enumerate() {
                let x = (i + 1) as f64 + offset;
                items.push((x, a.percent, String::new(), g));
                segments.push(vec![[x, a.lower], [x, a.upper]]);
                segments.push(vec![[x - 0.05, a.lower], [x + 0.05, a.lower]]);
                segments.push(vec![[x - 0.05, a.upper], [x + 0.05, a.upper]]);
            }
            groups.push(label.to_owned());
        }
        Some(PlotSpec {
            x_label: "Appraiser".to_owned(),
            y_label: format!("Parts matched (%), {} interval", level_label(self.request.confidence)),
            x_ticks: Some(self.data.appraisers.iter().enumerate().map(|(i, n)| ((i + 1) as f64, n.clone())).collect()),
            segments,
            annotations: Some(Annotations { groups, items, labels: Vec::new() }),
            ..PlotSpec::default()
        })
    }
}

// ---------------------------------------------------------------------------
// Attribute gauge study
// ---------------------------------------------------------------------------

/// A go/no-go gauge study: acceptances at reference values.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeGaugeRequest {
    /// Reference value column (numeric).
    pub reference: usize,
    /// Acceptance column: a count (with `checks`) or each check's result.
    pub acceptance: usize,
    /// Number-of-checks column (`None`: one row per check).
    pub checks: Option<usize>,
    /// The result meaning "accepted" when there is one row per check.
    pub accept_label: String,
    /// The specification limit the gauge decides at.
    pub limit: f64,
}

impl AttributeGaugeRequest {
    /// Counts per row: `acceptance` accepted out of `checks`.
    #[must_use]
    pub fn counts(reference: usize, acceptance: usize, checks: usize, limit: f64) -> Self {
        Self { reference, acceptance, checks: Some(checks), accept_label: String::new(), limit }
    }

    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.reference, self.acceptance];
        c.extend(self.checks);
        c
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.reference = f(self.reference);
        self.acceptance = f(self.acceptance);
        self.checks = self.checks.map(&f);
        self
    }

    /// The project-file form (`"analysis": "attribute-gauge"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        obj([
            ("analysis", s("attribute-gauge")),
            ("reference", column_name(table, self.reference)),
            ("acceptance", column_name(table, self.acceptance)),
            ("checks", self.checks.map_or(JsonValue::Null, |i| column_name(table, i))),
            ("accept_label", s(&self.accept_label)),
            ("limit", JsonValue::number(self.limit)),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing or unknown columns, or no limit.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        Ok(Self {
            reference: find_column(v, table, "reference")?,
            acceptance: find_column(v, table, "acceptance")?,
            checks: optional_column(v, table, "checks")?,
            accept_label: v.get("accept_label").and_then(JsonValue::as_str).unwrap_or("").to_owned(),
            limit: member(v, "limit")?.as_f64().ok_or_else(|| invalid("limit: expected a number"))?,
        })
    }
}

/// A computed attribute gauge report.
#[derive(Debug, Clone)]
pub struct AttributeGaugeAnalysis {
    request: AttributeGaugeRequest,
    reference_name: String,
    study: AttributeGaugeStudy,
    n_excluded: usize,
}

impl AttributeGaugeAnalysis {
    /// Run the study on the non-excluded rows.
    ///
    /// # Errors
    /// Repeated or non-numeric columns, a reference value given twice in the
    /// counts layout, or any error of [`attribute_gauge_study`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &AttributeGaugeRequest) -> Result<Self> {
        let cols = request.columns();
        if (0..cols.len()).any(|i| cols[i + 1..].contains(&cols[i])) {
            return Err(invalid("reference, acceptance and checks must be different columns"));
        }
        let numeric = |i: usize| {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))
        };
        let reference = numeric(request.reference)?;
        let mut n_excluded = 0;
        let mut points: Vec<AcceptancePoint> = Vec::new();
        if let Some(ch) = request.checks {
            let (acc, checks) = (numeric(request.acceptance)?, numeric(ch)?);
            for r in 0..table.n_rows() {
                if !(reference[r].is_finite() && acc[r].is_finite() && checks[r].is_finite()) {
                    continue;
                }
                if states.is_excluded(r) {
                    n_excluded += 1;
                    continue;
                }
                points.push(AcceptancePoint { reference: reference[r], accepted: acc[r], trials: checks[r] });
            }
        } else {
            if request.accept_label.is_empty() {
                return Err(invalid("choose the result that means accepted"));
            }
            let res = table.column(request.acceptance).ok_or_else(|| invalid("no acceptance column"))?;
            for r in 0..table.n_rows() {
                let t = res.level_text(r);
                if !reference[r].is_finite() || t.is_empty() {
                    continue;
                }
                if states.is_excluded(r) {
                    n_excluded += 1;
                    continue;
                }
                let ok = f64::from(u8::from(t == request.accept_label));
                match points.iter_mut().find(|p| p.reference == reference[r]) {
                    Some(p) => {
                        p.accepted += ok;
                        p.trials += 1.0;
                    }
                    None => points.push(AcceptancePoint { reference: reference[r], accepted: ok, trials: 1.0 }),
                }
            }
        }
        let study = attribute_gauge_study(&points, request.limit)?;
        Ok(Self { request: request.clone(), reference_name: name_of(table, request.reference), study, n_excluded })
    }

    /// The computed study.
    #[must_use]
    pub fn study(&self) -> &AttributeGaugeStudy {
        &self.study
    }

    /// The request this analysis answers.
    #[must_use]
    pub fn request(&self) -> &AttributeGaugeRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Attribute Gauge Bias and Repeatability: {}", self.reference_name)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let s = &self.study;
        let g = |v: f64| format_number(v, 5);
        let mut summary = TextTable::new(["Item", "Value"]);
        let mut row = |a: &str, b: String| summary.push([a.to_owned(), b]);
        row(
            "Specification limit",
            format!(
                "{} ({})",
                g(s.limit),
                match s.side {
                    LimitSide::Lower => "lower: acceptance rises with the reference value",
                    LimitSide::Upper => "upper: acceptance falls as the reference value rises",
                }
            ),
        );
        row("Reference value accepted half the time, x(0.5)", g(s.median_acceptance));
        row("Bias (limit − x(0.5))", g(s.bias));
        row("Repeatability (|x(0.995) − x(0.005)| / 1.08)", g(s.repeatability));
        row("Gauge standard deviation (normal curve)", g(s.sigma));
        row("t = 31.3 |bias| / repeatability", g(s.t));
        row("DF", g(s.df));
        row("p-value (bias = 0)", format_p_value(s.p_value));
        row("Checks per reference value", g(s.trials));
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        let mut pts = TextTable::new([
            "Reference value",
            "Accepted",
            "Checks",
            "Adjusted probability of acceptance",
            "Fitted probability",
            "In the fit",
        ]);
        for e in &s.points {
            pts.push([
                g(e.point.reference),
                g(e.point.accepted),
                g(e.point.trials),
                num(e.adjusted),
                num(s.acceptance_probability(e.point.reference)),
                if e.fitted { "yes" } else { "no (accepted never or always)" }.to_owned(),
            ]);
        }
        let (lo, hi) = (s.points[0].point.reference, s.points[s.points.len() - 1].point.reference);
        let curve: Vec<[f64; 2]> = (0..=120)
            .map(|i| {
                let x = lo + (hi - lo) * f64::from(i) / 120.0;
                [x, s.acceptance_probability(x)]
            })
            .collect();
        let items = s.points.iter().map(|e| (e.point.reference, e.adjusted, String::new(), 0)).collect();
        let plot = PlotSpec {
            x_label: self.reference_name.clone(),
            y_label: "Probability of acceptance".to_owned(),
            line: Some(curve),
            line_name: Some("Fitted normal gauge curve".to_owned()),
            segments: vec![vec![[s.limit, 0.0], [s.limit, 1.0]], vec![[lo, 0.5], [hi, 0.5]]],
            annotations: Some(Annotations {
                groups: vec!["Adjusted acceptance".to_owned()],
                items,
                labels: vec![(s.limit, 1.04, "Limit".to_owned())],
            }),
            ..PlotSpec::default()
        };
        let mut note = "Adjusted probabilities move counts half a check away from 0 and 1 (AIAG Measurement \
                        Systems Analysis, 4th ed., analytic method). Reference values accepted some but not all \
                        of the time are fitted by least squares as Φ⁻¹(P′) = b₀ + b₁·x; x(P) is the reference \
                        value accepted with probability P."
            .to_owned();
        for n in s.design_notes() {
            note.push(' ');
            note.push_str(&n);
        }
        vec![
            section(SectionKind::AttributeGaugeSummary, "Bias and Repeatability", SectionContent::Table(summary)),
            section(SectionKind::AttributeGaugePoints, "Acceptance by Reference Value", SectionContent::Table(pts)),
            section(SectionKind::AttributeGaugeCurve, "Gauge Performance Curve", SectionContent::Plot(Box::new(plot))),
            section(SectionKind::AttributeGaugeNote, "About This Study", SectionContent::Note(note)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;

    #[test]
    fn agreement_report_on_the_inspection_sample() {
        let t = sample::visual_inspection();
        let states = RowStateManager::new(t.n_rows());
        let mut req = AgreementRequest::new(&t, 0, 1, t.find("Result").unwrap());
        req.trial = t.find("Trial");
        req.standard = t.find("Standard result");
        assert!(!req.ordinal, "a text column is nominal");
        let a = AgreementAnalysis::run(&t, &states, &req).unwrap();
        assert_eq!(a.ratings().categories, ["Fail", "Pass"]);
        assert_eq!(a.study().within.len(), 3);
        assert_eq!(a.study().trials, 2);
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert!(kinds.contains(&SectionKind::AgreementPlot) && kinds.contains(&SectionKind::AgreementPairs));
        assert!(!kinds.contains(&SectionKind::AgreementOrdinal));
        let back = AgreementRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);

        let mut ord = AgreementRequest::new(&t, 0, 1, t.find("Grade").unwrap());
        ord.trial = t.find("Trial");
        ord.standard = t.find("Standard grade");
        assert!(ord.ordinal, "the grade column is ordinal");
        let b = AgreementAnalysis::run(&t, &states, &ord).unwrap();
        assert!(b.study().between.as_ref().unwrap().concordance.is_some());
        assert!(b.sections().iter().any(|x| x.kind == SectionKind::AgreementOrdinal));

        // Excluding one rating unbalances the study.
        let mut st = RowStateManager::new(t.n_rows());
        st.set_excluded(0..1, true);
        assert!(AgreementAnalysis::run(&t, &st, &req).is_err());
        // Without the trial column, ratings are trials in row order.
        let no_trial = AgreementRequest { trial: None, ..req.clone() };
        let c = AgreementAnalysis::run(&t, &states, &no_trial).unwrap();
        assert_eq!(c.study().trials, 2);
        assert!(AgreementAnalysis::run(&t, &states, &AgreementRequest { rating: 0, ..req.clone() }).is_err());
    }

    /// A grade found only in an excluded row is not a category: the study,
    /// including the weighted kappas, is the same as without that row.
    #[test]
    fn categories_come_from_the_rows_used() {
        let base = sample::visual_inspection();
        let request = |t: &DataTable| {
            let mut r = AgreementRequest::new(t, 0, 1, t.find("Grade").unwrap());
            r.trial = t.find("Trial");
            r.standard = t.find("Standard grade");
            r
        };
        let reference = AgreementAnalysis::run(&base, &RowStateManager::new(base.n_rows()), &request(&base)).unwrap();
        // An excluded row (a copy of the first) with an in-between grade.
        let grade = base.find("Grade").unwrap();
        let first = reference.ratings().categories[0].parse::<f64>().unwrap();
        let second = reference.ratings().categories[1].parse::<f64>().unwrap();
        let mut cols = base.columns().to_vec();
        for (j, c) in cols.iter_mut().enumerate() {
            match &mut c.data {
                stat_data::ColumnData::Text(v) => v.push(v[0].clone()),
                stat_data::ColumnData::Numeric(v) => v.push(if j == grade { 0.5 * (first + second) } else { v[0] }),
                stat_data::ColumnData::DateTime { values, .. } => values.push(values[0]),
            }
        }
        let t = DataTable::new("t", cols).unwrap();
        let mut states = RowStateManager::new(t.n_rows());
        states.set_excluded([base.n_rows()], true);
        let a = AgreementAnalysis::run(&t, &states, &request(&t)).unwrap();
        assert_eq!(a.ratings().categories, reference.ratings().categories);
        let (v, v0) = (&a.study().versus_standard[0], &reference.study().versus_standard[0]);
        let (w, w0) = (v.weighted.as_ref().unwrap(), v0.weighted.as_ref().unwrap());
        assert!((w.0.kappa - w0.0.kappa).abs() < 1e-12 && (w.1.kappa - w0.1.kappa).abs() < 1e-12);
        // The dialog's list (no row states) still offers every label.
        let every = agreement_categories(&t, &request(&t)).unwrap();
        assert_eq!(every.len(), reference.ratings().categories.len() + 1);
        // An order listing it (as the dialog sends) does not bring it back.
        let ordered = AgreementRequest { order: every, ..request(&t) };
        let b = AgreementAnalysis::run(&t, &states, &ordered).unwrap();
        assert_eq!(b.ratings().categories, reference.ratings().categories);
    }

    #[test]
    fn category_order_follows_the_request() {
        let t = sample::visual_inspection();
        let mut req = AgreementRequest::new(&t, 0, 1, t.find("Result").unwrap());
        req.order = vec!["Pass".to_owned()];
        assert_eq!(agreement_categories(&t, &req).unwrap(), ["Pass", "Fail"]);
    }

    #[test]
    fn gauge_report_on_the_sample_in_both_layouts() {
        let t = sample::go_no_go_gauge();
        let states = RowStateManager::new(t.n_rows());
        let req = AttributeGaugeRequest::counts(
            t.find("Reference (mm)").unwrap(),
            t.find("Accepted").unwrap(),
            t.find("Checks").unwrap(),
            sample::GO_NO_GO_LIMIT,
        );
        let a = AttributeGaugeAnalysis::run(&t, &states, &req).unwrap();
        assert_eq!(a.study().side, LimitSide::Lower);
        assert!(a.study().bias.abs() < 0.01);
        assert_eq!(a.sections().len(), 4);
        assert_eq!(AttributeGaugeRequest::from_json(&req.to_json(&t), &t).unwrap(), req);

        // The same study with one row per check.
        let (refs, acc) = (t.column(req.reference).unwrap(), t.column(req.acceptance).unwrap());
        let (refs, acc) = (refs.as_numeric().unwrap(), acc.as_numeric().unwrap());
        let (mut x, mut result) = (Vec::new(), Vec::new());
        for (r, a) in refs.iter().zip(acc) {
            for i in 0..20 {
                x.push(*r);
                result.push(if f64::from(i) < *a { "Go" } else { "No go" }.to_owned());
            }
        }
        let long = DataTable::new(
            "long",
            vec![stat_data::Column::numeric("Reference", x), stat_data::Column::text("Result", result)],
        )
        .unwrap();
        let long_req =
            AttributeGaugeRequest { reference: 0, acceptance: 1, checks: None, accept_label: "Go".to_owned(), ..req };
        let b = AttributeGaugeAnalysis::run(&long, &RowStateManager::new(long.n_rows()), &long_req).unwrap();
        assert!((b.study().bias - a.study().bias).abs() < 1e-12);
        let unlabelled = AttributeGaugeRequest { accept_label: String::new(), ..long_req };
        assert!(AttributeGaugeAnalysis::run(&long, &RowStateManager::new(long.n_rows()), &unlabelled).is_err());
    }
}
