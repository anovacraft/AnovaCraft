//! Intraclass correlation and the evaluation of a measurement process.
//!
//! # Shrout & Fleiss intraclass correlations
//! `n` targets (parts) are each rated (measured) once by each of `k` judges
//! (operators): an `n × k` table. From the two-way ANOVA of the table —
//! between-targets `BMS` (`n − 1` df), between-judges `JMS` (`k − 1` df),
//! residual `EMS` (`(n − 1)(k − 1)` df) and the one-way within-target
//! `WMS = (SS_J + SS_E)/(n(k − 1))` — the six forms of Shrout & Fleiss (1979),
//! "Intraclass correlations: uses in assessing rater reliability",
//! *Psychological Bulletin* 86(2), 420–428, are
//!
//! ```text
//! ICC(1,1) = (BMS − WMS) / (BMS + (k − 1) WMS)                    one-way random
//! ICC(2,1) = (BMS − EMS) / (BMS + (k − 1) EMS + k (JMS − EMS)/n)  two-way random (agreement)
//! ICC(3,1) = (BMS − EMS) / (BMS + (k − 1) EMS)                    two-way mixed (consistency)
//! ICC(1,k) = (BMS − WMS) / BMS
//! ICC(2,k) = (BMS − EMS) / (BMS + (JMS − EMS)/n)
//! ICC(3,k) = (BMS − EMS) / BMS
//! ```
//!
//! the `(·,1)` forms describing one judge's single rating and the `(·,k)`
//! forms the mean of the `k` judges. Confidence intervals: forms 1 and 3 from
//! the exact F distribution of `BMS/WMS` and `BMS/EMS`; form 2 from
//! Satterthwaite's approximate degrees of freedom (Shrout & Fleiss, Table 4;
//! McGraw & Wong 1996, *Psychological Methods* 1(1), 30–46), with the
//! `(2,k)` limits obtained from the `(2,1)` limits by the Spearman–Brown step
//! `kL / (1 + (k − 1)L)`.
//!
//! # Wheeler's evaluation of a measurement process
//! With product (part) variance `σ²_p` and measurement-error variance `σ²_e`
//! (repeatability plus reproducibility), the intraclass correlation
//! `ρ = σ²_p / (σ²_p + σ²_e)` is the share of the variation in the
//! measurements that comes from the product. A process behaviour chart of the
//! measurements sees product signals shrunk by the factor `√ρ`, so the
//! **attenuation** of process signals is `1 − √ρ`. Wheeler groups
//! measurement systems into four classes as monitors of a production process
//! (Wheeler, *EMP III: Using Imperfect Data*, SPC Press 2006, ch. 15):
//!
//! | class  | ρ          | attenuation |
//! |--------|------------|-------------|
//! | first  | 0.8 – 1.0  | under 10%   |
//! | second | 0.5 – 0.8  | 10 – 30%    |
//! | third  | 0.2 – 0.5  | 30 – 55%    |
//! | fourth | under 0.2  | over 55%    |
//!
//! The **probable error** `PE = 0.675 σ_e` is the median absolute error of a
//! single measurement: half of all measurements err by less. The measurement
//! increment (the unit in which values are recorded) is effective between
//! `0.2 PE` and `2 PE`: finer increments record noise, coarser ones discard
//! information (Wheeler & Lyday, *Evaluating the Measurement Process*, 2nd ed.,
//! SPC Press 1989, ch. 4).

use stat_core::distributions::{ContinuousDistribution, FisherSnedecor};
use stat_core::summation::sum_iter;
use stat_core::{Result, StatError};

/// One of the Shrout & Fleiss intraclass correlation forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IccForm {
    /// ICC(1,1): one-way random effects, single rating.
    OneWaySingle,
    /// ICC(2,1): two-way random effects, absolute agreement, single rating.
    TwoWayRandomSingle,
    /// ICC(3,1): two-way mixed effects, consistency, single rating.
    TwoWayMixedSingle,
    /// ICC(1,k): one-way random effects, mean of k ratings.
    OneWayAverage,
    /// ICC(2,k): two-way random effects, absolute agreement, mean of k ratings.
    TwoWayRandomAverage,
    /// ICC(3,k): two-way mixed effects, consistency, mean of k ratings.
    TwoWayMixedAverage,
}

impl IccForm {
    /// All six forms in Shrout & Fleiss's order.
    pub const ALL: [Self; 6] = [
        Self::OneWaySingle,
        Self::TwoWayRandomSingle,
        Self::TwoWayMixedSingle,
        Self::OneWayAverage,
        Self::TwoWayRandomAverage,
        Self::TwoWayMixedAverage,
    ];

    /// Shrout & Fleiss notation, e.g. `ICC(2,1)`.
    #[must_use]
    pub fn notation(self, k: usize) -> String {
        let (model, single) = self.parts();
        if single { format!("ICC({model},1)") } else { format!("ICC({model},{k})") }
    }

    /// A plain description.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::OneWaySingle => "One-way random, single measurement",
            Self::TwoWayRandomSingle => "Two-way random, agreement, single measurement",
            Self::TwoWayMixedSingle => "Two-way mixed, consistency, single measurement",
            Self::OneWayAverage => "One-way random, mean of the operators",
            Self::TwoWayRandomAverage => "Two-way random, agreement, mean of the operators",
            Self::TwoWayMixedAverage => "Two-way mixed, consistency, mean of the operators",
        }
    }

    fn parts(self) -> (u8, bool) {
        match self {
            Self::OneWaySingle => (1, true),
            Self::TwoWayRandomSingle => (2, true),
            Self::TwoWayMixedSingle => (3, true),
            Self::OneWayAverage => (1, false),
            Self::TwoWayRandomAverage => (2, false),
            Self::TwoWayMixedAverage => (3, false),
        }
    }
}

/// One intraclass correlation with its interval and test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IccEstimate {
    /// The form.
    pub form: IccForm,
    /// Estimate (may be negative when targets differ less than chance).
    pub value: f64,
    /// Lower confidence limit.
    pub lower: f64,
    /// Upper confidence limit.
    pub upper: f64,
    /// F statistic of the test that the correlation is zero.
    pub f: f64,
    /// Numerator degrees of freedom of `f`.
    pub df1: f64,
    /// Denominator degrees of freedom of `f`.
    pub df2: f64,
    /// Upper-tail p-value of `f`.
    pub p_value: f64,
}

/// Mean squares of the targets × judges table and the six correlations.
#[derive(Debug, Clone, PartialEq)]
pub struct ShroutFleiss {
    /// Targets `n`.
    pub targets: usize,
    /// Judges `k`.
    pub judges: usize,
    /// Between-targets mean square.
    pub bms: f64,
    /// Within-target mean square (one-way).
    pub wms: f64,
    /// Between-judges mean square.
    pub jms: f64,
    /// Residual mean square (two-way).
    pub ems: f64,
    /// Confidence level of the intervals.
    pub confidence: f64,
    /// The six forms, in [`IccForm::ALL`] order.
    pub estimates: Vec<IccEstimate>,
}

/// The six Shrout & Fleiss intraclass correlations of an `n × k` table
/// (`ratings[i][j]` is judge `j`'s rating of target `i`).
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two targets or judges, rows
/// of different lengths, a confidence level outside (0, 1), or a table with
/// no residual variation; [`StatError::NonFiniteInput`] for NaN or ±∞.
pub fn shrout_fleiss(ratings: &[Vec<f64>], confidence: f64) -> Result<ShroutFleiss> {
    let n = ratings.len();
    let k = ratings.first().map_or(0, Vec::len);
    if n < 2 || k < 2 {
        return Err(invalid("intraclass correlations need at least two parts and two operators"));
    }
    if ratings.iter().any(|r| r.len() != k) {
        return Err(invalid("every part needs one measurement from every operator"));
    }
    if ratings.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "intraclass correlation" });
    }
    if !(confidence > 0.0 && confidence < 1.0) {
        return Err(invalid("the confidence level must lie strictly between 0 and 1"));
    }
    let (nf, kf) = (n as f64, k as f64);
    let grand = sum_iter(ratings.iter().flatten().copied()) / (nf * kf);
    let row_means: Vec<f64> = ratings.iter().map(|r| sum_iter(r.iter().copied()) / kf).collect();
    let col_means: Vec<f64> = (0..k).map(|j| sum_iter(ratings.iter().map(|r| r[j])) / nf).collect();
    let ss_b = kf * sum_iter(row_means.iter().map(|m| (m - grand).powi(2)));
    let ss_j = nf * sum_iter(col_means.iter().map(|m| (m - grand).powi(2)));
    let ss_e = sum_iter(
        ratings
            .iter()
            .zip(&row_means)
            .flat_map(|(r, rm)| r.iter().zip(&col_means).map(move |(x, cm)| (x - rm - cm + grand).powi(2))),
    );
    let (df_b, df_j, df_e, df_w) = (nf - 1.0, kf - 1.0, (nf - 1.0) * (kf - 1.0), nf * (kf - 1.0));
    let (bms, jms, ems, wms) = (ss_b / df_b, ss_j / df_j, ss_e / df_e, (ss_j + ss_e) / df_w);
    if ems <= 0.0 || wms <= 0.0 {
        return Err(invalid("the measurements show no variation within parts, so no correlation can be estimated"));
    }
    let upper_q = |d1: f64, d2: f64| -> Result<f64> { FisherSnedecor::new(d1, d2)?.quantile(0.5 + confidence / 2.0) };
    let sf = |f: f64, d1: f64, d2: f64| -> Result<f64> { FisherSnedecor::new(d1, d2)?.sf(f) };

    // Forms 1 and 3: exact F intervals.
    let f1 = bms / wms;
    let (fl1, fu1) = (f1 / upper_q(df_b, df_w)?, f1 * upper_q(df_w, df_b)?);
    let f3 = bms / ems;
    let (fl3, fu3) = (f3 / upper_q(df_b, df_e)?, f3 * upper_q(df_e, df_b)?);
    let single = |fl: f64, fu: f64| ((fl - 1.0) / (fl + kf - 1.0), (fu - 1.0) / (fu + kf - 1.0));
    let average = |fl: f64, fu: f64| (1.0 - 1.0 / fl, 1.0 - 1.0 / fu);

    // Form 2: Satterthwaite degrees of freedom.
    let icc2 = (bms - ems) / (bms + (kf - 1.0) * ems + kf * (jms - ems) / nf);
    let a = kf * icc2 / (nf * (1.0 - icc2));
    let b = 1.0 + kf * icc2 * (nf - 1.0) / (nf * (1.0 - icc2));
    let v = (a * jms + b * ems).powi(2) / ((a * jms).powi(2) / df_j + (b * ems).powi(2) / df_e);
    let (l2, u2) = if v.is_finite() && v > 0.0 {
        let f_star = upper_q(df_b, v)?;
        let f_2star = upper_q(v, df_b)?;
        let c = kf * jms + (kf * nf - kf - nf) * ems;
        (nf * (bms - f_star * ems) / (f_star * c + nf * bms), nf * (f_2star * bms - ems) / (c + nf * f_2star * bms))
    } else {
        (f64::NAN, f64::NAN)
    };
    let spearman_brown = |r: f64| kf * r / (1.0 + (kf - 1.0) * r);

    let p1 = sf(f1, df_b, df_w)?;
    let p3 = sf(f3, df_b, df_e)?;
    let make = |form, value, (lower, upper): (f64, f64), f, df1, df2, p_value| IccEstimate {
        form,
        value,
        lower,
        upper,
        f,
        df1,
        df2,
        p_value,
    };
    let estimates = vec![
        make(IccForm::OneWaySingle, (bms - wms) / (bms + (kf - 1.0) * wms), single(fl1, fu1), f1, df_b, df_w, p1),
        make(IccForm::TwoWayRandomSingle, icc2, (l2, u2), f3, df_b, df_e, p3),
        make(IccForm::TwoWayMixedSingle, (bms - ems) / (bms + (kf - 1.0) * ems), single(fl3, fu3), f3, df_b, df_e, p3),
        make(IccForm::OneWayAverage, (bms - wms) / bms, average(fl1, fu1), f1, df_b, df_w, p1),
        make(
            IccForm::TwoWayRandomAverage,
            (bms - ems) / (bms + (jms - ems) / nf),
            (spearman_brown(l2), spearman_brown(u2)),
            f3,
            df_b,
            df_e,
            p3,
        ),
        make(IccForm::TwoWayMixedAverage, (bms - ems) / bms, average(fl3, fu3), f3, df_b, df_e, p3),
    ];
    Ok(ShroutFleiss { targets: n, judges: k, bms, wms, jms, ems, confidence, estimates })
}

/// Wheeler's four classes of process monitors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MonitorClass {
    /// ρ ≥ 0.8.
    First,
    /// 0.5 ≤ ρ < 0.8.
    Second,
    /// 0.2 ≤ ρ < 0.5.
    Third,
    /// ρ < 0.2.
    Fourth,
}

impl MonitorClass {
    /// The class of a measurement system with intraclass correlation `rho`.
    #[must_use]
    pub fn of(rho: f64) -> Self {
        if rho >= 0.8 {
            Self::First
        } else if rho >= 0.5 {
            Self::Second
        } else if rho >= 0.2 {
            Self::Third
        } else {
            Self::Fourth
        }
    }

    /// Name, e.g. "First class".
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::First => "First class",
            Self::Second => "Second class",
            Self::Third => "Third class",
            Self::Fourth => "Fourth class",
        }
    }

    /// What the class means for monitoring the process.
    #[must_use]
    pub fn meaning(self) -> &'static str {
        match self {
            Self::First => "process signals are attenuated by less than 10%; suitable for tracking process changes",
            Self::Second => "process signals are attenuated by 10–30%; still useful for tracking process changes",
            Self::Third => "process signals are attenuated by 30–55%; large process changes can still be detected",
            Self::Fourth => "process signals are attenuated by more than 55%; of little use for tracking the process",
        }
    }
}

/// Where the measurement increment falls relative to the effective range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IncrementVerdict {
    /// Smaller than `0.2 PE`: the last digit is noise; one fewer would do.
    TooFine,
    /// Between `0.2 PE` and `2 PE`.
    Effective,
    /// Larger than `2 PE`: an extra digit would carry information.
    TooCoarse,
}

/// Wheeler's evaluation of a measurement process.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasurementEvaluation {
    /// Product (part-to-part) variance `σ²_p`.
    pub product_variance: f64,
    /// Measurement-error variance `σ²_e`.
    pub error_variance: f64,
    /// `ρ = σ²_p / (σ²_p + σ²_e)`.
    pub intraclass_correlation: f64,
    /// `1 − √ρ`.
    pub attenuation: f64,
    /// Wheeler's class.
    pub class: MonitorClass,
    /// `0.675 σ_e`.
    pub probable_error: f64,
    /// `0.2 PE`.
    pub smallest_effective_increment: f64,
    /// `2 PE`.
    pub largest_effective_increment: f64,
    /// The increment in which measurements are recorded, if known.
    pub increment: Option<f64>,
    /// How the increment compares with the effective range.
    pub increment_verdict: Option<IncrementVerdict>,
}

/// Multiplier of σ_e giving the probable error (the normal 75th percentile,
/// 0.6745, rounded as Wheeler & Lyday do).
pub const PROBABLE_ERROR_FACTOR: f64 = 0.675;

/// Evaluate a measurement process from its variance components.
///
/// # Errors
/// [`StatError::InvalidArgument`] if a variance is negative or not finite,
/// both are zero, or the increment is not positive.
pub fn evaluate_measurement(
    product_variance: f64,
    error_variance: f64,
    increment: Option<f64>,
) -> Result<MeasurementEvaluation> {
    if !(product_variance >= 0.0 && error_variance >= 0.0 && product_variance.is_finite() && error_variance.is_finite())
    {
        return Err(invalid("variances must be finite and non-negative"));
    }
    if product_variance + error_variance <= 0.0 {
        return Err(invalid("the measurements do not vary"));
    }
    if increment.is_some_and(|u| !(u > 0.0 && u.is_finite())) {
        return Err(invalid("the measurement increment must be positive"));
    }
    let rho = product_variance / (product_variance + error_variance);
    let pe = PROBABLE_ERROR_FACTOR * error_variance.sqrt();
    let (lo, hi) = (0.2 * pe, 2.0 * pe);
    let increment_verdict = increment.filter(|_| pe > 0.0).map(|u| {
        if u < lo {
            IncrementVerdict::TooFine
        } else if u > hi {
            IncrementVerdict::TooCoarse
        } else {
            IncrementVerdict::Effective
        }
    });
    Ok(MeasurementEvaluation {
        product_variance,
        error_variance,
        intraclass_correlation: rho,
        attenuation: 1.0 - rho.sqrt(),
        class: MonitorClass::of(rho),
        probable_error: pe,
        smallest_effective_increment: lo,
        largest_effective_increment: hi,
        increment,
        increment_verdict,
    })
}

/// The recording increment of `values`: `10^−d` for the fewest decimal
/// places `d ≤ 9` that represent every value, as values are usually written
/// (`None` when there are no values). Values recorded in steps other than a
/// power of ten (for example halves) should be given explicitly.
#[must_use]
pub fn decimal_increment(values: &[f64]) -> Option<f64> {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return None;
    }
    // Allow for integers recorded in tens, hundreds…: use the largest power of
    // ten dividing every value (up to 10⁶).
    // A value counts as a multiple of p only when it is within rounding of a
    // nonzero multiple (or is zero), so tiny values never divide a large p.
    let divides = |p: f64| {
        finite.iter().all(|&v| {
            let m = (v / p).round();
            (v == 0.0 || m != 0.0) && (v - p * m).abs() <= 1e-9 * v.abs().max(p)
        })
    };
    if finite.iter().any(|&v| v != 0.0) {
        for e in (1..=6).rev() {
            let p = 10f64.powi(e);
            if divides(p) {
                return Some(p);
            }
        }
    }
    (0..=9).map(|d| 10f64.powi(-d)).find(|&p| divides(p)).or(Some(1e-9))
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} vs {b}");
    }

    /// Shrout & Fleiss (1979), Table 2: six targets rated by four judges.
    /// The paper reports BMS = 11.24, WMS = 6.26, JMS = 32.49, EMS = 1.02 and
    /// ICC(1,1) = .17, ICC(2,1) = .29, ICC(3,1) = .71, ICC(1,4) = .44,
    /// ICC(2,4) = .62, ICC(3,4) = .91. The full-precision values below were
    /// computed independently in Python (standard library).
    fn table2() -> Vec<Vec<f64>> {
        [[9, 2, 5, 8], [6, 1, 3, 2], [8, 4, 6, 8], [7, 1, 2, 6], [10, 5, 6, 9], [6, 2, 4, 7]]
            .iter()
            .map(|r| r.iter().map(|&v| f64::from(v)).collect())
            .collect()
    }

    #[test]
    fn shrout_fleiss_table_2() {
        let s = shrout_fleiss(&table2(), 0.95).unwrap();
        close(s.bms, 11.241_666_666_666_667, 1e-12);
        close(s.wms, 6.263_888_888_888_889, 1e-12);
        close(s.jms, 32.486_111_111_111_114, 1e-12);
        close(s.ems, 1.019_444_444_444_443_7, 1e-12);
        let v: Vec<f64> = s.estimates.iter().map(|e| e.value).collect();
        let expected = [
            0.165_741_768_405_475_4,
            0.289_763_779_527_559_05,
            0.714_840_714_840_715_1,
            0.442_797_133_679_268_6,
            0.620_050_547_598_989,
            0.909_315_542_377_069_5,
        ];
        for (a, b) in v.iter().zip(expected) {
            close(*a, b, 1e-12);
        }
        // Published two-digit values.
        let published = [0.17, 0.29, 0.71, 0.44, 0.62, 0.91];
        for (a, b) in v.iter().zip(published) {
            close(*a, b, 0.005);
        }
        assert_eq!(s.estimates[1].form.notation(4), "ICC(2,1)");
        assert_eq!(s.estimates[4].form.notation(4), "ICC(2,4)");
        for e in &s.estimates {
            assert!(e.lower < e.value && e.value < e.upper, "{:?}", e.form);
        }
        // F tests: BMS/WMS on (5, 18) and BMS/EMS on (5, 15) df.
        close(s.estimates[0].f, 11.241_666_666_666_667 / 6.263_888_888_888_889, 1e-12);
        assert_eq!((s.estimates[0].df1, s.estimates[0].df2), (5.0, 18.0));
        assert_eq!((s.estimates[2].df1, s.estimates[2].df2), (5.0, 15.0));
        // The average-measure limits follow from the single-measure ones by
        // Spearman–Brown for forms 1 and 3 as well (an identity of the F limits).
        for (single, avg) in [(0, 3), (2, 5), (1, 4)] {
            let sb = |r: f64| 4.0 * r / (1.0 + 3.0 * r);
            close(sb(s.estimates[single].lower), s.estimates[avg].lower, 1e-12);
            close(sb(s.estimates[single].upper), s.estimates[avg].upper, 1e-12);
        }
        // ICC(3,1) lower limit by hand: F₀ = 11.0272; F₀.₉₇₅(5, 15) = 3.5764
        // (F tables), FL = 3.0833, lower = (FL − 1)/(FL + 3) = 0.3424.
        close(s.estimates[2].lower, 0.3424, 5e-4);
    }

    #[test]
    fn shrout_fleiss_validation() {
        assert!(shrout_fleiss(&[vec![1.0, 2.0]], 0.95).is_err());
        assert!(shrout_fleiss(&[vec![1.0, 2.0], vec![1.0]], 0.95).is_err());
        assert!(shrout_fleiss(&[vec![1.0, 1.0], vec![2.0, 2.0]], 0.95).is_err(), "no residual variation");
        assert!(shrout_fleiss(&table2(), 1.5).is_err());
    }

    /// Hand computation: σ²_p = 9, σ²_e = 1 gives ρ = 0.9, attenuation
    /// 1 − √0.9 = 0.051317, PE = 0.675, effective increments 0.135 to 1.35.
    #[test]
    fn wheeler_evaluation() {
        let e = evaluate_measurement(9.0, 1.0, Some(0.1)).unwrap();
        close(e.intraclass_correlation, 0.9, 1e-15);
        close(e.attenuation, 1.0 - 0.9f64.sqrt(), 1e-15);
        assert_eq!(e.class, MonitorClass::First);
        close(e.probable_error, 0.675, 1e-15);
        close(e.smallest_effective_increment, 0.135, 1e-15);
        close(e.largest_effective_increment, 1.35, 1e-15);
        assert_eq!(e.increment_verdict, Some(IncrementVerdict::TooFine));
        assert_eq!(
            evaluate_measurement(9.0, 1.0, Some(1.0)).unwrap().increment_verdict,
            Some(IncrementVerdict::Effective)
        );
        assert_eq!(
            evaluate_measurement(9.0, 1.0, Some(2.0)).unwrap().increment_verdict,
            Some(IncrementVerdict::TooCoarse)
        );
        assert_eq!(MonitorClass::of(0.79), MonitorClass::Second);
        assert_eq!(MonitorClass::of(0.2), MonitorClass::Third);
        assert_eq!(MonitorClass::of(0.1), MonitorClass::Fourth);
        // Class boundaries match the attenuation bands: 1 − √0.8 = 10.6%,
        // 1 − √0.5 = 29.3%, 1 − √0.2 = 55.3%.
        close(1.0 - 0.5f64.sqrt(), 0.293, 1e-3);
        assert!(evaluate_measurement(-1.0, 1.0, None).is_err());
        assert!(evaluate_measurement(0.0, 0.0, None).is_err());
        assert!(evaluate_measurement(1.0, 1.0, Some(0.0)).is_err());
    }

    #[test]
    fn increments_from_recorded_values() {
        close(decimal_increment(&[1.23, 4.5, 6.0]).unwrap(), 0.01, 1e-15);
        close(decimal_increment(&[10.0, 20.0, 150.0]).unwrap(), 10.0, 1e-12);
        close(decimal_increment(&[1.0, 2.0, 3.0]).unwrap(), 1.0, 1e-15);
        close(decimal_increment(&[0.004, 0.0]).unwrap(), 0.001, 1e-15);
        assert!(decimal_increment(&[]).is_none());
        // Values far below 1 are not multiples of a large power of ten.
        close(decimal_increment(&[0.00012, 0.00015, 0.00011, 0.00052]).unwrap(), 1e-5, 1e-18);
    }
}
