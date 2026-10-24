//! Screening designs.
//!
//! # Plackett–Burman
//! Two-level orthogonal main-effect designs in `N` runs (`N` a multiple of 4)
//! for up to `N − 1` factors (Plackett & Burman 1946, "The design of optimum
//! multifactorial experiments", *Biometrika* 33). For `N = 12, 20, 24` the rows
//! are the cyclic shifts of the published first row followed by a row of all
//! −1; for `N = 8, 16, 32, 64` the Sylvester Hadamard matrix is used (these are
//! the corresponding resolution III fractional factorials). Each design uses the
//! smallest supported `N > k`.
//!
//! # Definitive screening designs
//! Three-level designs in which main effects are orthogonal to each other and to
//! every second-order effect, and quadratic effects are estimable (Jones &
//! Nachtsheim 2011, "A class of three-level designs for definitive screening in
//! the presence of second-order effects", *J. Quality Technology* 43; built from
//! conference matrices as in Xiao, Lin & Bai 2012, *J. Quality Technology* 44).
//! For a conference matrix `C` of order `m` (zero diagonal, ±1 elsewhere,
//! `CᵀC = (m − 1)I`) the design is `[C; −C; 0]` with `2m + 1` runs. `m` is the
//! smallest available even order of at least `max(k, 6)`; extra columns are
//! dropped. Conference matrices come from Paley's construction over the finite
//! field `GF(q)`, `q = m − 1` a prime power: with `χ` the quadratic character,
//! `Q = [χ(j − i)]`, and `C = [[0, 1ᵀ], [±1, Q]]` (symmetric when
//! `q ≡ 1 mod 4`, antisymmetric when `q ≡ 3 mod 4`).

use crate::design::{Design, Factor, PointType, default_factor_names};
use stat_core::{Result, StatError};

/// First rows of the cyclic Plackett–Burman designs.
const PB_ROWS: [(usize, &str); 3] = [(12, "++-+++---+-"), (20, "++--++++-+-+----++-"), (24, "+++++-+-++--++--+-+----")];

/// Run sizes supported by [`plackett_burman`].
pub const PLACKETT_BURMAN_SIZES: [usize; 7] = [8, 12, 16, 20, 24, 32, 64];

/// The `N × (N − 1)` matrix of a Plackett–Burman design.
fn pb_matrix(n: usize) -> Option<Vec<Vec<f64>>> {
    if let Some(&(_, row)) = PB_ROWS.iter().find(|(m, _)| *m == n) {
        let first: Vec<f64> = row.chars().map(|c| if c == '+' { 1.0 } else { -1.0 }).collect();
        let w = first.len();
        let mut rows: Vec<Vec<f64>> = (0..w).map(|s| (0..w).map(|j| first[(j + w - s) % w]).collect()).collect();
        rows.push(vec![-1.0; w]);
        return Some(rows);
    }
    if n.is_power_of_two() && n >= 4 {
        // Sylvester: H[i][j] = (−1)^popcount(i & j); drop the constant column 0.
        let rows =
            (0..n).map(|i| (1..n).map(|j| if (i & j).count_ones() % 2 == 0 { 1.0 } else { -1.0 }).collect()).collect();
        return Some(rows);
    }
    None
}

/// Plackett–Burman design for `k` factors in the smallest supported run size
/// greater than `k`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `k = 0` or `k > 63`.
pub fn plackett_burman(k: usize) -> Result<Design> {
    let n = PLACKETT_BURMAN_SIZES
        .iter()
        .copied()
        .find(|&n| n > k)
        .filter(|_| k > 0)
        .ok_or_else(|| invalid("Plackett–Burman designs here support 1 to 63 factors"))?;
    plackett_burman_runs(k, n)
}

/// Plackett–Burman design for `k` factors in `n` runs.
///
/// # Errors
/// [`StatError::InvalidArgument`] if `n` is not supported or `k ≥ n`.
pub fn plackett_burman_runs(k: usize, n: usize) -> Result<Design> {
    if k == 0 || k >= n {
        return Err(invalid(format!("a {n}-run Plackett–Burman design holds 1 to {} factors", n.saturating_sub(1))));
    }
    let m = pb_matrix(n)
        .ok_or_else(|| invalid(format!("supported Plackett–Burman run sizes are {PLACKETT_BURMAN_SIZES:?}")))?;
    let runs: Vec<Vec<f64>> = m.into_iter().map(|r| r[..k].to_vec()).collect();
    let factors = default_factor_names(k).into_iter().map(Factor::coded).collect();
    let mut d =
        Design::new(format!("Plackett–Burman, {k} factors ({n} runs)"), factors, runs, vec![PointType::Factorial; n])?;
    d.notes.push(
        "Main effects are orthogonal; each two-factor interaction is partially aliased with main effects.".to_owned(),
    );
    Ok(d)
}

/// A finite field `GF(p^e)` with elements numbered `0..q` (base-`p` digits are
/// polynomial coefficients), enough for Paley's construction.
struct FiniteField {
    p: usize,
    e: usize,
    /// Coefficients of the monic irreducible polynomial, low degree first,
    /// excluding the leading 1.
    modulus: Vec<usize>,
}

impl FiniteField {
    fn order(&self) -> usize {
        self.p.pow(self.e as u32)
    }

    fn digits(&self, x: usize) -> Vec<usize> {
        let mut d = Vec::with_capacity(self.e);
        let mut v = x;
        for _ in 0..self.e {
            d.push(v % self.p);
            v /= self.p;
        }
        d
    }

    fn number_from_digits(&self, d: &[usize]) -> usize {
        d.iter().rev().fold(0, |acc, &c| acc * self.p + c)
    }

    fn sub(&self, a: usize, b: usize) -> usize {
        let (x, y) = (self.digits(a), self.digits(b));
        let d: Vec<usize> = x.iter().zip(&y).map(|(u, v)| (u + self.p - v) % self.p).collect();
        self.number_from_digits(&d)
    }

    fn mul(&self, a: usize, b: usize) -> usize {
        let (x, y) = (self.digits(a), self.digits(b));
        let mut prod = vec![0usize; 2 * self.e];
        for (i, &u) in x.iter().enumerate() {
            for (j, &v) in y.iter().enumerate() {
                prod[i + j] = (prod[i + j] + u * v) % self.p;
            }
        }
        // Reduce: t^e = −(modulus).
        for deg in (self.e..prod.len()).rev() {
            let c = prod[deg];
            if c != 0 {
                prod[deg] = 0;
                for (i, &m) in self.modulus.iter().enumerate() {
                    let idx = deg - self.e + i;
                    prod[idx] = (prod[idx] + self.p * self.p - c * m % self.p) % self.p;
                }
            }
        }
        self.number_from_digits(&prod[..self.e])
    }

    /// Quadratic character: 0 for 0, +1 for non-zero squares, −1 otherwise.
    fn characters(&self) -> Vec<i8> {
        let q = self.order();
        let mut chi = vec![-1i8; q];
        chi[0] = 0;
        for x in 1..q {
            chi[self.mul(x, x)] = 1;
        }
        chi
    }
}

/// Fields for the conference-matrix orders used (prime powers `q = m − 1`).
fn field_for(q: usize) -> Option<FiniteField> {
    match q {
        3 | 5 | 7 | 11 | 13 | 17 | 19 | 23 | 29 | 31 | 37 | 41 | 43 | 47 => {
            Some(FiniteField { p: q, e: 1, modulus: vec![0] })
        }
        // t² + 1 over GF(3); t² + 2 over GF(5); t³ + 2t + 1 over GF(3).
        9 => Some(FiniteField { p: 3, e: 2, modulus: vec![1, 0] }),
        25 => Some(FiniteField { p: 5, e: 2, modulus: vec![2, 0] }),
        27 => Some(FiniteField { p: 3, e: 3, modulus: vec![1, 2, 0] }),
        _ => None,
    }
}

/// Orders `m` for which [`conference_matrix`] can build a matrix.
#[must_use]
pub fn conference_orders() -> Vec<usize> {
    (4..=48).filter(|m| m % 2 == 0 && field_for(m - 1).is_some()).collect()
}

/// A conference matrix of order `m` by Paley's construction.
///
/// # Errors
/// [`StatError::InvalidArgument`] if `m − 1` is not one of the supported prime
/// powers (see [`conference_orders`]).
pub fn conference_matrix(m: usize) -> Result<Vec<Vec<f64>>> {
    let field = field_for(m.saturating_sub(1))
        .ok_or_else(|| invalid(format!("no conference matrix of order {m} is available here")))?;
    let q = field.order();
    let chi = field.characters();
    let symmetric = q % 4 == 1;
    let mut c = vec![vec![0.0; m]; m];
    for j in 1..m {
        c[0][j] = 1.0;
        c[j][0] = if symmetric { 1.0 } else { -1.0 };
    }
    for i in 0..q {
        for j in 0..q {
            c[i + 1][j + 1] = f64::from(chi[field.sub(j, i)]);
        }
    }
    Ok(c)
}

/// Definitive screening design for `k` continuous factors (`k ≥ 3`): the
/// smallest conference matrix and one center run.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `k < 3` or `k > 48`.
pub fn definitive_screening(k: usize) -> Result<Design> {
    definitive_screening_with(k, &DsdOptions::default())
}

/// Choices for a definitive screening design beyond the factor count.
///
/// **Extra runs** come from a larger conference matrix whose surplus
/// columns are left unassigned ("fake factors", Jones & Nachtsheim 2013,
/// "Definitive screening designs with added two-level categorical factors",
/// *J. Quality Technology* 45(2), §4): every two extra columns add four runs
/// (one fold-over pair per column), keep all the design's orthogonality
/// properties and give degrees of freedom for error after the main effects
/// and quadratic effects. **Center runs** beyond the first add pure-error
/// degrees of freedom and sharpen the quadratic effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DsdOptions {
    /// At least this many runs beyond the smallest design (rounded up to the
    /// next available conference-matrix order).
    pub extra_runs: usize,
    /// Number of center runs (at least 1; the design has one by
    /// construction).
    pub center_runs: usize,
}

impl Default for DsdOptions {
    fn default() -> Self {
        Self { extra_runs: 0, center_runs: 1 }
    }
}

/// Most center runs of a definitive screening design.
pub const MAX_DSD_CENTER_RUNS: usize = 50;

/// The conference-matrix order of the definitive screening design for `k`
/// factors with `options`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `k < 3`, no order large enough
/// (more than 48 factors and extra columns), or a center-run count outside
/// 1 to [`MAX_DSD_CENTER_RUNS`].
pub fn dsd_order(k: usize, options: &DsdOptions) -> Result<usize> {
    if k < 3 {
        return Err(invalid("definitive screening designs need at least 3 factors"));
    }
    if !(1..=MAX_DSD_CENTER_RUNS).contains(&options.center_runs) {
        return Err(invalid(format!("use 1 to {MAX_DSD_CENTER_RUNS} center runs")));
    }
    let orders = conference_orders();
    let base = orders
        .iter()
        .copied()
        .find(|&m| m >= k.max(6))
        .ok_or_else(|| invalid("definitive screening designs here support up to 48 factors"))?;
    orders.into_iter().find(|&m| m >= base && 2 * m >= 2 * base + options.extra_runs).ok_or_else(|| {
        invalid(format!(
            "no conference matrix is large enough for {k} factors and {} extra runs (the largest design here has \
             97 runs before center runs)",
            options.extra_runs
        ))
    })
}

/// Definitive screening design for `k` continuous factors with extra runs
/// and center runs (see [`DsdOptions`]). For a conference matrix `C` of
/// order `m` the runs are `[C; −C]` restricted to the first `k` columns,
/// followed by `center_runs` center runs: `2m + c` runs.
///
/// # Errors
/// As [`dsd_order`].
pub fn definitive_screening_with(k: usize, options: &DsdOptions) -> Result<Design> {
    let m = dsd_order(k, options)?;
    let c = conference_matrix(m)?;
    let mut runs: Vec<Vec<f64>> = Vec::with_capacity(2 * m + options.center_runs);
    let mut types = Vec::with_capacity(2 * m + options.center_runs);
    for row in &c {
        runs.push(row[..k].to_vec());
        types.push(PointType::Other);
    }
    for row in &c {
        runs.push(row[..k].iter().map(|v| -v).collect());
        types.push(PointType::Foldover);
    }
    for _ in 0..options.center_runs {
        runs.push(vec![0.0; k]);
        types.push(PointType::Center);
    }
    let n = runs.len();
    let factors = default_factor_names(k).into_iter().map(Factor::coded).collect();
    let mut d = Design::new(format!("Definitive screening, {k} factors ({n} runs)"), factors, runs, types)?;
    d.notes.push(format!(
        "Built from a conference matrix of order {m}{}. Main effects are orthogonal to each other and to all \
         two-factor interactions and quadratic effects.",
        if m > k { format!(" ({} unused columns dropped)", m - k) } else { String::new() }
    ));
    if options.center_runs > 1 {
        d.notes.push(format!(
            "{} center runs ({} degrees of freedom for pure error).",
            options.center_runs,
            options.center_runs - 1
        ));
    }
    let s = dsd_summary(&d.runs, m);
    d.notes.push(format!(
        "Error degrees of freedom: {} after the main effects, {} after main and quadratic effects; largest \
         |correlation| between quadratic effects {:.4}, between two-factor interactions {:.4}.",
        s.error_df_main, s.error_df_quadratic, s.max_quadratic_correlation, s.max_interaction_correlation
    ));
    Ok(d)
}

/// Properties of one definitive screening design, for comparing choices
/// of extra runs and center runs ([`compare_definitive_screening`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DsdSummary {
    /// Extra runs asked for.
    pub extra_runs: usize,
    /// Center runs.
    pub center_runs: usize,
    /// Conference-matrix order `m`.
    pub order: usize,
    /// Runs `2m + c`.
    pub runs: usize,
    /// `n − 1 − k`: error degrees of freedom after the main effects.
    pub error_df_main: i64,
    /// `n − 1 − 2k`: error degrees of freedom after main and quadratic effects.
    pub error_df_quadratic: i64,
    /// `100 · det(XᵀX)^{1/p} / n` of the main-effects model.
    pub main_d_efficiency: f64,
    /// Largest |correlation| between two quadratic-effect columns (NaN for
    /// one factor).
    pub max_quadratic_correlation: f64,
    /// Largest |correlation| between two two-factor-interaction columns.
    pub max_interaction_correlation: f64,
    /// Average standard error of a quadratic coefficient in the model of main
    /// and quadratic effects, in units of σ (NaN if not estimable).
    pub quadratic_se: f64,
}

/// Pearson correlation of two columns (NaN if either is constant).
fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma).powi(2);
        sbb += (y - mb).powi(2);
    }
    if saa <= 0.0 || sbb <= 0.0 { f64::NAN } else { sab / (saa * sbb).sqrt() }
}

fn dsd_summary(runs: &[Vec<f64>], order: usize) -> DsdSummary {
    let n = runs.len();
    let k = runs.first().map_or(0, Vec::len);
    let center_runs = runs.len() - 2 * order;
    let col = |j: usize| -> Vec<f64> { runs.iter().map(|r| r[j]).collect() };
    let squares: Vec<Vec<f64>> = (0..k).map(|j| col(j).iter().map(|v| v * v).collect()).collect();
    let max_abs =
        |pairs: &mut dyn Iterator<Item = f64>| pairs.filter(|v| v.is_finite()).map(f64::abs).fold(f64::NAN, f64::max);
    let max_quadratic_correlation = max_abs(
        &mut (0..k).flat_map(|a| (a + 1..k).map(move |b| (a, b))).map(|(a, b)| correlation(&squares[a], &squares[b])),
    );
    let interactions: Vec<Vec<f64>> = (0..k)
        .flat_map(|a| (a + 1..k).map(move |b| (a, b)))
        .map(|(a, b)| runs.iter().map(|r| r[a] * r[b]).collect())
        .collect();
    let m = interactions.len();
    let max_interaction_correlation = max_abs(
        &mut (0..m)
            .flat_map(|a| (a + 1..m).map(move |b| (a, b)))
            .map(|(a, b)| correlation(&interactions[a], &interactions[b])),
    );
    let main = crate::optimal::ModelFormula::main_effects(k);
    let main_d_efficiency = crate::optimal::d_efficiency(&main, runs);
    let quadratic = crate::optimal::ModelFormula::from_powers(
        k,
        (0..k)
            .flat_map(|j| {
                let mut a = vec![0u8; k];
                a[j] = 1;
                let mut b = vec![0u8; k];
                b[j] = 2;
                [a, b]
            })
            .collect(),
    );
    let rows: Vec<Vec<f64>> = runs.iter().map(|r| quadratic.expand(r)).collect();
    let p = quadratic.n_terms();
    let info: Vec<Vec<f64>> =
        (0..p).map(|a| (0..p).map(|b| rows.iter().map(|r| r[a] * r[b]).sum()).collect()).collect();
    let quadratic_se = crate::optimal::invert(&info).map_or(f64::NAN, |inv| {
        let se: Vec<f64> = quadratic
            .terms()
            .iter()
            .enumerate()
            .filter(|(_, t)| t.powers.contains(&2))
            .map(|(i, _)| inv[i][i].max(0.0).sqrt())
            .collect();
        se.iter().sum::<f64>() / se.len().max(1) as f64
    });
    DsdSummary {
        extra_runs: 0,
        center_runs,
        order,
        runs: n,
        error_df_main: n as i64 - 1 - k as i64,
        error_df_quadratic: n as i64 - 1 - 2 * k as i64,
        main_d_efficiency,
        max_quadratic_correlation,
        max_interaction_correlation,
        quadratic_se,
    }
}

/// Compare definitive screening designs for `k` factors across choices of
/// extra runs and center runs: one [`DsdSummary`] per choice, in the order
/// given.
///
/// # Errors
/// As [`dsd_order`] for any choice.
pub fn compare_definitive_screening(k: usize, choices: &[DsdOptions]) -> Result<Vec<DsdSummary>> {
    choices
        .iter()
        .map(|o| {
            let d = definitive_screening_with(k, o)?;
            let m = dsd_order(k, o)?;
            Ok(DsdSummary { extra_runs: o.extra_runs, ..dsd_summary(&d.runs, m) })
        })
        .collect()
}

/// The standard choices to compare with `chosen`: no extra runs and the
/// next two conference-matrix orders up, each with one center run and with
/// the chosen number, plus `chosen` itself; duplicates removed, by run count.
#[must_use]
pub fn dsd_comparison_choices(k: usize, chosen: DsdOptions) -> Vec<DsdOptions> {
    let mut out = Vec::new();
    let Ok(base) = dsd_order(k, &DsdOptions::default()) else { return out };
    let orders: Vec<usize> = conference_orders().into_iter().filter(|&m| m >= base).take(3).collect();
    for m in orders {
        for c in [1, chosen.center_runs] {
            let o = DsdOptions { extra_runs: 2 * (m - base), center_runs: c.clamp(1, MAX_DSD_CENTER_RUNS) };
            if !out.contains(&o) {
                out.push(o);
            }
        }
    }
    if let Ok(m) = dsd_order(k, &chosen) {
        let normal = DsdOptions { extra_runs: 2 * (m - base), ..chosen };
        if !out.contains(&normal) {
            out.push(normal);
        }
    }
    out.sort_by_key(|o| (o.extra_runs, o.center_runs));
    out
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(a: &[f64], b: &[f64]) -> f64 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn plackett_burman_columns_are_orthogonal_and_balanced() {
        for n in PLACKETT_BURMAN_SIZES {
            let d = plackett_burman_runs(n - 1, n).unwrap();
            assert_eq!(d.n_runs(), n);
            for a in 0..n - 1 {
                let ca = d.column(a);
                assert_eq!(ca.iter().sum::<f64>(), 0.0, "N = {n}, column {a} balanced");
                for b in a + 1..n - 1 {
                    assert_eq!(dot(&ca, &d.column(b)), 0.0, "N = {n}, columns {a}, {b}");
                }
            }
        }
        assert_eq!(plackett_burman(11).unwrap().n_runs(), 12);
        assert_eq!(plackett_burman(12).unwrap().n_runs(), 16);
        assert_eq!(plackett_burman(5).unwrap().n_runs(), 8);
        assert_eq!(plackett_burman(25).unwrap().n_runs(), 32);
        assert!(plackett_burman(0).is_err());
        assert!(plackett_burman_runs(12, 12).is_err());
        assert!(plackett_burman_runs(5, 28).is_err());
    }

    #[test]
    fn conference_matrices_satisfy_the_defining_identity() {
        let orders = conference_orders();
        assert_eq!(orders, [4, 6, 8, 10, 12, 14, 18, 20, 24, 26, 28, 30, 32, 38, 42, 44, 48]);
        for m in orders {
            let c = conference_matrix(m).unwrap();
            for i in 0..m {
                assert_eq!(c[i][i], 0.0);
                for j in 0..m {
                    let cols: f64 = (0..m).map(|r| c[r][i] * c[r][j]).sum();
                    let expected = if i == j { (m - 1) as f64 } else { 0.0 };
                    assert_eq!(cols, expected, "order {m}, columns {i}, {j}");
                    if i != j {
                        assert_eq!(c[i][j].abs(), 1.0);
                    }
                }
            }
        }
        assert!(conference_matrix(16).is_err());
    }

    #[test]
    fn definitive_screening_properties() {
        for k in [3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 17] {
            let d = definitive_screening(k).unwrap();
            let n = d.n_runs();
            assert_eq!(n % 2, 1);
            assert!(n >= 13, "at least order 6");
            let cols: Vec<Vec<f64>> = (0..k).map(|j| d.column(j)).collect();
            for a in 0..k {
                // Each factor at 0 in exactly 3 runs (its two conference zeros and the centre).
                assert_eq!(cols[a].iter().filter(|v| **v == 0.0).count(), 3, "k = {k}");
                for b in 0..k {
                    if a != b {
                        assert_eq!(dot(&cols[a], &cols[b]), 0.0, "main effects orthogonal");
                    }
                    // Main effects orthogonal to quadratic and interaction columns.
                    let sq: Vec<f64> = cols[b].iter().map(|v| v * v).collect();
                    assert_eq!(dot(&cols[a], &sq), 0.0);
                    for c in 0..k {
                        let inter: Vec<f64> = cols[b].iter().zip(&cols[c]).map(|(x, y)| x * y).collect();
                        assert_eq!(dot(&cols[a], &inter), 0.0);
                    }
                }
            }
        }
        assert_eq!(definitive_screening(6).unwrap().n_runs(), 13);
        assert_eq!(definitive_screening(7).unwrap().n_runs(), 17);
        assert_eq!(definitive_screening(10).unwrap().n_runs(), 21);
        assert_eq!(definitive_screening(15).unwrap().n_runs(), 37, "order 16 is unavailable; 18 is used");
        assert!(definitive_screening(2).is_err());
        // The largest conference order is 48, so 48 factors is the limit.
        let d = definitive_screening(48).unwrap();
        assert_eq!((d.n_runs(), d.factors.len()), (97, 48));
        let e = definitive_screening(49).unwrap_err().to_string();
        assert!(e.contains("48"), "{e}");
    }

    /// Extra runs come from a larger conference matrix and extra center runs
    /// are appended; main effects stay orthogonal to each other and to every
    /// second-order column, and the quadratic columns' correlation follows
    /// from the zero pattern: with `n = 2m + c` runs, each squared column is 0
    /// in its two conference zeros and the `c` centers and both are 0 only at
    /// the centers, so `r = (n(n − 4 − c) − (n − 2 − c)²) / ((n − 2 − c)(2 + c))`
    /// (`(n − 9) / (3(n − 3))` with one center run, Jones & Nachtsheim 2011).
    #[test]
    fn extra_runs_and_center_runs_keep_the_properties() {
        for (k, extra, centers, runs) in [(6, 0, 1, 13), (6, 4, 1, 17), (6, 2, 4, 20), (8, 0, 3, 19), (5, 8, 2, 22)] {
            let o = DsdOptions { extra_runs: extra, center_runs: centers };
            let d = definitive_screening_with(k, &o).unwrap();
            assert_eq!(d.n_runs(), runs, "k = {k}, extra = {extra}, centers = {centers}");
            assert_eq!(d.point_types.iter().filter(|t| **t == PointType::Center).count(), centers);
            let cols: Vec<Vec<f64>> = (0..k).map(|j| d.column(j)).collect();
            for a in 0..k {
                for b in 0..k {
                    if a != b {
                        assert_eq!(dot(&cols[a], &cols[b]), 0.0);
                    }
                    let sq: Vec<f64> = cols[b].iter().map(|v| v * v).collect();
                    assert_eq!(dot(&cols[a], &sq), 0.0);
                    for c in 0..k {
                        let inter: Vec<f64> = cols[b].iter().zip(&cols[c]).map(|(x, y)| x * y).collect();
                        assert_eq!(dot(&cols[a], &inter), 0.0);
                    }
                }
            }
            let m = dsd_order(k, &o).unwrap();
            let s = compare_definitive_screening(k, &[o]).unwrap()[0];
            assert_eq!((s.order, s.runs, s.center_runs, s.extra_runs), (m, runs, centers, extra));
            assert_eq!(s.error_df_main, runs as i64 - 1 - k as i64);
            assert_eq!(s.error_df_quadratic, runs as i64 - 1 - 2 * k as i64);
            let (nf, cf) = (runs as f64, centers as f64);
            let r = (nf * (nf - 4.0 - cf) - (nf - 2.0 - cf).powi(2)) / ((nf - 2.0 - cf) * (2.0 + cf));
            assert!((s.max_quadratic_correlation - r.abs()).abs() < 1e-12, "{} vs {r}", s.max_quadratic_correlation);
            if centers == 1 {
                assert!((r - (nf - 9.0) / (3.0 * (nf - 3.0))).abs() < 1e-12);
            }
            // Main-effects columns have n − 2 − c non-zero entries: XᵀX = diag(n, n − 2 − c, …).
            let p = (k + 1) as f64;
            let want = 100.0 * (nf * (nf - 2.0 - cf).powf(k as f64)).powf(1.0 / p) / nf;
            assert!((s.main_d_efficiency - want).abs() < 1e-9, "{} vs {want}", s.main_d_efficiency);
            assert!(s.quadratic_se.is_finite() && s.quadratic_se > 0.0);
            assert!(s.max_interaction_correlation > 0.0 && s.max_interaction_correlation < 1.0);
        }
        // More center runs sharpen the quadratic effects.
        let ses: Vec<f64> = compare_definitive_screening(
            6,
            &[DsdOptions { extra_runs: 0, center_runs: 1 }, DsdOptions { extra_runs: 0, center_runs: 4 }],
        )
        .unwrap()
        .iter()
        .map(|s| s.quadratic_se)
        .collect();
        assert!(ses[1] < ses[0], "{ses:?}");
        assert!(definitive_screening_with(6, &DsdOptions { extra_runs: 0, center_runs: 0 }).is_err());
        assert!(definitive_screening_with(40, &DsdOptions { extra_runs: 40, center_runs: 1 }).is_err());
        let choices = dsd_comparison_choices(6, DsdOptions { extra_runs: 0, center_runs: 3 });
        assert_eq!(choices.len(), 6);
        assert_eq!(choices[0], DsdOptions::default());
        assert!(choices.contains(&DsdOptions { extra_runs: 4, center_runs: 3 }));
    }
}
