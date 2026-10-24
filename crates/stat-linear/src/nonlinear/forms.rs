//! A library of common one-predictor nonlinear model forms, each with
//! analytic derivatives and automatic starting values.
//!
//! | Form | `f(x; θ)` |
//! |---|---|
//! | Exponential | `Scale·exp(Rate·x)` |
//! | Exponential with asymptote | `Asymptote + Scale·exp(Rate·x)` |
//! | Logistic (3 parameters) | `Asymptote / (1 + exp(−Rate·(x − Midpoint)))` |
//! | Logistic (4 parameters) | `Lower + (Upper − Lower) / (1 + exp(−Rate·(x − Midpoint)))` |
//! | Gompertz | `Asymptote·exp(−exp(−Rate·(x − Inflection)))` |
//! | Michaelis–Menten | `Vmax·x / (Km + x)` |
//! | Power | `Scale·x^Exponent` |
//! | Rational (linear / linear) | `(a + b·x) / (1 + c·x)` |
//! | Straight line | `Intercept + Slope·x` |
//! | Dose–response, log dose | `Bottom + (Top − Bottom) / (1 + 10^((LogEC50 − x)·HillSlope))` |
//! | Dose–response, dose | `Bottom + (Top − Bottom) / (1 + (EC50 / x)^HillSlope)`, `x ≥ 0` |
//! | Asymmetric logistic (5 parameters) | `Bottom + (Top − Bottom) / (1 + 10^((LogC − x)·HillSlope))^Asymmetry` |
//! | Weibull growth | `Asymptote − (Asymptote − Initial)·exp(−(Rate·x)^Shape)`, `x ≥ 0` |
//! | Beta growth | `Ymax·(1 + (End − m)/(End − Peak))·(m/End)^(End/(End − Peak))`, `m = min(x, End)`, `x ≥ 0` |
//! | Segmented line | `Intercept + Slope1·x + (Slope2 − Slope1)·max(x − Breakpoint, 0)` |
//! | Sum of two exponentials | `Plateau + Span1·exp(−Rate1·x) + Span2·exp(−Rate2·x)` |
//! | Gaussian peak | `Baseline + Height·exp(−(x − Center)² / (2·Width²))` |
//! | Lorentzian peak | `Baseline + Height / (1 + ((x − Center) / HalfWidth)²)` |
//!
//! The dose–response forms are the four-parameter Hill (log-logistic) curve,
//! on the log₁₀ dose scale or the dose scale; a negative `HillSlope` gives a
//! falling (inhibition) curve, whose midpoint is then an IC50. The
//! asymmetric logistic is the five-parameter logistic of Gottschalk & Dunn
//! ("The five-parameter logistic: a characterization and comparison with the
//! four-parameter logistic", *Anal. Biochem.* 343, 2005) on the log-dose
//! scale; `Asymmetry = 1` gives the four-parameter curve. Weibull growth is
//! from Ratkowsky (*Handbook of Nonlinear Regression Models*, 1990); the beta
//! growth function, rising from 0 at `x = 0` to its maximum `Ymax` at
//! `x = End` with its fastest growth at `x = Peak` (`0 ≤ Peak < End`) and
//! constant after `End`, is from Yin, Goudriaan, Lantinga, Vos & Spiertz ("A
//! flexible sigmoid function of determinate growth", *Annals of Botany* 91,
//! 2003). The segmented line is continuous at its breakpoint (Muggeo,
//! "Estimating regression models with unknown break-points", *Stat. Med.* 22,
//! 2003); its derivative in `Breakpoint` is `−(Slope2 − Slope1)` for
//! `x > Breakpoint` and 0 otherwise.
//!
//! # Starting values
//! Every form is conditionally linear: given its nonlinear parameters (a rate,
//! midpoint, Km, exponent or `c`) the others enter linearly and follow from a
//! small linear least-squares problem. The starting values are the best point
//! of a grid over the nonlinear parameters, scaled to the data (rates
//! `±k / range(x)` for `k` on a log grid from 0.01 to 30, midpoints at
//! quantiles of `x`, Km from 0.01 to 10 times the largest `x`, exponents from
//! −3 to 3; Hill slopes `±rate / ln 10` for the log-dose forms, ±0.2 to 8 on
//! the dose scale with EC50 on a geometric grid over the positive `x`;
//! asymmetries 0.2 to 5; Weibull shapes 0.3 to 8; beta-growth ends at upper
//! quantiles of `x` and up to three times its maximum, with peaks at 0 to 95%
//! of the end; breakpoints between each pair of adjacent distinct `x` values
//! that leave two values on each side; rate pairs `Rate1 > Rate2 > 0`; peak
//! centers at quantiles of `x` and widths 0.01 to 1 times its range), each
//! with its conditionally optimal linear parameters — the
//! idea of separable (variable projection) least squares (Golub & Pereyra,
//! "The differentiation of pseudo-inverses and nonlinear least squares
//! problems whose variables separate", *SIAM J. Numer. Anal.* 10, 1973),
//! used here only to start the full fit.

use super::ModelFunction;
use stat_core::{Matrix, QrDecomposition, Result, StatError};
use std::f64::consts::LN_10;

/// `ln(1 + e^z)` without overflow.
fn softplus(z: f64) -> f64 {
    if z > 0.0 { z + (-z).exp().ln_1p() } else { z.exp().ln_1p() }
}

/// One of the library forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurveForm {
    /// `Scale·exp(Rate·x)`: exponential growth (`Rate > 0`) or decay.
    Exponential,
    /// `Asymptote + Scale·exp(Rate·x)`: decay to, or rise towards, a level.
    ExponentialAsymptote,
    /// `Asymptote / (1 + exp(−Rate·(x − Midpoint)))`.
    Logistic3,
    /// `Lower + (Upper − Lower) / (1 + exp(−Rate·(x − Midpoint)))`.
    Logistic4,
    /// `Asymptote·exp(−exp(−Rate·(x − Inflection)))`.
    Gompertz,
    /// `Vmax·x / (Km + x)`.
    MichaelisMenten,
    /// `Scale·x^Exponent` (needs `x ≥ 0`).
    Power,
    /// `(a + b·x) / (1 + c·x)`.
    Rational,
    /// `Intercept + Slope·x`.
    StraightLine,
    /// `Bottom + (Top − Bottom) / (1 + 10^((LogEC50 − x)·HillSlope))`, with
    /// `x` the log₁₀ dose; a negative `HillSlope` gives a falling curve.
    DoseResponseLog,
    /// `Bottom + (Top − Bottom) / (1 + (EC50 / x)^HillSlope)` (needs `x ≥ 0`;
    /// at `x = 0` the limit, `Bottom` for `HillSlope > 0`, `Top` for `< 0`).
    DoseResponse,
    /// `Bottom + (Top − Bottom) / (1 + 10^((LogC − x)·HillSlope))^Asymmetry`
    /// (five-parameter logistic, log-dose scale).
    AsymmetricLogistic5,
    /// `Asymptote − (Asymptote − Initial)·exp(−(Rate·x)^Shape)` (needs `x ≥ 0`).
    WeibullGrowth,
    /// `Ymax·(1 + (End − m)/(End − Peak))·(m/End)^(End/(End − Peak))` with
    /// `m = min(x, End)`: `Ymax` beyond `End` (needs `x ≥ 0`, `0 ≤ Peak < End`).
    BetaGrowth,
    /// `Intercept + Slope1·x + (Slope2 − Slope1)·max(x − Breakpoint, 0)`.
    Segmented,
    /// `Plateau + Span1·exp(−Rate1·x) + Span2·exp(−Rate2·x)`.
    TwoPhaseExponential,
    /// `Baseline + Height·exp(−(x − Center)² / (2·Width²))`.
    GaussianPeak,
    /// `Baseline + Height / (1 + ((x − Center) / HalfWidth)²)`.
    LorentzianPeak,
}

impl CurveForm {
    /// Every form, in menu order.
    pub const ALL: [Self; 18] = [
        Self::Exponential,
        Self::ExponentialAsymptote,
        Self::Logistic3,
        Self::Logistic4,
        Self::Gompertz,
        Self::MichaelisMenten,
        Self::Power,
        Self::Rational,
        Self::StraightLine,
        Self::DoseResponseLog,
        Self::DoseResponse,
        Self::AsymmetricLogistic5,
        Self::WeibullGrowth,
        Self::BetaGrowth,
        Self::Segmented,
        Self::TwoPhaseExponential,
        Self::GaussianPeak,
        Self::LorentzianPeak,
    ];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Exponential => "Exponential growth or decay",
            Self::ExponentialAsymptote => "Exponential with asymptote",
            Self::Logistic3 => "Logistic (3 parameters)",
            Self::Logistic4 => "Logistic (4 parameters)",
            Self::Gompertz => "Gompertz",
            Self::MichaelisMenten => "Michaelis–Menten",
            Self::Power => "Power",
            Self::Rational => "Rational (linear / linear)",
            Self::StraightLine => "Straight line",
            Self::DoseResponseLog => "Dose–response, log dose (4 parameters)",
            Self::DoseResponse => "Dose–response, dose (4 parameters)",
            Self::AsymmetricLogistic5 => "Asymmetric logistic, log dose (5 parameters)",
            Self::WeibullGrowth => "Weibull growth",
            Self::BetaGrowth => "Beta growth",
            Self::Segmented => "Segmented line (one breakpoint)",
            Self::TwoPhaseExponential => "Sum of two exponentials",
            Self::GaussianPeak => "Gaussian peak",
            Self::LorentzianPeak => "Lorentzian peak",
        }
    }

    /// Stable identifier (project files and scripts).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Exponential => "exponential",
            Self::ExponentialAsymptote => "exponential_asymptote",
            Self::Logistic3 => "logistic3",
            Self::Logistic4 => "logistic4",
            Self::Gompertz => "gompertz",
            Self::MichaelisMenten => "michaelis_menten",
            Self::Power => "power",
            Self::Rational => "rational",
            Self::StraightLine => "line",
            Self::DoseResponseLog => "dose_response_log",
            Self::DoseResponse => "dose_response",
            Self::AsymmetricLogistic5 => "logistic5",
            Self::WeibullGrowth => "weibull_growth",
            Self::BetaGrowth => "beta_growth",
            Self::Segmented => "segmented",
            Self::TwoPhaseExponential => "two_exponentials",
            Self::GaussianPeak => "gaussian_peak",
            Self::LorentzianPeak => "lorentzian_peak",
        }
    }

    /// The form with identifier `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == key)
    }

    /// Parameter names, in model order.
    #[must_use]
    pub fn parameter_names(self) -> &'static [&'static str] {
        match self {
            Self::Exponential => &["Scale", "Rate"],
            Self::ExponentialAsymptote => &["Asymptote", "Scale", "Rate"],
            Self::Logistic3 => &["Asymptote", "Rate", "Midpoint"],
            Self::Logistic4 => &["Lower", "Upper", "Rate", "Midpoint"],
            Self::Gompertz => &["Asymptote", "Rate", "Inflection"],
            Self::MichaelisMenten => &["Vmax", "Km"],
            Self::Power => &["Scale", "Exponent"],
            Self::Rational => &["a", "b", "c"],
            Self::StraightLine => &["Intercept", "Slope"],
            Self::DoseResponseLog => &["Bottom", "Top", "LogEC50", "HillSlope"],
            Self::DoseResponse => &["Bottom", "Top", "EC50", "HillSlope"],
            Self::AsymmetricLogistic5 => &["Bottom", "Top", "LogC", "HillSlope", "Asymmetry"],
            Self::WeibullGrowth => &["Asymptote", "Initial", "Rate", "Shape"],
            Self::BetaGrowth => &["Ymax", "End", "Peak"],
            Self::Segmented => &["Intercept", "Slope1", "Slope2", "Breakpoint"],
            Self::TwoPhaseExponential => &["Plateau", "Span1", "Rate1", "Span2", "Rate2"],
            Self::GaussianPeak => &["Baseline", "Height", "Center", "Width"],
            Self::LorentzianPeak => &["Baseline", "Height", "Center", "HalfWidth"],
        }
    }

    /// The model as a formula in `x` (text written for the predictor, e.g. a
    /// column reference) with parameters `p` (text for each: names, or
    /// numbers written with every digit).
    #[must_use]
    pub fn formula(self, x: &str, p: &[String]) -> String {
        let q = |i: usize| -> String {
            let s = &p[i];
            if s.starts_with('-') { format!("({s})") } else { s.clone() }
        };
        match self {
            Self::Exponential => format!("{} * exp({} * {x})", q(0), q(1)),
            Self::ExponentialAsymptote => format!("{} + {} * exp({} * {x})", q(0), q(1), q(2)),
            Self::Logistic3 => format!("{} / (1 + exp(-{} * ({x} - {})))", q(0), q(1), q(2)),
            Self::Logistic4 => {
                format!("{} + ({} - {}) / (1 + exp(-{} * ({x} - {})))", q(0), q(1), q(0), q(2), q(3))
            }
            Self::Gompertz => format!("{} * exp(-exp(-{} * ({x} - {})))", q(0), q(1), q(2)),
            Self::MichaelisMenten => format!("{} * {x} / ({} + {x})", q(0), q(1)),
            Self::Power => format!("{} * {x} ** {}", q(0), q(1)),
            Self::Rational => format!("({} + {} * {x}) / (1 + {} * {x})", q(0), q(1), q(2)),
            Self::StraightLine => format!("{} + {} * {x}", q(0), q(1)),
            Self::DoseResponseLog => {
                format!("{} + ({} - {}) / (1.0 + 10.0 ** (({} - {x}) * {}))", q(0), q(1), q(0), q(2), q(3))
            }
            Self::DoseResponse => format!("{} + ({} - {}) / (1.0 + ({} / {x}) ** {})", q(0), q(1), q(0), q(2), q(3)),
            Self::AsymmetricLogistic5 => {
                format!("{} + ({} - {}) / (1.0 + 10.0 ** (({} - {x}) * {})) ** {}", q(0), q(1), q(0), q(2), q(3), q(4))
            }
            Self::WeibullGrowth => {
                format!("{} - ({} - {}) * exp(-(({} * {x}) ** {}))", q(0), q(0), q(1), q(2), q(3))
            }
            Self::BetaGrowth => {
                let (ymax, end, peak) = (q(0), q(1), q(2));
                format!(
                    "{ymax} * (1.0 + ({end} - min({x}, {end})) / ({end} - {peak})) * (min({x}, {end}) / {end}) ** ({end} / ({end} - {peak}))"
                )
            }
            Self::Segmented => format!("{} + {} * {x} + ({} - {}) * max({x} - {}, 0.0)", q(0), q(1), q(2), q(1), q(3)),
            Self::TwoPhaseExponential => {
                format!("{} + {} * exp(-{} * {x}) + {} * exp(-{} * {x})", q(0), q(1), q(2), q(3), q(4))
            }
            Self::GaussianPeak => {
                format!("{} + {} * exp(-(({x} - {}) ** 2.0) / (2.0 * {} ** 2.0))", q(0), q(1), q(2), q(3))
            }
            Self::LorentzianPeak => format!("{} + {} / (1.0 + (({x} - {}) / {}) ** 2.0)", q(0), q(1), q(2), q(3)),
        }
    }

    /// `f(x; θ)`.
    #[must_use]
    pub fn value(self, x: f64, t: &[f64]) -> f64 {
        match self {
            Self::Exponential => t[0] * (t[1] * x).exp(),
            Self::ExponentialAsymptote => t[0] + t[1] * (t[2] * x).exp(),
            Self::Logistic3 => t[0] / (1.0 + (-t[1] * (x - t[2])).exp()),
            Self::Logistic4 => t[0] + (t[1] - t[0]) / (1.0 + (-t[2] * (x - t[3])).exp()),
            Self::Gompertz => t[0] * (-(-t[1] * (x - t[2])).exp()).exp(),
            Self::MichaelisMenten => t[0] * x / (t[1] + x),
            Self::Power => t[0] * x.powf(t[1]),
            Self::Rational => (t[0] + t[1] * x) / (1.0 + t[2] * x),
            Self::StraightLine => t[0] + t[1] * x,
            Self::DoseResponseLog => t[0] + (t[1] - t[0]) / (1.0 + 10f64.powf((t[2] - x) * t[3])),
            Self::DoseResponse => t[0] + (t[1] - t[0]) / (1.0 + (t[2] / x).powf(t[3])),
            Self::AsymmetricLogistic5 => {
                // (1 + 10^z)^−S evaluated as exp(−S·ln(1 + e^(z·ln 10))).
                t[0] + (t[1] - t[0]) * (-t[4] * softplus((t[2] - x) * t[3] * LN_10)).exp()
            }
            Self::WeibullGrowth => t[0] - (t[0] - t[1]) * (-(t[2] * x).powf(t[3])).exp(),
            Self::BetaGrowth => {
                let (end, d) = (t[1], t[1] - t[2]);
                if !(end > 0.0 && d > 0.0) {
                    return f64::NAN;
                }
                let m = x.min(end);
                t[0] * (1.0 + (end - m) / d) * (m / end).powf(end / d)
            }
            Self::Segmented => t[0] + t[1] * x + (t[2] - t[1]) * (x - t[3]).max(0.0),
            Self::TwoPhaseExponential => t[0] + t[1] * (-t[2] * x).exp() + t[3] * (-t[4] * x).exp(),
            Self::GaussianPeak => t[0] + t[1] * (-((x - t[2]).powi(2)) / (2.0 * t[3] * t[3])).exp(),
            Self::LorentzianPeak => t[0] + t[1] / (1.0 + ((x - t[2]) / t[3]).powi(2)),
        }
    }

    /// `∂f(x; θ)/∂θ` into `g`.
    pub fn gradient(self, x: f64, t: &[f64], g: &mut [f64]) {
        match self {
            Self::Exponential => {
                let e = (t[1] * x).exp();
                g[0] = e;
                g[1] = t[0] * x * e;
            }
            Self::ExponentialAsymptote => {
                let e = (t[2] * x).exp();
                g[0] = 1.0;
                g[1] = e;
                g[2] = t[1] * x * e;
            }
            // q(1 − q) with both factors from their own exponentials, so
            // the derivative stays finite (zero) when exp overflows.
            Self::Logistic3 => {
                let z = -t[1] * (x - t[2]);
                let (q, qc) = (1.0 / (1.0 + z.exp()), 1.0 / (1.0 + (-z).exp()));
                g[0] = q;
                g[1] = t[0] * q * qc * (x - t[2]);
                g[2] = -t[0] * q * qc * t[1];
            }
            Self::Logistic4 => {
                let z = -t[2] * (x - t[3]);
                let (q, qc) = (1.0 / (1.0 + z.exp()), 1.0 / (1.0 + (-z).exp()));
                let span = t[1] - t[0];
                g[0] = qc;
                g[1] = q;
                g[2] = span * q * qc * (x - t[3]);
                g[3] = -span * q * qc * t[2];
            }
            Self::Gompertz => {
                let z = -t[1] * (x - t[2]);
                let w = z.exp();
                // e·w = exp(z − w), which is 0 rather than 0·∞ when w overflows.
                let (e, ew) = ((-w).exp(), (z - w).exp());
                g[0] = e;
                g[1] = t[0] * ew * (x - t[2]);
                g[2] = -t[0] * ew * t[1];
            }
            Self::MichaelisMenten => {
                let d = t[1] + x;
                g[0] = x / d;
                g[1] = -t[0] * x / (d * d);
            }
            Self::Power => {
                let v = x.powf(t[1]);
                g[0] = v;
                g[1] = if x == 0.0 { 0.0 } else { t[0] * v * x.ln() };
            }
            Self::Rational => {
                let d = 1.0 + t[2] * x;
                g[0] = 1.0 / d;
                g[1] = x / d;
                g[2] = -x * (t[0] + t[1] * x) / (d * d);
            }
            Self::StraightLine => {
                g[0] = 1.0;
                g[1] = x;
            }
            Self::DoseResponseLog => {
                // q = 1/(1 + e^z), z = (LogEC50 − x)·HillSlope·ln 10;
                // dq/dz = −q(1 − q), with 1 − q computed as 1/(1 + e^−z).
                let z = (t[2] - x) * t[3] * LN_10;
                let (q, r) = (1.0 / (1.0 + z.exp()), 1.0 / (1.0 + (-z).exp()));
                let span = t[1] - t[0];
                g[0] = r;
                g[1] = q;
                g[2] = -span * q * r * t[3] * LN_10;
                g[3] = -span * q * r * (t[2] - x) * LN_10;
            }
            Self::DoseResponse => {
                let q = 1.0 / (1.0 + (t[2] / x).powf(t[3]));
                g[0] = 1.0 - q;
                g[1] = q;
                // q = 1/(1 + e^z), z = HillSlope·ln(EC50/x), dq/dz = −q(1 − q);
                // the shape derivatives vanish at x = 0.
                let span = t[1] - t[0];
                if x > 0.0 {
                    let l = (t[2] / x).ln();
                    let z = t[3] * l;
                    let w = 1.0 / ((1.0 + z.exp()) * (1.0 + (-z).exp()));
                    g[2] = -span * w * t[3] / t[2];
                    g[3] = -span * w * l;
                } else {
                    g[2] = 0.0;
                    g[3] = 0.0;
                }
            }
            Self::AsymmetricLogistic5 => {
                // q = (1 + e^z)^−S, z = (LogC − x)·HillSlope·ln 10;
                // dq/dz = −S·q·e^z/(1 + e^z), dq/dS = −q·ln(1 + e^z).
                let z = (t[2] - x) * t[3] * LN_10;
                let lp = softplus(z);
                let q = (-t[4] * lp).exp();
                let r = 1.0 / (1.0 + (-z).exp());
                let span = t[1] - t[0];
                let dz = -span * t[4] * q * r * LN_10;
                g[0] = 1.0 - q;
                g[1] = q;
                g[2] = dz * t[3];
                g[3] = dz * (t[2] - x);
                g[4] = -span * q * lp;
            }
            Self::WeibullGrowth => {
                let w = (t[2] * x).powf(t[3]);
                let e = (-w).exp();
                g[0] = 1.0 - e;
                g[1] = e;
                let ok = x > 0.0 && w > 0.0 && t[2] != 0.0;
                g[2] = if ok { (t[0] - t[1]) * e * t[3] * w / t[2] } else { 0.0 };
                g[3] = if ok { (t[0] - t[1]) * e * w * (t[2] * x).ln() } else { 0.0 };
            }
            Self::BetaGrowth => {
                let (end, peak) = (t[1], t[2]);
                let d = end - peak;
                let v = self.value(x, &[1.0, end, peak]);
                g[0] = v;
                if x >= end || x <= 0.0 || !v.is_finite() || v == 0.0 {
                    // Constant beyond End; zero with zero slope at x = 0.
                    g[1] = 0.0;
                    g[2] = 0.0;
                } else {
                    // ln g = ln h + a·ln(x/End), h = 1 + (End − x)/d, a = End/d.
                    let h = 1.0 + (end - x) / d;
                    let lr = (x / end).ln();
                    let d2 = d * d;
                    g[1] = t[0] * v * ((x - peak) / (d2 * h) - peak * lr / d2 - 1.0 / d);
                    g[2] = t[0] * v * ((end - x) / (d2 * h) + end * lr / d2);
                }
            }
            Self::Segmented => {
                let h = (x - t[3]).max(0.0);
                g[0] = 1.0;
                g[1] = x - h;
                g[2] = h;
                g[3] = if x > t[3] { -(t[2] - t[1]) } else { 0.0 };
            }
            Self::TwoPhaseExponential => {
                let (e1, e2) = ((-t[2] * x).exp(), (-t[4] * x).exp());
                g[0] = 1.0;
                g[1] = e1;
                g[2] = -t[1] * x * e1;
                g[3] = e2;
                g[4] = -t[3] * x * e2;
            }
            Self::GaussianPeak => {
                let u = x - t[2];
                let w2 = t[3] * t[3];
                let e = (-(u * u) / (2.0 * w2)).exp();
                g[0] = 1.0;
                g[1] = e;
                g[2] = t[1] * e * u / w2;
                g[3] = t[1] * e * u * u / (w2 * t[3]);
            }
            Self::LorentzianPeak => {
                let v = (x - t[2]) / t[3];
                let d = 1.0 + v * v;
                g[0] = 1.0;
                g[1] = 1.0 / d;
                g[2] = t[1] * 2.0 * v / (t[3] * d * d);
                g[3] = t[1] * 2.0 * v * v / (t[3] * d * d);
            }
        }
    }

    /// For a form defined only for `x ≥ 0`, its name in the error message.
    fn nonnegative_predictor(self) -> Option<&'static str> {
        match self {
            Self::Power => Some("power"),
            Self::DoseResponse => Some("dose–response (dose scale)"),
            Self::WeibullGrowth => Some("Weibull growth"),
            Self::BetaGrowth => Some("beta growth"),
            _ => None,
        }
    }

    /// Check that the form can be fitted to predictor values `x`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a negative `x` in a form defined
    /// only for `x ≥ 0` (power, dose–response on the dose scale, Weibull and
    /// beta growth), or a predictor with fewer distinct values than the form
    /// has parameters.
    pub fn check(self, x: &[f64]) -> Result<()> {
        if let Some(name) = self.nonnegative_predictor()
            && x.iter().any(|&v| v < 0.0)
        {
            return Err(StatError::InvalidArgument {
                message: format!("the {name} model needs a predictor with no negative values"),
            });
        }
        let mut sorted = x.to_vec();
        sorted.sort_by(f64::total_cmp);
        sorted.dedup();
        let p = self.parameter_names().len();
        if sorted.len() < p {
            return Err(StatError::InvalidArgument {
                message: format!(
                    "the {} model has {p} parameters and needs at least {p} distinct predictor values",
                    self.label()
                ),
            });
        }
        Ok(())
    }

    /// Automatic starting values for data `(x, y)` (see the module
    /// documentation).
    ///
    /// # Errors
    /// As [`check`](Self::check), or [`StatError::InvalidArgument`] when no
    /// grid point gives a computable fit.
    pub fn starting_values(self, x: &[f64], y: &[f64]) -> Result<Vec<f64>> {
        self.check(x)?;
        let (min, max) = x.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
        let range = (max - min).max(f64::MIN_POSITIVE);
        let log_grid: Vec<f64> = (0..=24).map(|k| 10f64.powf(-2.0 + 3.5 * f64::from(k) / 24.0)).collect();
        let rates: Vec<f64> = log_grid.iter().flat_map(|&k| [k / range, -k / range]).collect::<Vec<_>>();
        let positive_rates: Vec<f64> = log_grid.iter().map(|&k| k / range).collect();
        let mut sorted = x.to_vec();
        sorted.sort_by(f64::total_cmp);
        let quantile = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
        let mids: Vec<f64> = (1..=15).map(|k| quantile(f64::from(k) / 16.0)).collect();
        let xmax_abs = x.iter().fold(0.0_f64, |m, v| m.max(v.abs())).max(f64::MIN_POSITIVE);
        let kms: Vec<f64> = (0..=30).map(|k| xmax_abs * 10f64.powf(-2.0 + 3.0 * f64::from(k) / 30.0)).collect();
        let exponents: Vec<f64> = (0..=60).map(|k| -3.0 + 0.1 * f64::from(k)).filter(|e| *e != 0.0).collect();
        let mut best: Option<(f64, Vec<f64>)> = None;
        let mut consider = |theta: Option<Vec<f64>>| {
            let Some(theta) = theta else { return };
            let sse: f64 = x.iter().zip(y).map(|(&xi, &yi)| (yi - self.value(xi, &theta)).powi(2)).sum();
            if sse.is_finite() && best.as_ref().is_none_or(|(b, _)| sse < *b) {
                best = Some((sse, theta));
            }
        };
        // Linear least squares of y on the given columns.
        let linear = |cols: &[Vec<f64>]| -> Option<Vec<f64>> {
            if cols.iter().flatten().any(|v| !v.is_finite()) {
                return None;
            }
            let m = Matrix::from_columns(cols).ok()?;
            let qr = QrDecomposition::new(m).ok()?;
            if qr.rank(1e-10) < cols.len() {
                return None;
            }
            qr.solve_least_squares(y).ok()
        };
        let ones = vec![1.0; x.len()];
        match self {
            Self::Exponential => {
                for &b in &rates {
                    let e: Vec<f64> = x.iter().map(|&v| (b * v).exp()).collect();
                    consider(linear(&[e]).map(|a| vec![a[0], b]));
                }
            }
            Self::ExponentialAsymptote => {
                for &b in &rates {
                    let e: Vec<f64> = x.iter().map(|&v| (b * v).exp()).collect();
                    consider(linear(&[ones.clone(), e]).map(|a| vec![a[0], a[1], b]));
                }
            }
            Self::Logistic3 | Self::Gompertz => {
                for &b in &rates {
                    for &m in &mids {
                        let shape: Vec<f64> = x.iter().map(|&v| self.value(v, &[1.0, b, m])).collect();
                        consider(linear(&[shape]).map(|a| vec![a[0], b, m]));
                    }
                }
            }
            Self::Logistic4 => {
                for &b in &positive_rates {
                    for &m in &mids {
                        let q: Vec<f64> = x.iter().map(|&v| 1.0 / (1.0 + (-b * (v - m)).exp())).collect();
                        let lower: Vec<f64> = q.iter().map(|q| 1.0 - q).collect();
                        consider(linear(&[lower, q]).map(|a| vec![a[0], a[1], b, m]));
                    }
                }
            }
            Self::MichaelisMenten => {
                for &k in &kms {
                    let h: Vec<f64> = x.iter().map(|&v| v / (k + v)).collect();
                    consider(linear(&[h]).map(|a| vec![a[0], k]));
                }
            }
            Self::Power => {
                for &e in &exponents {
                    let h: Vec<f64> = x.iter().map(|&v| v.powf(e)).collect();
                    consider(linear(&[h]).map(|a| vec![a[0], e]));
                }
            }
            Self::Rational => {
                let cs = log_grid.iter().flat_map(|&k| [k / xmax_abs, -k / xmax_abs]).chain([0.0]);
                for c in cs {
                    // The denominator must keep one sign over the data.
                    if x.iter().any(|&v| 1.0 + c * v <= 1e-6) {
                        continue;
                    }
                    let a: Vec<f64> = x.iter().map(|&v| 1.0 / (1.0 + c * v)).collect();
                    let b: Vec<f64> = x.iter().map(|&v| v / (1.0 + c * v)).collect();
                    consider(linear(&[a, b]).map(|ab| vec![ab[0], ab[1], c]));
                }
            }
            Self::StraightLine => consider(linear(&[ones.clone(), x.to_vec()])),
            Self::DoseResponseLog => {
                for &b in &rates {
                    let h = b / LN_10;
                    for &m in &mids {
                        let q: Vec<f64> = x.iter().map(|&v| self.value(v, &[0.0, 1.0, m, h])).collect();
                        let lower: Vec<f64> = q.iter().map(|q| 1.0 - q).collect();
                        consider(linear(&[lower, q]).map(|a| vec![a[0], a[1], m, h]));
                    }
                }
            }
            Self::DoseResponse => {
                let lo = x.iter().copied().filter(|&v| v > 0.0).fold(f64::INFINITY, f64::min);
                if lo.is_finite() {
                    let ec50s = (0..=20).map(|k| lo * (max / lo).powf(f64::from(k) / 20.0));
                    let hills: Vec<f64> =
                        (0..=15).map(|k| 0.2 * 40f64.powf(f64::from(k) / 15.0)).flat_map(|h| [h, -h]).collect();
                    for e in ec50s {
                        for &h in &hills {
                            let q: Vec<f64> = x.iter().map(|&v| self.value(v, &[0.0, 1.0, e, h])).collect();
                            let lower: Vec<f64> = q.iter().map(|q| 1.0 - q).collect();
                            consider(linear(&[lower, q]).map(|a| vec![a[0], a[1], e, h]));
                        }
                    }
                }
            }
            Self::AsymmetricLogistic5 => {
                let asymmetries = [0.2, 0.3, 0.5, 0.7, 1.0, 1.4, 2.0, 3.0, 5.0];
                for &b in &rates {
                    let h = b / LN_10;
                    for &m in &mids {
                        for &s in &asymmetries {
                            let q: Vec<f64> = x.iter().map(|&v| self.value(v, &[0.0, 1.0, m, h, s])).collect();
                            let lower: Vec<f64> = q.iter().map(|q| 1.0 - q).collect();
                            consider(linear(&[lower, q]).map(|a| vec![a[0], a[1], m, h, s]));
                        }
                    }
                }
            }
            Self::WeibullGrowth => {
                let shapes: Vec<f64> = (0..=15).map(|k| 0.3 * (8.0_f64 / 0.3).powf(f64::from(k) / 15.0)).collect();
                for &k in &log_grid {
                    let r = k / xmax_abs;
                    for &s in &shapes {
                        let e: Vec<f64> = x.iter().map(|&v| (-(r * v).powf(s)).exp()).collect();
                        let rise: Vec<f64> = e.iter().map(|e| 1.0 - e).collect();
                        consider(linear(&[rise, e]).map(|a| vec![a[0], a[1], r, s]));
                    }
                }
            }
            Self::BetaGrowth => {
                let ends = (4..=15)
                    .map(|k| quantile(f64::from(k) / 16.0))
                    .chain([1.0, 1.1, 1.25, 1.5, 2.0, 3.0].map(|f| f * max))
                    .filter(|&e| e > 0.0);
                for end in ends {
                    for k in 0..20 {
                        let peak = end * f64::from(k) / 20.0;
                        let shape: Vec<f64> = x.iter().map(|&v| self.value(v, &[1.0, end, peak])).collect();
                        consider(linear(&[shape]).map(|a| vec![a[0], end, peak]));
                    }
                }
            }
            Self::Segmented => {
                let mut distinct = sorted.clone();
                distinct.dedup();
                // Breakpoints leaving at least two distinct values on each side.
                let breaks = (1..distinct.len().saturating_sub(2)).map(|i| 0.5 * (distinct[i] + distinct[i + 1]));
                for b in breaks {
                    let hinge: Vec<f64> = x.iter().map(|&v| (v - b).max(0.0)).collect();
                    consider(linear(&[ones.clone(), x.to_vec(), hinge]).map(|a| vec![a[0], a[1], a[1] + a[2], b]));
                }
            }
            Self::TwoPhaseExponential => {
                for (i, &slow) in positive_rates.iter().enumerate() {
                    let e2: Vec<f64> = x.iter().map(|&v| (-slow * v).exp()).collect();
                    for &fast in &positive_rates[i + 1..] {
                        let e1: Vec<f64> = x.iter().map(|&v| (-fast * v).exp()).collect();
                        consider(linear(&[ones.clone(), e1, e2.clone()]).map(|a| vec![a[0], a[1], fast, a[2], slow]));
                    }
                }
            }
            Self::GaussianPeak | Self::LorentzianPeak => {
                let centers: Vec<f64> = (1..=31).map(|k| quantile(f64::from(k) / 32.0)).collect();
                let widths: Vec<f64> = (0..=20).map(|k| range * 10f64.powf(-2.0 + 2.0 * f64::from(k) / 20.0)).collect();
                for &c in &centers {
                    for &w in &widths {
                        let shape: Vec<f64> = x.iter().map(|&v| self.value(v, &[0.0, 1.0, c, w])).collect();
                        consider(linear(&[ones.clone(), shape]).map(|a| vec![a[0], a[1], c, w]));
                    }
                }
            }
        }
        let mut theta = best.map(|(_, t)| t).ok_or_else(|| StatError::InvalidArgument {
            message: format!("no starting values could be found for the {} model", self.label()),
        })?;
        // The four-parameter dose–response forms are unchanged by swapping
        // Bottom and Top and negating the Hill slope; start from the form
        // with Bottom the lower plateau, so the sign of the slope gives the
        // direction of the curve.
        if matches!(self, Self::DoseResponseLog | Self::DoseResponse) && theta[1] < theta[0] {
            theta.swap(0, 1);
            theta[3] = -theta[3];
        }
        Ok(theta)
    }
}

/// A library form evaluated at fixed predictor values, as a
/// [`ModelFunction`] with analytic derivatives.
#[derive(Debug, Clone)]
pub struct FormModel<'a> {
    /// The form.
    pub form: CurveForm,
    /// Predictor values.
    pub x: &'a [f64],
}

impl ModelFunction for FormModel<'_> {
    fn n_observations(&self) -> usize {
        self.x.len()
    }
    fn values(&mut self, theta: &[f64], out: &mut [f64]) -> bool {
        for (o, &x) in out.iter_mut().zip(self.x) {
            *o = self.form.value(x, theta);
        }
        out.iter().all(|v| v.is_finite())
    }
    fn derivatives(&mut self, theta: &[f64], out: &mut Matrix) -> Option<bool> {
        let p = theta.len();
        let mut g = vec![0.0; p];
        for (i, &x) in self.x.iter().enumerate() {
            self.form.gradient(x, theta, &mut g);
            for j in 0..p {
                out[(i, j)] = g[j];
            }
        }
        Some(out.is_finite())
    }
}
