//! Standard orthogonal arrays and robust (parameter) design.
//!
//! # Orthogonal arrays
//! An orthogonal array of strength 2 is a table of factor levels in which,
//! for every pair of columns, each combination of levels appears equally
//! often, so main effects are estimated independently of one another. The
//! standard arrays catalogued by Taguchi and reproduced in Phadke, *Quality
//! Engineering Using Robust Design* (Prentice Hall 1989, appendix C), are
//! built here from their published constructions:
//!
//! * **L4 (2³), L8 (2⁷), L16 (2¹⁵)** — the `2^m − 1` columns of a `2^m`
//!   two-level array are the non-zero linear combinations over GF(2) of `m`
//!   basic columns. With rows numbered `r = 0 … 2^m − 1`, basic column `i`
//!   takes bit `m − 1 − i` of `r`, and column `c` (1-based) adds the basic
//!   columns named by the bits of `c`: column 3 is columns 1 + 2, column 5 is
//!   1 + 4, and so on — the standard column order, in which the interaction
//!   of columns `a` and `b` appears in column `a XOR b`.
//! * **L9 (3⁴), L27 (3¹³)** — likewise over GF(3): the columns are one
//!   representative of each projective point (`a`, `b`, `a + b`, `2a + b` for
//!   L9; for L27 also `c`, `a + c`, `2a + c`, `b + c`, `a + b + c`,
//!   `2a + b + c`, `2b + c`, `a + 2b + c`, `2a + 2b + c`), so that the
//!   interaction of columns 1 and 2 is carried by columns 3 and 4.
//! * **L12 (2¹¹)** — the 12-run Plackett–Burman design (cyclic generator,
//!   Plackett & Burman 1946), arranged with the all-low run first.
//!   Interactions are spread over the columns rather than confounded with one.
//! * **L18 (2¹ × 3⁷)** — the standard mixed array as tabulated by Taguchi.
//!
//! Levels are numbered 1, 2, 3 as in the published tables.
//!
//! # Robust design and signal-to-noise ratios
//! Control factors are assigned to the columns of an **inner array**; each
//! inner run is repeated under the conditions of an **outer array** of noise
//! factors (or simply replicated). For each inner run the `n` responses give
//! a signal-to-noise ratio (Phadke 1989, ch. 5–6), in decibels, to be
//! maximized:
//!
//! ```text
//! smaller the better   η = −10 log₁₀( Σ y² / n )
//! larger the better    η = −10 log₁₀( Σ (1/y²) / n )
//! nominal the best     η =  10 log₁₀( ȳ² / s² )      (scale adjustable)
//! nominal, variance    η = −10 log₁₀( s² )           (location adjustable)
//! ```
//!
//! The analysis of means of the array then tabulates, for each factor, the
//! mean of η (and of the response) at each level; the range of those means
//! ranks the factors' influence, and the additive model predicts η at any
//! combination of levels as `η̄ + Σ_factors (η̄_level − η̄)`.

use crate::screening::plackett_burman_runs;
use stat_core::summation::sum_iter;
use stat_core::{Result, StatError};

/// The standard orthogonal arrays offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardArray {
    /// 4 runs, three 2-level columns.
    L4,
    /// 8 runs, seven 2-level columns.
    L8,
    /// 9 runs, four 3-level columns.
    L9,
    /// 12 runs, eleven 2-level columns.
    L12,
    /// 16 runs, fifteen 2-level columns.
    L16,
    /// 18 runs, one 2-level and seven 3-level columns.
    L18,
    /// 27 runs, thirteen 3-level columns.
    L27,
}

impl StandardArray {
    /// Every array, smallest first.
    pub const ALL: [Self; 7] = [Self::L4, Self::L8, Self::L9, Self::L12, Self::L16, Self::L18, Self::L27];

    /// Name with its level structure, e.g. `L18 (2¹ × 3⁷)`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::L4 => "L4 (2³)",
            Self::L8 => "L8 (2⁷)",
            Self::L9 => "L9 (3⁴)",
            Self::L12 => "L12 (2¹¹)",
            Self::L16 => "L16 (2¹⁵)",
            Self::L18 => "L18 (2¹ × 3⁷)",
            Self::L27 => "L27 (3¹³)",
        }
    }

    /// Short key, e.g. `L18`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::L4 => "L4",
            Self::L8 => "L8",
            Self::L9 => "L9",
            Self::L12 => "L12",
            Self::L16 => "L16",
            Self::L18 => "L18",
            Self::L27 => "L27",
        }
    }

    /// The array with key `key` (case-insensitive).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key().eq_ignore_ascii_case(key.trim()))
    }

    /// Number of runs.
    #[must_use]
    pub fn runs(self) -> usize {
        match self {
            Self::L4 => 4,
            Self::L8 => 8,
            Self::L9 => 9,
            Self::L12 => 12,
            Self::L16 => 16,
            Self::L18 => 18,
            Self::L27 => 27,
        }
    }

    /// Number of levels of each column.
    #[must_use]
    pub fn levels(self) -> Vec<usize> {
        match self {
            Self::L4 => vec![2; 3],
            Self::L8 => vec![2; 7],
            Self::L9 => vec![3; 4],
            Self::L12 => vec![2; 11],
            Self::L16 => vec![2; 15],
            Self::L18 => std::iter::once(2).chain(std::iter::repeat_n(3, 7)).collect(),
            Self::L27 => vec![3; 13],
        }
    }

    /// The array: `runs()` rows of 1-based levels, one per column.
    #[must_use]
    pub fn array(self) -> Vec<Vec<usize>> {
        match self {
            Self::L4 => two_level(2),
            Self::L8 => two_level(3),
            Self::L16 => two_level(4),
            Self::L9 => three_level(&L9_COLUMNS, 2),
            Self::L27 => three_level(&L27_COLUMNS, 3),
            Self::L12 => l12(),
            Self::L18 => L18.iter().map(|r| r.to_vec()).collect(),
        }
    }
}

/// `2^m` runs, `2^m − 1` columns over GF(2), in the standard column order.
fn two_level(m: u32) -> Vec<Vec<usize>> {
    let n = 1usize << m;
    (0..n)
        .map(|r| {
            // Basic column i (bit i of the column number) takes bit m − 1 − i of r.
            let reversed = (0..m).fold(0usize, |acc, i| acc | (((r >> (m - 1 - i)) & 1) << i));
            (1..n).map(|c| 1 + (c & reversed).count_ones() as usize % 2).collect()
        })
        .collect()
}

/// Coefficients (on the basic columns a, b, c) of the L9 columns.
const L9_COLUMNS: [[usize; 3]; 4] = [[1, 0, 0], [0, 1, 0], [1, 1, 0], [2, 1, 0]];
/// Coefficients of the L27 columns.
const L27_COLUMNS: [[usize; 3]; 13] = [
    [1, 0, 0],
    [0, 1, 0],
    [1, 1, 0],
    [2, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [2, 0, 1],
    [0, 1, 1],
    [1, 1, 1],
    [2, 1, 1],
    [0, 2, 1],
    [1, 2, 1],
    [2, 2, 1],
];

/// `3^m` runs over GF(3); basic column `a` is the most significant ternary digit.
fn three_level(columns: &[[usize; 3]], m: u32) -> Vec<Vec<usize>> {
    let n = 3usize.pow(m);
    (0..n)
        .map(|r| {
            let digits: Vec<usize> = (0..m as usize).map(|i| (r / 3usize.pow(m - 1 - i as u32)) % 3).collect();
            columns.iter().map(|c| 1 + (0..m as usize).map(|i| c[i] * digits[i]).sum::<usize>() % 3).collect()
        })
        .collect()
}

/// The 12-run Plackett–Burman design with the all-low run first, −1 → 1, +1 → 2.
fn l12() -> Vec<Vec<usize>> {
    let design = plackett_burman_runs(11, 12).expect("the 12-run Plackett–Burman design exists");
    let mut rows: Vec<Vec<usize>> =
        design.runs.iter().map(|r| r.iter().map(|&v| if v < 0.0 { 1 } else { 2 }).collect()).collect();
    if let Some(i) = rows.iter().position(|r| r.iter().all(|&v| v == 1)) {
        let low = rows.remove(i);
        rows.insert(0, low);
    }
    rows
}

/// The standard L18 array.
const L18: [[usize; 8]; 18] = [
    [1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 2, 2, 2, 2, 2, 2],
    [1, 1, 3, 3, 3, 3, 3, 3],
    [1, 2, 1, 1, 2, 2, 3, 3],
    [1, 2, 2, 2, 3, 3, 1, 1],
    [1, 2, 3, 3, 1, 1, 2, 2],
    [1, 3, 1, 2, 1, 3, 2, 3],
    [1, 3, 2, 3, 2, 1, 3, 1],
    [1, 3, 3, 1, 3, 2, 1, 2],
    [2, 1, 1, 3, 3, 2, 2, 1],
    [2, 1, 2, 1, 1, 3, 3, 2],
    [2, 1, 3, 2, 2, 1, 1, 3],
    [2, 2, 1, 2, 3, 1, 3, 2],
    [2, 2, 2, 3, 1, 2, 1, 3],
    [2, 2, 3, 1, 2, 3, 2, 1],
    [2, 3, 1, 3, 2, 3, 1, 2],
    [2, 3, 2, 1, 3, 1, 2, 3],
    [2, 3, 3, 2, 1, 2, 3, 1],
];

/// `true` when every pair of columns of `rows` (1-based levels, `levels[j]`
/// for column `j`) shows each level combination equally often (strength 2).
#[must_use]
pub fn is_orthogonal(rows: &[Vec<usize>], levels: &[usize]) -> bool {
    let n = rows.len();
    let k = levels.len();
    if rows.iter().any(|r| r.len() != k || r.iter().zip(levels).any(|(&v, &l)| v == 0 || v > l)) {
        return false;
    }
    for a in 0..k {
        for b in a + 1..k {
            let cells = levels[a] * levels[b];
            if n % cells != 0 {
                return false;
            }
            let mut counts = vec![0usize; cells];
            for r in rows {
                counts[(r[a] - 1) * levels[b] + (r[b] - 1)] += 1;
            }
            if counts.iter().any(|&c| c != n / cells) {
                return false;
            }
        }
    }
    true
}

/// A signal-to-noise ratio (Phadke 1989, ch. 5–6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalToNoise {
    /// `−10 log₁₀(Σy²/n)`: the response should be as small as possible.
    SmallerTheBetter,
    /// `−10 log₁₀(Σy⁻²/n)`: the response should be as large as possible.
    LargerTheBetter,
    /// `10 log₁₀(ȳ²/s²)`: on target, with a factor that scales the response.
    NominalTheBest,
    /// `−10 log₁₀(s²)`: on target, with a factor that shifts the response.
    NominalVariance,
}

impl SignalToNoise {
    /// Every ratio.
    pub const ALL: [Self; 4] =
        [Self::SmallerTheBetter, Self::LargerTheBetter, Self::NominalTheBest, Self::NominalVariance];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::SmallerTheBetter => "Smaller the better",
            Self::LargerTheBetter => "Larger the better",
            Self::NominalTheBest => "Nominal the best (mean²/variance)",
            Self::NominalVariance => "Nominal the best (variance only)",
        }
    }

    /// Short key for project files.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::SmallerTheBetter => "smaller",
            Self::LargerTheBetter => "larger",
            Self::NominalTheBest => "nominal",
            Self::NominalVariance => "nominal-variance",
        }
    }

    /// The ratio with key `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == key)
    }

    /// The formula, for reports.
    #[must_use]
    pub fn formula(self) -> &'static str {
        match self {
            Self::SmallerTheBetter => "η = −10 log₁₀(Σy²/n)",
            Self::LargerTheBetter => "η = −10 log₁₀(Σ(1/y²)/n)",
            Self::NominalTheBest => "η = 10 log₁₀(ȳ²/s²)",
            Self::NominalVariance => "η = −10 log₁₀(s²)",
        }
    }

    /// The ratio of `values`, in decibels.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for no values, a zero value
    /// (larger the better), fewer than two values or no variation (nominal
    /// forms), or a ratio that is not finite; [`StatError::NonFiniteInput`]
    /// for NaN or ±∞.
    pub fn ratio(self, values: &[f64]) -> Result<f64> {
        if values.is_empty() {
            return Err(invalid("a signal-to-noise ratio needs at least one value"));
        }
        if values.iter().any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "signal-to-noise ratio" });
        }
        let n = values.len() as f64;
        let variance = || -> Result<f64> {
            if values.len() < 2 {
                return Err(invalid("the nominal-the-best ratios need at least two values per run"));
            }
            let m = sum_iter(values.iter().copied()) / n;
            let v = sum_iter(values.iter().map(|y| (y - m).powi(2))) / (n - 1.0);
            if v > 0.0 { Ok(v) } else { Err(invalid("the values of a run do not vary, so the ratio is infinite")) }
        };
        let eta = match self {
            Self::SmallerTheBetter => -10.0 * (sum_iter(values.iter().map(|y| y * y)) / n).log10(),
            Self::LargerTheBetter => {
                if values.contains(&0.0) {
                    return Err(invalid("larger the better needs non-zero values."));
                }
                -10.0 * (sum_iter(values.iter().map(|y| 1.0 / (y * y))) / n).log10()
            }
            Self::NominalTheBest => {
                let m = sum_iter(values.iter().copied()) / n;
                10.0 * (m * m / variance()?).log10()
            }
            Self::NominalVariance => -10.0 * variance()?.log10(),
        };
        if eta.is_finite() {
            Ok(eta)
        } else {
            Err(invalid("the signal-to-noise ratio is undefined for these values (all zero, or no variation)."))
        }
    }
}

/// The mean of a run summary at each level of one factor.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelMeans {
    /// Mean at each level (index 0 = level 1); NaN for a level not run.
    pub means: Vec<f64>,
    /// Largest minus smallest level mean.
    pub delta: f64,
    /// Rank of `delta` among the factors (1 = largest).
    pub rank: usize,
}

/// The response table of an orthogonal-array experiment: for each factor the
/// mean of `values` (one per run) at each of its levels.
///
/// `settings[j][r]` is the 0-based level of factor `j` in run `r`, with
/// `levels[j]` levels.
///
/// # Errors
/// [`StatError::DimensionMismatch`] if a factor's settings do not have one
/// entry per value; [`StatError::InvalidArgument`] for a level out of range
/// or no factors.
pub fn level_means(settings: &[Vec<usize>], levels: &[usize], values: &[f64]) -> Result<Vec<LevelMeans>> {
    if settings.is_empty() || settings.len() != levels.len() {
        return Err(invalid("give the settings and number of levels of at least one factor"));
    }
    let mut out = Vec::with_capacity(settings.len());
    for (s, &l) in settings.iter().zip(levels) {
        if s.len() != values.len() {
            return Err(StatError::DimensionMismatch {
                context: "factor settings vs. runs",
                expected: values.len(),
                found: s.len(),
            });
        }
        if s.iter().any(|&v| v >= l) {
            return Err(invalid("a factor setting is outside its levels"));
        }
        let means: Vec<f64> = (0..l)
            .map(|level| {
                let v: Vec<f64> = s.iter().zip(values).filter(|&(&a, _)| a == level).map(|(_, &y)| y).collect();
                if v.is_empty() { f64::NAN } else { sum_iter(v.iter().copied()) / v.len() as f64 }
            })
            .collect();
        let finite = means.iter().copied().filter(|m| m.is_finite());
        let (lo, hi) = finite.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), m| (a.min(m), b.max(m)));
        out.push(LevelMeans { means, delta: if hi >= lo { hi - lo } else { f64::NAN }, rank: 0 });
    }
    let mut order: Vec<usize> = (0..out.len()).collect();
    order.sort_by(|&a, &b| out[b].delta.total_cmp(&out[a].delta));
    for (rank, &j) in order.iter().enumerate() {
        out[j].rank = rank + 1;
    }
    Ok(out)
}

/// The best level of each factor (largest mean) and the additive-model
/// prediction there: `overall + Σ (best mean − overall)`.
#[must_use]
pub fn best_levels(table: &[LevelMeans], overall: f64) -> (Vec<usize>, f64) {
    let best: Vec<usize> = table
        .iter()
        .map(|f| {
            f.means
                .iter()
                .enumerate()
                .filter(|(_, m)| m.is_finite())
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(0, |(i, _)| i)
        })
        .collect();
    let predicted = overall + table.iter().zip(&best).map(|(f, &b)| f.means[b] - overall).sum::<f64>();
    (best, predicted)
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &[&str]) -> Vec<Vec<usize>> {
        text.iter().map(|r| r.bytes().map(|b| usize::from(b - b'0')).collect()).collect()
    }

    #[test]
    fn every_array_is_orthogonal_with_its_shape() {
        for a in StandardArray::ALL {
            let x = a.array();
            assert_eq!(x.len(), a.runs(), "{}", a.label());
            assert!(x.iter().all(|r| r.len() == a.levels().len()));
            assert!(is_orthogonal(&x, &a.levels()), "{} is orthogonal", a.label());
            assert!(x[0].iter().all(|&v| v == 1), "{}: first run at level 1 throughout", a.label());
            assert_eq!(StandardArray::from_key(a.key()), Some(a));
        }
        assert!(!is_orthogonal(&rows(&["11", "12", "21", "21"]), &[2, 2]));
    }

    /// L4, L8 and L9 as tabulated by Phadke (1989, appendix C) and in every
    /// text on Taguchi's method.
    #[test]
    fn small_arrays_match_the_published_tables() {
        assert_eq!(StandardArray::L4.array(), rows(&["111", "122", "212", "221"]));
        assert_eq!(
            StandardArray::L8.array(),
            rows(&["1111111", "1112222", "1221122", "1222211", "2121212", "2122121", "2211221", "2212112"])
        );
        assert_eq!(
            StandardArray::L9.array(),
            rows(&["1111", "1222", "1333", "2123", "2231", "2312", "3132", "3213", "3321"])
        );
    }

    #[test]
    fn interaction_columns_of_the_two_level_arrays() {
        // In the standard order the interaction of columns a and b is column
        // a XOR b: the level of column 3 is "same/different" of columns 1, 2.
        for a in [StandardArray::L8, StandardArray::L16] {
            let x = a.array();
            for (c1, c2) in [(1usize, 2usize), (1, 4), (2, 4), (3, 4)] {
                let c3 = c1 ^ c2;
                for r in &x {
                    assert_eq!(r[c3 - 1] == 1, r[c1 - 1] == r[c2 - 1]);
                }
            }
        }
        // L27: columns 3 and 4 carry the interaction of columns 1 and 2.
        let x = StandardArray::L27.array();
        for r in &x {
            assert_eq!((r[2] - 1) % 3, (r[0] - 1 + r[1] - 1) % 3);
            assert_eq!((r[3] - 1) % 3, (2 * (r[0] - 1) + r[1] - 1) % 3);
        }
    }

    /// Hand computations (Python check: `-10*math.log10(...)`):
    /// y = 1, 2, 3 — smaller: −10 log₁₀(14/3) = −6.690067;
    /// larger: −10 log₁₀((1 + 1/4 + 1/9)/3) = 3.432277;
    /// ȳ = 2, s² = 1 — nominal: 10 log₁₀ 4 = 6.020600; variance: 0.
    #[test]
    fn signal_to_noise_ratios() {
        let y = [1.0, 2.0, 3.0];
        let close = |a: f64, b: f64| assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        close(SignalToNoise::SmallerTheBetter.ratio(&y).unwrap(), -6.690_067);
        close(SignalToNoise::LargerTheBetter.ratio(&y).unwrap(), 3.432_277);
        close(SignalToNoise::NominalTheBest.ratio(&y).unwrap(), 6.020_600);
        close(SignalToNoise::NominalVariance.ratio(&y).unwrap(), 0.0);
        assert!(SignalToNoise::LargerTheBetter.ratio(&[0.0, 1.0]).is_err());
        assert!(SignalToNoise::NominalTheBest.ratio(&[1.0]).is_err());
        assert!(SignalToNoise::NominalTheBest.ratio(&[2.0, 2.0]).is_err());
        assert!(SignalToNoise::SmallerTheBetter.ratio(&[]).is_err());
        assert_eq!(SignalToNoise::from_key("nominal-variance"), Some(SignalToNoise::NominalVariance));
    }

    /// An L4 with ratios 10, 12, 20, 26 (hand computation): factor 1 levels
    /// (10 + 12)/2 = 11 and (20 + 26)/2 = 23, Δ = 12; factor 2: 15, 19, Δ = 4;
    /// factor 3: (10 + 26)/2 = 18, (12 + 20)/2 = 16, Δ = 2. Overall 17; best
    /// levels 2, 2, 1 predict 17 + 6 + 2 + 1 = 26.
    #[test]
    fn response_table_and_prediction() {
        let x = StandardArray::L4.array();
        let settings: Vec<Vec<usize>> = (0..3).map(|j| x.iter().map(|r| r[j] - 1).collect()).collect();
        let t = level_means(&settings, &[2, 2, 2], &[10.0, 12.0, 20.0, 26.0]).unwrap();
        assert_eq!(t[0].means, vec![11.0, 23.0]);
        assert_eq!(t[1].means, vec![15.0, 19.0]);
        assert_eq!(t[2].means, vec![18.0, 16.0]);
        assert_eq!(t.iter().map(|f| f.delta).collect::<Vec<_>>(), vec![12.0, 4.0, 2.0]);
        assert_eq!(t.iter().map(|f| f.rank).collect::<Vec<_>>(), vec![1, 2, 3]);
        let (best, predicted) = best_levels(&t, 17.0);
        assert_eq!(best, vec![1, 1, 0]);
        assert!((predicted - 26.0).abs() < 1e-12);
        assert!(level_means(&settings, &[2, 2, 2], &[1.0]).is_err());
        assert!(level_means(&[vec![0, 2]], &[2], &[1.0, 2.0]).is_err());
    }
}
