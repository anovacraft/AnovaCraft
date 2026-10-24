//! Derived quantities and acceptance checks for assay curves.
//!
//! The assay curve templates (receptor binding, enzyme kinetics,
//! pharmacological dose–response, immunoassay standard curves, growth and
//! survival) are ordinary nonlinear regressions ([`super::fit_nonlinear`]);
//! this module holds what they report beyond the parameter estimates.
//!
//! # Derived quantities
//! A quantity `q = g(θ)` of the fitted parameters has the delta-method
//! standard error `se(q̂) = √(∇gᵀ Σ ∇g)`, `Σ` the covariance of the estimates
//! and `∇g` the gradient at `θ̂` (central differences), and the Wald interval
//! `q̂ ± t·se` ([`delta_method`]; Seber & Wild, *Nonlinear Regression*, Wiley
//! 1989, §5.1). When `q` is a monotone function of a single parameter its
//! interval is that parameter's interval mapped through `g`
//! ([`map_interval`]), which keeps the asymmetry of a profile interval and
//! never leaves the quantity's range (for example a dissociation constant
//! from a log concentration stays positive). The standard formulas:
//!
//! * inhibition constant from a competition curve (Cheng & Prusoff,
//!   *Biochem. Pharmacol.* 22, 1973): `Ki = IC50 / (1 + [L]/Kd)`, `[L]` the
//!   labelled ligand's concentration and `Kd` its dissociation constant
//!   ([`cheng_prusoff_ki`]);
//! * the concentration giving `F`% of the maximal effect of a logistic curve
//!   with slope `h`: `EC_F = EC50·(F/(100 − F))^{1/h}` ([`ec_f`]);
//! * half-life of a first-order process with rate constant `k`:
//!   `t½ = ln 2 / k` ([`half_life`]);
//! * equilibrium dissociation constant from binding kinetics:
//!   `Kd = koff / kon` ([`kinetic_kd`]);
//! * relative potency of a test preparation against a standard from
//!   parallel log-dose curves (Finney, *Statistical Method in Biological
//!   Assay*, 3rd ed., Griffin 1978, ch. 4): `ρ = 10^(log EC50_S − log EC50_T)`,
//!   its interval `10^` of the t interval of the difference
//!   ([`relative_potency`]).
//!
//! # Standard curves (ligand-binding assays)
//! * **Back-calculation:** each calibration standard's response is read off
//!   the fitted curve and compared with its nominal concentration,
//!   `recovery = 100·x̂/x`. ICH M10 *Bioanalytical Method Validation and Study
//!   Sample Analysis* (2022), §7.2.1 (ligand-binding assays): back-calculated
//!   standards within ±20% of nominal (±25% at the lowest and highest
//!   standards, the LLOQ and ULOQ), at least 75% of them, and at least six
//!   concentration levels, meeting it ([`standard_curve_acceptance`]; the
//!   limits are settings, so the chromatographic ±15%/±20% can be used).
//! * **Precision profile:** the coefficient of variation of a concentration
//!   read off the curve from one response, `CV(c) = 100·sd(x̂)/c` with
//!   `sd(x̂) = √(s²/w(c) + se(f(c))²) / |f′(c)|` (Ekins, "The precision
//!   profile: its use in assay design, assessment and quality control", in
//!   *Immunoassays for Clinical Chemistry*, 1983; Findlay & Dillard,
//!   *AAPS J.* 9, 2007). The working (quantitation) range is the widest
//!   stretch of concentrations, within the standards, where `CV ≤` a limit
//!   (20% by default) ([`quantitation_range`]).
//!
//! # Failure modes
//! A quantity that cannot be computed at the estimates (a division by zero,
//! a logarithm of a negative value) or whose gradient cannot be formed is
//! returned as NaN with a NaN standard error; callers show it as missing.

use stat_core::Matrix;

/// A quantity computed from the estimates, with its uncertainty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DerivedValue {
    /// Value at the estimates.
    pub estimate: f64,
    /// Delta-method standard error (NaN when it cannot be computed).
    pub std_error: f64,
    /// Confidence interval.
    pub interval: (f64, f64),
}

impl DerivedValue {
    /// A value with no uncertainty attached.
    #[must_use]
    pub fn missing() -> Self {
        Self { estimate: f64::NAN, std_error: f64::NAN, interval: (f64::NAN, f64::NAN) }
    }
}

/// Delta-method standard error and Wald interval `q̂ ± t·se` of
/// `q = g(θ)` at `theta` with covariance `covariance` (`P × P`; NaN rows and
/// columns for parameters held at a bound are treated as zero variance).
/// The gradient is taken by central differences with step
/// `ε^{1/3}·max(|θⱼ|, 10⁻⁸)`.
#[must_use]
pub fn delta_method(g: &dyn Fn(&[f64]) -> f64, theta: &[f64], covariance: &Matrix, t_critical: f64) -> DerivedValue {
    let q = g(theta);
    if !q.is_finite() {
        return DerivedValue::missing();
    }
    let p = theta.len();
    let mut grad = vec![0.0; p];
    let mut point = theta.to_vec();
    for j in 0..p {
        let h = f64::EPSILON.cbrt() * theta[j].abs().max(1e-8);
        point[j] = theta[j] + h;
        let up = g(&point);
        point[j] = theta[j] - h;
        let down = g(&point);
        point[j] = theta[j];
        if !(up.is_finite() && down.is_finite()) {
            return DerivedValue { estimate: q, std_error: f64::NAN, interval: (f64::NAN, f64::NAN) };
        }
        grad[j] = (up - down) / (2.0 * h);
    }
    let cov = |i: usize, j: usize| {
        let v = covariance[(i, j)];
        if v.is_finite() { v } else { 0.0 }
    };
    let mut var = 0.0;
    for i in 0..p {
        if grad[i] == 0.0 {
            continue;
        }
        for j in 0..p {
            var += grad[i] * cov(i, j) * grad[j];
        }
    }
    let se = var.max(0.0).sqrt();
    DerivedValue { estimate: q, std_error: se, interval: (q - t_critical * se, q + t_critical * se) }
}

/// The interval of a monotone function `g` of one parameter: `g` applied to
/// each limit, ordered (an open limit, NaN, stays NaN).
#[must_use]
pub fn map_interval(g: &dyn Fn(f64) -> f64, interval: (f64, f64)) -> (f64, f64) {
    let (a, b) = (g(interval.0), g(interval.1));
    if a.is_nan() || b.is_nan() {
        return (a, b);
    }
    if a <= b { (a, b) } else { (b, a) }
}

/// Inhibition constant `Ki = IC50 / (1 + [L]/Kd)` (Cheng & Prusoff 1973),
/// for a competitor's `ic50` against a labelled ligand at concentration
/// `ligand` with dissociation constant `kd` (same units). NaN when
/// `kd ≤ 0` or `ligand < 0`.
#[must_use]
pub fn cheng_prusoff_ki(ic50: f64, ligand: f64, kd: f64) -> f64 {
    if kd.is_nan() || kd <= 0.0 || ligand.is_nan() || ligand < 0.0 {
        return f64::NAN;
    }
    ic50 / (1.0 + ligand / kd)
}

/// `EC_F = EC50·(F/(100 − F))^{1/h}`: the concentration giving `f_percent`
/// of the way from the bottom to the top of a logistic curve with Hill
/// slope `hill` (`0 < F < 100`, `h ≠ 0`; NaN otherwise).
#[must_use]
pub fn ec_f(ec50: f64, hill: f64, f_percent: f64) -> f64 {
    if !(f_percent > 0.0 && f_percent < 100.0) || hill == 0.0 {
        return f64::NAN;
    }
    ec50 * (f_percent / (100.0 - f_percent)).powf(1.0 / hill)
}

/// Half-life `ln 2 / k` of a first-order rate constant `k` (NaN for `k ≤ 0`).
#[must_use]
pub fn half_life(k: f64) -> f64 {
    if k > 0.0 { std::f64::consts::LN_2 / k } else { f64::NAN }
}

/// Equilibrium dissociation constant `koff / kon` (NaN for `kon ≤ 0`).
#[must_use]
pub fn kinetic_kd(kon: f64, koff: f64) -> f64 {
    if kon > 0.0 { koff / kon } else { f64::NAN }
}

/// Relative potency `ρ = 10^(d)`, `d = log EC50_standard − log EC50_test`,
/// with its interval `10^(d ± t·se(d))`,
/// `se(d)² = Var_S + Var_T − 2·Cov_ST` (Finney 1978, ch. 4). `se` is the
/// standard error of the difference already formed by the caller.
#[must_use]
pub fn relative_potency(log_ec50_standard: f64, log_ec50_test: f64, se: f64, t_critical: f64) -> DerivedValue {
    let d = log_ec50_standard - log_ec50_test;
    DerivedValue {
        estimate: 10f64.powf(d),
        std_error: f64::NAN,
        interval: (10f64.powf(d - t_critical * se), 10f64.powf(d + t_critical * se)),
    }
}

/// Limits of [`standard_curve_acceptance`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceptanceLimits {
    /// Allowed deviation of back-calculated standards, percent (20 for
    /// ligand-binding assays under ICH M10).
    pub tolerance_percent: f64,
    /// Allowed deviation at the lowest and highest standard, percent (25).
    pub end_tolerance_percent: f64,
    /// Fraction of standards that must pass (0.75).
    pub min_fraction: f64,
    /// Fewest concentration levels that must pass (6).
    pub min_levels: usize,
}

impl Default for AcceptanceLimits {
    /// ICH M10 (2022) §7.2.1, ligand-binding assays.
    fn default() -> Self {
        Self { tolerance_percent: 20.0, end_tolerance_percent: 25.0, min_fraction: 0.75, min_levels: 6 }
    }
}

/// One calibration standard read back off the curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackCalculated {
    /// Nominal concentration.
    pub nominal: f64,
    /// Concentration read off the curve (`None`: response outside the curve).
    pub back_calculated: Option<f64>,
    /// `100·x̂/x` (NaN without a back-calculated value).
    pub recovery_percent: f64,
    /// Allowed deviation for this standard, percent.
    pub limit_percent: f64,
    /// Whether `|recovery − 100| ≤ limit`.
    pub passes: bool,
}

/// Verdict of [`standard_curve_acceptance`].
#[derive(Debug, Clone, PartialEq)]
pub struct Acceptance {
    /// Each standard in input order.
    pub standards: Vec<BackCalculated>,
    /// Number of standards that pass, of the total.
    pub passing: (usize, usize),
    /// Concentration levels with at least one passing standard, of the total.
    pub levels: (usize, usize),
    /// Whether the curve meets every criterion.
    pub accepted: bool,
    /// The criteria that failed, in words.
    pub failures: Vec<String>,
}

/// Back-calculated standards checked against `limits`. `nominal` are the
/// standards' nominal concentrations (positive; zero-concentration blanks
/// should be left out) and `back` their concentrations read off the curve.
/// The lowest and highest nominal concentrations get the end tolerance.
#[must_use]
pub fn standard_curve_acceptance(nominal: &[f64], back: &[Option<f64>], limits: AcceptanceLimits) -> Acceptance {
    let positive: Vec<f64> = nominal.iter().copied().filter(|v| *v > 0.0 && v.is_finite()).collect();
    let lo = positive.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = positive.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs());
    let standards: Vec<BackCalculated> = nominal
        .iter()
        .zip(back)
        .map(|(&x, &b)| {
            let end = close(x, lo) || close(x, hi);
            let limit = if end { limits.end_tolerance_percent } else { limits.tolerance_percent };
            let recovery = b.map_or(f64::NAN, |b| 100.0 * b / x);
            BackCalculated {
                nominal: x,
                back_calculated: b,
                recovery_percent: recovery,
                limit_percent: limit,
                passes: recovery.is_finite() && (recovery - 100.0).abs() <= limit + 1e-9,
            }
        })
        .collect();
    let n = standards.len();
    let n_pass = standards.iter().filter(|s| s.passes).count();
    let mut levels: Vec<(f64, bool)> = Vec::new();
    for s in &standards {
        match levels.iter_mut().find(|(x, _)| close(*x, s.nominal)) {
            Some(level) => level.1 |= s.passes,
            None => levels.push((s.nominal, s.passes)),
        }
    }
    let levels_pass = levels.iter().filter(|l| l.1).count();
    let mut failures = Vec::new();
    if n == 0 || (n_pass as f64) < limits.min_fraction * n as f64 - 1e-9 {
        failures.push(format!(
            "{n_pass} of {n} standards are within limits; at least {}% must be",
            (100.0 * limits.min_fraction).round()
        ));
    }
    if levels_pass < limits.min_levels {
        failures.push(format!("{levels_pass} concentration levels pass; at least {} must", limits.min_levels));
    }
    Acceptance {
        standards,
        passing: (n_pass, n),
        levels: (levels_pass, levels.len()),
        accepted: failures.is_empty(),
        failures,
    }
}

/// The widest stretch of a precision profile where `CV ≤ limit`.
/// `profile` is `(concentration, CV %)` in increasing concentration; a
/// crossing between two points is placed by linear interpolation in
/// `log c`. Returns the lower and upper limits of quantitation, or `None`
/// when no point meets the limit.
#[must_use]
pub fn quantitation_range(profile: &[(f64, f64)], limit: f64) -> Option<(f64, f64)> {
    let ok = |cv: f64| cv.is_finite() && cv <= limit;
    let cross = |a: (f64, f64), b: (f64, f64)| -> f64 {
        if !(a.1.is_finite() && b.1.is_finite()) || a.1 == b.1 || a.0 <= 0.0 || b.0 <= 0.0 {
            return if ok(a.1) { a.0 } else { b.0 };
        }
        let u = ((limit - a.1) / (b.1 - a.1)).clamp(0.0, 1.0);
        (a.0.ln() + u * (b.0.ln() - a.0.ln())).exp()
    };
    let mut best: Option<(f64, f64)> = None;
    let mut i = 0;
    while i < profile.len() {
        if !ok(profile[i].1) {
            i += 1;
            continue;
        }
        let start = i;
        while i + 1 < profile.len() && ok(profile[i + 1].1) {
            i += 1;
        }
        let lo = if start > 0 { cross(profile[start - 1], profile[start]) } else { profile[start].0 };
        let hi = if i + 1 < profile.len() { cross(profile[i], profile[i + 1]) } else { profile[i].0 };
        let wider = best.is_none_or(|(a, b)| (hi / lo).ln() > (b / a).ln());
        if wider {
            best = Some((lo, hi));
        }
        i += 1;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_formulas() {
        // Cheng & Prusoff: [L] = Kd halves the IC50.
        assert!((cheng_prusoff_ki(10.0, 2.0, 2.0) - 5.0).abs() < 1e-15);
        assert!((cheng_prusoff_ki(3e-8, 1e-9, 5e-10) - 1e-8).abs() < 1e-22);
        assert!(cheng_prusoff_ki(1.0, 1.0, 0.0).is_nan());
        // EC50 at F = 50; EC90 of a Hill-1 curve is 9·EC50; EC10 = EC50/9.
        assert!((ec_f(2.0, 1.0, 50.0) - 2.0).abs() < 1e-15);
        assert!((ec_f(2.0, 1.0, 90.0) - 18.0).abs() < 1e-13);
        assert!((ec_f(2.0, 2.0, 90.0) - 6.0).abs() < 1e-13);
        assert!((ec_f(2.0, 1.0, 10.0) - 2.0 / 9.0).abs() < 1e-15);
        assert!(ec_f(1.0, 1.0, 100.0).is_nan());
        assert!((half_life(std::f64::consts::LN_2) - 1.0).abs() < 1e-15);
        assert!(half_life(0.0).is_nan());
        assert!((kinetic_kd(1e6, 1e-3) - 1e-9).abs() < 1e-24);
    }

    #[test]
    fn delta_method_matches_linear_and_known_cases() {
        // Linear g: exact variance aᵀΣa.
        let cov = Matrix::from_rows(&[vec![4.0, 1.0], vec![1.0, 9.0]]).unwrap();
        let d = delta_method(&|t: &[f64]| 2.0 * t[0] - t[1], &[1.0, 3.0], &cov, 2.0);
        let var: f64 = 4.0 * 4.0 - 2.0 * 2.0 * 1.0 + 9.0;
        assert!((d.estimate + 1.0).abs() < 1e-15);
        assert!((d.std_error - var.sqrt()).abs() < 1e-8);
        assert!((d.interval.1 - (-1.0 + 2.0 * var.sqrt())).abs() < 1e-7);
        // 10^x: se = ln10·10^x·sd(x).
        let cov = Matrix::from_rows(&[vec![0.01]]).unwrap();
        let d = delta_method(&|t: &[f64]| 10f64.powf(t[0]), &[-2.0], &cov, 1.0);
        assert!((d.std_error - std::f64::consts::LN_10 * 0.01 * 0.1).abs() < 1e-10);
        // A parameter at a bound (NaN covariance) contributes nothing.
        let cov = Matrix::from_rows(&[vec![1.0, f64::NAN], vec![f64::NAN, f64::NAN]]).unwrap();
        let d = delta_method(&|t: &[f64]| t[0] + t[1], &[1.0, 1.0], &cov, 1.0);
        assert!((d.std_error - 1.0).abs() < 1e-9);
        let d = delta_method(&|t: &[f64]| t[0].ln(), &[-1.0], &Matrix::from_rows(&[vec![1.0]]).unwrap(), 1.0);
        assert!(d.estimate.is_nan());
    }

    #[test]
    fn mapped_intervals_and_potency() {
        let (a, b) = map_interval(&|x| 10f64.powf(-x), (1.0, 2.0));
        assert!((a - 0.01).abs() < 1e-15 && (b - 0.1).abs() < 1e-15);
        let rp = relative_potency(-6.0, -6.30103, 0.05, 2.0);
        assert!((rp.estimate - 2.0).abs() < 1e-5);
        assert!((rp.interval.0 - 2.0 * 10f64.powf(-0.1)).abs() < 1e-4);
        assert!((rp.interval.1 - 2.0 * 10f64.powf(0.1)).abs() < 1e-4);
    }

    #[test]
    fn acceptance_rules() {
        // Eight levels in duplicate; one standard at 130% in the middle and
        // one at 124% at the lowest level (end tolerance 25%).
        let nominal: Vec<f64> = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0].iter().flat_map(|&x| [x, x]).collect();
        let mut back: Vec<Option<f64>> = nominal.iter().map(|&x| Some(x * 1.05)).collect();
        back[0] = Some(1.24);
        back[6] = Some(4.0 * 1.3);
        let a = standard_curve_acceptance(&nominal, &back, AcceptanceLimits::default());
        assert!(a.standards[0].passes && (a.standards[0].limit_percent - 25.0).abs() < 1e-12);
        assert!(!a.standards[6].passes && (a.standards[6].limit_percent - 20.0).abs() < 1e-12);
        assert_eq!(a.passing, (15, 16));
        assert_eq!(a.levels, (8, 8));
        assert!(a.accepted && a.failures.is_empty());
        // Too many failures: five of sixteen out (69% pass).
        for b in back.iter_mut().skip(1).step_by(3) {
            *b = None;
        }
        let a = standard_curve_acceptance(&nominal, &back, AcceptanceLimits::default());
        assert!(!a.accepted);
        assert!(a.failures[0].contains("standards are within limits"));
        // Too few levels.
        let a = standard_curve_acceptance(
            &[1.0, 2.0, 3.0],
            &[Some(1.0), Some(2.0), Some(3.0)],
            AcceptanceLimits::default(),
        );
        assert!(!a.accepted && a.failures.iter().any(|f| f.contains("levels")));
    }

    #[test]
    fn quantitation_range_crossings() {
        // U-shaped CV profile: 40, 20, 10, 10, 30 at 1, 10, 100, 1000, 10000.
        let p = [(1.0, 40.0), (10.0, 20.0), (100.0, 10.0), (1000.0, 10.0), (10000.0, 30.0)];
        let (lo, hi) = quantitation_range(&p, 20.0).unwrap();
        assert!((lo - 10.0).abs() < 1e-9);
        // Crossing halfway (CV 10 → 30 passes 20 at u = 1/2) in log c: 10^3.5.
        assert!((hi - 10f64.powf(3.5)).abs() < 1e-6);
        // Wider of two stretches, and none at all.
        let p = [(1.0, 5.0), (2.0, 50.0), (4.0, 5.0), (8.0, 5.0), (16.0, 5.0)];
        let (lo, hi) = quantitation_range(&p, 20.0).unwrap();
        assert!(lo > 2.0 && (hi - 16.0).abs() < 1e-12);
        assert!(quantitation_range(&[(1.0, 50.0), (2.0, 60.0)], 20.0).is_none());
    }
}
