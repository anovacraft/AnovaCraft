//! A fitted model's prediction equation written as a formula column
//! (see [`crate::formula`]), so the predictions follow later edits to the
//! data and can be computed for new rows.
//!
//! The linear predictor `η = b₀ + Σ bⱼ xⱼ` is written effect by effect, the
//! way [`ModelDesign`] codes the design columns:
//!
//! * a continuous main effect is `b * x`;
//! * a continuous factor inside an interaction or power is centred at its
//!   mean over the rows the model was fitted to: `b * (x - x̄) ** 2`;
//! * a spline term is one `natural_spline(x, [knots], k)` or
//!   `bspline(x, [knots], degree, k)` per basis column (boundary knots first
//!   and last; see [`crate::formula`]);
//! * an effect with categorical factors becomes a `switch` on the level (or,
//!   for several factors, on the levels joined by a tab), giving each level
//!   combination its effective coefficient `Σ bⱼ cⱼ` under the design's
//!   coding (reference or sum-to-zero), times the effect's continuous part.
//!   A level the model was not fitted to falls to `0.0 / 0.0` (NaN), which
//!   makes that row's prediction missing without an evaluation error.
//!
//! Effects whose coefficients are all exactly zero (e.g. predictors a lasso
//! dropped) are left out. The mean is the inverse link of `η`: `logistic(η)`
//! for the logit link, `exp(η)` for the log link. Coefficients are written
//! with every digit needed to read back the same `f64`, so the formula
//! reproduces the model's predictions to rounding.

use stat_core::{Result, StatError};
use stat_data::{ColumnKind, DataTable};
use stat_linear::{CategoricalCoding, ModelDesign, TermKind};

use crate::formula::column_reference;
use crate::script_text::{number, string};

/// Most level combinations one effect may list.
const MAX_COMBINATIONS: usize = 5_000;

/// How the model maps its linear predictor `η` to the predicted mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// The prediction is `η`.
    Identity,
    /// The prediction is `1 / (1 + e^(−η))` (a probability).
    Logit,
    /// The prediction is `e^η` (a mean count).
    Log,
    /// The prediction is `Φ(η)`, the standard normal cdf (a probability).
    Probit,
    /// The prediction is `1 − exp(−e^η)` (a probability).
    ComplementaryLogLog,
    /// The prediction is `1/η`.
    Inverse,
}

/// A predictor column as the formula reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactorColumn {
    /// Column name.
    pub name: String,
    /// Storage type, which decides how a category's label is read.
    pub kind: ColumnKind,
    /// The column's value labels (stored value → label): the model names
    /// levels by label, while the formula reads stored values.
    pub value_labels: std::collections::BTreeMap<String, String>,
}

impl FactorColumn {
    /// The stored value's text for level `name` (its code when `name` is a
    /// value label).
    fn code<'a>(&'a self, name: &'a str) -> &'a str {
        self.value_labels.iter().find(|(_, l)| l.as_str() == name).map_or(name, |(c, _)| c.as_str())
    }
}

/// The predictor columns `indices` of `table`, in order (skipping unknown
/// indices, which a fit would already have refused).
#[must_use]
pub fn factor_columns(table: &DataTable, indices: &[usize]) -> Vec<FactorColumn> {
    indices
        .iter()
        .filter_map(|&i| table.column(i))
        .map(|c| FactorColumn { name: c.name.clone(), kind: c.kind(), value_labels: c.value_labels.clone() })
        .collect()
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// The formula predicting the response of a model with design `design`
/// (predictors read from `factors`, one per design term), intercept
/// `intercept`, one coefficient per design column in `slopes`, and link
/// `link`.
///
/// # Errors
/// [`StatError::InvalidArgument`] if the sizes disagree, a categorical
/// predictor is a date-time column (its labels have no formula equivalent),
/// an effect has more than 5,000 level combinations, or a coefficient is
/// not finite.
pub fn prediction_formula(
    design: &ModelDesign,
    factors: &[FactorColumn],
    intercept: f64,
    slopes: &[f64],
    link: Link,
) -> Result<String> {
    let terms = design.terms();
    if factors.len() != terms.len() || slopes.len() != design.column_names().len() {
        return Err(invalid("the model and its columns do not match"));
    }
    if !intercept.is_finite() || slopes.iter().any(|b| !b.is_finite()) {
        return Err(invalid("the model has a coefficient that is not a finite number"));
    }
    let centers = design.centers();
    let reference = |i: usize| column_reference(&factors[i].name);
    let mut lines = vec![number(intercept)];
    for effect in design.effects() {
        let b = &slopes[effect.columns.clone()];
        if b.iter().all(|&v| v == 0.0) {
            continue;
        }
        // A spline term: one basis function per coefficient.
        if let ([(i, 1)], Some(basis)) =
            (effect.factors.as_slice(), effect.factors.first().and_then(|f| design.spline(f.0)))
        {
            let (a, z) = basis.boundary();
            let knots: Vec<String> =
                std::iter::once(a).chain(basis.interior().iter().copied()).chain([z]).map(number).collect();
            let knots = format!("[{}]", knots.join(", "));
            for (k, &bk) in b.iter().enumerate() {
                let (sign, size) = signed(bk);
                let call = match basis.kind() {
                    stat_linear::SplineKind::Natural => {
                        format!("natural_spline({}, {knots}, {})", reference(*i), k + 1)
                    }
                    stat_linear::SplineKind::BSpline { degree } => {
                        format!("bspline({}, {knots}, {degree}, {})", reference(*i), k + 1)
                    }
                };
                lines.push(format!("{sign} {} * {call}", number(size)));
            }
            continue;
        }
        // The continuous part (a product of powers) and the categorical factors.
        let mut continuous = Vec::new();
        let mut categorical = Vec::new();
        for &(i, p) in &effect.factors {
            match &terms[i].kind {
                TermKind::Continuous => {
                    let x = reference(i);
                    let c = centers[i];
                    let base = if effect.is_main() || c == 0.0 {
                        x
                    } else if c > 0.0 {
                        format!("({x} - {})", number(c))
                    } else {
                        format!("({x} + {})", number(-c))
                    };
                    continuous.push(if p == 1 { base } else { format!("{base} ** {p}") });
                }
                TermKind::Categorical { levels } => {
                    if matches!(factors[i].kind, ColumnKind::DateTime(_)) {
                        return Err(invalid(format!(
                            "the date-time column {:?} is used as a category; a prediction formula cannot match its \
                             labels",
                            factors[i].name
                        )));
                    }
                    categorical.push((i, levels.as_slice()));
                }
            }
        }
        let continuous = continuous.join(" * ");
        if categorical.is_empty() {
            let (sign, size) = signed(b[0]);
            lines.push(format!("{sign} {} * {continuous}", number(size)));
            continue;
        }
        let combinations: usize = categorical.iter().map(|(_, l)| l.len()).product();
        if combinations > MAX_COMBINATIONS {
            return Err(invalid(format!(
                "the effect {} has {combinations} level combinations, more than a formula can list ({MAX_COMBINATIONS})",
                effect.name
            )));
        }
        let parts: Vec<String> = categorical.iter().map(|&(i, _)| format!("text({})", reference(i))).collect();
        let key = if parts.len() == 1 { parts[0].clone() } else { format!("({})", parts.join(" + \"\\t\" + ")) };
        let mut cases = Vec::with_capacity(combinations + 1);
        let mut at = vec![0usize; categorical.len()];
        for _ in 0..combinations {
            let label: Vec<&str> =
                categorical.iter().zip(&at).map(|((i, l), &k)| factors[*i].code(l[k].as_str())).collect();
            let coefficient = combination_coefficient(b, &categorical, &at, design.coding());
            cases.push(format!("{} => {}", string(&label.join("\t")), number(coefficient)));
            // Next combination, the last factor varying fastest.
            for k in (0..at.len()).rev() {
                at[k] += 1;
                if at[k] < categorical[k].1.len() {
                    break;
                }
                at[k] = 0;
            }
        }
        // NaN, not `missing()`: arithmetic on NaN gives a missing prediction,
        // while `()` would make the row fail with a type error.
        cases.push("_ => 0.0 / 0.0".to_owned());
        // Short lists on one line, long ones a case per line.
        let switch = if cases.len() <= 4 {
            format!("(switch {key} {{ {} }})", cases.join(", "))
        } else {
            let pad = if link == Link::Identity { "    " } else { "        " };
            let close = &pad[4..];
            format!("(switch {key} {{\n{pad}{}\n{close}}})", cases.join(&format!(",\n{pad}")))
        };
        lines.push(if continuous.is_empty() { format!("+ {switch}") } else { format!("+ {switch} * {continuous}") });
    }
    let indent = |l: &String| format!("    {l}");
    Ok(match link {
        Link::Identity => lines.join("\n"),
        Link::Logit => format!("logistic(\n{}\n)", lines.iter().map(indent).collect::<Vec<_>>().join("\n")),
        Link::Log => format!("exp(\n{}\n)", lines.iter().map(indent).collect::<Vec<_>>().join("\n")),
        Link::Probit => format!("normal_cdf(\n{}\n)", lines.iter().map(indent).collect::<Vec<_>>().join("\n")),
        Link::ComplementaryLogLog => {
            format!("1.0 - exp(-exp(\n{}\n))", lines.iter().map(indent).collect::<Vec<_>>().join("\n"))
        }
        Link::Inverse => format!("1.0 / (\n{}\n)", lines.iter().map(indent).collect::<Vec<_>>().join("\n")),
    })
}

/// `("+", b)` or `("-", |b|)`.
fn signed(b: f64) -> (&'static str, f64) {
    if b < 0.0 { ("-", -b) } else { ("+", b) }
}

/// Coding vector of level `k` of a factor with `levels` levels.
fn code(levels: usize, k: usize, coding: CategoricalCoding) -> Vec<f64> {
    let mut c = vec![0.0; levels - 1];
    if k > 0 {
        c[k - 1] = 1.0;
    } else if coding == CategoricalCoding::SumToZero {
        c.iter_mut().for_each(|v| *v = -1.0);
    }
    c
}

/// `Σ bⱼ Π cₖ[jₖ]` over the effect's columns: the coefficient of one level
/// combination (`at[k]` the level of categorical factor `k`). Columns are the
/// product of the factors' codes, later factors varying fastest.
fn combination_coefficient(
    b: &[f64],
    categorical: &[(usize, &[String])],
    at: &[usize],
    coding: CategoricalCoding,
) -> f64 {
    let mut weights = vec![1.0];
    for ((_, levels), &k) in categorical.iter().zip(at) {
        let c = code(levels.len(), k, coding);
        weights = weights.iter().flat_map(|w| c.iter().map(move |v| w * v)).collect();
    }
    let total: f64 = weights.iter().zip(b).map(|(w, v)| w * v).sum();
    total + 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formula::{FormulaValue, evaluate};
    use crate::project::SavedReport;
    use crate::sample::catalog;
    use crate::{GlmAnalysis, LinearModelAnalysis, RegularizedAnalysis};
    use stat_data::RowStateManager;

    /// The formula's value on every row of `table`.
    fn evaluated(table: &DataTable, formula: &str) -> Vec<f64> {
        let eval = evaluate(table, None, formula, 0..table.n_rows()).unwrap_or_else(|e| panic!("{e}\n{formula}"));
        eval.values
            .iter()
            .map(|v| match v {
                FormulaValue::Number(x) => *x,
                _ => f64::NAN,
            })
            .collect()
    }

    fn assert_matches(table: &DataTable, formula: &str, predicted: &[f64]) {
        let got = evaluated(table, formula);
        let mut compared = 0;
        for (r, (g, p)) in got.iter().zip(predicted).enumerate() {
            if p.is_finite() {
                assert!((g - p).abs() <= 1e-9 * p.abs().max(1.0), "row {r}: formula {g}, model {p}\n{formula}");
                compared += 1;
            }
        }
        assert!(compared > 0, "no rows compared");
    }

    #[test]
    fn linear_models_of_the_samples_are_reproduced() {
        let mut checked = 0;
        for info in catalog() {
            let s = (info.build)();
            let rows = RowStateManager::new(s.table.n_rows());
            for saved in &s.reports {
                let SavedReport::Linear { spec, .. } = saved else { continue };
                let a = LinearModelAnalysis::run(&s.table, &rows, spec).unwrap();
                let Ok(formula) = a.prediction_formula() else { continue };
                let predicted = &a.saveable_columns()[0].values;
                assert_matches(&s.table, &formula, predicted);
                checked += 1;
            }
        }
        assert!(checked >= 5, "only {checked} linear models checked");
    }

    /// x (continuous, with spaces in its name), g (text categories), h
    /// (numbers used as categories) and a response y.
    fn mixed_table() -> DataTable {
        let x: Vec<f64> = (0..24).map(|i| f64::from(i) * 0.5 - 2.0).collect();
        let g: Vec<String> = (0..24).map(|i| ["lo", "mid", "hi"][i % 3].to_owned()).collect();
        let h: Vec<f64> = (0..24).map(|i| f64::from((i / 3) % 2) + 1.0).collect();
        let y: Vec<f64> = (0..24)
            .map(|i| {
                let k = i % 3;
                1.0 + 0.7 * x[i]
                    + 0.3 * x[i] * x[i]
                    + [0.0, 2.0, -1.0][k] * (1.0 + 0.4 * x[i])
                    + h[i]
                    + 0.05 * ((i * 7) % 5) as f64
            })
            .collect();
        let mut hc = stat_data::Column::numeric("h", h);
        hc.level = stat_data::MeasurementLevel::Nominal;
        DataTable::new(
            "t",
            vec![
                stat_data::Column::numeric("x value", x),
                stat_data::Column::text("g", g),
                hc,
                stat_data::Column::numeric("y", y),
            ],
        )
        .unwrap()
    }

    #[test]
    fn interactions_powers_and_categories() {
        let t = mixed_table();
        let spec = crate::LinearModelSpec {
            response: 3,
            predictors: vec![0, 1, 2],
            effects: vec![
                vec![(0, 1)],
                vec![(1, 1)],
                vec![(2, 1)],
                vec![(0, 1), (1, 1)],
                vec![(0, 2)],
                vec![(1, 1), (2, 1)],
            ],
            splines: Vec::new(),
        };
        let rows = RowStateManager::new(t.n_rows());
        let a = LinearModelAnalysis::run(&t, &rows, &spec).unwrap();
        let formula = a.prediction_formula().unwrap();
        assert!(formula.contains("col(\"x value\")") && formula.contains("text(h)"), "{formula}");
        assert_matches(&t, &formula, &a.saveable_columns()[0].values);
    }

    #[test]
    fn unknown_levels_are_missing_and_dates_are_refused() {
        let mut t = mixed_table();
        t.set_value(0, 1, stat_data::CellValue::Text("new".into())).unwrap();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([0], true);
        let spec =
            crate::LinearModelSpec { response: 3, predictors: vec![0, 1], effects: Vec::new(), splines: Vec::new() };
        let a = LinearModelAnalysis::run(&t, &rows, &spec).unwrap();
        let formula = a.prediction_formula().unwrap();
        let values = evaluated(&t, &formula);
        assert!(values[0].is_nan() && values[1].is_finite());
        // F-22: the unseen level computes (to a missing value) without an
        // error, so the formula editor reports no failed rows.
        let eval = evaluate(&t, None, &formula, 0..t.n_rows()).unwrap();
        assert!(eval.failures.is_empty(), "{:?}", eval.failures);

        let mut design_table = mixed_table();
        let when = stat_data::Column::date_time(
            "when",
            (0..24).map(|i| f64::from(i % 2) * 86_400.0).collect(),
            stat_data::TemporalDisplay::new(stat_data::TemporalFormat::Date),
        );
        design_table
            .insert_column(3, stat_data::Column { level: stat_data::MeasurementLevel::Nominal, ..when })
            .unwrap();
        let spec =
            crate::LinearModelSpec { response: 4, predictors: vec![0, 3], effects: Vec::new(), splines: Vec::new() };
        let a = LinearModelAnalysis::run(&design_table, &RowStateManager::new(24), &spec).unwrap();
        assert!(a.prediction_formula().unwrap_err().to_string().contains("date-time"));
    }

    #[test]
    fn generalized_and_penalized_models_of_the_samples() {
        let mut checked = 0;
        for info in catalog() {
            let s = (info.build)();
            let rows = RowStateManager::new(s.table.n_rows());
            for saved in &s.reports {
                let columns = match saved {
                    SavedReport::Glm { request, .. } => {
                        GlmAnalysis::run(&s.table, &rows, request).unwrap().saveable_columns()
                    }
                    SavedReport::Regularized { request, .. } => {
                        RegularizedAnalysis::run(&s.table, &rows, request).unwrap().saveable_columns()
                    }
                    _ => continue,
                };
                let [predicted, formula] = columns.as_slice() else { panic!("{columns:?}") };
                assert_matches(&s.table, formula.formula.as_deref().unwrap(), &predicted.values);
                checked += 1;
            }
        }
        assert!(checked >= 3, "only {checked} models checked");
    }

    #[test]
    fn reports_show_their_formula() {
        let s = catalog()
            .into_iter()
            .map(|i| (i.build)())
            .find(|s| s.reports.iter().any(|r| matches!(r, SavedReport::Glm { .. })));
        let s = s.unwrap();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::Glm { request, .. } = s.reports.iter().find(|r| matches!(r, SavedReport::Glm { .. })).unwrap()
        else {
            unreachable!()
        };
        let a = GlmAnalysis::run(&s.table, &rows, request).unwrap();
        let section = a.sections().into_iter().find(|s| s.kind == crate::SectionKind::PredictionFormula).unwrap();
        assert_eq!(section.content, crate::SectionContent::Formula(a.prediction_formula().unwrap()));

        let t = mixed_table();
        let spec =
            crate::LinearModelSpec { response: 3, predictors: vec![0, 2], effects: Vec::new(), splines: Vec::new() };
        let a = LinearModelAnalysis::run(&t, &RowStateManager::new(24), &spec).unwrap();
        let sections = a.sections(&crate::report::ReportOptions::default());
        let section = sections.iter().find(|s| s.kind == crate::SectionKind::PredictionFormula).unwrap();
        assert!(matches!(&section.content, crate::SectionContent::Formula(f) if f.contains("text(h)")));
    }
}
