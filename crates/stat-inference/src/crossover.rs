//! The 2×2 crossover design: two sequences of subjects receive a reference
//! formulation R and a test formulation T in opposite orders (RT and TR),
//! one in each of two periods. Analysis of treatment, period and carryover
//! (sequence) effects, and average equivalence of T and R by two one-sided
//! tests (Chow & Liu, *Design and Analysis of Bioavailability and
//! Bioequivalence Studies*, 3rd ed., 2009, chs. 3–4; Schuirmann 1987, *J.
//! Pharmacokinet. Biopharm.* 15:657; Jones & Kenward, *Design and Analysis
//! of Cross-Over Trials*, 3rd ed., §2.3).
//!
//! For subject `k` of sequence `i` (`i = 1` RT, `i = 2` TR, sizes `n₁`, `n₂`)
//! with responses `y₁`, `y₂` in periods 1 and 2, the period difference
//! `dᵢₖ = (y₂ − y₁)/2` and subject total `uᵢₖ = y₁ + y₂` give
//!
//! ```text
//! treatment  F̂ = d̄₁ − d̄₂            (estimates μ_T − μ_R)
//! period     P̂ = d̄₁ + d̄₂            (period 2 − period 1)
//! carryover  Ĉ = ū₂ − ū₁            (sequence effect)
//! σ̂²_d = [Σ(d₁ₖ − d̄₁)² + Σ(d₂ₖ − d̄₂)²]/(n₁ + n₂ − 2),   SE(F̂) = SE(P̂) = σ̂_d √(1/n₁ + 1/n₂),
//! σ̂²_u likewise from the totals,                      SE(Ĉ) = σ̂_u √(1/n₁ + 1/n₂),
//! ```
//!
//! each a t statistic on `n₁ + n₂ − 2` df. The analysis-of-variance table
//! (Chow & Liu Table 3.4.1) splits the total sum of squares into carryover
//! and subjects within sequences (between subjects), and treatment, period
//! and residual (within subjects); the F tests equal the squared t tests.
//!
//! Equivalence within `(L, U)` is concluded when both one-sided tests reject,
//! i.e. the `1 − 2α` interval `F̂ ± t_{1−α, n₁+n₂−2} SE` lies within the
//! bounds. On the **ratio scale** the responses are log-transformed first,
//! the bounds are given as ratios (e.g. 0.80 and 1.25) and compared on the
//! log scale, and the estimate and interval are reported as the ratio of
//! geometric means `exp(F̂)` with limits `exp(·)`.

use crate::common::{check_finite, invalid, mean_var};
use crate::equivalence::{EquivalenceTest, tost_from_parts};
use stat_core::distributions::{ContinuousDistribution, FisherSnedecor, StudentsT};
use stat_core::{Result, StatError};

/// One subject of a 2×2 crossover.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossoverSubject {
    /// `true` for sequence TR (test first), `false` for RT.
    pub test_first: bool,
    /// Response in period 1.
    pub period1: f64,
    /// Response in period 2.
    pub period2: f64,
}

/// One effect of the crossover analysis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossoverEffect {
    /// Estimate (on the analysis scale: logs on the ratio scale).
    pub estimate: f64,
    /// Standard error.
    pub std_error: f64,
    /// `t` statistic against zero.
    pub t: f64,
    /// Two-sided p-value.
    pub p_value: f64,
}

/// One line of the analysis-of-variance table.
#[derive(Debug, Clone, PartialEq)]
pub struct CrossoverAnovaRow {
    /// Source of variation.
    pub source: &'static str,
    /// Degrees of freedom.
    pub df: f64,
    /// Sum of squares.
    pub sum_sq: f64,
    /// Mean square.
    pub mean_sq: f64,
    /// F statistic (NaN where no test is made).
    pub f: f64,
    /// p-value (NaN where no test is made).
    pub p_value: f64,
}

/// The analysis of a 2×2 crossover.
#[derive(Debug, Clone, PartialEq)]
pub struct CrossoverAnalysis {
    /// Subjects in sequence RT and TR.
    pub sizes: (usize, usize),
    /// `true` when responses were log-transformed (ratio scale).
    pub log_scale: bool,
    /// Treatment effect `μ_T − μ_R` (difference of log means on the ratio scale).
    pub treatment: CrossoverEffect,
    /// Period effect (period 2 − period 1).
    pub period: CrossoverEffect,
    /// Carryover (sequence) effect, TR − RT subject totals.
    pub carryover: CrossoverEffect,
    /// Least-squares means of the reference and test formulations
    /// (analysis scale).
    pub means: (f64, f64),
    /// Analysis-of-variance table.
    pub anova: Vec<CrossoverAnovaRow>,
    /// Within-subject coefficient of variation `√(e^{σ²_w} − 1)` (ratio
    /// scale only; NaN otherwise), with `σ²_w` the residual mean square.
    pub within_cv: f64,
    /// Two one-sided tests on the treatment effect (analysis scale).
    pub equivalence: EquivalenceTest,
}

impl CrossoverAnalysis {
    /// The estimate and `1 − 2α` interval on the scale of the data: the
    /// difference, or the ratio of geometric means.
    #[must_use]
    pub fn reported_interval(&self) -> (f64, f64, f64) {
        let e = &self.equivalence;
        let (lo, hi) = e.confidence_interval;
        if self.log_scale { (e.estimate.exp(), lo.exp(), hi.exp()) } else { (e.estimate, lo, hi) }
    }
}

fn effect(estimate: f64, se: f64, t: &StudentsT) -> Result<CrossoverEffect> {
    let stat = estimate / se;
    Ok(CrossoverEffect { estimate, std_error: se, t: stat, p_value: t.two_sided_p_value(stat)? })
}

/// Analyse a 2×2 crossover and test the equivalence of the formulations
/// within `bounds` (differences, or ratios when `log_scale`) at level `alpha`
/// for each one-sided test.
///
/// # Errors
/// [`StatError::InsufficientData`] unless each sequence has at least one
/// subject and there are at least three subjects; [`StatError::NonFiniteInput`]
/// for NaN/±∞; [`StatError::InvalidArgument`] for non-positive responses on
/// the ratio scale, invalid bounds or `α`, or no within-subject variation.
pub fn crossover_equivalence(
    subjects: &[CrossoverSubject],
    bounds: (f64, f64),
    alpha: f64,
    log_scale: bool,
) -> Result<CrossoverAnalysis> {
    let values: Vec<f64> = subjects.iter().flat_map(|s| [s.period1, s.period2]).collect();
    check_finite(&values, "crossover responses")?;
    let tr = |y: f64| if log_scale { y.ln() } else { y };
    if log_scale && values.iter().any(|&v| v <= 0.0) {
        return Err(invalid("the ratio scale needs positive responses (they are log-transformed)"));
    }
    let bounds = if log_scale {
        if !(bounds.0 > 0.0 && bounds.1 > 0.0) {
            return Err(invalid("ratio equivalence bounds must be positive"));
        }
        (bounds.0.ln(), bounds.1.ln())
    } else {
        bounds
    };
    let mut d = [Vec::new(), Vec::new()];
    let mut u = [Vec::new(), Vec::new()];
    let mut y = [[Vec::new(), Vec::new()], [Vec::new(), Vec::new()]];
    for s in subjects {
        let i = usize::from(s.test_first);
        let (a, b) = (tr(s.period1), tr(s.period2));
        d[i].push(0.5 * (b - a));
        u[i].push(a + b);
        y[i][0].push(a);
        y[i][1].push(b);
    }
    let (n1, n2) = (d[0].len(), d[1].len());
    if n1 == 0 || n2 == 0 || n1 + n2 < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n1 + n2 });
    }
    let df = (n1 + n2 - 2) as f64;
    let ss = |x: &[f64]| {
        let (m, _) = mean_var(x);
        x.iter().map(|v| (v - m).powi(2)).sum::<f64>()
    };
    let var_d = (ss(&d[0]) + ss(&d[1])) / df;
    let var_u = (ss(&u[0]) + ss(&u[1])) / df;
    if var_d <= 0.0 {
        return Err(invalid("the within-subject differences have no variation"));
    }
    let k = (1.0 / n1 as f64 + 1.0 / n2 as f64).sqrt();
    let t = StudentsT::new(df)?;
    let (d1, d2) = (mean_var(&d[0]).0, mean_var(&d[1]).0);
    let (u1, u2) = (mean_var(&u[0]).0, mean_var(&u[1]).0);
    let treatment = effect(d1 - d2, var_d.sqrt() * k, &t)?;
    let period = effect(d1 + d2, var_d.sqrt() * k, &t)?;
    let carryover = if var_u > 0.0 {
        effect(u2 - u1, var_u.sqrt() * k, &t)?
    } else {
        CrossoverEffect { estimate: u2 - u1, std_error: 0.0, t: f64::NAN, p_value: f64::NAN }
    };
    // Least-squares means: average of the two sequence-period cell means.
    let cell = |i: usize, j: usize| mean_var(&y[i][j]).0;
    let reference = 0.5 * (cell(0, 0) + cell(1, 1));
    let test = 0.5 * (cell(0, 1) + cell(1, 0));
    // ANOVA: within-subject residual SS = 2Σ(d − d̄)², between = Σ(u − ū)²/2.
    let ms_within = 2.0 * var_d;
    let ms_between = 0.5 * var_u;
    let all: Vec<f64> = y.iter().flat_map(|s| s.iter().flatten().copied()).collect();
    let total_ss = ss(&all);
    let f_dist = FisherSnedecor::new(1.0, df)?;
    let row = |source, ss_effect: f64, ms_error: f64| -> Result<CrossoverAnovaRow> {
        let f = if ms_error > 0.0 { ss_effect / ms_error } else { f64::NAN };
        Ok(CrossoverAnovaRow {
            source,
            df: 1.0,
            sum_sq: ss_effect,
            mean_sq: ss_effect,
            f,
            p_value: if f.is_finite() { f_dist.sf(f)? } else { f64::NAN },
        })
    };
    let plain = |source, df: f64, sum_sq: f64| CrossoverAnovaRow {
        source,
        df,
        sum_sq,
        mean_sq: if df > 0.0 { sum_sq / df } else { f64::NAN },
        f: f64::NAN,
        p_value: f64::NAN,
    };
    let anova = vec![
        row("Carryover (sequence)", carryover.t.powi(2) * ms_between, ms_between)?,
        plain("Subjects within sequences", df, ms_between * df),
        row("Treatment", treatment.t.powi(2) * ms_within, ms_within)?,
        row("Period", period.t.powi(2) * ms_within, ms_within)?,
        plain("Residual (within subjects)", df, ms_within * df),
        plain("Total", 2.0 * (n1 + n2) as f64 - 1.0, total_ss),
    ];
    let method = if log_scale { "Crossover equivalence (TOST, ratio)" } else { "Crossover equivalence (TOST)" };
    let equivalence = tost_from_parts(method, treatment.estimate, treatment.std_error, df, bounds, alpha)?;
    Ok(CrossoverAnalysis {
        sizes: (n1, n2),
        log_scale,
        treatment,
        period,
        carryover,
        means: (reference, test),
        anova,
        within_cv: if log_scale { (2.0 * var_d).exp_m1().sqrt() } else { f64::NAN },
        equivalence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    /// Six RT and five TR subjects. Every reference value was computed
    /// independently in Python (NumPy 2 least squares on the full model
    /// `y ~ subject + period + treatment` for the treatment and period
    /// estimates, their standard errors and the residual mean square; sums
    /// of squares of the subject totals for the carryover test; SciPy 1.18
    /// `t` for the p-values and the 90% interval). The script is in the
    /// validation record.
    pub(crate) const RT: [(f64, f64); 6] =
        [(74.7, 82.3), (96.3, 94.0), (60.2, 73.6), (101.8, 99.9), (82.0, 90.1), (71.5, 70.2)];
    pub(crate) const TR: [(f64, f64); 5] = [(80.1, 76.0), (93.4, 90.8), (66.9, 61.0), (97.6, 101.2), (78.3, 75.1)];

    fn subjects() -> Vec<CrossoverSubject> {
        RT.iter()
            .map(|&(a, b)| CrossoverSubject { test_first: false, period1: a, period2: b })
            .chain(TR.iter().map(|&(a, b)| CrossoverSubject { test_first: true, period1: a, period2: b }))
            .collect()
    }

    #[test]
    fn difference_scale_matches_least_squares() {
        let r = crossover_equivalence(&subjects(), (-5.0, 5.0), 0.05, false).unwrap();
        assert_eq!(r.sizes, (6, 5));
        close(r.treatment.estimate, REF_DIFF[0], 1e-10);
        close(r.treatment.std_error, REF_DIFF[1], 1e-10);
        close(r.treatment.p_value, REF_DIFF[2], 1e-8);
        close(r.period.estimate, REF_DIFF[3], 1e-10);
        close(r.period.p_value, REF_DIFF[4], 1e-8);
        close(r.anova[4].mean_sq, REF_DIFF[5], 1e-10);
        close(r.carryover.estimate, REF_DIFF[6], 1e-10);
        close(r.carryover.p_value, REF_DIFF[7], 1e-8);
        close(r.equivalence.confidence_interval.0, REF_DIFF[8], 1e-9);
        close(r.equivalence.confidence_interval.1, REF_DIFF[9], 1e-9);
        close(r.equivalence.p_value, REF_DIFF[10], 1e-8);
        close(r.anova[5].sum_sq, REF_DIFF[11], 1e-9);
        // The F tests equal the squared t tests.
        close(r.anova[2].f, r.treatment.t.powi(2), 1e-10);
        close(r.anova[2].p_value, r.treatment.p_value, 1e-10);
    }

    #[test]
    fn ratio_scale_matches_least_squares_on_logs() {
        let r = crossover_equivalence(&subjects(), (0.8, 1.25), 0.05, true).unwrap();
        let (ratio, lo, hi) = r.reported_interval();
        close(ratio, REF_LOG[0], 1e-10);
        close(lo, REF_LOG[1], 1e-9);
        close(hi, REF_LOG[2], 1e-9);
        close(r.equivalence.p_value, REF_LOG[3], 1e-8);
        close(r.within_cv, REF_LOG[4], 1e-9);
        assert!(r.equivalence.equivalent());
        assert!(crossover_equivalence(&subjects(), (-0.2, 0.2), 0.05, true).is_err(), "ratio bounds are positive");
        let one_sequence: Vec<CrossoverSubject> = subjects().into_iter().filter(|s| !s.test_first).collect();
        assert!(crossover_equivalence(&one_sequence, (0.8, 1.25), 0.05, true).is_err());
    }

    // treatment est, se, p; period est, p; residual MS; carryover est, p; CI lo, hi; TOST p; total SS
    const REF_DIFF: [f64; 12] = [
        3.186_666_666_666_619_5,
        1.665_988_380_496_879_1,
        0.088_069_067_648_438_66,
        0.746_666_666_666_593_4,
        0.664_616_210_345_008_3,
        15.139_185_185_185_17,
        -2.020_000_000_000_010_2,
        0.905_428_826_120_768_4,
        0.132_721_820_722_770_72,
        6.240_611_512_610_468,
        0.152_342_979_106_578_12,
        3_556.858_181_818_181_5,
    ];
    // ratio, lo, hi, TOST p, within-subject CV
    const REF_LOG: [f64; 5] = [
        1.046_466_242_757_261,
        1.003_871_117_542_960_2,
        1.090_868_716_206_127,
        1.300_279_626_131_582_3e-5,
        0.052_981_168_452_150_354,
    ];
}
