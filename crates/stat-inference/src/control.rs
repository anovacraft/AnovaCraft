//! Comparisons of several treatments with a control (Dunnett 1955, *JASA*
//! 50; Hsu, *Multiple Comparisons*, 1996, §3.1; Bretz, Hothorn & Westfall,
//! *Multiple Comparisons Using R*, 2010, §4.1).
//!
//! With the pooled mean square `MSE` on `ν = N − k` df, treatment `i` against
//! control `0` has `Tᵢ = (ȳᵢ − ȳ₀)/√(MSE(1/nᵢ + 1/n₀))`. The `Tᵢ` follow a
//! multivariate t with correlations `ρᵢⱼ = λᵢλⱼ`, `λᵢ = √(nᵢ/(nᵢ + n₀))`,
//! which factor through one standard normal `Z`: `Tᵢ = (λᵢZ + √(1 − λᵢ²)Eᵢ)/S`
//! with independent `Eᵢ` and `S = √(χ²_ν/ν)`. So (Dunnett 1955, eq. 2.3)
//!
//! ```text
//! P(max |Tᵢ| ≤ c) = ∫₀^∞ f_S(s) ∫ φ(z) Πᵢ [Φ((cs − λᵢz)/√(1−λᵢ²)) − Φ((−cs − λᵢz)/√(1−λᵢ²))] dz ds,
//! ```
//!
//! and `P(max Tᵢ ≤ c)` drops the second Φ. Both integrals are evaluated by
//! composite Gauss–Legendre quadrature (the outer over the central
//! `1 − 2·10⁻¹⁴` of `S`, as for the studentized range). When a group is much
//! larger than the control, `λᵢ` is close to 1 and its factor is nearly a step
//! at `z = ±cs/λᵢ` of width `√(1 − λᵢ²)`; the inner rule then adds fine panels
//! (two step widths each) over eight step widths either side of each step. The single-step
//! adjusted p-value of comparison `i` is `1 − P(max |T| ≤ |tᵢ|)` (two-sided)
//! or `1 − P(max T ≤ tᵢ)` (one-sided), and the simultaneous intervals are
//! `ȳᵢ − ȳ₀ ± c·SEᵢ` with `c` the `level` quantile of the maximum.

use crate::common::{Alternative, check_finite, check_level, increasing_root, invalid, mean_var};
use rayon::prelude::*;
use stat_core::distributions::{ChiSquared, ContinuousDistribution};
use stat_core::quadrature::gauss_legendre;
use stat_core::special::{erfc, ln_gamma};
use stat_core::{Result, StatError};

fn phi_cdf(x: f64) -> f64 {
    0.5 * erfc(-x * std::f64::consts::FRAC_1_SQRT_2)
}

/// `Φ(x)`, taken as exactly 0 or 1 beyond ±8.5 (error below 10⁻¹⁷), which
/// skips most evaluations on the flat sides of steep factors.
fn phi_cdf_clipped(x: f64) -> f64 {
    if x > 8.5 {
        1.0
    } else if x < -8.5 {
        0.0
    } else {
        phi_cdf(x)
    }
}

/// Composite 16-point Gauss–Legendre nodes and weights on `[a, b]`.
pub(crate) fn composite(a: f64, b: f64, panels: usize) -> Vec<(f64, f64)> {
    let (nodes, weights) = gauss_legendre(16);
    let half = 0.5 * (b - a) / panels as f64;
    (0..panels)
        .flat_map(|p| {
            let mid = a + half * (2 * p + 1) as f64;
            nodes.iter().zip(&weights).map(move |(x, w)| (mid + half * x, half * w)).collect::<Vec<_>>()
        })
        .collect()
}

/// Quadrature nodes `(s, weight·f_S(s))` for the scale `S = √(χ²_ν/ν)` of a
/// studentized statistic, over the central `1 − 2·10⁻¹⁴` of `S` (12 panels of
/// 16 points), or `None` when `ν > 10⁶` (treated as `S = 1`). Shared by
/// Dunnett's and the analysis-of-means distributions.
pub(crate) fn scale_nodes(df: f64) -> Result<Option<Vec<(f64, f64)>>> {
    if df > 1e6 {
        return Ok(None);
    }
    let chi = ChiSquared::new(df)?;
    let lo = (chi.quantile(1e-14)? / df).sqrt();
    let hi = (chi.quantile(1.0 - 1e-14)? / df).sqrt();
    let ln_c = std::f64::consts::LN_2 + 0.5 * df * (0.5 * df).ln() - ln_gamma(0.5 * df);
    Ok(Some(
        composite(lo, hi, 12)
            .into_iter()
            .map(|(s, w)| (s, w * (ln_c + (df - 1.0) * s.ln() - 0.5 * df * s * s).exp()))
            .collect(),
    ))
}

/// Distribution of the largest of several comparisons with a control.
#[derive(Debug, Clone)]
pub struct DunnettDistribution {
    /// Distinct `(λ, √(1 − λ²), multiplicity)`: groups of equal size share a
    /// factor, so many comparisons cost no more than their distinct sizes.
    lambdas: Vec<(f64, f64, i32)>,
    /// `(s, weight·f_S(s))`, or `None` for infinite df (`S = 1`).
    outer: Option<Vec<(f64, f64)>>,
    /// `(z, weight·φ(z))`.
    inner: Vec<(f64, f64)>,
    /// Distinct `(λ, √(1 − λ²))` whose factor is too steep for `inner`.
    steep: Vec<(f64, f64)>,
}

/// `√(1 − λ²)` below which a factor gets its own fine panels.
const STEEP_WIDTH: f64 = 0.2;

/// Range of the inner (standard normal) integral.
const Z_RANGE: (f64, f64) = (-8.5, 8.5);

impl DunnettDistribution {
    /// Comparisons with correlation factors `lambdas` (each in [0, 1)) and
    /// `df` error degrees of freedom (`f64::INFINITY` for a known variance).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for no comparisons, a factor outside
    /// [0, 1), or `df ≤ 0`.
    pub fn new(lambdas: Vec<f64>, df: f64) -> Result<Self> {
        if lambdas.is_empty() {
            return Err(invalid("give at least one comparison"));
        }
        if lambdas.iter().any(|l| !(0.0..1.0).contains(l)) {
            return Err(invalid("correlation factors must lie in [0, 1)"));
        }
        if df.is_nan() || df <= 0.0 {
            return Err(invalid(format!("degrees of freedom must be positive (got {df})")));
        }
        let outer = scale_nodes(df)?;
        let norm = 1.0 / (2.0 * std::f64::consts::PI).sqrt();
        let inner = composite(Z_RANGE.0, Z_RANGE.1, 12)
            .into_iter()
            .map(|(z, w)| (z, w * norm * (-0.5 * z * z).exp()))
            .collect();
        let mut distinct: Vec<(f64, f64, i32)> = Vec::new();
        for &l in &lambdas {
            match distinct.iter_mut().find(|d| d.0 == l) {
                Some(d) => d.2 += 1,
                None => distinct.push((l, (1.0 - l * l).sqrt(), 1)),
            }
        }
        let steep = distinct.iter().filter(|d| d.1 < STEEP_WIDTH).map(|d| (d.0, d.1)).collect();
        Ok(Self { lambdas: distinct, outer, inner, steep })
    }

    /// Equal-size groups: every `λ = √½`, correlation ½.
    ///
    /// # Errors
    /// As [`new`](Self::new).
    pub fn balanced(comparisons: usize, df: f64) -> Result<Self> {
        Self::new(vec![std::f64::consts::FRAC_1_SQRT_2; comparisons], df)
    }

    /// Inner nodes `(z, weight·φ(z))` refined around the steps of the steep
    /// factors at `z = ±cs/λ`.
    fn refined_nodes(&self, cs: f64) -> Vec<(f64, f64)> {
        let (lo, hi) = Z_RANGE;
        // Zones (start, end, panel width) around each step.
        let mut zones: Vec<(f64, f64, f64)> = Vec::new();
        for &(l, r) in &self.steep {
            for b in [cs / l, -cs / l] {
                let (a, e) = ((b - 8.0 * r).max(lo), (b + 8.0 * r).min(hi));
                if a < e {
                    zones.push((a, e, 2.0 * r));
                }
            }
        }
        let mut cuts: Vec<f64> = vec![lo, hi];
        for &(a, e, _) in &zones {
            cuts.push(a);
            cuts.push(e);
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let coarse = (hi - lo) / 12.0;
        let norm = 1.0 / (2.0 * std::f64::consts::PI).sqrt();
        let mut nodes = Vec::new();
        for pair in cuts.windows(2) {
            let (a, e) = (pair[0], pair[1]);
            let mid = 0.5 * (a + e);
            let width = zones.iter().filter(|z| z.0 <= mid && mid <= z.1).map(|z| z.2).fold(coarse, f64::min);
            let panels = ((e - a) / width).ceil().max(1.0) as usize;
            nodes.extend(composite(a, e, panels).into_iter().map(|(z, w)| (z, w * norm * (-0.5 * z * z).exp())));
        }
        nodes
    }

    fn conditional(&self, cs: f64, two_sided: bool) -> f64 {
        let refined;
        let nodes = if self.steep.is_empty() {
            &self.inner
        } else {
            refined = self.refined_nodes(cs);
            &refined
        };
        let mut total = 0.0;
        for &(z, w) in nodes {
            let mut prod = 1.0;
            for &(l, r, count) in &self.lambdas {
                let up = phi_cdf_clipped((cs - l * z) / r);
                let p = if two_sided { up - phi_cdf_clipped((-cs - l * z) / r) } else { up };
                prod *= p.max(0.0).powi(count);
                if prod == 0.0 {
                    break;
                }
            }
            total += w * prod;
        }
        total
    }

    /// `P(max |Tᵢ| ≤ c)` when `two_sided`, else `P(max Tᵢ ≤ c)`.
    #[must_use]
    pub fn cdf(&self, c: f64, two_sided: bool) -> f64 {
        if two_sided && c <= 0.0 {
            return 0.0;
        }
        let v = match &self.outer {
            None => self.conditional(c, two_sided),
            Some(outer) => outer.iter().map(|&(s, w)| w * self.conditional(c * s, two_sided)).sum(),
        };
        v.clamp(0.0, 1.0)
    }

    /// The `c` with `cdf(c) = level`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for `level ∉ (0, 1)`;
    /// [`StatError::ConvergenceFailure`] if root finding fails.
    pub fn quantile(&self, level: f64, two_sided: bool) -> Result<f64> {
        check_level(level)?;
        let lo = if two_sided { 0.0 } else { -10.0 };
        increasing_root(|c| Ok(self.cdf(c, two_sided) - level), lo, 4.0, "Dunnett critical value")
    }
}

/// One treatment compared with the control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlComparison {
    /// Treatment index (into the `treatments` given).
    pub treatment: usize,
    /// `ȳᵢ − ȳ₀`.
    pub difference: f64,
    /// Standard error of the difference.
    pub std_error: f64,
    /// `t` statistic.
    pub statistic: f64,
    /// Adjusted p-value.
    pub p_value: f64,
    /// Simultaneous confidence interval (one side infinite for a one-sided
    /// alternative).
    pub confidence_interval: (f64, f64),
}

/// Result of Dunnett's comparisons.
#[derive(Debug, Clone, PartialEq)]
pub struct DunnettTest {
    /// Pooled mean square error.
    pub mse: f64,
    /// Its degrees of freedom.
    pub df: f64,
    /// Critical value `c` for the intervals.
    pub critical_value: f64,
    /// Simultaneous confidence level.
    pub level: f64,
    /// Alternative (`Greater`: treatment mean above control).
    pub alternative: Alternative,
    /// One entry per treatment.
    pub comparisons: Vec<ControlComparison>,
}

/// Dunnett's many-to-one comparisons of `treatments` with `control`.
///
/// # Errors
/// [`StatError::InsufficientData`] for an empty group;
/// [`StatError::NonFiniteInput`] for NaN/±∞; [`StatError::InvalidArgument`]
/// for no treatments, no error degrees of freedom, zero variance or a level
/// outside (0, 1).
pub fn dunnett_test<G: AsRef<[f64]> + Sync>(
    control: &[f64],
    treatments: &[G],
    level: f64,
    alternative: Alternative,
) -> Result<DunnettTest> {
    check_level(level)?;
    if treatments.is_empty() {
        return Err(invalid("give at least one treatment group"));
    }
    let groups: Vec<&[f64]> = std::iter::once(control).chain(treatments.iter().map(AsRef::as_ref)).collect();
    for g in &groups {
        if g.is_empty() {
            return Err(StatError::InsufficientData { required: 1, found: 0 });
        }
        check_finite(g, "group values")?;
    }
    let total: usize = groups.iter().map(|g| g.len()).sum();
    let df = (total - groups.len()) as f64;
    if df < 1.0 {
        return Err(invalid("no error degrees of freedom: every group has a single value"));
    }
    let ss: f64 = groups
        .iter()
        .map(|g| {
            let (m, _) = mean_var(g);
            g.iter().map(|v| (v - m).powi(2)).sum::<f64>()
        })
        .sum();
    let mse = ss / df;
    if mse <= 0.0 {
        return Err(invalid("the groups have no within-group variation"));
    }
    let n0 = control.len() as f64;
    let m0 = mean_var(control).0;
    let lambdas: Vec<f64> = treatments
        .iter()
        .map(|g| {
            let ni = g.as_ref().len() as f64;
            (ni / (ni + n0)).sqrt()
        })
        .collect();
    let dist = DunnettDistribution::new(lambdas, df)?;
    let two_sided = alternative == Alternative::TwoSided;
    let critical_value = dist.quantile(level, two_sided)?;
    // Each adjusted p-value is a full double integral; with many groups they
    // dominate, so they run in parallel.
    let comparisons = treatments
        .par_iter()
        .enumerate()
        .map(|(i, g)| {
            let g = g.as_ref();
            let difference = mean_var(g).0 - m0;
            let std_error = (mse * (1.0 / g.len() as f64 + 1.0 / n0)).sqrt();
            let t = difference / std_error;
            let p_value = match alternative {
                Alternative::TwoSided => 1.0 - dist.cdf(t.abs(), true),
                Alternative::Greater => 1.0 - dist.cdf(t, false),
                Alternative::Less => 1.0 - dist.cdf(-t, false),
            }
            .clamp(0.0, 1.0);
            let half = critical_value * std_error;
            let confidence_interval = alternative.bound(difference - half, difference + half);
            ControlComparison { treatment: i, difference, std_error, statistic: t, p_value, confidence_interval }
        })
        .collect();
    Ok(DunnettTest { mse, df, critical_value, level, alternative, comparisons })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dunnett's (1955) published critical values for equal group sizes
    /// (correlation ½), from his tables (two decimals).
    #[test]
    fn critical_values_match_dunnetts_tables() {
        // (comparisons, df, one-sided 95%, two-sided 95%)
        let table = [
            (1, f64::INFINITY, 1.64, 1.96),
            (2, f64::INFINITY, 1.92, 2.21),
            (3, f64::INFINITY, 2.06, 2.35),
            (4, f64::INFINITY, 2.16, 2.44),
            (5, f64::INFINITY, 2.23, 2.51),
            (2, 12.0, 2.11, 2.50),
            (1, 10.0, 1.81, 2.23),
        ];
        for (p, df, one, two) in table {
            let d = DunnettDistribution::balanced(p, df).unwrap();
            let c1 = d.quantile(0.95, false).unwrap();
            let c2 = d.quantile(0.95, true).unwrap();
            assert!((c1 - one).abs() < 0.006, "p={p} df={df} one-sided {c1}");
            assert!((c2 - two).abs() < 0.006, "p={p} df={df} two-sided {c2}");
        }
        // One comparison is Student's t exactly.
        let t = stat_core::StudentsT::new(10.0).unwrap();
        let d = DunnettDistribution::balanced(1, 10.0).unwrap();
        assert!((d.quantile(0.975, false).unwrap() - t.quantile(0.975).unwrap()).abs() < 1e-8);
        assert!((d.quantile(0.95, true).unwrap() - t.quantile(0.975).unwrap()).abs() < 1e-8);
    }

    /// Dunnett's (1955) blood-count example, checked against SciPy 1.18's
    /// `scipy.stats.dunnett` (which integrates the multivariate t by
    /// randomized quasi-Monte Carlo, so agreement is to about 10⁻³).
    #[test]
    fn blood_count_example_matches_scipy() {
        let control = [7.40, 8.50, 7.20, 8.24, 9.84, 8.32];
        let drug_a = [9.76, 8.80, 7.68, 9.36];
        let drug_b = [12.80, 9.68, 12.16, 9.20, 10.55];
        let r = dunnett_test(&control, &[&drug_a[..], &drug_b[..]], 0.95, Alternative::TwoSided).unwrap();
        assert_eq!(r.df, 12.0);
        let stats = [0.85703213, 3.69375231];
        let p = [0.62007566, 0.00592395];
        let low = [-1.25778979, 0.83833454];
        let high = [2.55778979, 4.41766546];
        for (k, c) in r.comparisons.iter().enumerate() {
            assert!((c.statistic - stats[k]).abs() < 1e-7);
            assert!((c.p_value - p[k]).abs() < 1e-3, "p {k}: {}", c.p_value);
            assert!((c.confidence_interval.0 - low[k]).abs() < 2e-3, "low {k}: {:?}", c.confidence_interval);
            assert!((c.confidence_interval.1 - high[k]).abs() < 2e-3);
        }
        let g = dunnett_test(&control, &[&drug_a[..], &drug_b[..]], 0.95, Alternative::Greater).unwrap();
        let p = [0.32489789, 0.00288571];
        let low = [-0.95861855, 1.11898204];
        for (k, c) in g.comparisons.iter().enumerate() {
            assert!((c.p_value - p[k]).abs() < 1e-3, "p> {k}: {}", c.p_value);
            assert!((c.confidence_interval.0 - low[k]).abs() < 2e-3);
            assert_eq!(c.confidence_interval.1, f64::INFINITY);
        }
        // `Less` mirrors `Greater`.
        let l = dunnett_test(&control, &[&drug_a[..], &drug_b[..]], 0.95, Alternative::Less).unwrap();
        assert!(l.comparisons[1].p_value > 0.99);
        assert!(dunnett_test::<&[f64]>(&control, &[], 0.95, Alternative::TwoSided).is_err());
        assert!(dunnett_test(&[1.0], &[&[2.0][..]], 0.95, Alternative::TwoSided).is_err());
    }

    /// Correlation factors near 1 (a treatment group ~1000 times the
    /// control's size): one comparison with a known variance is exactly
    /// normal, `P(|Z| ≤ 1) = 2Φ(1) − 1`; two comparisons with λ = 0.9995 are
    /// bivariate normal with correlation 0.9995² (SciPy 1.18.1
    /// `multivariate_normal.cdf`), and the two-sided 95% point for
    /// λ = √(1000/1002), ν = 2000 is from SciPy 1.18.1 `multivariate_t.cdf`
    /// (quasi-Monte Carlo, about 10⁻⁵).
    #[test]
    fn steep_factors_near_one() {
        let one = DunnettDistribution::new(vec![0.9995], f64::INFINITY).unwrap();
        let normal = stat_core::distributions::Normal::standard();
        let exact = 2.0 * normal.cdf(1.0).unwrap() - 1.0;
        assert!((one.cdf(1.0, true) - exact).abs() < 1e-10);
        assert!((one.cdf(1.0, false) - normal.cdf(1.0).unwrap()).abs() < 1e-10);
        let two = DunnettDistribution::new(vec![0.9995, 0.9995], f64::INFINITY).unwrap();
        for (c, p) in [(1.0, 0.837_028_227_738_310_3), (2.0, 0.976_286_963_369_715_7), (3.0, 0.998_571_094_854_920_8)] {
            assert!((two.cdf(c, false) - p).abs() < 1e-9, "c={c}");
        }
        let l = (1000.0_f64 / 1002.0).sqrt();
        let d = DunnettDistribution::new(vec![l, l], 2000.0).unwrap();
        assert!((d.quantile(0.95, true).unwrap() - 1.985_764_6).abs() < 2e-5);
    }
}

#[cfg(test)]
mod many_groups {
    use super::*;

    /// Groups of equal size share one factor, so hundreds of them cost no
    /// more than one: this took minutes before and froze the report.
    #[test]
    fn many_equal_groups_finish_quickly() {
        let groups: Vec<Vec<f64>> = (0..400).map(|g| (0..3).map(|i| f64::from(g % 7 + i * i)).collect()).collect();
        let start = std::time::Instant::now();
        let d = dunnett_test(&groups[0], &groups[1..], 0.95, Alternative::TwoSided).unwrap();
        assert_eq!(d.comparisons.len(), 399);
        assert!(start.elapsed().as_secs() < 30, "took {:?}", start.elapsed());
        // Same answer as listing the factor once per comparison would give:
        // P(max ≤ c) for k equal factors is the k-th power inside the integral.
        let one = DunnettDistribution::balanced(399, d.df).unwrap();
        assert!((one.quantile(0.95, true).unwrap() - d.critical_value).abs() < 1e-9);
    }
}
