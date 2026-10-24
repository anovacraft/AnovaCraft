//! Chi-square goodness of fit for counts (Pearson 1900; Agresti,
//! *Categorical Data Analysis*, 3rd ed., §1.5; Cochran 1954, *Biometrics*
//! 10).
//!
//! With observed counts `Oᵢ` in `c` cells and expected counts `Eᵢ = n·πᵢ`
//! under the hypothesized cell probabilities,
//!
//! ```text
//! X² = Σ (Oᵢ − Eᵢ)²/Eᵢ,    G² = 2 Σ Oᵢ ln(Oᵢ/Eᵢ),
//! ```
//!
//! each referred to χ² with `c − 1 − m` degrees of freedom, `m` the number
//! of parameters estimated from the data. Each cell's contribution
//! `(Oᵢ − Eᵢ)²/Eᵢ` and its Pearson residual `(Oᵢ − Eᵢ)/√Eᵢ` show where the
//! fit fails.
//!
//! * **Given proportions:** `πᵢ` are the given proportions (or ratios,
//!   rescaled to sum to one); `m = 0`.
//! * **Poisson:** the mean is estimated by the sample mean `λ̂ = Σ k·f_k / n`
//!   (`m = 1`); the cells are the counts `0, 1, 2, …` with the last cell open
//!   (`≥ K`), so the expected counts add up to `n`. Neighbouring cells are
//!   pooled, from the left, until each expected count reaches the minimum
//!   (Cochran's rule of 5 by default); a remainder short of it at the right
//!   joins the last full group.

use crate::common::{check_finite, invalid};
use stat_core::Result;
use stat_core::discrete::{poisson_pmf, poisson_sf_inclusive};
use stat_core::distributions::{ChiSquared, ContinuousDistribution};

/// One cell (or pooled group of cells) of a goodness-of-fit table.
#[derive(Debug, Clone, PartialEq)]
pub struct FitCell {
    /// Smallest value in the cell (Poisson counts; the cell index otherwise).
    pub low: u64,
    /// Largest value in the cell (`None`: open, `low` and above).
    pub high: Option<u64>,
    /// Observed count.
    pub observed: f64,
    /// Expected count.
    pub expected: f64,
    /// Hypothesized probability of the cell.
    pub probability: f64,
    /// `(O − E)²/E`.
    pub contribution: f64,
    /// `(O − E)/√E`.
    pub residual: f64,
}

/// Result of a goodness-of-fit test.
#[derive(Debug, Clone, PartialEq)]
pub struct GoodnessOfFit {
    /// The cells, after any pooling.
    pub cells: Vec<FitCell>,
    /// Total count `n`.
    pub total: f64,
    /// Pearson's `X²`.
    pub pearson: f64,
    /// Likelihood-ratio `G²`.
    pub likelihood_ratio: f64,
    /// Degrees of freedom `c − 1 − m`.
    pub df: f64,
    /// p-value of `X²`.
    pub pearson_p: f64,
    /// p-value of `G²`.
    pub likelihood_ratio_p: f64,
    /// Parameters estimated from the data (`m`).
    pub estimated: usize,
    /// Estimated Poisson mean (NaN for given proportions).
    pub mean: f64,
    /// Number of cells with an expected count below 5.
    pub small_expected: usize,
}

fn finish(cells: Vec<(u64, Option<u64>, f64, f64)>, estimated: usize, mean: f64) -> Result<GoodnessOfFit> {
    let total: f64 = cells.iter().map(|c| c.2).sum();
    let df = cells.len() as f64 - 1.0 - estimated as f64;
    if df < 1.0 {
        return Err(invalid(format!(
            "too few cells for a test: {} cell(s) with {estimated} estimated parameter(s) leave no degrees of freedom",
            cells.len()
        )));
    }
    let mut pearson = 0.0;
    let mut g = 0.0;
    let mut out = Vec::with_capacity(cells.len());
    for (low, high, observed, probability) in cells {
        let expected = total * probability;
        if expected <= 0.0 {
            return Err(invalid("a cell has an expected count of zero; give it a positive proportion"));
        }
        let contribution = (observed - expected).powi(2) / expected;
        pearson += contribution;
        if observed > 0.0 {
            g += 2.0 * observed * (observed / expected).ln();
        }
        out.push(FitCell {
            low,
            high,
            observed,
            expected,
            probability,
            contribution,
            residual: (observed - expected) / expected.sqrt(),
        });
    }
    let chi = ChiSquared::new(df)?;
    Ok(GoodnessOfFit {
        small_expected: out.iter().filter(|c| c.expected < 5.0).count(),
        cells: out,
        total,
        pearson,
        likelihood_ratio: g,
        df,
        pearson_p: chi.sf(pearson)?,
        likelihood_ratio_p: chi.sf(g.max(0.0))?,
        estimated,
        mean,
    })
}

/// Test `observed` counts against hypothesized `proportions` (any positive
/// weights; they are rescaled to sum to one).
///
/// # Errors
/// Fewer than two cells, lengths that differ, negative or non-finite counts,
/// non-positive proportions, or a zero total.
pub fn goodness_of_fit_proportions(observed: &[f64], proportions: &[f64]) -> Result<GoodnessOfFit> {
    if observed.len() != proportions.len() {
        return Err(invalid(format!(
            "give one proportion per cell ({} cells, {} proportions)",
            observed.len(),
            proportions.len()
        )));
    }
    check_finite(observed, "observed counts")?;
    check_finite(proportions, "proportions")?;
    if observed.iter().any(|&o| o < 0.0) {
        return Err(invalid("counts cannot be negative"));
    }
    if proportions.iter().any(|&p| p <= 0.0) {
        return Err(invalid("every hypothesized proportion must be positive"));
    }
    if observed.iter().sum::<f64>() <= 0.0 {
        return Err(invalid("the counts add up to zero"));
    }
    let sum: f64 = proportions.iter().sum();
    let cells = observed
        .iter()
        .zip(proportions)
        .enumerate()
        .map(|(i, (&o, &p))| (i as u64, Some(i as u64), o, p / sum))
        .collect();
    finish(cells, 0, f64::NAN)
}

/// Test whether counts follow a Poisson distribution with the mean estimated
/// from them. `values` are `(count value, frequency)` pairs (a value may
/// appear more than once); cells are pooled until every expected count is at
/// least `min_expected`.
///
/// # Errors
/// Non-integer or negative values, negative frequencies, a zero total, a
/// mean of zero, or too few cells left after pooling (fewer than three).
pub fn goodness_of_fit_poisson(values: &[(f64, f64)], min_expected: f64) -> Result<GoodnessOfFit> {
    if values.iter().any(|&(v, f)| !(v >= 0.0 && v.fract() == 0.0 && v <= 1e6) || !(f >= 0.0 && f.is_finite())) {
        return Err(invalid("Poisson counts must be whole numbers from 0 to 1,000,000, with non-negative frequencies"));
    }
    let n: f64 = values.iter().map(|v| v.1).sum();
    if n <= 0.0 {
        return Err(invalid("there are no counts"));
    }
    let mean = values.iter().map(|&(v, f)| v * f).sum::<f64>() / n;
    if mean <= 0.0 {
        return Err(invalid("every count is zero: the Poisson mean is zero"));
    }
    let max = values.iter().map(|v| v.0 as u64).max().unwrap_or(0);
    let mut observed = vec![0.0; max as usize + 1];
    for &(v, f) in values {
        observed[v as usize] += f;
    }
    // Cells 0..max, the last open (≥ max).
    let mut cells: Vec<(u64, Option<u64>, f64, f64)> = (0..=max)
        .map(|k| {
            let p = if k == max {
                // P(X ≥ k) directly: 1 − cdf cancels to 0 far in the tail.
                poisson_sf_inclusive(k, mean).unwrap_or(0.0)
            } else {
                poisson_pmf(k, mean).unwrap_or(0.0)
            };
            (k, (k < max).then_some(k), observed[k as usize], p)
        })
        .collect();
    // Pool from the left until each group's expected count reaches the minimum.
    let min = min_expected.max(0.0) / n;
    let mut pooled: Vec<(u64, Option<u64>, f64, f64)> = Vec::new();
    let mut current: Option<(u64, Option<u64>, f64, f64)> = None;
    for cell in cells.drain(..) {
        current = Some(match current {
            None => cell,
            Some((low, _, o, p)) => (low, cell.1, o + cell.2, p + cell.3),
        });
        if let Some(c) = current {
            if c.3 >= min * (1.0 - 1e-12) {
                pooled.push(c);
                current = None;
            }
        }
    }
    if let Some((_, high, o, p)) = current {
        match pooled.last_mut() {
            Some(last) => {
                last.1 = high;
                last.2 += o;
                last.3 += p;
            }
            None => pooled.push((0, high, o, p)),
        }
    }
    finish(pooled, 1, mean)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    /// Mendel's dihybrid cross (round-yellow 315, round-green 108,
    /// wrinkled-yellow 101, wrinkled-green 32) against 9:3:3:1:
    /// X² = 0.470 on 3 df, p = 0.925 (Agresti 2013, §1.5.4). Exact values
    /// by hand: E = 312.75, 104.25, 104.25, 34.75; G² and p-values from
    /// SciPy 1.18 (`chisquare`, `power_divergence`).
    #[test]
    fn mendel_peas() {
        let r = goodness_of_fit_proportions(&[315.0, 108.0, 101.0, 32.0], &[9.0, 3.0, 3.0, 1.0]).unwrap();
        close(r.pearson, 0.470_023_980_815_347_7, 1e-12);
        close(r.df, 3.0, 0.0);
        close(r.pearson_p, 0.925_425_895_103_616, 1e-9);
        close(r.likelihood_ratio, 0.475_445_238_998_233_3, 1e-12);
        close(r.likelihood_ratio_p, 0.924_251_903_974_532_5, 1e-9);
        close(r.cells[3].expected, 34.75, 1e-12);
        close(r.cells[0].contribution, (315.0 - 312.75f64).powi(2) / 312.75, 1e-12);
        assert_eq!(r.small_expected, 0);
        assert!(goodness_of_fit_proportions(&[1.0, 2.0], &[1.0]).is_err());
        assert!(goodness_of_fit_proportions(&[1.0, 2.0], &[1.0, 0.0]).is_err());
        assert!(goodness_of_fit_proportions(&[5.0], &[1.0]).is_err());
    }

    /// Bortkiewicz's deaths from horse kicks in the Prussian cavalry (200
    /// corps-years: 109, 65, 22, 3, 1 with 0–4 deaths; mean 0.61). With
    /// cells of expected count at least 5 the counts 2, 3 and 4+ pool into
    /// one cell, leaving 3 cells and 1 df. Reference values computed
    /// independently in Python with SciPy 1.18 (`poisson.pmf`, `chi2.sf`).
    #[test]
    fn horse_kicks() {
        let data = [(0.0, 109.0), (1.0, 65.0), (2.0, 22.0), (3.0, 3.0), (4.0, 1.0)];
        let r = goodness_of_fit_poisson(&data, 5.0).unwrap();
        close(r.mean, 0.61, 1e-12);
        assert_eq!(r.cells.len(), 3);
        assert_eq!((r.cells[2].low, r.cells[2].high), (2, None));
        close(r.cells[0].expected, 108.670_173_814_899_97, 1e-10);
        close(r.cells[2].observed, 26.0, 0.0);
        close(r.pearson, 0.062_783_831_046_967_76, 1e-9);
        close(r.df, 1.0, 0.0);
        close(r.pearson_p, 0.802_148_883_341_608_3, 1e-9);
        // Without pooling every cell stays: 5 cells, 3 df.
        let all = goodness_of_fit_poisson(&data, 0.0).unwrap();
        assert_eq!(all.cells.len(), 5);
        close(all.df, 3.0, 0.0);
        let total: f64 = all.cells.iter().map(|c| c.expected).sum();
        close(total, 200.0, 1e-10);
        // Frequencies may repeat a value; rows of single counts give the same test.
        let rows: Vec<(f64, f64)> = data.iter().flat_map(|&(v, f)| std::iter::repeat_n((v, 1.0), f as usize)).collect();
        close(goodness_of_fit_poisson(&rows, 5.0).unwrap().pearson, r.pearson, 1e-12);
        assert!(goodness_of_fit_poisson(&[(1.5, 1.0)], 5.0).is_err());
        assert!(goodness_of_fit_poisson(&[(0.0, 10.0)], 5.0).is_err());
        assert!(goodness_of_fit_poisson(&[(0.0, 3.0), (1.0, 2.0)], 5.0).is_err());
    }

    /// Without pooling, an outlying count's open cell P(X ≥ 25) ≈ 1.9e-23
    /// used to cancel to 0 (as 1 − cdf) and fail the test with a zero
    /// expected count. Reference: 100 · scipy.stats.poisson.sf(24, 1.32).
    #[test]
    fn poisson_far_upper_tail() {
        let data = [(0.0, 30.0), (1.0, 40.0), (2.0, 20.0), (3.0, 9.0), (25.0, 1.0)];
        let r = goodness_of_fit_poisson(&data, 0.0).unwrap();
        let last = r.cells.last().unwrap();
        assert_eq!((last.low, last.high), (25, None));
        let reference = 1.875_065_415_968_643_4e-21;
        assert!((last.expected - reference).abs() < 1e-9 * reference, "{}", last.expected);
        assert!(r.pearson.is_finite() && r.pearson_p < 1e-10);
    }
}
