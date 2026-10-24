//! Optional sections of a distribution report that test or bound the values
//! of one continuous column: tolerance intervals
//! ([`stat_inference::tolerance`]), outlier tests
//! ([`stat_inference::outliers`]) and runs tests for randomness in row order
//! ([`stat_inference::runs`]).
//!
//! Each is switched on from the report's options menu with its own settings;
//! the settings are saved with the report.

use crate::format::{format_number, format_p_value};
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_inference::Alternative;
use stat_inference::outliers::{DixonRatio, dixon_test, generalized_esd_test, grubbs_test};
use stat_inference::runs::{RunsTest, runs_test, up_down_runs_test};
use stat_inference::tolerance::{ToleranceSides, nonparametric_tolerance_interval, normal_tolerance_interval};

const DIGITS: usize = 6;

/// Settings of a tolerance interval: an interval that contains at least
/// `proportion` of the population with the stated `confidence`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToleranceSettings {
    /// Proportion of the population to contain, in (0, 1).
    pub proportion: f64,
    /// Confidence, in (0, 1).
    pub confidence: f64,
    /// Both limits, or one.
    pub sides: ToleranceSides,
    /// `true` for the distribution-free interval from order statistics
    /// (Wilks 1941); `false` for the normal-theory interval.
    pub distribution_free: bool,
}

impl Default for ToleranceSettings {
    fn default() -> Self {
        Self { proportion: 0.90, confidence: 0.95, sides: ToleranceSides::TwoSided, distribution_free: false }
    }
}

/// Which outlier test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutlierMethod {
    /// Grubbs' test for one outlier (maximum normed residual).
    #[default]
    Grubbs,
    /// Dixon's ratio test for one outlier (small samples).
    Dixon,
    /// Rosner's generalized extreme studentized deviate test for up to a
    /// given number of outliers.
    GeneralizedEsd,
}

impl OutlierMethod {
    /// Every method.
    pub const ALL: [Self; 3] = [Self::Grubbs, Self::Dixon, Self::GeneralizedEsd];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Grubbs => "Grubbs",
            Self::Dixon => "Dixon",
            Self::GeneralizedEsd => "Generalized ESD (Rosner)",
        }
    }
}

/// Settings of an outlier test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlierSettings {
    /// The test.
    pub method: OutlierMethod,
    /// End tested: `TwoSided` the more extreme value, `Greater` the largest,
    /// `Less` the smallest (Grubbs and Dixon; the generalized ESD is two-sided).
    pub side: Alternative,
    /// Significance level, in (0, 1).
    pub alpha: f64,
    /// Dixon's ratio (`None`: chosen by sample size).
    pub dixon_ratio: Option<DixonRatio>,
    /// Most outliers the generalized ESD looks for (`None`: ⌊n/10⌋, at least 1).
    pub max_outliers: Option<usize>,
}

impl Default for OutlierSettings {
    fn default() -> Self {
        Self {
            method: OutlierMethod::Grubbs,
            side: Alternative::TwoSided,
            alpha: 0.05,
            dixon_ratio: None,
            max_outliers: None,
        }
    }
}

/// Settings of the runs tests for randomness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunsSettings {
    /// Reference for runs above and below (`None`: the median).
    pub reference: Option<f64>,
    /// `Less`: too few runs (clustering, trend); `Greater`: too many
    /// (alternation); `TwoSided`: either.
    pub alternative: Alternative,
}

impl Default for RunsSettings {
    fn default() -> Self {
        Self { reference: None, alternative: Alternative::TwoSided }
    }
}

/// Name of an outlier side.
#[must_use]
pub fn side_label(side: Alternative) -> &'static str {
    match side {
        Alternative::TwoSided => "either end",
        Alternative::Greater => "largest value",
        Alternative::Less => "smallest value",
    }
}

/// Text for a limit, "none" for an unbounded side.
fn limit_text(x: f64, text: &dyn Fn(f64) -> String) -> String {
    if x.is_finite() { text(x) } else { "none".to_owned() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn pct(x: f64) -> String {
    format!("{}%", format_number(100.0 * x, 4))
}

/// The tolerance interval of `values` and its limits, or the reason it could
/// not be computed.
pub(crate) fn tolerance_limits(values: &[f64], s: &ToleranceSettings) -> Result<(f64, f64)> {
    let ti = if s.distribution_free {
        nonparametric_tolerance_interval(values, s.proportion, s.confidence, s.sides)?
    } else {
        normal_tolerance_interval(values, s.proportion, s.confidence, s.sides)?
    };
    Ok((ti.lower, ti.upper))
}

/// The "Tolerance Interval" section. `text` formats a value of the column.
pub(crate) fn tolerance_section(values: &[f64], s: &ToleranceSettings, text: &dyn Fn(f64) -> String) -> ReportSection {
    let result = if s.distribution_free {
        nonparametric_tolerance_interval(values, s.proportion, s.confidence, s.sides)
    } else {
        normal_tolerance_interval(values, s.proportion, s.confidence, s.sides)
    };
    let title = format!(
        "Tolerance Interval ({} of the population, {} confidence, {})",
        pct(s.proportion),
        pct(s.confidence),
        s.sides.label()
    );
    let ti = match result {
        Ok(ti) => ti,
        Err(e) => {
            return section(SectionKind::ToleranceInterval, title, SectionContent::Note(format!("Not available: {e}")));
        }
    };
    let mut t = TextTable::new(["Statistic", "Value"]);
    t.push([
        "Method".to_owned(),
        if s.distribution_free { "Distribution-free (order statistics)".to_owned() } else { ti.method.to_owned() },
    ]);
    t.push(["N".to_owned(), ti.n.to_string()]);
    t.push(["Proportion contained".to_owned(), pct(ti.proportion)]);
    t.push(["Confidence requested".to_owned(), pct(ti.confidence)]);
    if s.distribution_free {
        t.push(["Confidence achieved".to_owned(), pct(ti.achieved_confidence)]);
    } else {
        t.push(["Mean".to_owned(), text(ti.mean)]);
        t.push(["Standard deviation".to_owned(), format_number(ti.std_dev, DIGITS)]);
        t.push(["Factor k (limits: mean ± k × SD)".to_owned(), format_number(ti.factor, DIGITS)]);
    }
    t.push(["Lower limit".to_owned(), limit_text(ti.lower, text)]);
    t.push(["Upper limit".to_owned(), limit_text(ti.upper, text)]);
    if let Some((lo, hi)) = ti.order_statistics {
        let rank = |r: usize| if r == 0 || r > ti.n { "none".to_owned() } else { format!("{r} of {}", ti.n) };
        t.push(["Order statistics used (lower, upper)".to_owned(), format!("{}, {}", rank(lo), rank(hi))]);
    }
    if s.distribution_free && ti.achieved_confidence < ti.confidence {
        t.push([
            "Note".to_owned(),
            format!(
                "the sample is too small to reach {} confidence; the interval spans the whole sample",
                pct(ti.confidence)
            ),
        ]);
    }
    section(SectionKind::ToleranceInterval, title, SectionContent::Table(t))
}

/// The outlier test's sections and the rows it flags. `rows[i]` is the
/// dataset row of `values[i]`.
pub(crate) fn outlier_sections(
    values: &[f64],
    rows: &[usize],
    s: &OutlierSettings,
    text: &dyn Fn(f64) -> String,
) -> (Vec<ReportSection>, Vec<usize>) {
    let alpha = format_number(s.alpha, 4);
    let row_text = |i: usize| (rows[i] + 1).to_string();
    let not_available = |title: String, e: stat_core::StatError| {
        (
            vec![section(SectionKind::OutlierTests, title, SectionContent::Note(format!("Not available: {e}")))],
            Vec::new(),
        )
    };
    let columns = ["Test", "End tested", "Value", "Row", "Statistic", "Critical value", "p-value", "Outlier"];
    match s.method {
        OutlierMethod::Grubbs => {
            let title = format!("Outlier Tests (Grubbs, α = {alpha})");
            match grubbs_test(values, s.side, s.alpha) {
                Ok(g) => {
                    let flagged = g.statistic > g.critical_value;
                    let mut t = TextTable::new(columns);
                    t.push([
                        "Grubbs G".to_owned(),
                        side_label(s.side).to_owned(),
                        text(g.value),
                        row_text(g.index),
                        format_number(g.statistic, DIGITS),
                        format_number(g.critical_value, DIGITS),
                        format_p_value(g.p_value),
                        if flagged { "yes".to_owned() } else { "no".to_owned() },
                    ]);
                    let flagged_rows = if flagged { vec![rows[g.index]] } else { Vec::new() };
                    (vec![section(SectionKind::OutlierTests, title, SectionContent::Table(t))], flagged_rows)
                }
                Err(e) => not_available(title, e),
            }
        }
        OutlierMethod::Dixon => {
            let title = format!("Outlier Tests (Dixon, α = {alpha})");
            match dixon_test(values, s.dixon_ratio, s.side, s.alpha) {
                Ok(d) => {
                    let flagged = d.statistic > d.critical_value;
                    let mut t = TextTable::new(columns);
                    let chosen = if s.dixon_ratio.is_none() { " (by sample size)" } else { "" };
                    t.push([
                        format!("Dixon {}{chosen}", d.ratio.label()),
                        side_label(s.side).to_owned(),
                        text(d.value),
                        row_text(d.index),
                        format_number(d.statistic, DIGITS),
                        format_number(d.critical_value, DIGITS),
                        format_p_value(d.p_value),
                        if flagged { "yes".to_owned() } else { "no".to_owned() },
                    ]);
                    let flagged_rows = if flagged { vec![rows[d.index]] } else { Vec::new() };
                    (vec![section(SectionKind::OutlierTests, title, SectionContent::Table(t))], flagged_rows)
                }
                Err(e) => not_available(title, e),
            }
        }
        OutlierMethod::GeneralizedEsd => {
            let max = s.max_outliers.unwrap_or((values.len() / 10).max(1));
            let title = format!("Outlier Tests (generalized ESD, up to {max}, α = {alpha})");
            match generalized_esd_test(values, max, s.alpha) {
                Ok(esd) => {
                    let mut t = TextTable::new(["Step", "Value", "Row", "Rᵢ", "λᵢ (critical)", "Rᵢ > λᵢ"]);
                    for (i, step) in esd.steps.iter().enumerate() {
                        t.push([
                            (i + 1).to_string(),
                            text(step.value),
                            row_text(step.index),
                            format_number(step.statistic, DIGITS),
                            format_number(step.critical_value, DIGITS),
                            if step.statistic > step.critical_value { "yes".to_owned() } else { "no".to_owned() },
                        ]);
                    }
                    let flagged: Vec<usize> = esd.outlier_indices().iter().map(|&i| rows[i]).collect();
                    let summary = format!(
                        "Outliers found: {} (the largest step i with Rᵢ > λᵢ; it and every earlier value are outliers).",
                        esd.outliers
                    );
                    (
                        vec![
                            section(SectionKind::OutlierTests, title, SectionContent::Note(summary)),
                            section(SectionKind::OutlierSteps, "Generalized ESD Steps", SectionContent::Table(t)),
                        ],
                        flagged,
                    )
                }
                Err(e) => not_available(title, e),
            }
        }
    }
}

fn runs_row(name: &str, r: &RunsTest) -> [String; 7] {
    [
        name.to_owned(),
        r.runs.to_string(),
        format_number(r.expected, DIGITS),
        format_number(r.std_dev, DIGITS),
        format_p_value(r.p_fewer),
        format_p_value(r.p_more),
        if r.exact { "exact".to_owned() } else { "normal approximation".to_owned() },
    ]
}

/// The "Runs Tests" section: runs above and below a reference, and runs up
/// and down, of `values` in row order.
pub(crate) fn runs_section(values: &[f64], s: &RunsSettings, text: &dyn Fn(f64) -> String) -> ReportSection {
    let mut t = TextTable::new(["Test", "Runs", "Expected", "SD", "p (too few)", "p (too many)", "Method"]);
    let mut notes = Vec::new();
    match runs_test(values, s.reference, s.alternative) {
        Ok(r) => {
            let what = if s.reference.is_some() { "value" } else { "median" };
            t.push(runs_row(&format!("Above and below the {what} {}", text(r.reference)), &r));
            notes.push(format!("above/below: p = {}", format_p_value(r.p_value)));
        }
        Err(e) => t.push([
            "Above and below".to_owned(),
            format!("not available: {e}"),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]),
    }
    match up_down_runs_test(values, s.alternative) {
        Ok(r) => {
            t.push(runs_row("Up and down", &r));
            notes.push(format!("up/down: p = {}", format_p_value(r.p_value)));
        }
        Err(e) => t.push([
            "Up and down".to_owned(),
            format!("not available: {e}"),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]),
    }
    let alternative = match s.alternative {
        Alternative::TwoSided => "too few or too many runs",
        Alternative::Less => "too few runs",
        Alternative::Greater => "too many runs",
    };
    let joined = if notes.is_empty() { String::new() } else { format!("; {}", notes.join(", ")) };
    section(
        SectionKind::RunsTests,
        format!("Runs Tests for Randomness (in row order; alternative: {alternative}{joined})"),
        SectionContent::Table(t),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(x: f64) -> String {
        format_number(x, DIGITS)
    }

    #[test]
    fn tolerance_section_matches_engine() {
        let x: Vec<f64> = (1..=20).map(|i| f64::from(i) * 0.5 + (f64::from(i) * 1.7).sin()).collect();
        let s = ToleranceSettings::default();
        let SectionContent::Table(t) = tolerance_section(&x, &s, &plain).content else { panic!() };
        let ti = normal_tolerance_interval(&x, 0.9, 0.95, ToleranceSides::TwoSided).unwrap();
        assert_eq!(t.cell(t.rows.len() - 2, "Value").unwrap(), plain(ti.lower));
        let (lo, hi) = tolerance_limits(&x, &s).unwrap();
        assert!((lo - ti.lower).abs() < 1e-12 && (hi - ti.upper).abs() < 1e-12);
        // Distribution-free on a small sample notes the shortfall.
        let small = ToleranceSettings { distribution_free: true, proportion: 0.99, ..s };
        let SectionContent::Table(t) = tolerance_section(&x[..5], &small, &plain).content else { panic!() };
        assert!(t.rows.iter().any(|r| r[0] == "Note"));
    }

    #[test]
    fn outlier_sections_flag_rows() {
        let mut x = vec![10.1, 9.9, 10.0, 10.2, 9.8, 10.05, 9.95, 10.1, 9.9, 10.0];
        x.push(13.0);
        let rows: Vec<usize> = (0..x.len()).map(|i| i + 100).collect();
        for method in OutlierMethod::ALL {
            let s = OutlierSettings { method, ..OutlierSettings::default() };
            let (sections, flagged) = outlier_sections(&x, &rows, &s, &plain);
            assert!(!sections.is_empty());
            assert_eq!(flagged, [110], "{method:?}");
        }
        let (s, flagged) = outlier_sections(&x[..2], &rows, &OutlierSettings::default(), &plain);
        assert!(matches!(s[0].content, SectionContent::Note(_)) && flagged.is_empty());
    }

    #[test]
    fn runs_section_lists_both_tests() {
        let x: Vec<f64> = (0..30).map(|i| f64::from(i % 7) + f64::from(i) * 0.01).collect();
        let SectionContent::Table(t) = runs_section(&x, &RunsSettings::default(), &plain).content else { panic!() };
        assert_eq!(t.rows.len(), 2);
        assert!(t.rows[0][0].starts_with("Above and below the median"));
        let r = runs_test(&x, None, Alternative::TwoSided).unwrap();
        assert_eq!(t.rows[0][1], r.runs.to_string());
    }
}
