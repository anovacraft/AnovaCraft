//! The computations behind the chart elements: jitter offsets, Tukey box
//! statistics, least-squares polynomial fits with confidence bands, and the
//! penalized cubic smoothing spline. All are total: degenerate data (no
//! observations, one observation, no spread) give `None` or a constant result,
//! never NaN or a panic.

use stat_core::{ContinuousDistribution, FisherSnedecor, Matrix, QrDecomposition, StudentsT};

// ---------------------------------------------------------------------------
// Jitter
// ---------------------------------------------------------------------------

/// How points on a categorical axis are spread out so they do not overplot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum JitterKind {
    /// No spreading.
    None,
    /// Uniform on `[−w, w]`.
    #[default]
    Uniform,
    /// Normal with standard deviation `w/2`, truncated to `[−w, w]`.
    Normal,
    /// A beeswarm: points moved sideways just enough not to overlap
    /// ([`swarm_offsets`]), within `[−w, w]`.
    Swarm,
}

impl JitterKind {
    /// Every kind, in menu order.
    pub const ALL: [JitterKind; 4] = [JitterKind::None, JitterKind::Uniform, JitterKind::Normal, JitterKind::Swarm];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Uniform => "Uniform",
            Self::Normal => "Normal",
            Self::Swarm => "Swarm (no overlap)",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Uniform => "uniform",
            Self::Normal => "normal",
            Self::Swarm => "swarm",
        }
    }

    /// The kind with this [`JitterKind::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// A deterministic uniform number in `[0, 1)` for `(row, stream)`
/// (SplitMix64 finalizer), so a row's jitter is the same in every frame and
/// every panel.
fn unit(row: usize, stream: u64) -> f64 {
    let mut z = (row as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ stream.wrapping_mul(0xD1B5_4A32_D192_ED03);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// The jitter offset of `row` for an axis (`axis` 0 or 1 keeps the two axes
/// independent), within `[−width, width]`. A swarm depends on the other
/// points of the slot, so here it is 0 (see [`swarm_offsets`]).
#[must_use]
pub fn jitter_offset(kind: JitterKind, width: f64, row: usize, axis: u64) -> f64 {
    let width = if width.is_finite() { width.abs() } else { 0.0 };
    match kind {
        JitterKind::None | JitterKind::Swarm => 0.0,
        JitterKind::Uniform => (2.0 * unit(row, 2 * axis + 1) - 1.0) * width,
        JitterKind::Normal => {
            // Box–Muller with u₁ in (0, 1].
            let u1 = 1.0 - unit(row, 2 * axis + 1);
            let u2 = unit(row, 2 * axis + 2);
            let z = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
            (0.5 * width * z).clamp(-width, width)
        }
    }
}

/// Most points in one slot laid out as a swarm; larger slots fall back to
/// uniform jitter (the greedy placement grows quadratically with ties).
pub const MAX_SWARM_POINTS: usize = 2000;

/// Sideways offsets that lay markers out as a beeswarm: no two markers
/// overlap and each sits as close to the centre line as it can.
///
/// `values[i]` is marker `i`'s position along the value axis and
/// `radii[i]` its radius, both in the same units (screen points); the
/// result is each marker's offset across, in those units.
///
/// **Algorithm** (the greedy "swarm" placement of Eklund's R package
/// *beeswarm*, CRAN 2016; the column of dots is the idea of Wilkinson's dot
/// plot, *The American Statistician* 53(3), 1999, without binning): markers are placed one at a
/// time in ascending order of value (ties in input order). A marker at value
/// `y` touches an already placed marker `j` (value `yⱼ`, offset `xⱼ`) when
/// `(x − xⱼ)² + (y − yⱼ)² < (r + rⱼ + gap)²`, with `gap` a half-point margin.
/// The candidate offsets are 0 and, for every placed marker within reach,
/// `xⱼ ± √((r + rⱼ + gap)² − (y − yⱼ)²)` (just touching it on either side);
/// the marker takes the candidate of smallest `|x|` (the negative one first
/// on a tie) that touches no placed marker. Only markers within `r + rⱼ +
/// gap` along the value axis can touch, so a sliding window over the sorted
/// values keeps the cost near `O(n·w²)` for `w` markers per window.
///
/// The layout is deterministic. Non-finite values or radii get offset 0.
#[must_use]
pub fn swarm_offsets(values: &[f64], radii: &[f64]) -> Vec<f64> {
    const GAP: f64 = 0.5;
    let n = values.len();
    let mut out = vec![0.0; n];
    let radius = |i: usize| radii.get(i).copied().filter(|r| r.is_finite() && *r >= 0.0).unwrap_or(0.0);
    let mut order: Vec<usize> = (0..n).filter(|&i| values[i].is_finite()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]).then(a.cmp(&b)));
    let max_r = order.iter().map(|&i| radius(i)).fold(0.0, f64::max);
    // Placed markers in ascending value: (value, offset, radius).
    let mut placed: Vec<(f64, f64, f64)> = Vec::with_capacity(order.len());
    let mut start = 0;
    let mut candidates: Vec<f64> = Vec::new();
    for &i in &order {
        let (y, r) = (values[i], radius(i));
        // Markers further below than any contact distance can never touch.
        while start < placed.len() && placed[start].0 < y - (r + max_r + GAP) {
            start += 1;
        }
        let window = &placed[start..];
        candidates.clear();
        candidates.push(0.0);
        for &(yj, xj, rj) in window {
            let reach = r + rj + GAP;
            let dy = y - yj;
            if dy.abs() < reach {
                let dx = (reach * reach - dy * dy).sqrt();
                candidates.push(xj - dx);
                candidates.push(xj + dx);
            }
        }
        candidates.sort_by(|a, b| a.abs().total_cmp(&b.abs()).then(a.total_cmp(b)));
        let free = |x: f64| {
            window.iter().all(|&(yj, xj, rj)| {
                let reach = r + rj + GAP;
                (x - xj).powi(2) + (y - yj).powi(2) >= reach * reach * (1.0 - 1e-9)
            })
        };
        let x = candidates.iter().copied().find(|&x| free(x)).unwrap_or(0.0);
        out[i] = x;
        // Keep `placed` sorted by value (values arrive in ascending order).
        placed.push((y, x, r));
    }
    out
}

// ---------------------------------------------------------------------------
// Box plots
// ---------------------------------------------------------------------------

/// Tukey's box-and-whisker summary (Tukey, *Exploratory Data Analysis*,
/// 1977): quartiles, whiskers to the most extreme observations within the
/// inner fences `Q1 − 1.5·IQR` and `Q3 + 1.5·IQR`, and the observations beyond
/// them. Quartiles use linear interpolation between order statistics
/// (Hyndman & Fan 1996, definition 7).
#[derive(Debug, Clone, PartialEq)]
pub struct BoxStats {
    /// Number of observations.
    pub n: usize,
    /// First quartile.
    pub q1: f64,
    /// Median.
    pub median: f64,
    /// Third quartile.
    pub q3: f64,
    /// Lower inner fence `Q1 − 1.5·IQR`.
    pub lower_fence: f64,
    /// Upper inner fence `Q3 + 1.5·IQR`.
    pub upper_fence: f64,
    /// Lowest observation within the fences.
    pub lower_whisker: f64,
    /// Highest observation within the fences.
    pub upper_whisker: f64,
    /// Observations beyond the fences, as `(index into the input, value)`.
    pub outliers: Vec<(usize, f64)>,
}

/// Quantile `p` of sorted finite values by linear interpolation (type 7).
fn quantile_sorted(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let h = (n - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

/// Box statistics of the finite values in `values` (`None` if there are none).
#[must_use]
pub fn box_stats(values: &[f64]) -> Option<BoxStats> {
    let mut sorted: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    let (q1, median, q3) =
        (quantile_sorted(&sorted, 0.25), quantile_sorted(&sorted, 0.5), quantile_sorted(&sorted, 0.75));
    let iqr = q3 - q1;
    let (lower_fence, upper_fence) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
    let inside = sorted.iter().copied().filter(|&v| v >= lower_fence && v <= upper_fence);
    let lower_whisker = inside.clone().fold(f64::INFINITY, f64::min).min(q1);
    let upper_whisker = inside.fold(f64::NEG_INFINITY, f64::max).max(q3);
    let outliers = values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite() && (**v < lower_fence || **v > upper_fence))
        .map(|(i, &v)| (i, v))
        .collect();
    Some(BoxStats { n: sorted.len(), q1, median, q3, lower_fence, upper_fence, lower_whisker, upper_whisker, outliers })
}

/// Quantile `p` of values with frequencies, `(value, frequency)` pairs in any
/// order: definition 7 of Hyndman & Fan applied to the sample in which each
/// value appears `frequency` times (fractional frequencies interpolate the
/// cumulative count). Pairs with a non-finite value or a frequency that is
/// not positive are ignored; `None` when nothing remains.
#[must_use]
pub fn weighted_quantile(pairs: &[(f64, f64)], p: f64) -> Option<f64> {
    let mut sorted: Vec<(f64, f64)> =
        pairs.iter().copied().filter(|(v, f)| v.is_finite() && f.is_finite() && *f > 0.0).collect();
    if sorted.is_empty() || !p.is_finite() {
        return None;
    }
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some(weighted_quantile_sorted(&sorted, p))
}

/// [`weighted_quantile`] of pairs already sorted by value and filtered.
fn weighted_quantile_sorted(sorted: &[(f64, f64)], p: f64) -> f64 {
    let total: f64 = sorted.iter().map(|q| q.1).sum();
    // Value at 0-based position `pos` of the expanded sample.
    let at = |pos: f64| -> f64 {
        let mut cumulative = 0.0;
        for &(v, f) in sorted {
            cumulative += f;
            if pos < cumulative - 1e-12 {
                return v;
            }
        }
        sorted[sorted.len() - 1].0
    };
    let h = ((total - 1.0) * p.clamp(0.0, 1.0)).max(0.0);
    let lo = h.floor();
    let (a, b) = (at(lo), at((lo + 1.0).min((total - 1.0).max(0.0))));
    a + (h - lo) * (b - a)
}

/// [`box_stats`] with frequencies: `freq[i]` copies of `values[i]` (a missing
/// or empty `freq` means one each; rows with frequency ≤ 0 are left out).
/// `n` counts the rows used; outlier indices refer to `values`.
#[must_use]
pub fn box_stats_weighted(values: &[f64], freq: &[f64]) -> Option<BoxStats> {
    if freq.is_empty() {
        return box_stats(values);
    }
    let f = |i: usize| freq.get(i).copied().unwrap_or(1.0);
    let used: Vec<(usize, f64)> =
        values.iter().copied().enumerate().filter(|&(i, v)| v.is_finite() && f(i).is_finite() && f(i) > 0.0).collect();
    if used.is_empty() {
        return None;
    }
    let mut sorted: Vec<(f64, f64)> = used.iter().map(|&(i, v)| (v, f(i))).collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (q1, median, q3) = (
        weighted_quantile_sorted(&sorted, 0.25),
        weighted_quantile_sorted(&sorted, 0.5),
        weighted_quantile_sorted(&sorted, 0.75),
    );
    let iqr = q3 - q1;
    let (lower_fence, upper_fence) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
    let inside = used.iter().map(|u| u.1).filter(|&v| v >= lower_fence && v <= upper_fence);
    let lower_whisker = inside.clone().fold(f64::INFINITY, f64::min).min(q1);
    let upper_whisker = inside.fold(f64::NEG_INFINITY, f64::max).max(q3);
    let outliers = used.iter().copied().filter(|&(_, v)| v < lower_fence || v > upper_fence).collect();
    Some(BoxStats { n: used.len(), q1, median, q3, lower_fence, upper_fence, lower_whisker, upper_whisker, outliers })
}

// ---------------------------------------------------------------------------
// Polynomial fits
// ---------------------------------------------------------------------------

/// Lower and upper curves of a confidence band.
pub type Band = (Vec<[f64; 2]>, Vec<[f64; 2]>);

/// A least-squares polynomial `y = β₀ + β₁x + … + β_d x^d` evaluated along
/// the range of `x`, with pointwise confidence bands for the mean and for a
/// new observation, and the usual fit statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct PolyFit {
    /// Degree actually fitted (lowered when there are too few distinct x).
    pub degree: usize,
    /// `[x, ŷ]` along the data's x range.
    pub curve: Vec<[f64; 2]>,
    /// Lower and upper confidence limits for the mean along the same grid
    /// (`None` when there are no residual degrees of freedom).
    pub band: Option<Band>,
    /// Lower and upper prediction limits for one new observation of unit
    /// weight (`None` without residual degrees of freedom).
    pub prediction_band: Option<Band>,
    /// Coefficients `β₀, β₁, …, β_d` of powers of x, in the units the fit was
    /// computed in.
    pub coefficients: Vec<f64>,
    /// Coefficient of determination (`None` when y has no spread).
    pub r_squared: Option<f64>,
    /// p-value of the overall F test that every slope term is zero (`None`
    /// without residual degrees of freedom or spread in y).
    pub p_value: Option<f64>,
    /// Root mean squared error `√(SSE / df)` (`None` without residual df).
    pub rmse: Option<f64>,
    /// Residual degrees of freedom: total frequency minus coefficients.
    pub df: f64,
    /// Rows used (positive frequency and weight).
    pub n: usize,
}

impl PolyFit {
    /// The fitted equation, e.g. `y = 1.5 + 2.03·x − 0.4·x²`, with `y` and `x`
    /// replaced by the given names and coefficients to four significant digits.
    #[must_use]
    pub fn equation(&self, y: &str, x: &str) -> String {
        let mut out = format!("{y} = {}", crate::view::format_value(self.coefficients.first().copied().unwrap_or(0.0)));
        for (k, &c) in self.coefficients.iter().enumerate().skip(1) {
            let sign = if c < 0.0 { "−" } else { "+" };
            let power = match k {
                1 => String::new(),
                2 => "²".to_owned(),
                3 => "³".to_owned(),
                4 => "⁴".to_owned(),
                5 => "⁵".to_owned(),
                _ => format!("^{k}"),
            };
            out.push_str(&format!(" {sign} {}·{x}{power}", crate::view::format_value(c.abs())));
        }
        out
    }
}

/// Points along each fitted curve.
pub const CURVE_POINTS: usize = 64;

/// Fit a polynomial of degree `degree` (1 to 5) to the finite `(x, y)` pairs,
/// with `level` confidence and prediction bands. See [`poly_fit_weighted`].
#[must_use]
pub fn poly_fit(x: &[f64], y: &[f64], degree: usize, level: f64) -> Option<PolyFit> {
    poly_fit_weighted(x, y, &[], &[], degree, level)
}

/// Weighted least-squares polynomial fit.
///
/// Each row `i` carries a frequency `fᵢ` (how many identical observations it
/// stands for) and a weight `wᵢ` (inverse relative variance); missing slices
/// mean 1 for every row. The estimates minimize `Σ fᵢ wᵢ (yᵢ − ŷᵢ)²`; the
/// residual degrees of freedom are `Σ fᵢ − (d + 1)` and `σ̂² = SSE / df`. The
/// confidence band is `ŷ(x₀) ± t·σ̂·√(x₀ᵀ(XᵀFWX)⁻¹x₀)`, the prediction band
/// adds `σ̂²` (a new observation of unit weight) under the root, and the
/// overall F test compares the model with the weighted mean.
///
/// The predictor is centred and scaled to `[−1, 1]` before forming powers, for
/// conditioning, and the system is solved by Householder QR. The degree is
/// lowered to (number of distinct x − 1); `None` if that leaves no slope
/// (fewer than two distinct x among rows with positive frequency and weight)
/// or the result is not finite.
#[must_use]
pub fn poly_fit_weighted(
    x: &[f64],
    y: &[f64],
    freq: &[f64],
    weight: &[f64],
    degree: usize,
    level: f64,
) -> Option<PolyFit> {
    let at = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(1.0);
    // (x, y, f, w) with everything finite and f·w > 0.
    let rows: Vec<[f64; 4]> = x
        .iter()
        .zip(y)
        .enumerate()
        .map(|(i, (&a, &b))| [a, b, at(freq, i), at(weight, i)])
        .filter(|r| r.iter().all(|v| v.is_finite()) && r[2] > 0.0 && r[3] > 0.0)
        .collect();
    let mut distinct: Vec<f64> = rows.iter().map(|r| r[0]).collect();
    distinct.sort_by(f64::total_cmp);
    distinct.dedup();
    let degree = degree.clamp(1, 5).min(distinct.len().saturating_sub(1));
    if degree == 0 {
        return None;
    }
    let p = degree + 1;
    let (lo, hi) = (distinct[0], distinct[distinct.len() - 1]);
    let centre = 0.5 * (lo + hi);
    let half = 0.5 * (hi - lo);
    let basis = |v: f64| -> Vec<f64> {
        let u = (v - centre) / half;
        (0..p).map(|k| u.powi(k as i32)).collect()
    };
    // Rows of √(fw)·[1, u, u², …] and √(fw)·y.
    let scaled: Vec<Vec<f64>> =
        rows.iter().map(|r| basis(r[0]).into_iter().map(|b| b * (r[2] * r[3]).sqrt()).collect()).collect();
    let yw: Vec<f64> = rows.iter().map(|r| r[1] * (r[2] * r[3]).sqrt()).collect();
    let qr = QrDecomposition::new(Matrix::from_rows(&scaled).ok()?).ok()?;
    let beta = qr.solve_least_squares(&yw).ok()?;
    let gram_inv = qr.inverse_gram().ok()?;
    let predict = |b: &[f64]| b.iter().zip(&beta).map(|(a, c)| a * c).sum::<f64>();
    let total_f: f64 = rows.iter().map(|r| r[2]).sum();
    let sum_fw: f64 = rows.iter().map(|r| r[2] * r[3]).sum();
    let mean_y = rows.iter().map(|r| r[2] * r[3] * r[1]).sum::<f64>() / sum_fw;
    let sse: f64 = rows.iter().map(|r| r[2] * r[3] * (r[1] - predict(&basis(r[0]))).powi(2)).sum();
    let sst: f64 = rows.iter().map(|r| r[2] * r[3] * (r[1] - mean_y).powi(2)).sum();
    let df = total_f - p as f64;
    let spread = sst > 1e-24 * sum_fw * mean_y.abs().max(1.0).powi(2) && sst.is_finite();
    let r_squared = spread.then(|| (1.0 - sse / sst).clamp(0.0, 1.0));
    let sigma2 = (df > 0.0).then(|| (sse / df).max(0.0));
    let p_value = match (sigma2, spread) {
        (Some(s2), true) if s2 > 0.0 => {
            let f_stat = ((sst - sse).max(0.0) / degree as f64) / s2;
            FisherSnedecor::new(degree as f64, df).ok().and_then(|d| d.sf(f_stat).ok())
        }
        (Some(_), true) => Some(0.0),
        _ => None,
    };
    let grid: Vec<f64> = (0..CURVE_POINTS).map(|i| lo + (hi - lo) * i as f64 / (CURVE_POINTS - 1) as f64).collect();
    let curve: Vec<[f64; 2]> = grid.iter().map(|&g| [g, predict(&basis(g))]).collect();
    if curve.iter().any(|c| !c[1].is_finite()) {
        return None;
    }
    let quadratic = |b: &[f64]| -> f64 {
        let mut s = 0.0;
        for i in 0..p {
            for j in 0..p {
                s += b[i] * gram_inv[(i, j)] * b[j];
            }
        }
        s.max(0.0)
    };
    let bands = sigma2.and_then(|s2| {
        let t = StudentsT::new(df).ok()?.quantile(0.5 + 0.5 * level.clamp(0.5, 0.999)).ok()?;
        let mut mean = (Vec::with_capacity(grid.len()), Vec::with_capacity(grid.len()));
        let mut new_obs = (Vec::with_capacity(grid.len()), Vec::with_capacity(grid.len()));
        for (&g, c) in grid.iter().zip(&curve) {
            let h = quadratic(&basis(g));
            let (m, n) = (t * (s2 * h).sqrt(), t * (s2 * (1.0 + h)).sqrt());
            mean.0.push([g, c[1] - m]);
            mean.1.push([g, c[1] + m]);
            new_obs.0.push([g, c[1] - n]);
            new_obs.1.push([g, c[1] + n]);
        }
        let finite = |b: &Band| b.0.iter().chain(&b.1).all(|q| q[1].is_finite());
        (finite(&mean) && finite(&new_obs)).then_some((mean, new_obs))
    });
    let (band, prediction_band) = match bands {
        Some((m, n)) => (Some(m), Some(n)),
        None => (None, None),
    };
    // Coefficients of powers of x: expand Σ βₖ ((x − c)/h)ᵏ.
    let mut coefficients = vec![0.0; p];
    for (k, &b) in beta.iter().enumerate() {
        let scale = b / half.powi(k as i32);
        for (j, slot) in coefficients.iter_mut().enumerate().take(k + 1) {
            *slot += scale * binomial(k, j) * (-centre).powi((k - j) as i32);
        }
    }
    Some(PolyFit {
        degree,
        curve,
        band,
        prediction_band,
        coefficients,
        r_squared,
        p_value,
        rmse: sigma2.map(f64::sqrt),
        df,
        n: rows.len(),
    })
}

/// The binomial coefficient `C(n, k)` as a float.
fn binomial(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

// ---------------------------------------------------------------------------
// Smoothing spline
// ---------------------------------------------------------------------------

/// The cubic smoothing spline: the function `g` minimizing
/// `Σ wᵢ (yᵢ − g(xᵢ))² + λ ∫ g''(t)² dt`, a natural cubic spline with knots at
/// the distinct x (Reinsch 1967; Green & Silverman, *Nonparametric Regression
/// and Generalized Linear Models*, 1994, §2.3). Ties are merged into weighted
/// means. `λ → 0` interpolates the (merged) data; `λ → ∞` gives the
/// least-squares line.
///
/// The spline is fitted in standardized units `u = (x − x_min) / range`, so
/// the smoothing parameter is unit-free and no intermediate quantity
/// overflows however large or small x is.
#[derive(Debug, Clone, PartialEq)]
pub struct SmoothingSpline {
    /// Distinct x (knots), ascending, in data units.
    pub knots: Vec<f64>,
    /// Fitted values at the knots.
    pub values: Vec<f64>,
    /// Second derivatives with respect to the standardized `u` at the knots
    /// (zero at both ends: a natural spline).
    pub second_derivatives: Vec<f64>,
    /// Knots in standardized units.
    standardized: Vec<f64>,
}

/// Most distinct x for which a smoother's confidence band is computed.
pub const MAX_BAND_KNOTS: usize = 2000;

/// A smoothed curve with its optional confidence band.
#[derive(Debug, Clone, PartialEq)]
pub struct SmoothFit {
    /// `[x, ŷ]` across the data.
    pub curve: Vec<[f64; 2]>,
    /// Pointwise confidence band (`None` when it cannot be computed).
    pub band: Option<Band>,
    /// Equivalent degrees of freedom of the smoother (trace of its hat
    /// matrix; 0 when not computed).
    pub equivalent_df: f64,
}

/// A fitted spline with the data and system behind it.
struct SplineFit {
    spline: SmoothingSpline,
    /// Sorted `(x, y, normalized weight)` of the observations used.
    pairs: Vec<(f64, f64, f64)>,
    /// Knot weights (sums of normalized weights).
    weights: Vec<f64>,
    /// The factored system (`None` with two knots or fewer).
    system: Option<SplineSystem>,
    /// Number of observations (total frequency of the rows used).
    observations: f64,
}

/// The Reinsch system `(R + λ QᵀW⁻¹Q) γ = Qᵀ v` for fixed knots, weights and
/// λ, factored once so the smoother can be applied to many `v`.
struct SplineSystem {
    h: Vec<f64>,
    w: Vec<f64>,
    lambda: f64,
    factor: Pentadiagonal,
}

impl SplineSystem {
    fn q(&self, j: usize) -> [f64; 3] {
        let h = &self.h;
        [1.0 / h[j], -1.0 / h[j] - 1.0 / h[j + 1], 1.0 / h[j + 1]]
    }

    fn new(h: Vec<f64>, w: Vec<f64>, lambda: f64) -> Option<Self> {
        let m = h.len() - 1;
        let q = |j: usize| -> [f64; 3] { [1.0 / h[j], -1.0 / h[j] - 1.0 / h[j + 1], 1.0 / h[j + 1]] };
        // A = R + λ QᵀW⁻¹Q is symmetric pentadiagonal: diagonals d0, d1, d2.
        let (mut d0, mut d1, mut d2) = (vec![0.0; m], vec![0.0; m], vec![0.0; m]);
        for j in 0..m {
            let a = q(j);
            d0[j] = (h[j] + h[j + 1]) / 3.0
                + lambda * (a[0] * a[0] / w[j] + a[1] * a[1] / w[j + 1] + a[2] * a[2] / w[j + 2]);
            if j + 1 < m {
                let b = q(j + 1);
                d1[j] = h[j + 1] / 6.0 + lambda * (a[1] * b[0] / w[j + 1] + a[2] * b[1] / w[j + 2]);
            }
            if j + 2 < m {
                d2[j] = lambda * (a[2] * q(j + 2)[0] / w[j + 2]);
            }
        }
        let factor = Pentadiagonal::factor(&d0, &d1, &d2)?;
        Some(Self { h, w, lambda, factor })
    }

    /// Smoothed values `g` and second derivatives `γ` (zero at the ends) for
    /// knot values `v`.
    fn smooth(&self, v: &[f64]) -> Option<(Vec<f64>, Vec<f64>)> {
        let n = v.len();
        let m = n - 2;
        let rhs: Vec<f64> = (0..m)
            .map(|j| {
                let a = self.q(j);
                a[0] * v[j] + a[1] * v[j + 1] + a[2] * v[j + 2]
            })
            .collect();
        let gamma_inner = self.factor.solve(&rhs)?;
        // g = v − λ W⁻¹ Q γ.
        let mut g = v.to_vec();
        for (j, &gj) in gamma_inner.iter().enumerate() {
            for (k, coef) in self.q(j).iter().enumerate() {
                g[j + k] -= self.lambda * coef * gj / self.w[j + k];
            }
        }
        let mut gamma = vec![0.0; n];
        gamma[1..n - 1].copy_from_slice(&gamma_inner);
        g.iter().all(|x| x.is_finite()).then_some((g, gamma))
    }
}

impl SmoothingSpline {
    /// Fit to the finite `(x, y)` pairs with smoothing parameter
    /// `relative_lambda` on the standardized scale (equivalently `λ =
    /// relative · range³` in data units), so one setting smooths alike
    /// whatever the units of x. `None` with no finite pairs or if the banded
    /// system cannot be solved.
    #[must_use]
    pub fn fit(x: &[f64], y: &[f64], relative_lambda: f64) -> Option<Self> {
        Self::fit_weighted(x, y, &[], relative_lambda)
    }

    /// [`SmoothingSpline::fit`] with a weight per observation (`wᵢ` in the
    /// criterion; an empty slice means 1 each). Observations whose weight is
    /// not positive are left out. Since the criterion scales with the total
    /// weight, weights are normalized to average 1, so λ means the same with
    /// or without them.
    #[must_use]
    pub fn fit_weighted(x: &[f64], y: &[f64], weight: &[f64], relative_lambda: f64) -> Option<Self> {
        Self::fit_internal(x, y, &[], weight, relative_lambda).map(|f| f.spline)
    }

    /// [`SmoothingSpline::fit_weighted`] where row `i` also stands for
    /// `freq[i]` identical observations (empty: 1 each; rows whose
    /// frequency is not positive are left out): the criterion is
    /// `Σ fᵢwᵢ (yᵢ − g(xᵢ))² + λ ∫ g''²` with weights normalized to average 1
    /// per observation, so a row of frequency 2 fits exactly as the row
    /// listed twice.
    #[must_use]
    pub fn fit_with_frequencies(
        x: &[f64],
        y: &[f64],
        freq: &[f64],
        weight: &[f64],
        relative_lambda: f64,
    ) -> Option<Self> {
        Self::fit_internal(x, y, freq, weight, relative_lambda).map(|f| f.spline)
    }

    /// The fit with what the confidence band needs.
    fn fit_internal(x: &[f64], y: &[f64], freq: &[f64], weight: &[f64], relative_lambda: f64) -> Option<SplineFit> {
        let wt = |i: usize| weight.get(i).copied().unwrap_or(1.0);
        let fr = |i: usize| freq.get(i).copied().unwrap_or(1.0);
        // (x, y, f, w) of the usable rows.
        let rows: Vec<[f64; 4]> = x
            .iter()
            .zip(y)
            .enumerate()
            .map(|(i, (&a, &b))| [a, b, fr(i), wt(i)])
            .filter(|r| r.iter().all(|v| v.is_finite()) && r[2] > 0.0 && r[3] > 0.0)
            .collect();
        if rows.is_empty() {
            return None;
        }
        let total_f: f64 = rows.iter().map(|r| r[2]).sum();
        let mut pairs: Vec<(f64, f64, f64)> = rows.iter().map(|r| (r[0], r[1], r[2] * r[3])).collect();
        let mean_w = pairs.iter().map(|p| p.2).sum::<f64>() / total_f;
        for p in &mut pairs {
            p.2 /= mean_w;
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Merge ties: knots t, weighted means v, weights w.
        let (mut t, mut v, mut w): (Vec<f64>, Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new(), Vec::new());
        for &(a, b, c) in &pairs {
            match t.last() {
                Some(&last) if a == last => {
                    let k = t.len() - 1;
                    w[k] += c;
                    v[k] += (b - v[k]) * c / w[k];
                }
                _ => {
                    t.push(a);
                    v.push(b);
                    w.push(c);
                }
            }
        }
        let n = t.len();
        let range = t[n - 1] - t[0];
        let standardized: Vec<f64> =
            if range > 0.0 { t.iter().map(|&k| (k - t[0]) / range).collect() } else { vec![0.0; n] };
        if n <= 2 || !(range.is_finite() && range > 0.0) {
            // No curvature to penalize: the (weighted) means at the knots.
            let spline = Self { second_derivatives: vec![0.0; n], knots: t, values: v, standardized };
            return Some(SplineFit { spline, pairs, weights: w, system: None, observations: total_f });
        }
        let lambda = if relative_lambda.is_finite() { relative_lambda.max(0.0) } else { 1e300 };
        let h: Vec<f64> = standardized.windows(2).map(|p| p[1] - p[0]).collect();
        if h.iter().any(|&d| d.is_nan() || d <= 0.0) {
            // Knots closer than floating point can separate after scaling.
            return None;
        }
        let system = SplineSystem::new(h, w.clone(), lambda)?;
        let (g, gamma) = system.smooth(&v)?;
        let spline = Self { knots: t, values: g, second_derivatives: gamma, standardized };
        Some(SplineFit { spline, pairs, weights: w, system: Some(system), observations: total_f })
    }

    /// The spline with a pointwise `level` confidence band, evaluated at
    /// `points` x across the data.
    ///
    /// The fit is linear in the data, `ĝ = S v` at the knots (merged means
    /// `v` with weights `w`), so `Var(ĝᵢ) = σ² Σⱼ Sᵢⱼ² / wⱼ`; `σ̂² = RSS / (n −
    /// tr S)` with `tr S` the equivalent degrees of freedom (Green & Silverman
    /// §3.3), and the band is `ĝ ± t·SE` with `n − tr S` degrees of freedom.
    /// Between knots the standard error is interpolated linearly. The band is
    /// left out (`None`) with more than [`MAX_BAND_KNOTS`] distinct x (the
    /// computation is quadratic in their number) or no residual degrees of
    /// freedom.
    #[must_use]
    pub fn fit_with_band(
        x: &[f64],
        y: &[f64],
        weight: &[f64],
        relative_lambda: f64,
        level: f64,
        points: usize,
    ) -> Option<SmoothFit> {
        Self::fit_with_band_and_frequencies(x, y, &[], weight, relative_lambda, level, points)
    }

    /// [`SmoothingSpline::fit_with_band`] with frequencies as in
    /// [`SmoothingSpline::fit_with_frequencies`]: `n` in `n − tr S` is the
    /// total frequency, so a row of frequency 2 gives the same band as the
    /// row listed twice.
    #[must_use]
    pub fn fit_with_band_and_frequencies(
        x: &[f64],
        y: &[f64],
        freq: &[f64],
        weight: &[f64],
        relative_lambda: f64,
        level: f64,
        points: usize,
    ) -> Option<SmoothFit> {
        let fit = Self::fit_internal(x, y, freq, weight, relative_lambda)?;
        let spline = &fit.spline;
        let curve = spline.curve(points);
        let n_obs = fit.observations;
        let k = spline.knots.len();
        // Columns of S: the smoother applied to each unit vector.
        let (trace, var_factor) = match &fit.system {
            Some(system) if k <= MAX_BAND_KNOTS => {
                let mut factor = vec![0.0; k];
                let mut trace = 0.0;
                let mut unit = vec![0.0; k];
                for j in 0..k {
                    unit[j] = 1.0;
                    let (col, _) = system.smooth(&unit)?;
                    unit[j] = 0.0;
                    trace += col[j];
                    for (f, s) in factor.iter_mut().zip(&col) {
                        *f += s * s / fit.weights[j];
                    }
                }
                (trace, Some(factor))
            }
            Some(_) => (0.0, None),
            // Means at one or two knots: S is the identity.
            None => (k as f64, Some(fit.weights.iter().map(|w| 1.0 / w).collect())),
        };
        let df = n_obs - trace;
        let rss: f64 = fit.pairs.iter().map(|&(a, b, c)| c * (b - spline.value(a)).powi(2)).sum();
        let band = match var_factor {
            Some(factor) if df > 0.5 && rss.is_finite() => {
                let sigma2 = rss / df;
                let t = StudentsT::new(df).ok()?.quantile(0.5 + 0.5 * level.clamp(0.5, 0.999)).ok()?;
                let se_at = |xv: f64| -> f64 {
                    let knots = &spline.knots;
                    let i = knots.partition_point(|&q| q <= xv).saturating_sub(1).min(k.saturating_sub(1));
                    let j = (i + 1).min(k - 1);
                    let f = if j > i && knots[j] > knots[i] {
                        let s = (xv - knots[i]) / (knots[j] - knots[i]);
                        factor[i] + s.clamp(0.0, 1.0) * (factor[j] - factor[i])
                    } else {
                        factor[i]
                    };
                    (sigma2 * f.max(0.0)).sqrt()
                };
                let lower: Vec<[f64; 2]> = curve.iter().map(|p| [p[0], p[1] - t * se_at(p[0])]).collect();
                let upper: Vec<[f64; 2]> = curve.iter().map(|p| [p[0], p[1] + t * se_at(p[0])]).collect();
                lower.iter().chain(&upper).all(|p| p[1].is_finite()).then_some((lower, upper))
            }
            _ => None,
        };
        Some(SmoothFit { curve, band, equivalent_df: trace })
    }

    /// The spline at `x` (Green & Silverman, eq. 2.4); outside the knots'
    /// range it is held at the end values (curves are drawn within the range).
    #[must_use]
    pub fn value(&self, x: f64) -> f64 {
        let (t, g, gamma) = (&self.knots, &self.values, &self.second_derivatives);
        let n = t.len();
        if n == 1 || x <= t[0] {
            return g[0];
        }
        if x >= t[n - 1] {
            return g[n - 1];
        }
        let u = (x - t[0]) / (t[n - 1] - t[0]);
        let s = &self.standardized;
        let i = s.partition_point(|&k| k <= u).saturating_sub(1).min(n - 2);
        let (a, b, h) = ((u - s[i]).max(0.0), (s[i + 1] - u).max(0.0), s[i + 1] - s[i]);
        (a * g[i + 1] + b * g[i]) / h - a * b / 6.0 * ((1.0 + a / h) * gamma[i + 1] + (1.0 + b / h) * gamma[i])
    }

    /// `[x, ĝ(x)]` at `points` evenly spaced x across the knots' range (a
    /// single point when all x are equal).
    #[must_use]
    pub fn curve(&self, points: usize) -> Vec<[f64; 2]> {
        let (lo, hi) = (self.knots[0], self.knots[self.knots.len() - 1]);
        if hi <= lo {
            return vec![[lo, self.values[0]]];
        }
        let points = points.max(2);
        (0..points)
            .map(|s| {
                let x = lo + (hi - lo) * s as f64 / (points - 1) as f64;
                [x, self.value(x)]
            })
            .collect()
    }
}

/// Banded Cholesky factor `A = LLᵀ` of a symmetric positive-definite
/// pentadiagonal matrix (main diagonal `d0`, super-diagonals `d1`, `d2`).
struct Pentadiagonal {
    /// Diagonal of L.
    l0: Vec<f64>,
    /// First sub-diagonal (row j, column j−1).
    l1: Vec<f64>,
    /// Second sub-diagonal (row j, column j−2).
    l2: Vec<f64>,
}

impl Pentadiagonal {
    /// Factor; `None` if the matrix is not positive definite.
    fn factor(d0: &[f64], d1: &[f64], d2: &[f64]) -> Option<Self> {
        let m = d0.len();
        let (mut l0, mut l1, mut l2) = (vec![0.0; m], vec![0.0; m], vec![0.0; m]);
        for j in 0..m {
            if j >= 2 {
                l2[j] = d2[j - 2] / l0[j - 2];
            }
            if j >= 1 {
                let tail = if j >= 2 { l2[j] * l1[j - 1] } else { 0.0 };
                l1[j] = (d1[j - 1] - tail) / l0[j - 1];
            }
            let s = d0[j] - l1[j] * l1[j] - l2[j] * l2[j];
            if !(s > 0.0 && s.is_finite()) {
                return None;
            }
            l0[j] = s.sqrt();
        }
        Some(Self { l0, l1, l2 })
    }

    /// Solve `A x = b`.
    fn solve(&self, b: &[f64]) -> Option<Vec<f64>> {
        let (l0, l1, l2) = (&self.l0, &self.l1, &self.l2);
        let m = l0.len();
        // Forward: L z = b.
        let mut z = vec![0.0; m];
        for j in 0..m {
            let mut s = b[j];
            if j >= 1 {
                s -= l1[j] * z[j - 1];
            }
            if j >= 2 {
                s -= l2[j] * z[j - 2];
            }
            z[j] = s / l0[j];
        }
        // Backward: Lᵀ x = z.
        let mut x = vec![0.0; m];
        for j in (0..m).rev() {
            let mut s = z[j];
            if j + 1 < m {
                s -= l1[j + 1] * x[j + 1];
            }
            if j + 2 < m {
                s -= l2[j + 2] * x[j + 2];
            }
            x[j] = s / l0[j];
        }
        x.iter().all(|v| v.is_finite()).then_some(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swarm_markers_never_overlap_and_hug_the_centre() {
        let values: Vec<f64> = (0..300).map(|i| ((i * 37) % 101) as f64 * 0.8).collect();
        let radii = vec![4.0; values.len()];
        let x = swarm_offsets(&values, &radii);
        for i in 0..values.len() {
            for j in i + 1..values.len() {
                let d = ((x[i] - x[j]).powi(2) + (values[i] - values[j]).powi(2)).sqrt();
                assert!(d >= 8.5 - 1e-6, "{i} and {j} overlap: {d}");
            }
        }
        assert_eq!(x, swarm_offsets(&values, &radii), "deterministic");
        // A single column of distinct, well separated values stays on the line.
        let apart = swarm_offsets(&[0.0, 10.0, 20.0], &[4.0; 3]);
        assert_eq!(apart, [0.0, 0.0, 0.0]);
        // Ties spread symmetrically: 0, −d, +d, −2d, +2d.
        let tied = swarm_offsets(&[5.0; 5], &[2.0; 5]);
        assert_eq!(tied, [0.0, -4.5, 4.5, -9.0, 9.0]);
        // Non-finite values stay put.
        assert_eq!(swarm_offsets(&[f64::NAN, 1.0], &[3.0, f64::NAN]), [0.0, 0.0]);
        assert_eq!(jitter_offset(JitterKind::Swarm, 0.3, 4, 0), 0.0);
        assert_eq!(JitterKind::from_key("swarm"), Some(JitterKind::Swarm));
    }

    #[test]
    fn jitter_is_bounded_deterministic_and_centred() {
        for kind in [JitterKind::Uniform, JitterKind::Normal] {
            let offsets: Vec<f64> = (0..5000).map(|r| jitter_offset(kind, 0.3, r, 0)).collect();
            assert!(offsets.iter().all(|o| o.abs() <= 0.3 + 1e-12));
            assert_eq!(offsets[17], jitter_offset(kind, 0.3, 17, 0), "same row, same offset");
            assert_ne!(jitter_offset(kind, 0.3, 17, 0), jitter_offset(kind, 0.3, 17, 1), "axes independent");
            let mean = offsets.iter().sum::<f64>() / offsets.len() as f64;
            assert!(mean.abs() < 0.01, "{kind:?} mean {mean}");
        }
        assert_eq!(jitter_offset(JitterKind::None, 0.3, 5, 0), 0.0);
        assert_eq!(jitter_offset(JitterKind::Uniform, f64::NAN, 5, 0), 0.0);
    }

    #[test]
    fn box_stats_follow_tukey() {
        // 1..=9 and an outlier.
        let mut v: Vec<f64> = (1..=9).map(f64::from).collect();
        v.push(40.0);
        let b = box_stats(&v).unwrap();
        assert_eq!((b.n, b.median), (10, 5.5));
        assert_eq!((b.q1, b.q3), (3.25, 7.75));
        assert_eq!(b.upper_fence, 7.75 + 1.5 * 4.5);
        assert_eq!((b.lower_whisker, b.upper_whisker), (1.0, 9.0));
        assert_eq!(b.outliers, [(9, 40.0)]);
        assert!(box_stats(&[]).is_none() && box_stats(&[f64::NAN]).is_none());
        let one = box_stats(&[3.0, f64::NAN]).unwrap();
        assert_eq!(
            (one.n, one.q1, one.median, one.q3, one.lower_whisker, one.upper_whisker),
            (1, 3.0, 3.0, 3.0, 3.0, 3.0)
        );
        let flat = box_stats(&[2.0; 6]).unwrap();
        assert!(flat.outliers.is_empty() && flat.lower_fence == 2.0);
    }

    #[test]
    fn polynomial_fits_and_degenerate_data() {
        // Exact quadratic: R² = 1 and the curve reproduces it.
        let x: Vec<f64> = (0..10).map(f64::from).collect();
        let y: Vec<f64> = x.iter().map(|v| 1.0 + 2.0 * v - 0.5 * v * v).collect();
        let f = poly_fit(&x, &y, 2, 0.95).unwrap();
        assert_eq!((f.degree, f.n, f.curve.len()), (2, 10, CURVE_POINTS));
        assert!((f.r_squared.unwrap() - 1.0).abs() < 1e-12);
        let mid = f.curve[CURVE_POINTS / 2];
        assert!((mid[1] - (1.0 + 2.0 * mid[0] - 0.5 * mid[0] * mid[0])).abs() < 1e-9);
        // Noisy line: the band brackets the curve.
        let y2: Vec<f64> = x.iter().enumerate().map(|(i, v)| 3.0 * v + if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let g = poly_fit(&x, &y2, 1, 0.95).unwrap();
        let (lo, hi) = g.band.as_ref().unwrap();
        assert!(g.curve.iter().zip(lo.iter().zip(hi)).all(|(c, (l, h))| l[1] < c[1] && c[1] < h[1]));
        // Zero variance in x, a single observation, nothing finite: no fit.
        assert!(poly_fit(&[2.0; 5], &[1.0, 2.0, 3.0, 4.0, 5.0], 1, 0.95).is_none());
        assert!(poly_fit(&[1.0], &[1.0], 1, 0.95).is_none());
        assert!(poly_fit(&[f64::NAN; 3], &[1.0; 3], 1, 0.95).is_none());
        // Two distinct x: the degree drops to 1 and there is no band.
        let two = poly_fit(&[1.0, 2.0], &[1.0, 3.0], 3, 0.95).unwrap();
        assert_eq!(two.degree, 1);
        assert!((two.r_squared.unwrap() - 1.0).abs() < 1e-12);
        assert!(two.band.is_none());
        let end = two.curve[CURVE_POINTS - 1];
        assert!(end[0] == 2.0 && (end[1] - 3.0).abs() < 1e-12);
        // Saturated quadratic through three points.
        let three = poly_fit(&[0.0, 1.0, 2.0], &[1.0, 0.0, 1.0], 2, 0.95).unwrap();
        assert!(
            (three.curve[CURVE_POINTS / 2 - 1][1] - (three.curve[CURVE_POINTS / 2 - 1][0] - 1.0).powi(2)).abs() < 1e-12
        );
        // Zero variance in y: a flat line, R² undefined, no NaN anywhere.
        let flat = poly_fit(&x, &[4.0; 10], 2, 0.95).unwrap();
        assert!(flat.curve.iter().all(|p| (p[1] - 4.0).abs() < 1e-9));
        assert!(flat.r_squared.is_none() || flat.r_squared.unwrap().is_finite());
        if let Some((lo, hi)) = &flat.band {
            assert!(lo.iter().chain(hi).all(|p| p[1].is_finite()));
        }
    }

    #[test]
    fn weighted_fits_statistics_and_equation() {
        let x: Vec<f64> = (0..12).map(f64::from).collect();
        let y: Vec<f64> = x.iter().enumerate().map(|(i, v)| 2.0 + 0.5 * v + [0.3, -0.2, 0.1, -0.4][i % 4]).collect();
        // A frequency of 2 is the same as listing the row twice.
        let mut f = vec![1.0; 12];
        f[3] = 2.0;
        let a = poly_fit_weighted(&x, &y, &f, &[], 1, 0.95).unwrap();
        let (mut x2, mut y2) = (x.clone(), y.clone());
        x2.push(x[3]);
        y2.push(y[3]);
        let b = poly_fit(&x2, &y2, 1, 0.95).unwrap();
        for (p, q) in a.curve.iter().zip(&b.curve) {
            assert!((p[1] - q[1]).abs() < 1e-10);
        }
        assert!((a.df - b.df).abs() < 1e-12 && a.df == 11.0);
        assert!((a.p_value.unwrap() - b.p_value.unwrap()).abs() < 1e-12);
        // Strong linear trend: tiny p-value; slope close to 0.5.
        assert!(a.p_value.unwrap() < 1e-6);
        assert!((a.coefficients[1] - 0.5).abs() < 0.05 && (a.coefficients[0] - 2.0).abs() < 0.3);
        // Prediction limits are wider than confidence limits everywhere.
        let (cl, cu) = a.band.as_ref().unwrap();
        let (pl, pu) = a.prediction_band.as_ref().unwrap();
        assert!(cl.iter().zip(pl).all(|(c, p)| p[1] < c[1]) && cu.iter().zip(pu).all(|(c, p)| p[1] > c[1]));
        // Zero weight drops a row; a heavy weight pulls the line towards it.
        let mut w = vec![1.0; 12];
        w[0] = 0.0;
        assert_eq!(poly_fit_weighted(&x, &y, &[], &w, 1, 0.95).unwrap().n, 11);
        w[0] = 1e6;
        let heavy = poly_fit_weighted(&x, &y, &[], &w, 1, 0.95).unwrap();
        assert!((heavy.curve[0][1] - y[0]).abs() < (a.curve[0][1] - y[0]).abs());
        // Coefficients reproduce the curve.
        let q = poly_fit(&x, &y, 3, 0.95).unwrap();
        for c in &q.curve {
            let v: f64 = q.coefficients.iter().enumerate().map(|(k, b)| b * c[0].powi(k as i32)).sum();
            assert!((v - c[1]).abs() < 1e-8, "{v} vs {}", c[1]);
        }
        let eq = poly_fit(&[0.0, 1.0, 2.0, 3.0], &[1.0, -1.0, -3.0, -5.0], 1, 0.95).unwrap().equation("Y", "X");
        assert_eq!(eq, "Y = 1 − 2·X");
        // Exact fit: σ̂ = 0, p-value 0, no NaN.
        let exact = poly_fit(&x, &x.iter().map(|v| 3.0 * v).collect::<Vec<_>>(), 1, 0.95).unwrap();
        assert!(exact.p_value.unwrap() < 1e-12 && exact.rmse.unwrap() < 1e-9);
        // No residual df: no p-value or bands.
        let two = poly_fit(&[1.0, 2.0], &[1.0, 3.0], 1, 0.95).unwrap();
        assert!(two.p_value.is_none() && two.prediction_band.is_none() && two.rmse.is_none());
    }

    #[test]
    fn weighted_quantiles_and_box_stats() {
        let v = [1.0, 2.0, 3.0, 4.0];
        // Frequencies act like repeated values.
        let b = box_stats_weighted(&v, &[1.0, 1.0, 1.0, 3.0]).unwrap();
        let r = box_stats(&[1.0, 2.0, 3.0, 4.0, 4.0, 4.0]).unwrap();
        assert_eq!((b.q1, b.median, b.q3, b.n), (r.q1, r.median, r.q3, 4));
        assert_eq!(box_stats_weighted(&v, &[]).unwrap(), box_stats(&v).unwrap());
        assert!(box_stats_weighted(&v, &[0.0; 4]).is_none());
        assert!(box_stats_weighted(&[f64::NAN], &[1.0]).is_none());
        assert_eq!(weighted_quantile(&[(5.0, 2.0)], 0.3), Some(5.0));
    }

    #[test]
    fn smoothing_spline_matches_scipy() {
        // SciPy 1.18 `make_smoothing_spline(x, y, lam=0.01 * 6**3)`: the same
        // criterion with λ in data units (relative λ × range³).
        let x = [0.0, 0.7, 1.3, 2.0, 3.1, 4.0, 5.2, 6.0];
        let y = [1.2, 1.9, 1.4, 2.8, 3.1, 2.2, 3.9, 4.4];
        let expected = [
            1.2974692629072697,
            1.6419976875229296,
            1.9298753930303338,
            2.2587535974094197,
            2.7029554878723805,
            3.0667212769411702,
            3.7439430913653724,
            4.258284202951114,
        ];
        let s = SmoothingSpline::fit(&x, &y, 0.01).unwrap();
        for (xi, e) in x.iter().zip(expected) {
            assert!((s.value(*xi) - e).abs() < 1e-10 * e.abs(), "x = {xi}: {} vs {e}", s.value(*xi));
        }
        assert!((s.value(2.5) - 2.4703000844147343).abs() < 1e-10);
    }

    #[test]
    fn smoothing_spline_limits() {
        let x: Vec<f64> = (0..20).map(|i| f64::from(i) * 0.5).collect();
        let y: Vec<f64> = x.iter().map(|v| v.sin() + 0.1 * v).collect();
        // λ → 0 interpolates the data.
        let rough = SmoothingSpline::fit(&x, &y, 1e-14).unwrap();
        for (xi, yi) in x.iter().zip(&y) {
            assert!((rough.value(*xi) - yi).abs() < 1e-6, "at {xi}");
        }
        // λ → ∞ gives the least-squares line.
        let stiff = SmoothingSpline::fit(&x, &y, 1e9).unwrap();
        let (mx, my) = (x.iter().sum::<f64>() / 20.0, y.iter().sum::<f64>() / 20.0);
        let sxy: f64 = x.iter().zip(&y).map(|(a, b)| (a - mx) * (b - my)).sum();
        let sxx: f64 = x.iter().map(|a| (a - mx) * (a - mx)).sum();
        let slope = sxy / sxx;
        for x0 in [0.0, 2.25, 5.0, 9.5] {
            assert!((stiff.value(x0) - (my + slope * (x0 - mx))).abs() < 1e-4, "x = {x0}");
        }
        // Intermediate λ: smoother than the data (smaller roughness), still finite.
        let mid = SmoothingSpline::fit(&x, &y, 1e-3).unwrap();
        let rough_sum: f64 = mid.second_derivatives.iter().map(|g| g * g).sum();
        let interp_sum: f64 = rough.second_derivatives.iter().map(|g| g * g).sum();
        assert!(rough_sum < interp_sum);
        assert_eq!(mid.curve(50).len(), 50);
        // Ties are merged: the interpolant passes through their mean.
        let s = SmoothingSpline::fit(&[0.0, 1.0, 1.0, 2.0, 3.0], &[0.0, 1.0, 3.0, 2.0, 3.0], 1e-14).unwrap();
        assert!((s.value(1.0) - 2.0).abs() < 1e-6);
        // Degenerate inputs never give NaN.
        assert!(SmoothingSpline::fit(&[], &[], 1.0).is_none());
        let one = SmoothingSpline::fit(&[1.0], &[5.0], 1.0).unwrap();
        assert_eq!((one.value(0.0), one.curve(10)), (5.0, vec![[1.0, 5.0]]));
        let two = SmoothingSpline::fit(&[1.0, 2.0], &[5.0, 7.0], 1.0).unwrap();
        assert_eq!(two.value(1.5), 6.0);
        let flat = SmoothingSpline::fit(&x, &[3.0; 20], 0.01).unwrap();
        assert!(flat.curve(30).iter().all(|p| (p[1] - 3.0).abs() < 1e-9));
        let same_x = SmoothingSpline::fit(&[2.0; 4], &[1.0, 2.0, 3.0, 4.0], 0.1).unwrap();
        assert_eq!(same_x.curve(10), [[2.0, 2.5]]);
        // Weights: a doubled weight equals a repeated observation; zero drops it.
        let a =
            SmoothingSpline::fit_weighted(&[0.0, 1.0, 2.0, 3.0], &[0.0, 2.0, 1.0, 3.0], &[1.0, 2.0, 1.0, 1.0], 0.05)
                .unwrap();
        let b = SmoothingSpline::fit(&[0.0, 1.0, 1.0, 2.0, 3.0], &[0.0, 2.0, 2.0, 1.0, 3.0], 0.05 * 5.0 / 4.0).unwrap();
        for x0 in [0.0, 0.7, 1.5, 3.0] {
            assert!((a.value(x0) - b.value(x0)).abs() < 1e-9, "{x0}");
        }
        let z = SmoothingSpline::fit_weighted(&[0.0, 1.0, 2.0], &[0.0, 9.0, 2.0], &[1.0, 0.0, 1.0], 0.1).unwrap();
        assert_eq!(z.knots, [0.0, 2.0]);
        // Confidence band: brackets the curve; stiffer fits have fewer
        // equivalent degrees of freedom; a near-line fit has df near 2.
        let noisy: Vec<f64> =
            x.iter().enumerate().map(|(i, v)| v.sin() + if i % 2 == 0 { 0.2 } else { -0.2 }).collect();
        let loose = SmoothingSpline::fit_with_band(&x, &noisy, &[], 1e-4, 0.95, 40).unwrap();
        let stiff = SmoothingSpline::fit_with_band(&x, &noisy, &[], 1e3, 0.95, 40).unwrap();
        assert!(loose.equivalent_df > stiff.equivalent_df && (stiff.equivalent_df - 2.0).abs() < 0.05);
        let (lo, hi) = loose.band.as_ref().unwrap();
        assert!(loose.curve.iter().zip(lo.iter().zip(hi)).all(|(c, (l, h))| l[1] <= c[1] && c[1] <= h[1]));
        // Interpolation leaves no residual degrees of freedom: no band.
        let exact = SmoothingSpline::fit_with_band(&x, &noisy, &[], 1e-14, 0.95, 40).unwrap();
        assert!(exact.band.is_none());
        // Two knots: the means, with a band from the replicates.
        let two =
            SmoothingSpline::fit_with_band(&[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0, 3.0, 5.0], &[], 0.1, 0.95, 5).unwrap();
        assert!(two.band.is_some() && (two.equivalent_df - 2.0).abs() < 1e-12);
    }
}
