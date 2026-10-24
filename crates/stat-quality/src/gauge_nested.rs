//! Nested gauge R&R for destructive tests: each part is measured by one
//! operator only.
//!
//! # Model
//! When measuring destroys (or changes) the part, no part can go to more than
//! one operator. The study then gives each of `o` operators their own `p`
//! parts, and each part is measured `r ≥ 2` times — in practice `r` pieces cut
//! from one homogeneous batch stand in for the repeat measurements. Parts are
//! *nested* within operators:
//!
//! ```text
//! y_ijk = μ + Oᵢ + P(O)ⱼ₍ᵢ₎ + ε_ijk,
//! Oᵢ ~ N(0, σ²_O), P(O)ⱼ₍ᵢ₎ ~ N(0, σ²_P), ε ~ N(0, σ²),
//! ```
//!
//! with `i = 1…o`, `j = 1…p` and `k = 1…r` (Burdick, Borror & Montgomery
//! 2005, *Design and Analysis of Gauge R&R Studies*, ch. 9 on nested
//! designs; Montgomery, *Design and Analysis of Experiments*, §14.1, for the
//! two-stage nested ANOVA). With `ȳᵢ..` the operator means, `ȳᵢⱼ.` the part
//! means and `ȳ...` the grand mean, the sums of squares are
//!
//! ```text
//! SS_O    = p r Σᵢ (ȳᵢ.. − ȳ...)²               df o − 1
//! SS_P(O) = r Σᵢ Σⱼ (ȳᵢⱼ. − ȳᵢ..)²              df o (p − 1)
//! SS_E    = Σᵢ Σⱼ Σₖ (y_ijk − ȳᵢⱼ.)²            df o p (r − 1)
//! ```
//!
//! and the expected mean squares `E[MS_O] = σ² + rσ²_P + prσ²_O`,
//! `E[MS_P(O)] = σ² + rσ²_P`, `E[MS_E] = σ²` give the ANOVA estimators
//! (truncated at zero)
//!
//! ```text
//! σ̂²   = MS_E                      (repeatability)
//! σ̂²_P = (MS_P(O) − MS_E) / r      (part-to-part)
//! σ̂²_O = (MS_O − MS_P(O)) / (p r)  (reproducibility)
//! ```
//!
//! Operators are tested against parts within operators and parts within
//! operators against repeatability. In a nested study the operator-by-part
//! interaction cannot be separated from part-to-part variation, so there is
//! no interaction component, and the repeatability estimate also absorbs the
//! within-batch variation of the pieces measured as "repeats". The summary
//! measures (%Contribution, %Study variation, %Tolerance, number of distinct
//! categories) are those of the crossed study in [`crate::gauge`].
//!
//! Unbalanced nested data (operators with different numbers of parts, or
//! parts with different numbers of measurements) are handled by the REML fit
//! of [`crate::gauge_expanded`].

use stat_core::distributions::{ContinuousDistribution, FisherSnedecor};
use stat_core::summation::sum_iter;
use stat_core::{Result, StatError};

use crate::gauge::{
    GaugeComponents, GaugeObservation, GaugeOptions, GaugeSource, GaugeStudy, distinct_categories, invalid, row,
    summarize_component, validate_options,
};

/// Analyse a balanced nested (destructive) gauge study by the ANOVA method.
///
/// Each observation's `part` index identifies one physical part (or batch)
/// and must belong to a single operator; part indices run over all parts of
/// all operators, `0..o·p`. The result uses the crossed study's types:
/// [`GaugeStudy::parts`] counts all parts, the ANOVA rows are operator, part
/// within operator ([`GaugeSource::PartWithinOperator`]), repeatability and
/// total, and the interaction component is zero. The pooling option of
/// [`GaugeOptions`] does not apply and is ignored.
///
/// # Errors
/// * [`StatError::InvalidArgument`] for fewer than two operators, an operator
///   with fewer than two parts, a part measured by more than one operator,
///   an unbalanced design (operators with different numbers of parts, or
///   parts with different numbers of measurements), fewer than two
///   measurements per part, or invalid options.
/// * [`StatError::NonFiniteInput`] if a value is NaN or ±∞.
pub fn gauge_rr_nested(observations: &[GaugeObservation], options: &GaugeOptions) -> Result<GaugeStudy> {
    validate_options(options)?;
    if observations.iter().any(|o| !o.value.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "gauge study measurements" });
    }
    let n_parts = observations.iter().map(|o| o.part + 1).max().unwrap_or(0);
    let operators = observations.iter().map(|o| o.operator + 1).max().unwrap_or(0);
    if operators < 2 {
        return Err(invalid("a nested gauge study needs at least two operators"));
    }
    // Owner of each part, and its measurements.
    let mut owner: Vec<Option<usize>> = vec![None; n_parts];
    let mut values: Vec<Vec<f64>> = vec![Vec::new(); n_parts];
    for o in observations {
        match owner[o.part] {
            Some(op) if op != o.operator => {
                return Err(invalid(format!(
                    "part {} is measured by operators {} and {}; in a nested study each part belongs to one operator \
                     (numbered from 1 in order of appearance)",
                    o.part + 1,
                    op + 1,
                    o.operator + 1
                )));
            }
            _ => owner[o.part] = Some(o.operator),
        }
        values[o.part].push(o.value);
    }
    let mut parts_of: Vec<Vec<usize>> = vec![Vec::new(); operators];
    for (part, op) in owner.iter().enumerate() {
        if let Some(op) = op {
            parts_of[*op].push(part);
        }
    }
    let per_operator = parts_of[0].len();
    if let Some((j, ps)) = parts_of.iter().enumerate().find(|(_, ps)| ps.len() != per_operator) {
        return Err(invalid(format!(
            "the study must be balanced: operator {} has {} parts, operator 1 has {per_operator}",
            j + 1,
            ps.len()
        )));
    }
    if per_operator < 2 {
        return Err(invalid("each operator must measure at least two parts"));
    }
    let used: Vec<usize> = parts_of.iter().flatten().copied().collect();
    let replicates = values[used[0]].len();
    if let Some(&i) = used.iter().find(|&&i| values[i].len() != replicates) {
        return Err(invalid(format!(
            "the study must be balanced: part {} has {} measurements, expected {replicates}",
            i + 1,
            values[i].len()
        )));
    }
    if replicates < 2 {
        return Err(invalid("each part must be measured at least twice"));
    }

    let (o, p, r) = (operators as f64, per_operator as f64, replicates as f64);
    let part_mean: Vec<f64> = values.iter().map(|v| sum_iter(v.iter().copied()) / v.len().max(1) as f64).collect();
    let oper_mean: Vec<f64> = parts_of.iter().map(|ps| sum_iter(ps.iter().map(|&i| part_mean[i])) / p).collect();
    let grand = sum_iter(oper_mean.iter().copied()) / o;

    let ss_oper = p * r * sum_iter(oper_mean.iter().map(|m| (m - grand).powi(2)));
    let pm = &part_mean;
    let ss_part = r * sum_iter(
        parts_of.iter().zip(&oper_mean).flat_map(|(ps, &om)| ps.iter().map(move |&i| (pm[i] - om).powi(2))),
    );
    let ss_error = sum_iter(used.iter().flat_map(|&i| values[i].iter().map(move |&y| (y - pm[i]).powi(2))));
    let ss_total = sum_iter(observations.iter().map(|ob| (ob.value - grand).powi(2)));

    let df_oper = operators - 1;
    let df_part = operators * (per_operator - 1);
    let df_error = operators * per_operator * (replicates - 1);
    let ms = |ss: f64, df: usize| ss / df as f64;
    let (ms_oper, ms_part, ms_error) = (ms(ss_oper, df_oper), ms(ss_part, df_part), ms(ss_error, df_error));
    let f_test = |num: f64, df_num: usize, den: f64, df_den: usize| -> Result<(Option<f64>, Option<f64>)> {
        if den <= 0.0 {
            return Ok((None, None));
        }
        let f = num / den;
        Ok((Some(f), Some(FisherSnedecor::new(df_num as f64, df_den as f64)?.sf(f)?)))
    };
    let (f_o, p_o) = f_test(ms_oper, df_oper, ms_part, df_part)?;
    let (f_p, p_p) = f_test(ms_part, df_part, ms_error, df_error)?;
    let anova = vec![
        row(GaugeSource::Operator, df_oper, ss_oper, Some(ms_oper), f_o, p_o),
        row(GaugeSource::PartWithinOperator, df_part, ss_part, Some(ms_part), f_p, p_p),
        row(GaugeSource::Repeatability, df_error, ss_error, Some(ms_error), None, None),
        row(GaugeSource::Total, operators * per_operator * replicates - 1, ss_total, None, None, None),
    ];

    let var_error = ms_error;
    let var_part = ((ms_part - ms_error) / r).max(0.0);
    let var_oper = ((ms_oper - ms_part) / (p * r)).max(0.0);
    let var_grr = var_error + var_oper;
    let var_total = var_grr + var_part;
    let c = |v: f64| summarize_component(v, var_total, options);
    let components = GaugeComponents {
        gauge_rr: c(var_grr),
        repeatability: c(var_error),
        reproducibility: c(var_oper),
        operator: c(var_oper),
        interaction: c(0.0),
        part: c(var_part),
        total: c(var_total),
    };
    Ok(GaugeStudy {
        parts: operators * per_operator,
        operators,
        replicates,
        anova,
        interaction_pooled: false,
        components,
        distinct_categories: distinct_categories(var_part, var_grr),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(table: &[(usize, usize, f64)]) -> Vec<GaugeObservation> {
        table.iter().map(|&(part, operator, value)| GaugeObservation { part, operator, value }).collect()
    }

    /// Two operators × two parts each × two measurements, worked by hand.
    ///
    /// Operator 1: part 1 {10, 12} mean 11, part 2 {14, 16} mean 15; operator
    /// mean 13. Operator 2: part 3 {20, 20} mean 20, part 4 {17, 19} mean 18;
    /// operator mean 19. Grand mean 16.
    /// SS_O = 2·2·(9 + 9) = 72; SS_P(O) = 2·(4 + 4 + 1 + 1) = 20;
    /// SS_E = 2 + 2 + 0 + 2 = 6 on 4 df; SS_T = 98 (36+16+4+0+16+16+1+9).
    /// MS_O = 72, MS_P(O) = 10, MS_E = 1.5.
    /// σ̂² = 1.5, σ̂²_P = (10 − 1.5)/2 = 4.25, σ̂²_O = (72 − 10)/4 = 15.5.
    #[test]
    fn hand_worked_example() {
        let data = obs(&[
            (0, 0, 10.0),
            (0, 0, 12.0),
            (1, 0, 14.0),
            (1, 0, 16.0),
            (2, 1, 20.0),
            (2, 1, 20.0),
            (3, 1, 17.0),
            (3, 1, 19.0),
        ]);
        let s = gauge_rr_nested(&data, &GaugeOptions { tolerance: Some(40.0), ..Default::default() }).unwrap();
        let ss: Vec<f64> = s.anova.iter().map(|r| r.sum_of_squares).collect();
        for (a, b) in ss.iter().zip([72.0, 20.0, 6.0, 98.0]) {
            assert!((a - b).abs() < 1e-12, "{ss:?}");
        }
        let df: Vec<usize> = s.anova.iter().map(|r| r.df).collect();
        assert_eq!(df, vec![1, 2, 4, 7]);
        assert_eq!(s.anova[1].source, GaugeSource::PartWithinOperator);
        assert!((s.anova[0].f_statistic.unwrap() - 7.2).abs() < 1e-12);
        assert!((s.anova[1].f_statistic.unwrap() - 10.0 / 1.5).abs() < 1e-12);
        let c = s.components;
        assert!((c.repeatability.variance - 1.5).abs() < 1e-12);
        assert!((c.part.variance - 4.25).abs() < 1e-12);
        assert!((c.operator.variance - 15.5).abs() < 1e-12);
        assert!((c.reproducibility.variance - 15.5).abs() < 1e-12);
        assert_eq!(c.interaction.variance, 0.0);
        assert!((c.gauge_rr.variance - 17.0).abs() < 1e-12);
        assert!((c.total.variance - 21.25).abs() < 1e-12);
        assert!((c.gauge_rr.percent_tolerance.unwrap() - 100.0 * 6.0 * 17.0_f64.sqrt() / 40.0).abs() < 1e-10);
        // ndc = ⌊√2 · √4.25 / √17⌋ = ⌊√0.5⌋ = 0.
        assert_eq!(s.distinct_categories, Some(0));
        assert_eq!((s.parts, s.operators, s.replicates), (4, 2, 2));
    }

    #[test]
    fn validation() {
        let crossed = obs(&[(0, 0, 1.0), (0, 0, 1.1), (0, 1, 1.0), (0, 1, 1.2), (1, 0, 2.0), (1, 0, 2.1)]);
        let e = gauge_rr_nested(&crossed, &GaugeOptions::default()).unwrap_err().to_string();
        assert!(e.contains("one operator"), "{e}");
        let uneven = obs(&[
            (0, 0, 1.0),
            (0, 0, 1.1),
            (1, 0, 2.0),
            (1, 0, 2.1),
            (2, 1, 3.0),
            (2, 1, 3.2),
            (3, 1, 3.0),
            (3, 1, 3.1),
            (4, 1, 3.0),
            (4, 1, 3.3),
        ]);
        let e = gauge_rr_nested(&uneven, &GaugeOptions::default()).unwrap_err().to_string();
        assert!(e.contains("operator 2 has 3 parts"), "{e}");
        let once = obs(&[(0, 0, 1.0), (1, 0, 2.0), (2, 1, 3.0), (3, 1, 4.0)]);
        assert!(gauge_rr_nested(&once, &GaugeOptions::default()).is_err());
        let one_op = obs(&[(0, 0, 1.0), (0, 0, 1.1), (1, 0, 2.0), (1, 0, 2.1)]);
        assert!(gauge_rr_nested(&one_op, &GaugeOptions::default()).is_err());
        let nan = obs(&[(0, 0, f64::NAN), (0, 0, 1.1), (1, 1, 2.0), (1, 1, 2.1)]);
        assert!(matches!(gauge_rr_nested(&nan, &GaugeOptions::default()), Err(StatError::NonFiniteInput { .. })));
    }
}
