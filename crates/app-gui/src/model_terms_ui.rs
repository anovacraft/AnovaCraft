//! Shared dialog controls for model terms and model comparisons: spline
//! terms for continuous predictors, and the pairwise adjustment of
//! comparisons within slices of an interaction or of slopes by group.

use crate::dialog_kit::{self as kit, ComboExt as _};
use app_model::{MAX_SPLINE_DEGREE, MAX_SPLINE_KNOTS, PairAdjust, SplineKind, SplineKnots, SplineSpec};
use eframe::egui;
use stat_data::DataTable;

/// Spline terms chosen in a dialog, by column (not position, so they survive
/// changes to the predictor list).
pub type ColumnSplines = Vec<(usize, SplineSpec)>;

/// The spline of each predictor, by position in `predictors`.
#[must_use]
pub fn by_position(splines: &ColumnSplines, predictors: &[usize]) -> Vec<Option<SplineSpec>> {
    if splines.iter().all(|(c, _)| !predictors.contains(c)) {
        return Vec::new();
    }
    predictors.iter().map(|p| splines.iter().find(|(c, _)| c == p).map(|(_, s)| s.clone())).collect()
}

/// The splines of a saved request (by position) keyed by column.
#[must_use]
pub fn by_column(splines: &[Option<SplineSpec>], predictors: &[usize]) -> ColumnSplines {
    predictors.iter().zip(splines).filter_map(|(&c, s)| s.clone().map(|s| (c, s))).collect()
}

fn kind_text(kind: Option<SplineKind>) -> String {
    match kind {
        None => "Straight line".to_owned(),
        Some(SplineKind::Natural) => "Natural cubic spline".to_owned(),
        Some(SplineKind::BSpline { degree }) => match degree {
            1 => "Linear B-spline (broken line)".to_owned(),
            2 => "Quadratic B-spline".to_owned(),
            3 => "Cubic B-spline".to_owned(),
            d => format!("B-spline of degree {d}"),
        },
    }
}

/// One row per continuous predictor in `eligible`: straight line or a
/// spline, with its number of knots (at quantiles) or knot positions.
pub fn spline_settings(ui: &mut egui::Ui, table: &DataTable, eligible: &[usize], splines: &mut ColumnSplines) {
    splines.retain(|(c, _)| eligible.contains(c));
    if eligible.is_empty() {
        return;
    }
    ui.label("Curved effects of continuous predictors (spline terms):").on_hover_text(
        "A spline lets a predictor's effect bend: the range is cut at knots and joined by smooth polynomial \
         pieces. Its effect test is the joint test of all its columns. A spline predictor cannot be in an \
         interaction or power.",
    );
    egui::Grid::new("spline_terms").num_columns(3).show(ui, |ui| {
        for &c in eligible {
            ui.label(kit::name(table, c));
            let pos = splines.iter().position(|(x, _)| *x == c);
            let current = pos.map(|i| splines[i].1.kind);
            let mut choice = current;
            egui::ComboBox::from_id_salt(("spline_kind", c)).selected_text(kind_text(current)).show_list(ui, |ui| {
                let mut kinds = vec![None, Some(SplineKind::Natural)];
                kinds.extend((1..=MAX_SPLINE_DEGREE.min(3)).map(|d| Some(SplineKind::BSpline { degree: d })));
                for k in kinds {
                    ui.selectable_value(&mut choice, k, kind_text(k));
                }
            });
            if choice != current {
                match (choice, pos) {
                    (None, Some(i)) => {
                        splines.remove(i);
                    }
                    (Some(kind), Some(i)) => splines[i].1.kind = kind,
                    (Some(kind), None) => splines.push((c, SplineSpec { kind, knots: SplineKnots::Count(3) })),
                    (None, None) => {}
                }
            }
            if let Some(i) = splines.iter().position(|(x, _)| *x == c) {
                knots_ui(ui, c, &mut splines[i].1.knots);
            } else {
                ui.label("");
            }
            ui.end_row();
        }
    });
}

fn knots_ui(ui: &mut egui::Ui, column: usize, knots: &mut SplineKnots) {
    ui.horizontal(|ui| {
        let at = matches!(knots, SplineKnots::At(_));
        if ui.radio(!at, "Knots:").on_hover_text("Interior knots at equally spaced quantiles of the values").clicked()
            && at
        {
            *knots = SplineKnots::Count(3);
        }
        let count = match knots {
            SplineKnots::Count(m) => *m,
            SplineKnots::At(v) => v.len(),
        };
        let mut m = count;
        let drag = ui.add_enabled(!at, egui::DragValue::new(&mut m).range(0..=MAX_SPLINE_KNOTS));
        if drag.changed() {
            *knots = SplineKnots::Count(m);
        }
        if ui.radio(at, "at").on_hover_text("Interior knots at values you type, separated by commas").clicked() && !at {
            *knots = SplineKnots::At(Vec::new());
        }
        if let SplineKnots::At(values) = knots {
            let id = ui.id().with(("spline_knots_text", column));
            let mut text = ui
                .data_mut(|d| d.get_temp::<String>(id))
                .unwrap_or_else(|| values.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(", "));
            let edit =
                ui.add(egui::TextEdit::singleline(&mut text).desired_width(kit::em(ui) * 9.0).hint_text("2, 5, 8"));
            if edit.changed() {
                *values = parse_knots(&text);
            }
            ui.data_mut(|d| d.insert_temp(id, text));
        }
    });
}

/// The numbers in `text`, separated by commas, semicolons or spaces, sorted.
#[must_use]
pub fn parse_knots(text: &str) -> Vec<f64> {
    let mut v: Vec<f64> = text
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter_map(|t| t.trim().replace('−', "-").parse::<f64>().ok())
        .filter(|x| x.is_finite())
        .collect();
    v.sort_by(f64::total_cmp);
    v.dedup();
    v
}

fn adjust_text(a: Option<PairAdjust>) -> &'static str {
    match a {
        None => "Off",
        Some(PairAdjust::None) => "Tests only (no pairwise comparisons)",
        Some(PairAdjust::Tukey) => "Tukey",
        Some(PairAdjust::Holm) => "Holm",
        Some(PairAdjust::Bonferroni) => "Bonferroni",
    }
}

/// A choice of off, tests only, or pairwise comparisons with an adjustment.
pub fn adjust_choice(ui: &mut egui::Ui, id: &str, label: &str, hover: &str, value: &mut Option<PairAdjust>) {
    ui.horizontal(|ui| {
        ui.label(label).on_hover_text(hover);
        egui::ComboBox::from_id_salt(id).selected_text(adjust_text(*value)).show_list(ui, |ui| {
            for a in [
                None,
                Some(PairAdjust::Tukey),
                Some(PairAdjust::Holm),
                Some(PairAdjust::Bonferroni),
                Some(PairAdjust::None),
            ] {
                ui.selectable_value(value, a, adjust_text(a));
            }
        });
    });
}

/// Radio buttons for the same choice, for report options menus.
pub fn adjust_radios(ui: &mut egui::Ui, value: &mut Option<PairAdjust>) {
    for a in
        [None, Some(PairAdjust::Tukey), Some(PairAdjust::Holm), Some(PairAdjust::Bonferroni), Some(PairAdjust::None)]
    {
        if ui.radio(*value == a, adjust_text(a)).clicked() {
            *value = a;
            ui.close_menu();
        }
    }
}

/// Slice and slope comparison choices for a model dialog.
pub fn comparison_settings(ui: &mut egui::Ui, slices: &mut Option<PairAdjust>, slopes: &mut Option<PairAdjust>) {
    adjust_choice(
        ui,
        "slice_comparisons",
        "Comparisons within slices:",
        "For each interaction of two categorical factors, test each factor at every level of the other and \
         compare its least-squares means within each slice",
        slices,
    );
    adjust_choice(
        ui,
        "slope_comparisons",
        "Slopes by group:",
        "For each effect of a categorical factor with a continuous predictor (separate slopes), each group's \
         slope with its interval, the test of equal slopes and pairwise slope differences",
        slopes,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knots_and_positions() {
        assert_eq!(parse_knots("5, 2; 8 −1 x 2"), vec![-1.0, 2.0, 5.0, 8.0]);
        let s: ColumnSplines = vec![(4, SplineSpec::natural(3))];
        assert_eq!(by_position(&s, &[1, 4]), vec![None, Some(SplineSpec::natural(3))]);
        assert!(by_position(&s, &[1, 2]).is_empty());
        assert_eq!(by_column(&[None, Some(SplineSpec::natural(3))], &[1, 4]), s);
    }
}
