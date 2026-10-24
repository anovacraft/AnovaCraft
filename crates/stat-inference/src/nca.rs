//! Non-compartmental pharmacokinetic analysis of one concentration–time
//! profile (Gibaldi & Perrier 1982, *Pharmacokinetics*, 2nd ed., ch. 11;
//! Gabrielsson & Weiner 2016, *Pharmacokinetic and Pharmacodynamic Data
//! Analysis*, 5th ed., ch. 2).
//!
//! **Exposure.** `Cmax` is the largest observed concentration and `Tmax` the
//! first time it is reached; `Clast` is the last positive concentration and
//! `tlast` its time. The areas under the concentration curve (AUC) and under
//! the first-moment curve `t·C(t)` (AUMC) run from time 0 to `tlast`,
//! segment by segment between consecutive samples `(t₁, C₁)`, `(t₂, C₂)`,
//! `Δt = t₂ − t₁`:
//!
//! * linear trapezoid: `AUC = Δt (C₁ + C₂)/2`, `AUMC = Δt (t₁C₁ + t₂C₂)/2`;
//! * log trapezoid (exact for an exponential decline), used by
//!   [`AucMethod::LinearUpLogDown`] only where `C₁ > C₂ > 0`: with
//!   `k = ln(C₁/C₂)/Δt`, `AUC = (C₁ − C₂)/k` and
//!   `AUMC = (t₁C₁ − t₂C₂)/k + (C₁ − C₂)/k²`.
//!
//! **Time zero.** Times must be non-negative. After extravascular dosing or
//! an infusion, a profile without a sample at time 0 is given `C(0) = 0`.
//! After an intravenous bolus the concentration at time 0, `C0`, is an
//! observed positive value at time 0 if there is one; otherwise it is
//! back-extrapolated by the log-linear line through the first two positive
//! concentrations, `C0 = C₁ (C₁/C₂)^{t₁/(t₂−t₁)}`, when they decline, and is
//! the first positive concentration when they do not (or when there is only
//! one). `C0` replaces any zero sample at time 0. `C0` enters the areas but not
//! `Cmax`, `Tmax` or the terminal phase.
//!
//! **Terminal phase.** `λz` is minus the slope of the least-squares line of
//! `ln C` on `t` through positive concentrations in the terminal phase
//! ([`LambdaZRule`]); its `R²` and adjusted `R² = 1 − (1 − R²)(n − 1)/(n − 2)`
//! are reported. A fit with `λz ≤ 0` is rejected. With `λz`:
//! `t½ = ln 2/λz`, `AUC∞ = AUClast + Clast/λz`,
//! `AUMC∞ = AUMClast + tlast·Clast/λz + Clast/λz²`,
//! `MRT = AUMC∞/AUC∞` (minus half the infusion duration after an infusion),
//! and with a dose `CL = Dose/AUC∞` (`CL/F` after extravascular dosing) and
//! `Vz = Dose/(λz·AUC∞)` (`Vz/F`). Clast is the observed value.
//!
//! **Undefined values** are `None` throughout [`NcaResult`]; no field holds
//! NaN.

use crate::common::invalid;
use stat_core::Result;

/// How the area between two samples is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AucMethod {
    /// Linear trapezoid on every segment.
    Linear,
    /// Linear trapezoid where the concentration rises or stays level, log
    /// trapezoid where it declines between two positive values.
    #[default]
    LinearUpLogDown,
}

impl AucMethod {
    /// Every method, in menu order.
    pub const ALL: [AucMethod; 2] = [AucMethod::Linear, AucMethod::LinearUpLogDown];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            AucMethod::Linear => "Linear trapezoid",
            AucMethod::LinearUpLogDown => "Linear up, log down",
        }
    }

    /// Stable key used in saved requests and scripts.
    pub fn key(self) -> &'static str {
        match self {
            AucMethod::Linear => "linear",
            AucMethod::LinearUpLogDown => "linear-up-log-down",
        }
    }

    /// The method with this [`key`](Self::key), if any.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Route of administration of the dose.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Administration {
    /// Oral or another route with absorption; `C(0) = 0` is assumed without a
    /// sample at time 0. Clearance and volume are apparent (`CL/F`, `Vz/F`).
    #[default]
    Extravascular,
    /// Intravenous bolus at time 0; `C0` is observed or back-extrapolated.
    IntravenousBolus,
    /// Constant-rate intravenous infusion starting at time 0.
    IntravenousInfusion {
        /// Length of the infusion (positive, in the time unit).
        duration: f64,
    },
}

/// Which points define the terminal phase.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LambdaZRule {
    /// Among regressions on the last `k ≥ 3` positive concentrations after
    /// `Cmax` (`Cmax` excluded) with `λz > 0`, the largest adjusted `R²`; of
    /// the fits within `10⁻⁴` of it, the one with the most points.
    #[default]
    BestFit,
    /// The last `k` positive concentrations after `Cmax` (`k ≥ 3`); no `λz`
    /// when there are fewer.
    LastPoints(usize),
    /// The positive concentrations with `start ≤ t ≤ end` (at least 3).
    TimeRange(f64, f64),
}

/// Options of [`non_compartmental`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NcaSettings {
    /// Area rule.
    pub method: AucMethod,
    /// Route of administration.
    pub administration: Administration,
    /// Terminal-phase point selection.
    pub lambda_rule: LambdaZRule,
    /// Dose (positive), needed for clearance and volume.
    pub dose: Option<f64>,
}

/// The log-linear terminal-phase regression `ln C = intercept − λz t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalPhase {
    /// Terminal rate constant `λz` (positive).
    pub lambda: f64,
    /// Intercept of the line on the natural-log scale (`ln C` at `t = 0`).
    pub intercept: f64,
    /// Points in the regression.
    pub n_points: usize,
    /// Coefficient of determination of the regression.
    pub r_squared: f64,
    /// Adjusted `R² = 1 − (1 − R²)(n − 1)/(n − 2)`.
    pub adj_r_squared: f64,
    /// Time of the first point used.
    pub start: f64,
    /// Time of the last point used.
    pub end: f64,
}

/// Non-compartmental parameters of one profile. `None` marks a value that is
/// undefined (no terminal phase, or no dose).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NcaResult {
    /// Largest observed concentration.
    pub cmax: f64,
    /// First time `Cmax` is observed.
    pub tmax: f64,
    /// Last positive concentration.
    pub clast: f64,
    /// Time of `Clast`.
    pub tlast: f64,
    /// Concentration at time 0 used for an intravenous bolus.
    pub c0: Option<f64>,
    /// Area under the curve from 0 to `tlast`.
    pub auc_last: f64,
    /// Area under the first-moment curve from 0 to `tlast`.
    pub aumc_last: f64,
    /// Terminal-phase regression.
    pub lambda_z: Option<TerminalPhase>,
    /// Terminal half-life `ln 2/λz`.
    pub half_life: Option<f64>,
    /// `AUClast + Clast/λz`.
    pub auc_inf: Option<f64>,
    /// Percentage of `AUC∞` beyond `tlast`.
    pub auc_pct_extrapolated: Option<f64>,
    /// `AUMClast + tlast·Clast/λz + Clast/λz²`.
    pub aumc_inf: Option<f64>,
    /// Mean residence time `AUMC∞/AUC∞` (less half the infusion duration).
    pub mrt: Option<f64>,
    /// `Dose/AUC∞` (`CL/F` after extravascular dosing).
    pub clearance: Option<f64>,
    /// `Dose/(λz·AUC∞)` (`Vz/F` after extravascular dosing).
    pub volume_z: Option<f64>,
    /// Observations used (pairs with both values present).
    pub n_points: usize,
}

/// Adjusted-R² tolerance of the best-fit rule.
const ADJ_R2_TOLERANCE: f64 = 1e-4;

/// Non-compartmental analysis of one profile (see the [module](self) docs
/// for the formulas). Rows may come in any order; pairs with a NaN are
/// skipped; concentrations below the limit of quantification should be
/// given as 0.
///
/// # Errors
/// Mismatched lengths, a negative or repeated time, a negative
/// concentration, a non-positive dose or infusion duration, a `LastPoints`
/// count below 3, an empty or reversed `TimeRange`, or no positive
/// concentration.
pub fn non_compartmental(times: &[f64], concentrations: &[f64], settings: &NcaSettings) -> Result<NcaResult> {
    if times.len() != concentrations.len() {
        return Err(invalid("times and concentrations must have the same length"));
    }
    if let Some(d) = settings.dose
        && !(d.is_finite() && d > 0.0)
    {
        return Err(invalid("the dose must be positive"));
    }
    if let Administration::IntravenousInfusion { duration } = settings.administration
        && !(duration.is_finite() && duration > 0.0)
    {
        return Err(invalid("the infusion duration must be positive"));
    }
    match settings.lambda_rule {
        LambdaZRule::LastPoints(k) if k < 3 => {
            return Err(invalid("the terminal phase needs at least 3 points"));
        }
        LambdaZRule::TimeRange(a, b) if !(a.is_finite() && b.is_finite() && a < b) => {
            return Err(invalid("the terminal-phase time range must have start < end"));
        }
        _ => {}
    }
    let mut obs: Vec<(f64, f64)> =
        times.iter().zip(concentrations).filter(|(t, c)| !t.is_nan() && !c.is_nan()).map(|(&t, &c)| (t, c)).collect();
    for &(t, c) in &obs {
        if !(t.is_finite() && t >= 0.0) {
            return Err(invalid("times must be finite and non-negative"));
        }
        if !(c.is_finite() && c >= 0.0) {
            return Err(invalid("concentrations must be finite and non-negative"));
        }
    }
    obs.sort_by(|a, b| a.0.total_cmp(&b.0));
    if obs.windows(2).any(|w| w[0].0 == w[1].0) {
        return Err(invalid("each time may appear only once in a profile"));
    }
    let n_points = obs.len();
    let last_pos =
        obs.iter().rposition(|p| p.1 > 0.0).ok_or_else(|| invalid("the profile has no positive concentration"))?;
    let (tlast, clast) = obs[last_pos];
    let mut imax = 0;
    for (i, p) in obs.iter().enumerate() {
        if p.1 > obs[imax].1 {
            imax = i;
        }
    }
    let (tmax, cmax) = obs[imax];

    // The curve integrated from time 0 to tlast.
    let mut curve: Vec<(f64, f64)> = obs[..=last_pos].to_vec();
    let mut c0 = None;
    match settings.administration {
        Administration::IntravenousBolus => {
            let value = match curve.first() {
                Some(&(t, c)) if t == 0.0 && c > 0.0 => c,
                _ => {
                    let pos: Vec<(f64, f64)> = curve.iter().copied().filter(|p| p.0 > 0.0 && p.1 > 0.0).collect();
                    match pos.as_slice() {
                        [(t1, c1), (t2, c2), ..] if c2 < c1 => c1 * (c1 / c2).powf(t1 / (t2 - t1)),
                        [(_, c1), ..] => *c1,
                        [] => unreachable!("clast is positive"),
                    }
                }
            };
            if curve[0].0 == 0.0 {
                curve[0].1 = value;
            } else {
                curve.insert(0, (0.0, value));
            }
            c0 = Some(value);
        }
        _ => {
            if curve[0].0 > 0.0 {
                curve.insert(0, (0.0, 0.0));
            }
        }
    }
    let (mut auc_last, mut aumc_last) = (0.0, 0.0);
    for w in curve.windows(2) {
        let (a, m) = segment(w[0], w[1], settings.method);
        auc_last += a;
        aumc_last += m;
    }

    // Terminal phase.
    let after_max: Vec<(f64, f64)> = obs[imax + 1..].iter().copied().filter(|p| p.1 > 0.0).collect();
    let lambda_z = match settings.lambda_rule {
        LambdaZRule::BestFit => {
            let fits: Vec<TerminalPhase> =
                (3..=after_max.len()).filter_map(|k| log_linear_fit(&after_max[after_max.len() - k..])).collect();
            let best = fits.iter().map(|f| f.adj_r_squared).fold(f64::NEG_INFINITY, f64::max);
            fits.into_iter().filter(|f| f.adj_r_squared >= best - ADJ_R2_TOLERANCE).max_by_key(|f| f.n_points)
        }
        LambdaZRule::LastPoints(k) => {
            if after_max.len() >= k {
                log_linear_fit(&after_max[after_max.len() - k..])
            } else {
                None
            }
        }
        LambdaZRule::TimeRange(a, b) => {
            let pts: Vec<(f64, f64)> = obs.iter().copied().filter(|p| p.1 > 0.0 && p.0 >= a && p.0 <= b).collect();
            if pts.len() >= 3 { log_linear_fit(&pts) } else { None }
        }
    };

    let lambda = lambda_z.map(|f| f.lambda);
    let auc_inf = lambda.map(|l| auc_last + clast / l);
    let aumc_inf = lambda.map(|l| aumc_last + tlast * clast / l + clast / (l * l));
    let infusion_shift = match settings.administration {
        Administration::IntravenousInfusion { duration } => duration / 2.0,
        _ => 0.0,
    };
    let mrt = auc_inf.zip(aumc_inf).map(|(a, m)| m / a - infusion_shift);
    let clearance = settings.dose.zip(auc_inf).map(|(d, a)| d / a);
    let volume_z = settings.dose.zip(auc_inf).zip(lambda).map(|((d, a), l)| d / (l * a));
    Ok(NcaResult {
        cmax,
        tmax,
        clast,
        tlast,
        c0,
        auc_last,
        aumc_last,
        lambda_z,
        half_life: lambda.map(|l| std::f64::consts::LN_2 / l),
        auc_inf,
        auc_pct_extrapolated: auc_inf.map(|a| 100.0 * (a - auc_last) / a),
        aumc_inf,
        mrt,
        clearance,
        volume_z,
        n_points,
    })
}

/// AUC and AUMC of one segment.
fn segment((t1, c1): (f64, f64), (t2, c2): (f64, f64), method: AucMethod) -> (f64, f64) {
    let dt = t2 - t1;
    if method == AucMethod::LinearUpLogDown && c2 < c1 && c2 > 0.0 {
        let k = (c1 / c2).ln() / dt;
        let auc = (c1 - c2) / k;
        (auc, (t1 * c1 - t2 * c2) / k + auc / k)
    } else {
        (dt * (c1 + c2) / 2.0, dt * (t1 * c1 + t2 * c2) / 2.0)
    }
}

/// Least-squares line of `ln C` on `t`; `None` unless `λz > 0` and `n ≥ 3`.
fn log_linear_fit(points: &[(f64, f64)]) -> Option<TerminalPhase> {
    let n = points.len();
    if n < 3 {
        return None;
    }
    let nf = n as f64;
    let tm = points.iter().map(|p| p.0).sum::<f64>() / nf;
    let ym = points.iter().map(|p| p.1.ln()).sum::<f64>() / nf;
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for &(t, c) in points {
        let (dx, dy) = (t - tm, c.ln() - ym);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    let slope = sxy / sxx;
    if !slope.is_finite() || slope >= 0.0 {
        return None;
    }
    let r_squared = sxy * sxy / (sxx * syy);
    let adj_r_squared = 1.0 - (1.0 - r_squared) * (nf - 1.0) / (nf - 2.0);
    Some(TerminalPhase {
        lambda: -slope,
        intercept: ym - slope * tm,
        n_points: n,
        r_squared,
        adj_r_squared,
        start: points[0].0,
        end: points[n - 1].0,
    })
}

/// Summary of one parameter across subjects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeometricSummary {
    /// Values present (finite).
    pub n: usize,
    /// Positive values, which the geometric statistics use.
    pub n_positive: usize,
    /// `exp(mean(ln x))` over the positive values.
    pub geometric_mean: Option<f64>,
    /// Geometric coefficient of variation `√(exp(s²) − 1)` (a fraction), `s²`
    /// the sample variance of `ln x`; needs two positive values.
    pub geometric_cv: Option<f64>,
    /// Arithmetic mean.
    pub arithmetic_mean: Option<f64>,
    /// Sample standard deviation (divisor `n − 1`); needs two values.
    pub sd: Option<f64>,
    /// Median.
    pub median: Option<f64>,
    /// Smallest value.
    pub min: Option<f64>,
    /// Largest value.
    pub max: Option<f64>,
}

/// Arithmetic and geometric summary of a pharmacokinetic parameter across
/// subjects (geometric statistics over the positive values only, as is usual
/// for log-normally distributed parameters such as AUC and Cmax).
/// Non-finite values are skipped; undefined statistics are `None`.
pub fn geometric_summary(values: &[f64]) -> GeometricSummary {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let mean_sd = |x: &[f64]| -> (Option<f64>, Option<f64>) {
        let k = x.len() as f64;
        if x.is_empty() {
            return (None, None);
        }
        let m = x.iter().sum::<f64>() / k;
        let sd = (x.len() > 1).then(|| (x.iter().map(|a| (a - m).powi(2)).sum::<f64>() / (k - 1.0)).sqrt());
        (Some(m), sd)
    };
    let (arithmetic_mean, sd) = mean_sd(&v);
    let logs: Vec<f64> = v.iter().filter(|&&x| x > 0.0).map(|x| x.ln()).collect();
    let (log_mean, log_sd) = mean_sd(&logs);
    let median = (n > 0).then(|| if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 });
    GeometricSummary {
        n,
        n_positive: logs.len(),
        geometric_mean: log_mean.map(f64::exp),
        geometric_cv: log_sd.map(|s| (s * s).exp_m1().sqrt()),
        arithmetic_mean,
        sd,
        median,
        min: v.first().copied(),
        max: v.last().copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() <= 1e-12 * b.abs().max(1.0), "{a} vs {b}");
    }

    #[test]
    fn keys_round_trip() {
        for m in AucMethod::ALL {
            assert_eq!(AucMethod::from_key(m.key()), Some(m));
        }
        assert_eq!(AucMethod::from_key("spline"), None);
    }

    #[test]
    fn too_few_terminal_points_give_no_lambda() {
        let r = non_compartmental(&[0.0, 1.0, 2.0, 4.0], &[0.0, 4.0, 2.0, 1.0], &NcaSettings::default()).unwrap();
        assert!(r.lambda_z.is_none() && r.auc_inf.is_none() && r.half_life.is_none() && r.mrt.is_none());
        close(r.cmax, 4.0);
        close(r.tmax, 1.0);
        // Linear 0–1, log thereafter: 2 + 2/ln 2 + 2/ln 2.
        close(r.auc_last, 2.0 + 4.0 / std::f64::consts::LN_2);
        let lin = NcaSettings { method: AucMethod::Linear, ..Default::default() };
        close(non_compartmental(&[0.0, 1.0, 2.0, 4.0], &[0.0, 4.0, 2.0, 1.0], &lin).unwrap().auc_last, 8.0);
    }

    #[test]
    fn flat_or_rising_terminal_phase_is_rejected() {
        let t = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let flat = non_compartmental(&t, &[0.0, 5.0, 2.0, 2.0, 2.0, 2.0], &NcaSettings::default()).unwrap();
        assert!(flat.lambda_z.is_none());
        let rising = non_compartmental(&t, &[0.0, 9.0, 1.0, 2.0, 3.0, 4.0], &NcaSettings::default()).unwrap();
        assert!(rising.lambda_z.is_none());
        let fixed = NcaSettings { lambda_rule: LambdaZRule::LastPoints(3), ..Default::default() };
        assert!(non_compartmental(&t, &[0.0, 9.0, 1.0, 2.0, 3.0, 4.0], &fixed).unwrap().lambda_z.is_none());
    }

    #[test]
    fn intravenous_bolus_back_extrapolates_c0() {
        // C = 10 e^{-t/2}: the log trapezoid is exact and C0 = 10.
        let t = [1.0, 2.0, 4.0, 8.0, 12.0];
        let c: Vec<f64> = t.iter().map(|x: &f64| 10.0 * (-0.5 * x).exp()).collect();
        let s =
            NcaSettings { administration: Administration::IntravenousBolus, dose: Some(100.0), ..Default::default() };
        let r = non_compartmental(&t, &c, &s).unwrap();
        close(r.c0.unwrap(), 10.0);
        let tp = r.lambda_z.unwrap();
        close(tp.lambda, 0.5);
        close(tp.intercept, 10f64.ln());
        assert_eq!(tp.n_points, 4); // Cmax (t = 1) excluded.
        close(r.auc_inf.unwrap(), 20.0);
        close(r.aumc_inf.unwrap(), 40.0);
        close(r.mrt.unwrap(), 2.0);
        close(r.clearance.unwrap(), 5.0);
        close(r.volume_z.unwrap(), 10.0);
        // A rising start: C0 is the first concentration.
        let r = non_compartmental(&[0.5, 1.0, 2.0], &[3.0, 4.0, 1.0], &s).unwrap();
        close(r.c0.unwrap(), 3.0);
        // An observed positive value at time 0 is kept.
        let r = non_compartmental(&[0.0, 1.0, 2.0], &[7.0, 4.0, 1.0], &s).unwrap();
        close(r.c0.unwrap(), 7.0);
    }

    #[test]
    fn infusion_mrt_subtracts_half_the_duration() {
        let t = [0.0, 1.0, 2.0, 4.0, 6.0, 8.0];
        let c = [0.0, 8.0, 6.0, 3.0, 1.5, 0.75];
        let ev = non_compartmental(&t, &c, &NcaSettings::default()).unwrap();
        let s =
            NcaSettings { administration: Administration::IntravenousInfusion { duration: 1.0 }, ..Default::default() };
        let inf = non_compartmental(&t, &c, &s).unwrap();
        close(inf.mrt.unwrap(), ev.mrt.unwrap() - 0.5);
    }

    #[test]
    fn input_errors_and_missing_values() {
        let s = NcaSettings::default();
        assert!(non_compartmental(&[0.0, 1.0], &[1.0], &s).is_err());
        assert!(non_compartmental(&[0.0, 1.0, 1.0], &[0.0, 1.0, 2.0], &s).is_err());
        assert!(non_compartmental(&[0.0, 1.0], &[0.0, 0.0], &s).is_err());
        assert!(non_compartmental(&[-1.0, 1.0], &[0.0, 1.0], &s).is_err());
        let bad = NcaSettings { lambda_rule: LambdaZRule::LastPoints(2), ..Default::default() };
        assert!(non_compartmental(&[0.0, 1.0], &[0.0, 1.0], &bad).is_err());
        // NaN pairs are skipped and rows sorted.
        let r = non_compartmental(&[2.0, f64::NAN, 1.0, 3.0], &[2.0, 5.0, 4.0, f64::NAN], &s).unwrap();
        assert_eq!(r.n_points, 2);
        close(r.tlast, 2.0);
    }

    #[test]
    fn geometric_summary_of_known_values() {
        let g = geometric_summary(&[1.0, 10.0, 100.0, f64::NAN]);
        assert_eq!(g.n, 3);
        close(g.geometric_mean.unwrap(), 10.0);
        let s = 10f64.ln();
        close(g.geometric_cv.unwrap(), ((s * s).exp() - 1.0).sqrt());
        close(g.median.unwrap(), 10.0);
        close(g.arithmetic_mean.unwrap(), 37.0);
        let e = geometric_summary(&[]);
        assert!(e.geometric_mean.is_none() && e.median.is_none() && e.sd.is_none());
        let z = geometric_summary(&[0.0, 2.0]);
        assert_eq!(z.n_positive, 1);
        assert!(z.geometric_cv.is_none());
    }
}
