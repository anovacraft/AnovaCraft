//! Report sections for comparisons within the slices of an interaction and
//! for comparing slopes across groups ([`stat_linear::marginal`]), shared by
//! the linear model, GLM and mixed model reports.

use crate::format::{format_number, format_p_value};
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use stat_linear::ModelDesign;
use stat_linear::marginal::{
    ContrastInference, JointTest, PairAdjust, PairDifference, group_slopes, interaction_pairs, slices, slope_pairs,
};

/// Significant digits in the tables.
const DIGITS: usize = 6;

fn section(kind: SectionKind, title: String, content: SectionContent) -> ReportSection {
    ReportSection { kind, title, content }
}

fn df_text(df: Option<f64>) -> String {
    df.map_or_else(|| "∞".to_owned(), |d| format_number(d, 4))
}

fn test_cells(t: &JointTest) -> [String; 4] {
    [t.num_df.to_string(), df_text(t.den_df), format_number(t.f, DIGITS), format_p_value(t.p_value)]
}

fn pair_row(label: &str, p: &PairDifference) -> Vec<String> {
    vec![
        label.to_owned(),
        format!("{} − {}", p.first, p.second),
        format_number(p.estimate, DIGITS),
        format_number(p.std_error, DIGITS),
        df_text(p.df),
        format_number(p.statistic, 4),
        format_p_value(p.p_value),
        format_number(p.interval.0, DIGITS),
        format_number(p.interval.1, DIGITS),
    ]
}

fn pair_columns(first: &str, level: f64) -> Vec<String> {
    let pct = (level * 100.0).round();
    [first, "Comparison", "Difference", "Std Error", "DF", "t", "p-value"]
        .iter()
        .map(|s| (*s).to_owned())
        .chain([format!("Lower {pct}%"), format!("Upper {pct}%")])
        .collect()
}

fn adjust_text(adjust: PairAdjust) -> &'static str {
    match adjust {
        PairAdjust::Tukey => "Tukey adjustment within each slice",
        PairAdjust::Holm => "Holm adjustment within each slice; Bonferroni intervals",
        PairAdjust::Bonferroni => "Bonferroni adjustment within each slice",
        PairAdjust::None => "no adjustment",
    }
}

/// The sections for every two-factor interaction of categorical predictors
/// (`slices`) and every `factor × continuous` effect (`slopes`) of `design`;
/// `PairAdjust::None` leaves out the pairwise tables. `scale` notes the scale
/// of the estimates (e.g. "log scale" for a log-link GLM).
#[must_use]
pub fn sections(
    design: &ModelDesign,
    inference: &dyn ContrastInference,
    slices_adjust: Option<PairAdjust>,
    slopes_adjust: Option<PairAdjust>,
    level: f64,
    scale: Option<&str>,
) -> Vec<ReportSection> {
    let mut out = Vec::new();
    let scale_note = scale.map_or_else(String::new, |s| format!("; {s}"));
    if let Some(adjust) = slices_adjust {
        let pairs = interaction_pairs(design);
        if pairs.is_empty() {
            out.push(section(
                SectionKind::SliceTests,
                "Comparisons Within Slices".to_owned(),
                SectionContent::Note(
                    "The model has no interaction of two categorical predictors to slice. Add an interaction such \
                     as A × B to compare one factor's levels at each level of the other."
                        .to_owned(),
                ),
            ));
        }
        for (a, b) in pairs {
            out.extend(slice_sections(design, inference, a, b, adjust, level, &scale_note));
        }
    }
    if let Some(adjust) = slopes_adjust {
        let pairs = slope_pairs(design);
        if pairs.is_empty() {
            out.push(section(
                SectionKind::SlopeComparisons,
                "Slopes by Group".to_owned(),
                SectionContent::Note(
                    "The model has no effect of a categorical predictor with a continuous one (separate slopes, \
                     such as Group × x)."
                        .to_owned(),
                ),
            ));
        }
        for (g, x) in pairs {
            out.extend(slope_sections(design, inference, g, x, adjust, level, &scale_note));
        }
    }
    out
}

fn slice_sections(
    design: &ModelDesign,
    inference: &dyn ContrastInference,
    a: usize,
    b: usize,
    adjust: PairAdjust,
    level: f64,
    scale_note: &str,
) -> Vec<ReportSection> {
    let names = (design.terms()[a].name.clone(), design.terms()[b].name.clone());
    let pairs = (adjust != PairAdjust::None).then_some(adjust);
    let both = slices(design, inference, a, b, pairs, level).and_then(|ab| {
        let ba = slices(design, inference, b, a, pairs, level)?;
        Ok((ab, ba))
    });
    let title = format!("Slices of {} × {}", names.0, names.1);
    let (ab, ba) = match both {
        Ok(v) => v,
        Err(e) => {
            return vec![section(SectionKind::SliceTests, title, SectionContent::Note(format!("Not available: {e}")))];
        }
    };
    let mut tests = TextTable::new(["Effect", "Within", "Num DF", "Den DF", "F", "p-value"]);
    for s in ab.iter().chain(&ba) {
        let [n, d, f, p] = test_cells(&s.test);
        tests.push([s.factor.clone(), format!("{} = {}", s.by, s.at), n, d, f, p]);
    }
    let mut out = vec![section(
        SectionKind::SliceTests,
        format!("{title}: tests of each factor at each level of the other (Wald F{scale_note})"),
        SectionContent::Table(tests),
    )];
    let pct = (level * 100.0).round();
    let mut means = TextTable::new([
        names.1.clone(),
        names.0.clone(),
        "LS Mean".to_owned(),
        "Std Error".to_owned(),
        format!("Lower {pct}%"),
        format!("Upper {pct}%"),
    ]);
    for s in &ab {
        for m in &s.means {
            means.push([
                s.at.clone(),
                m.level.clone(),
                format_number(m.estimate, DIGITS),
                format_number(m.std_error, DIGITS),
                format_number(m.interval.0, DIGITS),
                format_number(m.interval.1, DIGITS),
            ]);
        }
    }
    out.push(section(
        SectionKind::SliceMeans,
        format!(
            "Least-Squares Means of {} × {} (other factors averaged, covariates at their means{scale_note})",
            names.0, names.1
        ),
        SectionContent::Table(means),
    ));
    if pairs.is_some() {
        let mut t = TextTable::new(pair_columns("Within", level));
        for s in ab.iter().chain(&ba) {
            let label = format!("{} = {}", s.by, s.at);
            for p in &s.pairs {
                t.push(pair_row(&label, p));
            }
        }
        out.push(section(
            SectionKind::SlicePairs,
            format!("Pairwise Differences Within Slices of {} × {} ({})", names.0, names.1, adjust_text(adjust)),
            SectionContent::Table(t),
        ));
    }
    out
}

fn slope_sections(
    design: &ModelDesign,
    inference: &dyn ContrastInference,
    g: usize,
    x: usize,
    adjust: PairAdjust,
    level: f64,
    scale_note: &str,
) -> Vec<ReportSection> {
    let (gn, xn) = (design.terms()[g].name.clone(), design.terms()[x].name.clone());
    let title = format!("Slopes of {xn} by {gn}");
    let c = match group_slopes(design, inference, g, x, adjust, level) {
        Ok(c) => c,
        Err(e) => {
            return vec![section(
                SectionKind::SlopeComparisons,
                title,
                SectionContent::Note(format!("Not available: {e}")),
            )];
        }
    };
    let pct = (level * 100.0).round();
    let mut t = TextTable::new([
        gn.clone(),
        "Slope".to_owned(),
        "Std Error".to_owned(),
        "DF".to_owned(),
        "t".to_owned(),
        "p-value".to_owned(),
        format!("Lower {pct}%"),
        format!("Upper {pct}%"),
    ]);
    for s in &c.slopes {
        t.push([
            s.level.clone(),
            format_number(s.estimate, DIGITS),
            format_number(s.std_error, DIGITS),
            df_text(s.df),
            format_number(s.statistic, 4),
            format_p_value(s.p_value),
            format_number(s.interval.0, DIGITS),
            format_number(s.interval.1, DIGITS),
        ]);
    }
    let [n, d, f, p] = test_cells(&c.equal_slopes);
    let mut eq = TextTable::new(["Test", "Num DF", "Den DF", "F", "p-value"]);
    eq.push(["Equal slopes".to_owned(), n, d, f, p]);
    let mut out = vec![
        section(
            SectionKind::SlopeComparisons,
            format!("{title} (at the mean of {xn}, other factors averaged{scale_note})"),
            SectionContent::Table(t),
        ),
        section(
            SectionKind::EqualSlopes,
            format!("Test of Equal Slopes of {xn} Across {gn}"),
            SectionContent::Table(eq),
        ),
    ];
    if adjust != PairAdjust::None {
        let mut t = TextTable::new(pair_columns("Slope of", level));
        for p in &c.pairs {
            t.push(pair_row(&xn, p));
        }
        let how = match adjust {
            PairAdjust::Tukey => "Tukey–Kramer adjustment",
            PairAdjust::Holm => "Holm adjustment; Bonferroni intervals",
            PairAdjust::Bonferroni => "Bonferroni adjustment",
            PairAdjust::None => "no adjustment",
        };
        out.push(section(
            SectionKind::SlopePairs,
            format!("Pairwise Differences of Slopes of {xn} ({how})"),
            SectionContent::Table(t),
        ));
    }
    out
}

/// The knots of each spline term of `design`, or `None` without splines.
#[must_use]
pub fn spline_knots_section(design: &ModelDesign) -> Option<ReportSection> {
    if !design.has_splines() {
        return None;
    }
    let mut t = TextTable::new(["Predictor", "Basis", "Columns", "Boundary knots", "Interior knots"]);
    for (i, term) in design.terms().iter().enumerate() {
        let Some(b) = design.spline(i) else { continue };
        let fmt = |v: f64| format_number(v, DIGITS);
        let interior: Vec<String> = b.interior().iter().map(|&k| fmt(k)).collect();
        t.push([
            term.name.clone(),
            b.kind().label(),
            b.n_columns().to_string(),
            format!("{}, {}", fmt(b.boundary().0), fmt(b.boundary().1)),
            if interior.is_empty() { "none".to_owned() } else { interior.join(", ") },
        ]);
    }
    Some(section(
        SectionKind::SplineKnots,
        "Spline Terms (the effect test of each is the joint test of its columns)".to_owned(),
        SectionContent::Table(t),
    ))
}
