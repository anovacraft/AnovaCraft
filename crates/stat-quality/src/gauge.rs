//! Measurement systems analysis: gauge repeatability and reproducibility by the
//! ANOVA method.
//!
//! # Model
//! A balanced crossed study: `p` parts, `o` operators, each part measured `r ≥ 2`
//! times by each operator. The two-way random-effects model with interaction is
//!
//! ```text
//! y_ijk = μ + Pᵢ + Oⱼ + (PO)ᵢⱼ + ε_ijk,
//! Pᵢ ~ N(0, σ²_P), Oⱼ ~ N(0, σ²_O), (PO)ᵢⱼ ~ N(0, σ²_PO), ε ~ N(0, σ²).
//! ```
//!
//! Sums of squares are the standard balanced two-way decompositions, and the
//! variance components are the method-of-moments (ANOVA) estimators obtained by
//! equating mean squares to their expectations, truncated at zero (Montgomery &
//! Runger 1993, "Gauge capability and designed experiments. Part II",
//! *Quality Engineering* 6(2); Burdick, Borror & Montgomery 2003, "A review of
//! methods for measurement systems capability analysis", *J. Quality Technology*
//! 35(4)):
//!
//! ```text
//! σ̂²         = MS_E                          (repeatability)
//! σ̂²_PO      = (MS_PO − MS_E) / r
//! σ̂²_O       = (MS_O − MS_PO) / (p r)
//! σ̂²_P       = (MS_P − MS_PO) / (o r)
//! reproducibility = σ̂²_O + σ̂²_PO,   gauge R&R = σ̂² + reproducibility
//! ```
//!
//! F tests use the random-effects denominators: parts and operators against
//! `MS_PO`, the interaction against `MS_E`.
//!
//! **Pooling.** When [`GaugeOptions::pool_interaction_above`] is set and the
//! interaction's p-value exceeds it, the interaction is pooled into error
//! (`MS_E' = (SS_PO + SS_E)/(df_PO + df_E)`), parts and operators are tested
//! against `MS_E'`, and `σ̂²_PO = 0`. By default the full model is always used.
//!
//! **Summary measures.** `%Contribution = 100 σ̂²_c / σ̂²_total`;
//! `%Study variation = 100 σ̂_c / σ̂_total`; study variation `= k σ̂_c` with
//! `k = 6` by default; `%Tolerance = 100 k σ̂_c / (USL − LSL)`; and the number of
//! distinct categories `⌊√2 · σ̂_P / σ̂_gauge⌋` (the number of non-overlapping
//! 97% confidence intervals that span the part variation; see Wheeler & Lyday,
//! *Evaluating the Measurement Process*, 1989).

use stat_core::distributions::{ContinuousDistribution, FisherSnedecor};
use stat_core::summation::sum_iter;
use stat_core::{Result, StatError};

/// One measurement in a gauge study.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaugeObservation {
    /// Part index, `0..parts`.
    pub part: usize,
    /// Operator (appraiser) index, `0..operators`.
    pub operator: usize,
    /// Measured value.
    pub value: f64,
}

/// Options for [`gauge_rr_anova`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaugeOptions {
    /// Multiplier `k` converting a standard deviation into study variation.
    /// Default 6 (±3σ covers 99.73% of a normal distribution).
    pub study_multiplier: f64,
    /// Specification width `USL − LSL`, for %Tolerance. Default `None`.
    pub tolerance: Option<f64>,
    /// Pool the part×operator interaction into error when its p-value exceeds this
    /// level. Default `None` (never pool).
    pub pool_interaction_above: Option<f64>,
}

impl Default for GaugeOptions {
    fn default() -> Self {
        Self { study_multiplier: 6.0, tolerance: None, pool_interaction_above: None }
    }
}

/// Source of variation in the study ANOVA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GaugeSource {
    /// Parts.
    Part,
    /// Operators.
    Operator,
    /// Part × operator interaction.
    Interaction,
    /// Parts within operators (nested study, see [`crate::gauge_nested`]).
    PartWithinOperator,
    /// Repeatability (pure error).
    Repeatability,
    /// Total.
    Total,
}

/// One row of the study ANOVA.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaugeAnovaRow {
    /// Source of variation.
    pub source: GaugeSource,
    /// Degrees of freedom.
    pub df: usize,
    /// Sum of squares.
    pub sum_of_squares: f64,
    /// Mean square (`None` for the total row).
    pub mean_square: Option<f64>,
    /// F statistic, for parts, operators and interaction.
    pub f_statistic: Option<f64>,
    /// Upper-tail p-value of `f_statistic`.
    pub p_value: Option<f64>,
}

/// A variance component with its summary measures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VarianceComponent {
    /// Estimated variance (truncated at zero).
    pub variance: f64,
    /// `100 · variance / total variance`.
    pub percent_contribution: f64,
    /// Standard deviation.
    pub std_dev: f64,
    /// `k · std_dev`.
    pub study_variation: f64,
    /// `100 · std_dev / total std_dev`.
    pub percent_study_variation: f64,
    /// `100 · k · std_dev / tolerance`, if a tolerance was given.
    pub percent_tolerance: Option<f64>,
}

/// All variance components of the study.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaugeComponents {
    /// Total gauge R&R (repeatability + reproducibility).
    pub gauge_rr: VarianceComponent,
    /// Repeatability (equipment variation).
    pub repeatability: VarianceComponent,
    /// Reproducibility (operator + interaction).
    pub reproducibility: VarianceComponent,
    /// Operator component of reproducibility.
    pub operator: VarianceComponent,
    /// Part × operator component of reproducibility.
    pub interaction: VarianceComponent,
    /// Part-to-part variation.
    pub part: VarianceComponent,
    /// Total variation.
    pub total: VarianceComponent,
}

/// Result of an ANOVA gauge R&R study (crossed, from [`gauge_rr_anova`], or
/// nested, from [`crate::gauge_nested::gauge_rr_nested`]).
#[derive(Debug, Clone, PartialEq)]
pub struct GaugeStudy {
    /// Number of parts (in a nested study, all parts of all operators).
    pub parts: usize,
    /// Number of operators.
    pub operators: usize,
    /// Replicates per part × operator cell.
    pub replicates: usize,
    /// ANOVA table: part, operator, interaction (omitted if pooled), repeatability, total.
    pub anova: Vec<GaugeAnovaRow>,
    /// Whether the interaction was pooled into error.
    pub interaction_pooled: bool,
    /// Variance components.
    pub components: GaugeComponents,
    /// Number of distinct categories, `⌊√2 σ̂_P / σ̂_gauge⌋` (`None` if σ̂_gauge = 0).
    pub distinct_categories: Option<u64>,
}

/// Analyse a balanced crossed gauge study by the ANOVA method.
///
/// # Errors
/// * [`StatError::InvalidArgument`] if there are fewer than two parts or two
///   operators, an index is out of range, the design is unbalanced, fewer than two
///   replicates per cell, or the options are invalid.
/// * [`StatError::NonFiniteInput`] if a value is NaN or ±∞.
pub fn gauge_rr_anova(observations: &[GaugeObservation], options: &GaugeOptions) -> Result<GaugeStudy> {
    validate_options(options)?;
    let parts = observations.iter().map(|o| o.part + 1).max().unwrap_or(0);
    let operators = observations.iter().map(|o| o.operator + 1).max().unwrap_or(0);
    if parts < 2 || operators < 2 {
        return Err(invalid("a gauge study needs at least two parts and two operators"));
    }
    if observations.iter().any(|o| !o.value.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "gauge study measurements" });
    }
    // Group into cells and check balance.
    let mut cells: Vec<Vec<f64>> = vec![Vec::new(); parts * operators];
    for o in observations {
        cells[o.part * operators + o.operator].push(o.value);
    }
    let replicates = cells[0].len();
    if let Some((i, c)) = cells.iter().enumerate().find(|(_, c)| c.len() != replicates) {
        return Err(invalid(format!(
            "the study must be balanced: part {} × operator {} (numbered from 1 in order of appearance) has {} \
             measurements, expected {replicates}",
            i / operators + 1,
            i % operators + 1,
            c.len()
        )));
    }
    if replicates < 2 {
        return Err(invalid("each part must be measured at least twice by each operator"));
    }

    let (p, o, r) = (parts as f64, operators as f64, replicates as f64);
    let cell_mean: Vec<f64> = cells.iter().map(|c| sum_iter(c.iter().copied()) / r).collect();
    let grand = sum_iter(cell_mean.iter().copied()) / (p * o);
    let part_mean: Vec<f64> =
        (0..parts).map(|i| sum_iter((0..operators).map(|j| cell_mean[i * operators + j])) / o).collect();
    let oper_mean: Vec<f64> =
        (0..operators).map(|j| sum_iter((0..parts).map(|i| cell_mean[i * operators + j])) / p).collect();

    let ss_part = o * r * sum_iter(part_mean.iter().map(|m| (m - grand).powi(2)));
    let ss_oper = p * r * sum_iter(oper_mean.iter().map(|m| (m - grand).powi(2)));
    let ss_inter = r * sum_iter((0..parts).flat_map(|i| {
        let (part_mean, oper_mean, cell_mean) = (&part_mean, &oper_mean, &cell_mean);
        (0..operators).map(move |j| (cell_mean[i * operators + j] - part_mean[i] - oper_mean[j] + grand).powi(2))
    }));
    let ss_error = sum_iter(cells.iter().zip(&cell_mean).flat_map(|(c, &m)| c.iter().map(move |&y| (y - m).powi(2))));
    let ss_total = sum_iter(observations.iter().map(|ob| (ob.value - grand).powi(2)));

    let df_part = parts - 1;
    let df_oper = operators - 1;
    let df_inter = df_part * df_oper;
    let df_error = parts * operators * (replicates - 1);
    let ms = |ss: f64, df: usize| ss / df as f64;
    let (ms_part, ms_oper, ms_inter, ms_error) =
        (ms(ss_part, df_part), ms(ss_oper, df_oper), ms(ss_inter, df_inter), ms(ss_error, df_error));

    let f_test = |num: f64, df_num: usize, den: f64, df_den: usize| -> Result<(Option<f64>, Option<f64>)> {
        if den <= 0.0 {
            return Ok((None, None));
        }
        let f = num / den;
        let p = FisherSnedecor::new(df_num as f64, df_den as f64)?.sf(f)?;
        Ok((Some(f), Some(p)))
    };
    let (f_inter, p_inter) = f_test(ms_inter, df_inter, ms_error, df_error)?;
    let pooled = matches!((options.pool_interaction_above, p_inter), (Some(alpha), Some(pv)) if pv > alpha);

    let mut anova = Vec::with_capacity(5);
    let (var_error, var_inter, var_oper, var_part);
    if pooled {
        let ss_e = ss_inter + ss_error;
        let df_e = df_inter + df_error;
        let ms_e = ms(ss_e, df_e);
        let (f_p, p_p) = f_test(ms_part, df_part, ms_e, df_e)?;
        let (f_o, p_o) = f_test(ms_oper, df_oper, ms_e, df_e)?;
        anova.push(row(GaugeSource::Part, df_part, ss_part, Some(ms_part), f_p, p_p));
        anova.push(row(GaugeSource::Operator, df_oper, ss_oper, Some(ms_oper), f_o, p_o));
        anova.push(row(GaugeSource::Repeatability, df_e, ss_e, Some(ms_e), None, None));
        var_error = ms_e;
        var_inter = 0.0;
        var_oper = ((ms_oper - ms_e) / (p * r)).max(0.0);
        var_part = ((ms_part - ms_e) / (o * r)).max(0.0);
    } else {
        let (f_p, p_p) = f_test(ms_part, df_part, ms_inter, df_inter)?;
        let (f_o, p_o) = f_test(ms_oper, df_oper, ms_inter, df_inter)?;
        anova.push(row(GaugeSource::Part, df_part, ss_part, Some(ms_part), f_p, p_p));
        anova.push(row(GaugeSource::Operator, df_oper, ss_oper, Some(ms_oper), f_o, p_o));
        anova.push(row(GaugeSource::Interaction, df_inter, ss_inter, Some(ms_inter), f_inter, p_inter));
        anova.push(row(GaugeSource::Repeatability, df_error, ss_error, Some(ms_error), None, None));
        var_error = ms_error;
        var_inter = ((ms_inter - ms_error) / r).max(0.0);
        var_oper = ((ms_oper - ms_inter) / (p * r)).max(0.0);
        var_part = ((ms_part - ms_inter) / (o * r)).max(0.0);
    }
    anova.push(row(GaugeSource::Total, parts * operators * replicates - 1, ss_total, None, None, None));

    let var_repro = var_oper + var_inter;
    let var_grr = var_error + var_repro;
    let var_total = var_grr + var_part;
    let component = |v: f64| summarize_component(v, var_total, options);
    let components = GaugeComponents {
        gauge_rr: component(var_grr),
        repeatability: component(var_error),
        reproducibility: component(var_repro),
        operator: component(var_oper),
        interaction: component(var_inter),
        part: component(var_part),
        total: component(var_total),
    };
    let distinct_categories = distinct_categories(var_part, var_grr);

    Ok(GaugeStudy { parts, operators, replicates, anova, interaction_pooled: pooled, components, distinct_categories })
}

/// Summary measures of a variance `v` against the total variance.
pub(crate) fn summarize_component(v: f64, var_total: f64, options: &GaugeOptions) -> VarianceComponent {
    let sd = v.max(0.0).sqrt();
    let k = options.study_multiplier;
    VarianceComponent {
        variance: v,
        percent_contribution: if var_total > 0.0 { 100.0 * v / var_total } else { f64::NAN },
        std_dev: sd,
        study_variation: k * sd,
        percent_study_variation: if var_total > 0.0 { 100.0 * sd / var_total.sqrt() } else { f64::NAN },
        percent_tolerance: options.tolerance.map(|t| 100.0 * k * sd / t),
    }
}

/// Number of distinct categories `⌊√2 σ̂_P / σ̂_gauge⌋`.
pub(crate) fn distinct_categories(var_part: f64, var_gauge: f64) -> Option<u64> {
    (var_gauge > 0.0).then(|| (std::f64::consts::SQRT_2 * var_part.max(0.0).sqrt() / var_gauge.sqrt()).floor() as u64)
}

pub(crate) fn row(
    source: GaugeSource,
    df: usize,
    ss: f64,
    ms: Option<f64>,
    f: Option<f64>,
    p: Option<f64>,
) -> GaugeAnovaRow {
    GaugeAnovaRow { source, df, sum_of_squares: ss, mean_square: ms, f_statistic: f, p_value: p }
}

pub(crate) fn validate_options(o: &GaugeOptions) -> Result<()> {
    if !(o.study_multiplier > 0.0 && o.study_multiplier.is_finite()) {
        return Err(invalid("the study multiplier must be positive"));
    }
    if o.tolerance.is_some_and(|t| !(t > 0.0 && t.is_finite())) {
        return Err(invalid("the tolerance must be positive"));
    }
    if o.pool_interaction_above.is_some_and(|a| !(0.0..=1.0).contains(&a)) {
        return Err(invalid("the pooling level must lie in [0, 1]"));
    }
    Ok(())
}

pub(crate) fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(table: &[(usize, usize, f64)]) -> Vec<GaugeObservation> {
        table.iter().map(|&(part, operator, value)| GaugeObservation { part, operator, value }).collect()
    }

    /// 2 parts × 2 operators × 2 replicates, worked by hand.
    ///
    /// Cells (part, operator): (0,0) {10, 12} mean 11; (0,1) {13, 13} mean 13;
    /// (1,0) {20, 22} mean 21; (1,1) {21, 25} mean 23. Grand mean 17.
    /// Part means 12, 22; operator means 16, 18.
    /// SS_P = 2·2·(25+25) = 200; SS_O = 2·2·(1+1) = 8;
    /// SS_PO = 2·Σ(cell − part − oper + grand)² = 2·(0+0+0+0) = 0;
    /// SS_E = (1+1) + 0 + (1+1) + (4+4) = 12; SS_T = 220.
    #[test]
    fn hand_worked_example() {
        let data = obs(&[
            (0, 0, 10.0),
            (0, 0, 12.0),
            (0, 1, 13.0),
            (0, 1, 13.0),
            (1, 0, 20.0),
            (1, 0, 22.0),
            (1, 1, 21.0),
            (1, 1, 25.0),
        ]);
        let s = gauge_rr_anova(&data, &GaugeOptions::default()).unwrap();
        let ss: Vec<f64> = s.anova.iter().map(|r| r.sum_of_squares).collect();
        assert_eq!(ss, vec![200.0, 8.0, 0.0, 12.0, 220.0]);
        let df: Vec<usize> = s.anova.iter().map(|r| r.df).collect();
        assert_eq!(df, vec![1, 1, 1, 4, 7]);
        let c = s.components;
        // σ̂² = 12/4 = 3; σ̂²_PO = max(0, (0 − 3)/2) = 0;
        // σ̂²_O = (8 − 0)/(2·2) = 2; σ̂²_P = (200 − 0)/(2·2) = 50.
        assert_eq!(c.repeatability.variance, 3.0);
        assert_eq!(c.interaction.variance, 0.0);
        assert_eq!(c.operator.variance, 2.0);
        assert_eq!(c.part.variance, 50.0);
        assert_eq!(c.gauge_rr.variance, 5.0);
        assert_eq!(c.total.variance, 55.0);
        assert!((c.gauge_rr.percent_contribution - 100.0 * 5.0 / 55.0).abs() < 1e-12);
        assert!((c.gauge_rr.percent_study_variation - 100.0 * (5.0_f64 / 55.0).sqrt()).abs() < 1e-12);
        assert!((c.part.study_variation - 6.0 * 50.0_f64.sqrt()).abs() < 1e-12);
        // ndc = ⌊√2 · √50 / √5⌋ = ⌊√20⌋ = 4.
        assert_eq!(s.distinct_categories, Some(4));
        // MS_PO = 0, so the part and operator F tests are undefined.
        assert_eq!(s.anova[0].f_statistic, None);
    }

    #[test]
    fn tolerance_and_pooling() {
        let data = obs(&[
            (0, 0, 10.0),
            (0, 0, 12.0),
            (0, 1, 13.0),
            (0, 1, 13.0),
            (1, 0, 20.0),
            (1, 0, 22.0),
            (1, 1, 21.0),
            (1, 1, 25.0),
        ]);
        let opts = GaugeOptions { tolerance: Some(60.0), pool_interaction_above: Some(0.05), ..Default::default() };
        let s = gauge_rr_anova(&data, &opts).unwrap();
        assert!(s.interaction_pooled, "interaction F = 0 has p = 1");
        assert_eq!(s.anova.len(), 4);
        // Pooled error: (0 + 12)/(1 + 4) = 2.4.
        let rep = s.anova.iter().find(|r| r.source == GaugeSource::Repeatability).unwrap();
        assert!((rep.mean_square.unwrap() - 2.4).abs() < 1e-12);
        let c = s.components;
        assert!((c.repeatability.variance - 2.4).abs() < 1e-12);
        assert!((c.operator.variance - (8.0 - 2.4) / 4.0).abs() < 1e-12);
        assert!((c.part.variance - (200.0 - 2.4) / 4.0).abs() < 1e-12);
        let pt = c.gauge_rr.percent_tolerance.unwrap();
        assert!((pt - 100.0 * 6.0 * c.gauge_rr.std_dev / 60.0).abs() < 1e-12);
        // F tests are now against the pooled error.
        assert!((s.anova[0].f_statistic.unwrap() - 200.0 / 2.4).abs() < 1e-10);
    }

    #[test]
    fn validation() {
        let ok = |p, o, v| GaugeObservation { part: p, operator: o, value: v };
        let unbalanced = vec![
            ok(0, 0, 1.0),
            ok(0, 0, 1.1),
            ok(0, 1, 1.0),
            ok(1, 0, 2.0),
            ok(1, 0, 2.1),
            ok(1, 1, 2.0),
            ok(1, 1, 2.2),
        ];
        let e = gauge_rr_anova(&unbalanced, &GaugeOptions::default()).unwrap_err().to_string();
        // Parts and operators are numbered from 1 for people (it read "part 0").
        assert!(e.contains("part 1 × operator 2"), "{e}");
        let single = vec![ok(0, 0, 1.0), ok(0, 1, 1.0), ok(1, 0, 2.0), ok(1, 1, 2.0)];
        assert!(gauge_rr_anova(&single, &GaugeOptions::default()).is_err(), "one replicate");
        let one_operator = vec![ok(0, 0, 1.0), ok(0, 0, 1.1), ok(1, 0, 2.0), ok(1, 0, 2.1)];
        assert!(gauge_rr_anova(&one_operator, &GaugeOptions::default()).is_err());
        let bad_opts = GaugeOptions { tolerance: Some(-1.0), ..Default::default() };
        assert!(gauge_rr_anova(&single, &bad_opts).is_err());
    }
}
