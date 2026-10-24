//! Report sections shared by several reports: ANOVA effect sizes with
//! noncentral-F intervals, measures of association with noncentral-χ²
//! intervals ([`stat_inference::effect_sizes`]), and the linear-trend
//! contrast across ordered groups ([`stat_inference::multiple::linear_trend`]).

use crate::compare::{DIGITS, section};
use crate::format::{format_number, format_p_value};
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_inference::effect_sizes::{AnovaEffectSize, AssociationEffectSize};
use stat_inference::multiple::linear_trend;

fn pct(level: f64) -> String {
    format_number(100.0 * level, 4)
}

/// Cohen's (1988) benchmarks for `f`: 0.10 small, 0.25 medium, 0.40 large.
fn f_size(f: f64) -> &'static str {
    match f {
        v if v < 0.1 => "negligible",
        v if v < 0.25 => "small",
        v if v < 0.4 => "medium",
        _ => "large",
    }
}

/// One row per effect: η², partial η², partial ω², Cohen's f, and intervals
/// for the population partial η² and f. `rows` pairs each effect's name with
/// its effect sizes (or why they are not available).
#[must_use]
pub fn anova_effect_size_section(rows: Vec<(String, Result<AnovaEffectSize>)>, level: f64) -> ReportSection {
    let p = pct(level);
    let mut t = TextTable::new([
        "Effect".to_owned(),
        "η²".to_owned(),
        "Partial η²".to_owned(),
        format!("Lower {p}%"),
        format!("Upper {p}%"),
        "Partial ω²".to_owned(),
        "Cohen's f".to_owned(),
        format!("f lower {p}%"),
        format!("f upper {p}%"),
        "Size (Cohen 1988)".to_owned(),
    ]);
    for (name, r) in rows {
        match r {
            Ok(e) => t.push([
                name,
                format_number(e.eta_squared, DIGITS),
                format_number(e.partial_eta_squared, DIGITS),
                format_number(e.partial_eta_interval.0, DIGITS),
                format_number(e.partial_eta_interval.1, DIGITS),
                format_number(e.omega_squared, DIGITS),
                format_number(e.cohens_f, DIGITS),
                format_number(e.cohens_f_interval.0, DIGITS),
                format_number(e.cohens_f_interval.1, DIGITS),
                f_size(e.cohens_f).to_owned(),
            ]),
            Err(e) => {
                let mut row = vec![name, format!("not available: {e}")];
                row.extend((0..8).map(|_| String::new()));
                t.push(row);
            }
        }
    }
    section(
        SectionKind::AnovaEffectSizes,
        format!(
            "Effect Sizes ({p}% intervals for the population partial η² and f by inverting the noncentral F; \
             Steiger 2004)"
        ),
        SectionContent::Table(t),
    )
}

/// φ and Cramér's V with intervals.
#[must_use]
pub fn association_section(r: Result<AssociationEffectSize>, two_by_two: bool, level: f64) -> ReportSection {
    let p = pct(level);
    let title = format!("Measures of Association ({p}% intervals from the noncentral χ²; Smithson 2003)");
    let a = match r {
        Ok(a) => a,
        Err(e) => {
            return section(SectionKind::EffectSizes, title, SectionContent::Note(format!("Not available: {e}")));
        }
    };
    let mut t =
        TextTable::new(["Measure".to_owned(), "Value".to_owned(), format!("Lower {p}%"), format!("Upper {p}%")]);
    let mut row = |name: &str, v: f64, (lo, hi): (f64, f64)| {
        t.push([name.to_owned(), format_number(v, DIGITS), format_number(lo, DIGITS), format_number(hi, DIGITS)]);
    };
    if two_by_two {
        row("φ (unsigned)", a.phi, a.phi_interval);
    }
    row("Cramér's V", a.cramers_v, a.cramers_v_interval);
    section(SectionKind::EffectSizes, title, SectionContent::Table(t))
}

/// The linear-trend contrast across `groups` in the order of `labels`.
#[must_use]
pub fn linear_trend_section(labels: &[String], groups: &[Vec<f64>], level: f64) -> ReportSection {
    let title = format!("Linear Trend across Ordered Groups ({})", labels.join(" < "));
    let tr = match linear_trend(groups, level) {
        Ok(t) => t,
        Err(e) => {
            return section(SectionKind::LinearTrend, title, SectionContent::Note(format!("Not available: {e}")));
        }
    };
    let p = pct(level);
    let k = groups.len();
    let coefs: Vec<String> = (1..=k).map(|i| format_number(i as f64 - (k as f64 + 1.0) / 2.0, 3)).collect();
    let mut t = TextTable::new([
        "Test".to_owned(),
        "Estimate".to_owned(),
        "Std error".to_owned(),
        "Statistic".to_owned(),
        "DF".to_owned(),
        "p-value".to_owned(),
        format!("Lower {p}%"),
        format!("Upper {p}%"),
    ]);
    t.push([
        format!("Linear contrast (coefficients {})", coefs.join(", ")),
        format_number(tr.contrast, DIGITS),
        format_number(tr.std_error, DIGITS),
        format!("t = {}", format_number(tr.t_statistic, DIGITS)),
        format_number(tr.df, 4),
        format_p_value(tr.p_value),
        format_number(tr.confidence_interval.0, DIGITS),
        format_number(tr.confidence_interval.1, DIGITS),
    ]);
    t.push([
        "Departure from a straight line".to_owned(),
        String::new(),
        String::new(),
        format!("F = {}", format_number(tr.nonlinearity_f, DIGITS)),
        format!("{}, {}", format_number(tr.nonlinearity_df, 4), format_number(tr.df, 4)),
        format_p_value(tr.nonlinearity_p),
        String::new(),
        String::new(),
    ]);
    section(
        SectionKind::LinearTrend,
        format!(
            "{title}: the trend explains {}% of the between-group sum of squares",
            format_number(100.0 * tr.share_of_between, 4)
        ),
        SectionContent::Table(t),
    )
}
