//! Analysis of means (ANOM): each group's mean, proportion or rate plotted
//! against decision limits around the overall value, so that a group outside
//! the limits differs significantly from the overall average (Ott 1967,
//! "Analysis of means — a graphical procedure", *Industrial Quality Control*
//! 24; Nelson 1982, "Exact critical points for the analysis of means",
//! *Communications in Statistics — Theory and Methods* 11; Nelson, Wludyka &
//! Copeland, *The Analysis of Means*, SIAM 2005).
//!
//! # Critical values
//!
//! With group sizes `nᵢ`, `N = Σnᵢ` and the weighted grand mean `ȳ`, the
//! deviations `dᵢ = ȳᵢ − ȳ` have variances `σ²(1/nᵢ − 1/N)` and covariances
//! `−σ²/N`, so the standardized deviations `Xᵢ = dᵢ / (σ√(1/nᵢ − 1/N))` are
//! multivariate normal with the negative product correlation
//! `ρᵢⱼ = −λᵢλⱼ`, `λᵢ = √(nᵢ/(N − nᵢ))` (equicorrelated, `ρ = −1/(k − 1)`, for
//! equal sizes). The decision limits are `ȳ ± h·s·√(1/nᵢ − 1/N)`, where the
//! exact critical value `h` solves `P(maxᵢ |Xᵢ|/S ≤ h) = 1 − α` with
//! `S = √(χ²_ν/ν)` independent of `X` (multivariate t; `S = 1` for a known
//! variance, as for proportions and rates).
//!
//! Negative product correlation cannot be written through one real common
//! factor, as Dunnett's positive correlation can ([`crate::control`]); the
//! standard device (Nelson 1982; Soong & Hsu 1997, "Using complex integration
//! to compute multivariate normal probabilities", *Technometrics* 39) is an
//! imaginary common factor, which gives the one-dimensional integral
//!
//! ```text
//! P(max |Xᵢ| ≤ x) = ∫ φ(τ) Πᵢ [Φ(bᵢ − iωᵢτ) − Φ(−bᵢ − iωᵢτ)] dτ,
//! ωᵢ = √(nᵢ/N),  bᵢ = x√(1 − ωᵢ²).
//! ```
//!
//! Each factor, multiplied by `e^{−ωᵢ²τ²/2}` (the product of these is
//! `√(2π)·φ(τ)`), is the real function
//! `Hᵢ(τ) = (2π)^{−½} ∫_{−bᵢ}^{bᵢ} e^{−u²/2} cos(ωᵢτu) du
//!        = e^{−qᵢ²/2} − e^{−bᵢ²/2} Re[e^{ibᵢqᵢ} w((qᵢ + ibᵢ)/√2)]`,
//! `qᵢ = ωᵢτ`, with `w` the Faddeeva function, evaluated by Weideman's (1994)
//! rational approximation ("Computation of the complex error function", *SIAM
//! J. Numer. Anal.* 31) with 32 terms. So
//! `P = √(2/π) ∫₀^∞ Πᵢ Hᵢ(τ) dτ`, integrated by composite 16-point
//! Gauss–Legendre panels of unit width. The integrand decays like `τ^{−k}`
//! while oscillating, so the range is 400 for three groups, 300 for four and
//! 200 (at least `12/minᵢ ωᵢ + 50`) otherwise, which keeps the truncation
//! error below about 10⁻⁸. Two groups are perfectly negatively correlated,
//! so `P = P(|X₁| ≤ x)` exactly.
//!
//! For a finite `ν`, `P(x)` is interpolated (barycentric Chebyshev, 81 nodes)
//! on `[0, x₉]`, where the Bonferroni bound `2kΦ(−x₉) = 10⁻¹⁶` makes it one
//! beyond, and integrated against the density of `S` with the nodes of
//! Dunnett's distribution.
//!
//! # Two-way layouts
//!
//! For a balanced `a × b` layout with `r` replicates per cell, the main-effect
//! means `ȳᵢ..` are charted with `ȳ ± h(α; a, ν)·s·√((a − 1)/(abr))` (and
//! likewise for the second factor), where `s² = MSE` on `ν = ab(r − 1)`
//! degrees of freedom; with one observation per cell the interaction mean
//! square is the error (`ν = (a − 1)(b − 1)`) and no interaction chart is
//! drawn. Interaction effects `ȳᵢⱼ. − ȳᵢ.. − ȳ.ⱼ. + ȳ...` are charted around
//! zero with limits `±h*·s·√((a − 1)(b − 1)/(abr))`; for a 2 × 2 layout the
//! four effects have equal size and `h* = t_{1−α/2, ν}` exactly, otherwise `h*`
//! is the studentized-maximum-modulus value for `ab` comparisons, which by
//! Šidák's (1967) inequality gives conservative limits (Nelson, Wludyka &
//! Copeland 2005, §6).
//!
//! # Failure modes
//!
//! Fewer than two groups, an empty group, non-finite values, no error degrees
//! of freedom, zero variation, a level outside (0, 1), a proportion or rate of
//! zero (or a proportion of one), or an unbalanced two-way layout are errors.

use crate::common::{check_finite, increasing_root, invalid, mean_var};
use crate::control::{composite, scale_nodes};
use stat_core::distributions::{ContinuousDistribution, Normal, StudentsT};
use stat_core::special::erfc;
use stat_core::{Result, StatError};
use std::f64::consts::{FRAC_1_SQRT_2, PI, SQRT_2};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    fn mul(self, o: Self) -> Self {
        Self::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
    fn recip(self) -> Self {
        let d = self.re * self.re + self.im * self.im;
        Self::new(self.re / d, -self.im / d)
    }
}

/// Terms of Weideman's rational approximation.
const WEIDEMAN_TERMS: usize = 32;

/// Weideman's (1994) polynomial coefficients (highest degree first) and his
/// scale `L = √(N/√2)`, from the discrete Fourier transform of
/// `e^{−t²}(L² + t²)` at `t = L tan(θ/2)`.
fn weideman() -> &'static ([f64; WEIDEMAN_TERMS], f64) {
    static TABLE: OnceLock<([f64; WEIDEMAN_TERMS], f64)> = OnceLock::new();
    TABLE.get_or_init(|| {
        let n = WEIDEMAN_TERMS;
        let m = 2 * n;
        let len = 2 * m;
        let l = (n as f64 / SQRT_2).sqrt();
        // f(θ_k) for k = −m+1 … m−1 at index k + m, with f at index 0 zero.
        let mut f = vec![0.0; len];
        for k in 1..len {
            let theta = (k as f64 - m as f64) * PI / m as f64;
            let t = l * (0.5 * theta).tan();
            f[k] = (-t * t).exp() * (l * l + t * t);
        }
        // fftshift (even length): g[i] = f[(i + len/2) mod len]; then the real
        // part of the DFT, divided by the length.
        let g: Vec<f64> = (0..len).map(|i| f[(i + len / 2) % len]).collect();
        let mut a = [0.0; WEIDEMAN_TERMS];
        for (j, slot) in a.iter_mut().enumerate() {
            // Coefficient of Z^j is DFT index j + 1; store highest degree first.
            let idx = j + 1;
            let re: f64 = g.iter().enumerate().map(|(i, v)| v * (2.0 * PI * (i * idx) as f64 / len as f64).cos()).sum();
            *slot = re / len as f64;
        }
        a.reverse();
        (a, l)
    })
}

/// The Faddeeva function `w(z) = e^{−z²} erfc(−iz)` for `Im z ≥ 0`
/// (Weideman 1994, 32 terms; absolute error about 10⁻¹⁴).
fn faddeeva(z: Complex) -> Complex {
    let (a, l) = weideman();
    // i·z
    let iz = Complex::new(-z.im, z.re);
    let inv = Complex::new(l - iz.re, -iz.im).recip();
    let big_z = Complex::new(l + iz.re, iz.im).mul(inv);
    let mut p = Complex::new(0.0, 0.0);
    for &c in a {
        p = p.mul(big_z);
        p.re += c;
    }
    let inv2 = inv.mul(inv);
    let first = p.mul(inv2);
    let s = 1.0 / PI.sqrt();
    Complex::new(2.0 * first.re + s * inv.re, 2.0 * first.im + s * inv.im)
}

/// `Hᵢ(τ) = (2π)^{−½} ∫_{−b}^{b} e^{−u²/2} cos(ωτu) du` (see the module notes).
fn deviation_factor(tau: f64, b: f64, omega: f64) -> f64 {
    let q = tau * omega;
    let w = faddeeva(Complex::new(q * FRAC_1_SQRT_2, b * FRAC_1_SQRT_2));
    let (s, c) = (b * q).sin_cos();
    (-0.5 * q * q).exp() - (-0.5 * b * b).exp() * (c * w.re - s * w.im)
}

fn phi_upper(x: f64) -> f64 {
    0.5 * erfc(x * FRAC_1_SQRT_2)
}

/// Chebyshev–Lobatto nodes used to interpolate the known-variance
/// probability for a finite `ν`.
const INTERPOLATION_DEGREE: usize = 80;

/// Distribution of the largest absolute standardized deviation of group
/// means from their weighted grand mean, `maxᵢ |Xᵢ|/S` (see the module
/// notes): the reference distribution of the analysis of means.
#[derive(Debug, Clone)]
pub struct AnomDistribution {
    /// `(ωᵢ, multiplicity)` for each distinct group size.
    weights: Vec<(f64, usize)>,
    groups: usize,
    df: f64,
    /// Upper end of the integration in τ.
    tau_max: usize,
    /// `(s, weight·f_S(s))`, or `None` for a known variance.
    outer: Option<Vec<(f64, f64)>>,
    /// Interpolation nodes and known-variance probabilities on `[0, x_max]`.
    nodes: Vec<(f64, f64)>,
    x_max: f64,
}

impl AnomDistribution {
    /// Groups of the given sizes (any positive weights; only their ratios
    /// matter) with `df` error degrees of freedom (`f64::INFINITY` for a
    /// known variance).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for fewer than two groups, a size that
    /// is not positive and finite, or `df ≤ 0`.
    pub fn new(sizes: &[f64], df: f64) -> Result<Self> {
        if sizes.len() < 2 {
            return Err(invalid("the analysis of means needs at least two groups"));
        }
        if sizes.iter().any(|n| !n.is_finite() || *n <= 0.0) {
            return Err(invalid("group sizes must be positive and finite"));
        }
        if df.is_nan() || df <= 0.0 {
            return Err(invalid(format!("degrees of freedom must be positive (got {df})")));
        }
        let total: f64 = sizes.iter().sum();
        let mut weights: Vec<(f64, usize)> = Vec::new();
        for &n in sizes {
            let omega = (n / total).sqrt();
            match weights.iter_mut().find(|(w, _)| (w - omega).abs() <= 1e-15 * omega) {
                Some(entry) => entry.1 += 1,
                None => weights.push((omega, 1)),
            }
        }
        let groups = sizes.len();
        let omega_min = weights.iter().map(|w| w.0).fold(f64::INFINITY, f64::min);
        let base: f64 = match groups {
            3 => 400.0,
            4 => 300.0,
            _ => 200.0,
        };
        let tau_max = base.max(12.0 / omega_min + 50.0).ceil() as usize;
        // 2kΦ(−x) = 1e−16.
        let x_max = -Normal::standard().quantile(1e-16 / (2.0 * groups as f64))?;
        let mut dist = Self { weights, groups, df, tau_max, outer: scale_nodes(df)?, nodes: Vec::new(), x_max };
        if dist.outer.is_some() {
            let d = INTERPOLATION_DEGREE;
            dist.nodes = (0..=d)
                .map(|j| {
                    let x = 0.5 * x_max * (1.0 - (PI * j as f64 / d as f64).cos());
                    (x, dist.known_variance_cdf(x))
                })
                .collect();
        }
        Ok(dist)
    }

    /// `k` groups of equal size.
    ///
    /// # Errors
    /// As [`new`](Self::new).
    pub fn balanced(groups: usize, df: f64) -> Result<Self> {
        Self::new(&vec![1.0; groups], df)
    }

    /// Number of groups.
    #[must_use]
    pub fn groups(&self) -> usize {
        self.groups
    }

    /// Error degrees of freedom.
    #[must_use]
    pub fn df(&self) -> f64 {
        self.df
    }

    /// `P(maxᵢ |Xᵢ| ≤ x)` for a known variance (`S = 1`).
    #[must_use]
    pub fn known_variance_cdf(&self, x: f64) -> f64 {
        if x.is_nan() || x <= 0.0 {
            return 0.0;
        }
        if self.groups == 2 {
            return (1.0 - 2.0 * phi_upper(x)).max(0.0);
        }
        if x >= self.x_max {
            return 1.0;
        }
        let bs: Vec<(f64, f64, i32)> = self
            .weights
            .iter()
            .map(|&(omega, count)| (x * (1.0 - omega * omega).sqrt(), omega, count as i32))
            .collect();
        let mut total = 0.0;
        for (tau, w) in composite(0.0, self.tau_max as f64, self.tau_max) {
            let mut prod = 1.0;
            for &(b, omega, count) in &bs {
                prod *= deviation_factor(tau, b, omega).powi(count);
            }
            total += w * prod;
        }
        ((2.0 / PI).sqrt() * total).clamp(0.0, 1.0)
    }

    fn interpolated(&self, x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= self.x_max {
            return 1.0;
        }
        // Barycentric formula for Chebyshev points of the second kind.
        let d = self.nodes.len() - 1;
        let (mut num, mut den) = (0.0, 0.0);
        for (j, &(xj, fj)) in self.nodes.iter().enumerate() {
            let diff = x - xj;
            if diff == 0.0 {
                return fj;
            }
            let mut wj = if j % 2 == 0 { 1.0 } else { -1.0 };
            if j == 0 || j == d {
                wj *= 0.5;
            }
            num += wj * fj / diff;
            den += wj / diff;
        }
        (num / den).clamp(0.0, 1.0)
    }

    /// `P(maxᵢ |Xᵢ|/S ≤ c)`.
    #[must_use]
    pub fn cdf(&self, c: f64) -> f64 {
        if c.is_nan() || c <= 0.0 {
            return 0.0;
        }
        match &self.outer {
            None => self.known_variance_cdf(c),
            Some(outer) => {
                if self.groups == 2 {
                    // Exactly Student's t.
                    return StudentsT::new(self.df)
                        .and_then(|t| t.sf(c))
                        .map_or(f64::NAN, |sf| (1.0 - 2.0 * sf).max(0.0));
                }
                outer.iter().map(|&(s, w)| w * self.interpolated(c * s)).sum::<f64>().clamp(0.0, 1.0)
            }
        }
    }

    /// The critical value `h` with `cdf(h) = level` (e.g. `level = 0.95` for
    /// `α = 0.05`).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for `level ∉ (0, 1)`;
    /// [`StatError::ConvergenceFailure`] if root finding fails.
    pub fn quantile(&self, level: f64) -> Result<f64> {
        if !(level > 0.0 && level < 1.0) {
            return Err(invalid(format!("level must lie strictly between 0 and 1 (got {level})")));
        }
        increasing_root(|c| Ok(self.cdf(c) - level), 0.0, 4.0, "analysis of means critical value")
    }
}

/// The studentized-maximum-modulus critical value: the `c` with
/// `∫ f_S(s) (2Φ(cs) − 1)^m ds = level`, which by Šidák's inequality bounds
/// the maximum of `m` correlated standardized effects (used for two-way
/// interaction effects).
///
/// # Errors
/// As [`AnomDistribution::quantile`], and for `m = 0` or `df ≤ 0`.
pub fn maximum_modulus_quantile(m: usize, df: f64, level: f64) -> Result<f64> {
    if m == 0 {
        return Err(invalid("give at least one comparison"));
    }
    if df.is_nan() || df <= 0.0 {
        return Err(invalid(format!("degrees of freedom must be positive (got {df})")));
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid(format!("level must lie strictly between 0 and 1 (got {level})")));
    }
    let outer = scale_nodes(df)?;
    let p = |x: f64| (1.0 - 2.0 * phi_upper(x)).max(0.0).powi(m as i32);
    let cdf = |c: f64| match &outer {
        None => p(c),
        Some(o) => o.iter().map(|&(s, w)| w * p(c * s)).sum(),
    };
    increasing_root(|c| Ok(cdf(c) - level), 0.0, 4.0, "maximum modulus critical value")
}

/// Which decision limit a group crosses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomSide {
    /// Above the upper decision limit.
    Above,
    /// Below the lower decision limit.
    Below,
}

/// One plotted group of an analysis of means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnomGroup {
    /// Group size (observations, trials or exposure).
    pub size: f64,
    /// Plotted value: mean, proportion, rate or interaction effect.
    pub value: f64,
    /// Lower decision limit.
    pub lower: f64,
    /// Upper decision limit.
    pub upper: f64,
    /// The limit crossed, if any.
    pub signal: Option<AnomSide>,
}

/// An analysis-of-means chart.
#[derive(Debug, Clone, PartialEq)]
pub struct AnomChart {
    /// Center line: the weighted grand mean, overall proportion or overall
    /// rate (zero for interaction effects).
    pub center: f64,
    /// Critical value `h` multiplying the standard error of each deviation.
    pub critical_value: f64,
    /// Error degrees of freedom (`∞` for proportions and rates).
    pub df: f64,
    /// Estimated standard deviation of one observation: `s = √MSE`,
    /// `√(p̄(1 − p̄))` or `√ū`.
    pub scale: f64,
    /// Significance level `α`.
    pub alpha: f64,
    /// One entry per group (or cell, for interaction effects).
    pub groups: Vec<AnomGroup>,
}

fn check_alpha(alpha: f64) -> Result<()> {
    if alpha > 0.0 && alpha < 1.0 {
        Ok(())
    } else {
        Err(invalid(format!("the significance level must lie strictly between 0 and 1 (got {alpha})")))
    }
}

fn signal(value: f64, lower: f64, upper: f64) -> Option<AnomSide> {
    if value > upper {
        Some(AnomSide::Above)
    } else if value < lower {
        Some(AnomSide::Below)
    } else {
        None
    }
}

/// Limits `center ± h·scale·√(1/nᵢ − 1/N)` with `h` exact for these sizes.
fn chart_from(
    values: &[f64],
    sizes: &[f64],
    center: f64,
    scale: f64,
    df: f64,
    alpha: f64,
    clamp: (f64, f64),
) -> Result<AnomChart> {
    let h = AnomDistribution::new(sizes, df)?.quantile(1.0 - alpha)?;
    let total: f64 = sizes.iter().sum();
    let groups = values
        .iter()
        .zip(sizes)
        .map(|(&value, &n)| {
            let half = h * scale * (1.0 / n - 1.0 / total).max(0.0).sqrt();
            let lower = (center - half).max(clamp.0);
            let upper = (center + half).min(clamp.1);
            AnomGroup { size: n, value, lower, upper, signal: signal(value, lower, upper) }
        })
        .collect();
    Ok(AnomChart { center, critical_value: h, df, scale, alpha, groups })
}

/// Analysis of means for normal data: group means against
/// `ȳ ± h(α; n₁…n_k, ν)·s·√(1/nᵢ − 1/N)`, `s² = MSE` on `ν = N − k` df.
///
/// # Errors
/// See the module notes.
pub fn anom_means<G: AsRef<[f64]>>(groups: &[G], alpha: f64) -> Result<AnomChart> {
    check_alpha(alpha)?;
    if groups.len() < 2 {
        return Err(invalid("the analysis of means needs at least two groups"));
    }
    for g in groups {
        if g.as_ref().is_empty() {
            return Err(StatError::InsufficientData { required: 1, found: 0 });
        }
        check_finite(g.as_ref(), "group values")?;
    }
    let total: usize = groups.iter().map(|g| g.as_ref().len()).sum();
    let df = (total - groups.len()) as f64;
    if df < 1.0 {
        return Err(invalid("no error degrees of freedom: every group has a single value"));
    }
    let means: Vec<f64> = groups.iter().map(|g| mean_var(g.as_ref()).0).collect();
    let ss: f64 = groups.iter().zip(&means).map(|(g, m)| g.as_ref().iter().map(|v| (v - m).powi(2)).sum::<f64>()).sum();
    let mse = ss / df;
    if mse <= 0.0 {
        return Err(invalid("the groups have no within-group variation"));
    }
    let sizes: Vec<f64> = groups.iter().map(|g| g.as_ref().len() as f64).collect();
    let grand = groups.iter().map(|g| g.as_ref().iter().sum::<f64>()).sum::<f64>() / total as f64;
    chart_from(&means, &sizes, grand, mse.sqrt(), df, alpha, (f64::NEG_INFINITY, f64::INFINITY))
}

fn check_counts(counts: &[f64], sizes: &[f64], what: &str) -> Result<()> {
    if counts.len() != sizes.len() {
        return Err(invalid(format!("give one {what} per group")));
    }
    if counts.len() < 2 {
        return Err(invalid("the analysis of means needs at least two groups"));
    }
    check_finite(counts, "counts")?;
    check_finite(sizes, "group sizes")?;
    if counts.iter().any(|c| *c < 0.0) || sizes.iter().any(|n| *n <= 0.0) {
        return Err(invalid(format!("counts must be non-negative and {what}s positive")));
    }
    Ok(())
}

/// Analysis of means for binomial proportions `pᵢ = xᵢ/nᵢ`: limits
/// `p̄ ± h(α; n₁…n_k, ∞)·√(p̄(1 − p̄))·√(1/nᵢ − 1/N)` (normal approximation),
/// kept within [0, 1].
///
/// # Errors
/// See the module notes; also more events than trials.
pub fn anom_proportions(events: &[f64], trials: &[f64], alpha: f64) -> Result<AnomChart> {
    check_alpha(alpha)?;
    check_counts(events, trials, "trial count")?;
    if events.iter().zip(trials).any(|(x, n)| x > n) {
        return Err(invalid("a group has more events than trials"));
    }
    let p = events.iter().sum::<f64>() / trials.iter().sum::<f64>();
    if p <= 0.0 || p >= 1.0 {
        return Err(invalid("the overall proportion is 0 or 1, so there is no variation to compare"));
    }
    let values: Vec<f64> = events.iter().zip(trials).map(|(x, n)| x / n).collect();
    chart_from(&values, trials, p, (p * (1.0 - p)).sqrt(), f64::INFINITY, alpha, (0.0, 1.0))
}

/// Analysis of means for Poisson rates `uᵢ = cᵢ/nᵢ` (counts per unit of
/// exposure): limits `ū ± h(α; n₁…n_k, ∞)·√ū·√(1/nᵢ − 1/N)`, lower limits
/// kept at or above zero.
///
/// # Errors
/// See the module notes.
pub fn anom_rates(counts: &[f64], exposures: &[f64], alpha: f64) -> Result<AnomChart> {
    check_alpha(alpha)?;
    check_counts(counts, exposures, "exposure")?;
    let u = counts.iter().sum::<f64>() / exposures.iter().sum::<f64>();
    if u <= 0.0 {
        return Err(invalid("there are no events, so there is no variation to compare"));
    }
    let values: Vec<f64> = counts.iter().zip(exposures).map(|(c, n)| c / n).collect();
    chart_from(&values, exposures, u, u.sqrt(), f64::INFINITY, alpha, (0.0, f64::INFINITY))
}

/// A two-way analysis of means: both main effects and, with replication,
/// the interaction.
#[derive(Debug, Clone, PartialEq)]
pub struct AnomTwoWay {
    /// Means of the first factor's levels.
    pub first: AnomChart,
    /// Means of the second factor's levels.
    pub second: AnomChart,
    /// Interaction effects, cell `(i, j)` at index `i·b + j` (`None` without
    /// replication).
    pub interaction: Option<AnomChart>,
    /// Observations per cell.
    pub replicates: usize,
    /// Error mean square.
    pub mse: f64,
}

/// Two-way analysis of means of a balanced layout: `cells[i][j]` holds the
/// `r` observations at level `i` of the first factor and level `j` of the
/// second (see the module notes for the limits).
///
/// # Errors
/// See the module notes.
pub fn anom_two_way(cells: &[Vec<Vec<f64>>], alpha: f64) -> Result<AnomTwoWay> {
    check_alpha(alpha)?;
    let a = cells.len();
    let b = cells.first().map_or(0, Vec::len);
    if a < 2 || b < 2 {
        return Err(invalid("a two-way analysis of means needs at least two levels of each factor"));
    }
    if cells.iter().any(|row| row.len() != b) {
        return Err(invalid("every level of the first factor needs every level of the second"));
    }
    let r = cells[0][0].len();
    if r == 0 || cells.iter().flatten().any(|c| c.len() != r) {
        return Err(invalid("the layout must be balanced: the same number of observations in every cell"));
    }
    for c in cells.iter().flatten() {
        check_finite(c, "cell values")?;
    }
    let (af, bf, rf) = (a as f64, b as f64, r as f64);
    let cell_mean: Vec<Vec<f64>> = cells.iter().map(|row| row.iter().map(|c| mean_var(c).0).collect()).collect();
    let row_mean: Vec<f64> = cell_mean.iter().map(|row| row.iter().sum::<f64>() / bf).collect();
    let col_mean: Vec<f64> = (0..b).map(|j| cell_mean.iter().map(|row| row[j]).sum::<f64>() / af).collect();
    let grand = row_mean.iter().sum::<f64>() / af;
    let effect = |i: usize, j: usize| cell_mean[i][j] - row_mean[i] - col_mean[j] + grand;
    let (mse, df) = if r >= 2 {
        let ss: f64 = cells
            .iter()
            .zip(&cell_mean)
            .flat_map(|(row, means)| row.iter().zip(means))
            .map(|(c, m)| c.iter().map(|v| (v - m).powi(2)).sum::<f64>())
            .sum();
        let df = af * bf * (rf - 1.0);
        (ss / df, df)
    } else {
        let ss: f64 = (0..a).flat_map(|i| (0..b).map(move |j| (i, j))).map(|(i, j)| effect(i, j).powi(2)).sum();
        let df = (af - 1.0) * (bf - 1.0);
        (ss / df, df)
    };
    if mse.is_nan() || mse <= 0.0 {
        return Err(invalid("the layout has no residual variation"));
    }
    let s = mse.sqrt();
    let main = |means: &[f64], others: f64| -> Result<AnomChart> {
        let sizes = vec![others * rf; means.len()];
        chart_from(means, &sizes, grand, s, df, alpha, (f64::NEG_INFINITY, f64::INFINITY))
    };
    let first = main(&row_mean, bf)?;
    let second = main(&col_mean, af)?;
    let interaction = if r >= 2 {
        let h = if a == 2 && b == 2 {
            StudentsT::new(df)?.quantile(1.0 - alpha / 2.0)?
        } else {
            maximum_modulus_quantile(a * b, df, 1.0 - alpha)?
        };
        let half = h * s * ((af - 1.0) * (bf - 1.0) / (af * bf * rf)).sqrt();
        let groups = (0..a)
            .flat_map(|i| (0..b).map(move |j| (i, j)))
            .map(|(i, j)| {
                let value = effect(i, j);
                AnomGroup { size: rf, value, lower: -half, upper: half, signal: signal(value, -half, half) }
            })
            .collect();
        Some(AnomChart { center: 0.0, critical_value: h, df, scale: s, alpha, groups })
    } else {
        None
    };
    Ok(AnomTwoWay { first, second, interaction, replicates: r, mse })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faddeeva_matches_closed_forms() {
        // On the imaginary axis w(iy) = e^{y²} erfc(y).
        for y in [0.1, 0.5, 1.0, 2.0, 5.0, 10.0] {
            let w = faddeeva(Complex::new(0.0, y));
            let exact = (y * y).exp() * erfc(y);
            assert!((w.re - exact).abs() < 1e-13 * exact.max(1e-3), "y={y}: {} vs {exact}", w.re);
            assert!(w.im.abs() < 1e-13);
        }
        // Off the axes, against (i/π)∫e^{−t²}/(z − t)dt evaluated with NumPy
        // (2·10⁶-point trapezoid on [−12, 12]).
        let cases = [
            ((1.0, 1.0), (0.304_744_205_256_912_65, 0.208_218_938_202_831_63)),
            ((3.0, 0.5), (0.037_126_366_054_692_34, 0.192_983_755_300_362_08)),
            ((0.5, 2.0), (0.245_275_990_226_358_54, 0.051_521_478_343_635_84)),
            ((10.0, 1.0), (0.005_669_942_566_902_179, 0.056_129_645_315_951_27)),
        ];
        for ((x, y), (re, im)) in cases {
            let w = faddeeva(Complex::new(x, y));
            assert!((w.re - re).abs() < 1e-12 && (w.im - im).abs() < 1e-12, "{x}+{y}i: {w:?}");
        }
    }

    #[test]
    fn deviation_factor_at_zero_is_a_normal_probability() {
        for b in [0.3, 1.0, 2.5] {
            let h = deviation_factor(0.0, b, 0.6);
            assert!((h - (1.0 - 2.0 * phi_upper(b))).abs() < 1e-13);
        }
    }

    /// Known-variance probabilities against an independent computation: for
    /// three groups the deviations reduce to two, and `P` is a 1-D integral
    /// of the first deviation's density times a closed-form normal
    /// probability for the second (NumPy, composite Gauss–Legendre split at
    /// the kinks), with no Fourier transform.
    #[test]
    fn three_group_probabilities_match_direct_integration() {
        let bal = AnomDistribution::new(&[5.0, 5.0, 5.0], f64::INFINITY).unwrap();
        assert!((bal.known_variance_cdf(2.2) - 0.928_840_144_177_647_8).abs() < 2e-9);
        let uneq = AnomDistribution::new(&[4.0, 6.0, 9.0], f64::INFINITY).unwrap();
        assert!((uneq.known_variance_cdf(1.0) - 0.424_988_692_495_964_5).abs() < 2e-7);
    }

    /// Critical values against the independent direct integration above
    /// (and a 2-D version of it for four groups); two groups give Student's t
    /// or the normal quantile exactly.
    #[test]
    fn critical_values_match_independent_computation() {
        let cases = [
            (vec![1.0; 3], 0.05, 2.343_700_586_378_407),
            (vec![1.0; 3], 0.01, 2.913_494_337_832_621_5),
            (vec![4.0, 6.0, 9.0], 0.05, 2.339_700_569_272_607_4),
            (vec![1.0; 4], 0.05, 2.468_477_405_685_227),
        ];
        for (sizes, alpha, expected) in cases {
            let h = AnomDistribution::new(&sizes, f64::INFINITY).unwrap().quantile(1.0 - alpha).unwrap();
            assert!((h - expected).abs() < 2e-6, "{sizes:?} α={alpha}: {h} vs {expected}");
        }
        let two = AnomDistribution::new(&[3.0, 7.0], 9.0).unwrap();
        let t = StudentsT::new(9.0).unwrap().quantile(0.975).unwrap();
        assert!((two.quantile(0.95).unwrap() - t).abs() < 1e-9);
        let z = AnomDistribution::balanced(2, f64::INFINITY).unwrap().quantile(0.95).unwrap();
        assert!((z - 1.959_963_984_540_054).abs() < 1e-9);
    }

    #[test]
    fn critical_values_order_and_bounds() {
        // h grows with k, falls with ν, and lies below the Šidák bound.
        let mut last = 0.0;
        for k in [3, 5, 8, 12] {
            let d = AnomDistribution::balanced(k, 20.0).unwrap();
            let h = d.quantile(0.95).unwrap();
            assert!(h > last);
            last = h;
            let sidak = maximum_modulus_quantile(k, 20.0, 0.95).unwrap();
            assert!(h < sidak, "k={k}: {h} vs Šidák {sidak}");
            let hinf = AnomDistribution::balanced(k, f64::INFINITY).unwrap().quantile(0.95).unwrap();
            assert!(hinf < h);
        }
    }

    #[test]
    fn finite_df_matches_direct_integration() {
        // Three equal groups, ν = 10, against the direct integral of the
        // known-variance probability over the density of S.
        let d = AnomDistribution::balanced(3, 10.0).unwrap();
        let h = d.quantile(0.95).unwrap();
        assert!((h - REF_K3_DF10).abs() < 2e-5, "{h}");
    }

    /// Three equal groups, ν = 10, α = 0.05: the direct NumPy integral of
    /// the three-group known-variance probability (as above) over the
    /// density of `S` (600-point Gauss–Legendre), solved by bisection.
    const REF_K3_DF10: f64 = 2.741_295_129_002_537_6;

    #[test]
    fn one_way_limits() {
        let groups = [vec![10.0, 12.0, 11.0], vec![14.0, 15.0, 13.0, 16.0], vec![9.0, 8.0, 10.0]];
        let c = anom_means(&groups, 0.05).unwrap();
        assert_eq!(c.df, 7.0);
        // Hand computation: grand mean 118/10, MSE = (2 + 5 + 2)/7.
        assert!((c.center - 11.8).abs() < 1e-12);
        assert!((c.scale - (9.0_f64 / 7.0).sqrt()).abs() < 1e-12);
        let h = AnomDistribution::new(&[3.0, 4.0, 3.0], 7.0).unwrap().quantile(0.95).unwrap();
        assert!((c.critical_value - h).abs() < 1e-12);
        let half = h * c.scale * (1.0_f64 / 4.0 - 0.1).sqrt();
        assert!((c.groups[1].upper - (11.8 + half)).abs() < 1e-12);
        assert_eq!(c.groups[1].signal, Some(AnomSide::Above));
        assert_eq!(c.groups[2].signal, Some(AnomSide::Below));
        assert!(anom_means(&[vec![1.0], vec![2.0]], 0.05).is_err());
        assert!(anom_means(&[vec![1.0, 2.0]], 0.05).is_err());
        assert!(anom_means(&groups, 1.5).is_err());
    }

    #[test]
    fn proportion_and_rate_limits() {
        let x = [12.0, 5.0, 8.0, 20.0];
        let n = [100.0; 4];
        let c = anom_proportions(&x, &n, 0.05).unwrap();
        let p: f64 = 45.0 / 400.0;
        let h = AnomDistribution::balanced(4, f64::INFINITY).unwrap().quantile(0.95).unwrap();
        let half = h * (p * (1.0 - p)).sqrt() * (0.01_f64 - 1.0 / 400.0).sqrt();
        assert!((c.groups[0].lower - (p - half)).abs() < 1e-12);
        assert_eq!(c.groups[3].signal, Some(AnomSide::Above));
        assert!(anom_proportions(&[0.0, 0.0], &[5.0, 5.0], 0.05).is_err());
        assert!(anom_proportions(&[6.0, 0.0], &[5.0, 5.0], 0.05).is_err());
        let r = anom_rates(&[1.0, 9.0, 30.0], &[10.0, 10.0, 20.0], 0.01).unwrap();
        assert!((r.center - 1.0).abs() < 1e-12);
        assert!(r.groups.iter().all(|g| g.lower >= 0.0));
        assert_eq!(r.groups[0].signal, Some(AnomSide::Below));
    }

    #[test]
    fn two_way_layout() {
        // 2 × 3 layout, two replicates.
        let cells = vec![
            vec![vec![10.0, 11.0], vec![12.0, 13.0], vec![15.0, 14.0]],
            vec![vec![9.0, 10.0], vec![14.0, 13.0], vec![20.0, 21.0]],
        ];
        let t = anom_two_way(&cells, 0.05).unwrap();
        assert_eq!(t.replicates, 2);
        // MSE: every cell has squared deviations 0.5, total 3 on 6 df.
        assert!((t.mse - 0.5).abs() < 1e-12);
        assert_eq!(t.first.df, 6.0);
        let grand = 162.0 / 12.0;
        assert!((t.first.center - grand).abs() < 1e-12);
        let h = AnomDistribution::balanced(2, 6.0).unwrap().quantile(0.95).unwrap();
        let half = h * 0.5_f64.sqrt() * (1.0_f64 / 12.0).sqrt();
        assert!((t.first.groups[0].upper - (grand + half)).abs() < 1e-12);
        let inter = t.interaction.unwrap();
        assert_eq!(inter.groups.len(), 6);
        let sum: f64 = inter.groups.iter().map(|g| g.value).sum();
        assert!(sum.abs() < 1e-12);
        let hm = maximum_modulus_quantile(6, 6.0, 0.95).unwrap();
        assert!((inter.critical_value - hm).abs() < 1e-12);
        // Without replication there is no interaction chart.
        let single: Vec<Vec<Vec<f64>>> = cells.iter().map(|row| row.iter().map(|c| vec![c[0]]).collect()).collect();
        let u = anom_two_way(&single, 0.05).unwrap();
        assert!(u.interaction.is_none());
        assert_eq!(u.first.df, 2.0);
        let mut bad = cells.clone();
        bad[0][0].push(1.0);
        assert!(anom_two_way(&bad, 0.05).is_err());
    }
}
