//! Augmenting an existing design with more runs, keeping the runs already
//! made.
//!
//! Every function takes the existing runs in coded units (−1 at a factor's
//! low setting, +1 at its high setting) and returns an [`Augmented`] design:
//! the existing runs (block 1) followed by the new ones (block 2), with notes.
//! A second block is usually run later than the first, so an analysis can
//! include the block as a factor to absorb a shift between the two.
//!
//! * [`fold_over`] — for a two-level design, the **full fold-over** repeats
//!   every run with all signs reversed; the combined design has no main
//!   effect aliased with any two-factor interaction (a resolution III
//!   fraction becomes resolution IV). The **fold-over on one factor**
//!   reverses that factor only; the factor and all its two-factor
//!   interactions become clear of main effects and of the other two-factor
//!   interactions. (Box, Hunter & Hunter, *Statistics for Experimenters*,
//!   2nd ed., §6.8; Montgomery, *Design and Analysis of Experiments*, §8.6.)
//! * [`add_center_runs`] — center runs (all factors at coded 0) give a
//!   pure-error estimate and a test for curvature.
//! * [`add_axial_runs`] — axial ("star") runs at `±α` on each factor axis,
//!   with optional center runs, turn a two-level factorial into a central
//!   composite design (α = 1 is face-centerd; a rotatable design uses
//!   `α = n_f^{1/4}` for `n_f` factorial runs; Box & Wilson 1951).
//! * [`augment_optimal`] — **D-optimal augmentation**: the existing runs stay
//!   fixed and `m` runs are added to maximize `det(XᵀX)` of all runs for a
//!   (possibly larger) model, by the same coordinate exchange as
//!   [`crate::optimal::d_optimal_in`] (only the new runs move; the fixed runs
//!   enter the information matrix), honouring a [`Region`]. Atkinson, Donev
//!   & Tobias, *Optimum Experimental Designs, with SAS*, §19.

use crate::constraints::Region;
use crate::design::PointType;
use crate::optimal::{ModelFormula, OptimalOptions, SearchSpace, d_search, log_det_information, validate};
use stat_core::{Result, StatError};

/// Tolerance for recognising coded levels −1, 0 and +1.
const LEVEL_TOL: f64 = 1e-9;

/// An augmented design.
#[derive(Debug, Clone, PartialEq)]
pub struct Augmented {
    /// What was added, e.g. `"Full fold-over (8 runs)"`.
    pub description: String,
    /// All runs in coded units: the existing ones, then the added ones.
    pub runs: Vec<Vec<f64>>,
    /// Block of each run: 1 for the existing runs, 2 for the added ones.
    pub blocks: Vec<usize>,
    /// Role of each added run.
    pub added_types: Vec<PointType>,
    /// Notes: what changed (aliases broken, efficiencies before and after).
    pub notes: Vec<String>,
}

impl Augmented {
    fn new(description: String, existing: &[Vec<f64>], added: Vec<Vec<f64>>, kind: PointType) -> Self {
        let added_types = vec![kind; added.len()];
        let blocks = std::iter::repeat_n(1, existing.len()).chain(std::iter::repeat_n(2, added.len())).collect();
        let runs = existing.iter().cloned().chain(added).collect();
        Self { description, runs, blocks, added_types, notes: Vec::new() }
    }

    /// Number of existing runs.
    #[must_use]
    pub fn n_existing(&self) -> usize {
        self.blocks.iter().filter(|&&b| b == 1).count()
    }

    /// The added runs.
    #[must_use]
    pub fn added(&self) -> &[Vec<f64>] {
        &self.runs[self.n_existing()..]
    }
}

/// Check the existing runs: at least one, all with `k ≥ 1` finite values.
fn check_runs(runs: &[Vec<f64>]) -> Result<usize> {
    let k = runs.first().map_or(0, Vec::len);
    if runs.is_empty() || k == 0 {
        return Err(invalid("the design has no runs or no factors"));
    }
    if runs.iter().any(|r| r.len() != k || r.iter().any(|v| !v.is_finite())) {
        return Err(invalid("every run needs a value for every factor"));
    }
    Ok(k)
}

fn is_level(v: f64, level: f64) -> bool {
    (v - level).abs() <= LEVEL_TOL
}

/// Fold over a two-level design: every run that is not a center run is
/// repeated with all signs reversed (`factor = None`) or with the sign of
/// one factor reversed. Center runs (all factors at 0) are not repeated.
/// The notes list the aliases between main effects and two-factor
/// interactions (and the number among two-factor interactions) that the
/// fold-over removes, judged by the normalized inner products
/// `Σ x_u x_v / n` of the effect columns over the two-level runs (the alias
/// coefficients of an orthogonal two-level design).
///
/// # Errors
/// [`StatError::InvalidArgument`] for no runs, a factor index out of range,
/// `names` of the wrong length, or a run with a value other than −1 and +1
/// (except all-zero center runs).
pub fn fold_over(runs: &[Vec<f64>], factor: Option<usize>, names: &[String]) -> Result<Augmented> {
    let k = check_runs(runs)?;
    if names.len() != k {
        return Err(invalid("give a name for every factor"));
    }
    if factor.is_some_and(|j| j >= k) {
        return Err(invalid("the factor to fold is not one of the design's factors"));
    }
    let mut corners = Vec::new();
    for (i, r) in runs.iter().enumerate() {
        if r.iter().all(|&v| is_level(v, 0.0)) {
            continue;
        }
        if !r.iter().all(|&v| is_level(v.abs(), 1.0)) {
            return Err(invalid(format!(
                "a fold-over needs a two-level design: run {} has a setting between the low and high levels",
                i + 1
            )));
        }
        corners.push(r.iter().map(|v| v.signum()).collect::<Vec<f64>>());
    }
    let added: Vec<Vec<f64>> = corners
        .iter()
        .map(|r| r.iter().enumerate().map(|(j, &v)| if factor.is_none_or(|f| f == j) { -v } else { v }).collect())
        .collect();
    let n = added.len();
    let description = match factor {
        None => format!("Full fold-over ({n} runs, every sign reversed)"),
        Some(j) => format!("Fold-over on {} ({n} runs, its sign reversed)", names[j]),
    };
    let combined: Vec<Vec<f64>> = corners.iter().chain(&added).cloned().collect();
    let mut out = Augmented::new(description, runs, added, PointType::Foldover);
    let before = aliases(&corners);
    let after = aliases(&combined);
    let label = |e: &[usize]| e.iter().map(|&j| names[j].as_str()).collect::<Vec<_>>().join("×");
    let broken: Vec<&Alias> = before.iter().filter(|a| !after.iter().any(|b| b.0 == a.0 && b.1 == a.1)).collect();
    let main_broken: Vec<String> =
        broken.iter().filter(|a| a.0.len() == 1).map(|a| format!("{} with {}", label(&a.0), label(&a.1))).collect();
    let pair_broken = broken.iter().filter(|a| a.0.len() == 2).count();
    if main_broken.is_empty() && pair_broken == 0 {
        out.notes.push("The fold-over removes no aliasing among main effects and two-factor interactions.".into());
    } else {
        if !main_broken.is_empty() {
            let shown: Vec<String> = main_broken.iter().take(12).cloned().collect();
            let more = main_broken.len().saturating_sub(12);
            out.notes.push(format!(
                "Aliases removed between main effects and two-factor interactions ({}): {}{}.",
                main_broken.len(),
                shown.join(", "),
                if more > 0 { format!(", and {more} more") } else { String::new() }
            ));
        }
        if pair_broken > 0 {
            out.notes.push(format!("Aliases removed among two-factor interactions: {pair_broken}."));
        }
    }
    let main_left = after.iter().filter(|a| a.0.len() == 1).count();
    out.notes.push(if main_left == 0 {
        "After the fold-over no main effect is aliased with a two-factor interaction.".into()
    } else {
        format!("After the fold-over {main_left} main-effect/two-factor-interaction aliases remain.")
    });
    if let Some(j) = factor {
        let clear = !after.iter().any(|a| a.0.contains(&j) || a.1.contains(&j));
        if clear {
            out.notes.push(format!(
                "{} and its two-factor interactions are clear of main effects and other two-factor interactions.",
                names[j]
            ));
        }
    }
    let skipped = runs.len() - corners.len();
    if skipped > 0 {
        out.notes.push(format!("{skipped} center run(s) were not repeated."));
    }
    Ok(out)
}

/// An aliased pair of effects (factor index lists, the lower-order or
/// earlier effect first) with its alias coefficient.
type Alias = (Vec<usize>, Vec<usize>, f64);

/// Pairs among main effects and two-factor interactions whose ±1 columns
/// have a non-zero normalized inner product over `runs`.
fn aliases(runs: &[Vec<f64>]) -> Vec<Alias> {
    let k = runs.first().map_or(0, Vec::len);
    let n = runs.len() as f64;
    let mut effects: Vec<Vec<usize>> = (0..k).map(|j| vec![j]).collect();
    for a in 0..k {
        for b in a + 1..k {
            effects.push(vec![a, b]);
        }
    }
    let column = |e: &[usize]| -> Vec<f64> { runs.iter().map(|r| e.iter().map(|&j| r[j]).product()).collect() };
    let columns: Vec<Vec<f64>> = effects.iter().map(|e| column(e)).collect();
    let mut out = Vec::new();
    for u in 0..effects.len() {
        for v in u + 1..effects.len() {
            if effects[u].iter().any(|j| effects[v].contains(j)) {
                continue; // e.g. A with A×B: not an alias in the usual sense
            }
            let r = columns[u].iter().zip(&columns[v]).map(|(a, b)| a * b).sum::<f64>() / n;
            if r.abs() > 1e-9 {
                out.push((effects[u].clone(), effects[v].clone(), r));
            }
        }
    }
    out
}

/// Add `count` center runs (every factor at coded 0).
///
/// # Errors
/// [`StatError::InvalidArgument`] for no runs or `count` of 0.
pub fn add_center_runs(runs: &[Vec<f64>], count: usize) -> Result<Augmented> {
    let k = check_runs(runs)?;
    if count == 0 {
        return Err(invalid("add at least one center run"));
    }
    let mut out =
        Augmented::new(format!("{count} center run(s) added"), runs, vec![vec![0.0; k]; count], PointType::Center);
    out.notes.push(format!(
        "Center runs give {} degree(s) of freedom for pure error among themselves and a test for curvature.",
        count.saturating_sub(1)
    ));
    Ok(out)
}

/// Add axial runs at `±alpha` on each factor axis (factor order, −α before
/// +α), then `centers` center runs: a two-level factorial becomes a central
/// composite design.
///
/// # Errors
/// [`StatError::InvalidArgument`] for no runs or an `alpha` that is not a
/// positive number.
pub fn add_axial_runs(runs: &[Vec<f64>], alpha: f64, centers: usize) -> Result<Augmented> {
    let k = check_runs(runs)?;
    if !(alpha.is_finite() && alpha > 0.0) {
        return Err(invalid("the axial distance α must be a positive number"));
    }
    let mut added = Vec::with_capacity(2 * k + centers);
    for j in 0..k {
        for s in [-alpha, alpha] {
            let mut r = vec![0.0; k];
            r[j] = s;
            added.push(r);
        }
    }
    let mut out = Augmented::new(
        format!(
            "{} axial runs at ±{}{}",
            2 * k,
            trim_number(alpha),
            if centers > 0 { format!(" and {centers} center run(s)") } else { String::new() }
        ),
        runs,
        added,
        PointType::Axial,
    );
    for _ in 0..centers {
        out.runs.push(vec![0.0; k]);
        out.blocks.push(2);
        out.added_types.push(PointType::Center);
    }
    let factorial = runs.iter().filter(|r| r.iter().all(|&v| is_level(v.abs(), 1.0))).count();
    if is_level(alpha, 1.0) {
        out.notes.push("α = 1: the axial runs are at the factors' low and high settings (face-centerd).".into());
    } else if alpha > 1.0 {
        out.notes.push(format!(
            "α = {}: the axial runs lie beyond the factors' low and high settings; check that they can be run.",
            trim_number(alpha)
        ));
    }
    if factorial > 0 {
        out.notes.push(format!(
            "With {factorial} factorial runs a rotatable design would use α = {} (n_f^¼).",
            trim_number((factorial as f64).powf(0.25))
        ));
    }
    if factorial < runs.len() {
        let others = runs.len() - factorial;
        out.notes.push(format!("{others} existing run(s) are not at the factorial corners."));
    }
    Ok(out)
}

/// D-optimal augmentation: add `options.runs` runs to the fixed coded runs
/// `existing`, maximizing `det(XᵀX)` of all runs for `formula`, with the new
/// runs inside `region` (see the module documentation). The notes give the
/// D-efficiency `100 · det(XᵀX)^{1/p} / n` of the existing runs (or say
/// they cannot estimate every term) and of the augmented design.
///
/// # Errors
/// [`StatError::InvalidArgument`] for no added runs, too few runs in all
/// for the model's terms, a region or runs with a different number of
/// factors, or an empty region; [`StatError::RankDeficient`] if even the
/// augmented design cannot estimate the model.
pub fn augment_optimal(
    existing: &[Vec<f64>],
    formula: &ModelFormula,
    region: &Region,
    options: &OptimalOptions,
    names: &[String],
) -> Result<Augmented> {
    let k = check_runs(existing)?;
    if k != formula.n_factors() {
        return Err(invalid(format!("the design has {k} factors but the model {}", formula.n_factors())));
    }
    if options.runs == 0 {
        return Err(invalid("add at least one run"));
    }
    validate(formula, region, existing.len() + options.runs, options)?;
    let space = SearchSpace::new(formula, region);
    let (log_det, added) = d_search(&space, existing, options.runs, options)?;
    let p = formula.n_terms() as f64;
    let m = added.len();
    let mut out = Augmented::new(
        format!("{m} D-optimal run(s) added for a {}-term model", formula.n_terms()),
        existing,
        added,
        PointType::Other,
    );
    out.notes.push(format!("Model: {}", formula.describe(names)));
    let before = log_det_information(formula, existing);
    let n_all = out.runs.len() as f64;
    out.notes.push(if before.is_finite() {
        format!(
            "D-efficiency of the {} existing runs {:.2}%; of all {} runs {:.2}%.",
            existing.len(),
            100.0 * (before / p).exp() / existing.len() as f64,
            out.runs.len(),
            100.0 * (log_det / p).exp() / n_all
        )
    } else {
        format!(
            "The {} existing runs cannot estimate every term of the model; all {} runs can (D-efficiency {:.2}%).",
            existing.len(),
            out.runs.len(),
            100.0 * (log_det / p).exp() / n_all
        )
    });
    out.notes.push(format!(
        "ln det(XᵀX) = {log_det:.6}; best of {} coordinate-exchange starts (seed {}).",
        options.starts, options.seed
    ));
    if !region.describe().is_empty() {
        out.notes.push(format!("Constraints (every added run satisfies them): {}.", region.describe().join("; ")));
        let outside = existing.iter().filter(|r| !region.contains(r)).count();
        if outside > 0 {
            out.notes.push(format!("{outside} existing run(s) lie outside the constraints."));
        }
    }
    Ok(out)
}

/// A number with up to 4 decimals, trailing zeros removed.
fn trim_number(x: f64) -> String {
    let s = format!("{x:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_owned()
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::default_factor_names;
    use crate::factorial::{fractional_factorial, two_level_factorial};
    use crate::response_surface::{AxialDistance, CcdOptions, central_composite};

    /// Σ over runs of the product of the listed columns (exact for ±1).
    fn inner(runs: &[Vec<f64>], cols: &[usize]) -> f64 {
        runs.iter().map(|r| cols.iter().map(|&j| r[j]).product::<f64>()).sum()
    }

    /// The 2^(7−4) resolution III design folded over becomes resolution IV:
    /// every main-effect column is orthogonal to every two-factor
    /// interaction column (exact integer sums).
    #[test]
    fn full_fold_over_of_resolution_three() {
        let ff = fractional_factorial(7, 4).unwrap();
        assert_eq!(ff.resolution, 3);
        let names = default_factor_names(7);
        let a = fold_over(&ff.design.runs, None, &names).unwrap();
        assert_eq!(a.runs.len(), 16);
        assert_eq!(a.blocks.iter().filter(|&&b| b == 2).count(), 8);
        let mut aliased_before = 0;
        for j in 0..7 {
            for x in 0..7 {
                for y in x + 1..7 {
                    if j == x || j == y {
                        continue;
                    }
                    assert_eq!(inner(&a.runs, &[j, x, y]), 0.0, "{j} with {x}{y}");
                    if inner(&ff.design.runs, &[j, x, y]) != 0.0 {
                        aliased_before += 1;
                    }
                }
            }
        }
        assert_eq!(aliased_before, 21, "each main effect aliased with three interactions");
        // Main effects stay orthogonal to each other and balanced.
        for j in 0..7 {
            assert_eq!(inner(&a.runs, &[j]), 0.0);
            for l in j + 1..7 {
                assert_eq!(inner(&a.runs, &[j, l]), 0.0);
            }
        }
        assert!(a.notes[0].starts_with("Aliases removed between main effects and two-factor interactions (21)"));
        assert!(a.notes.iter().any(|n| n.contains("no main effect is aliased")), "{:?}", a.notes);
        // Folding again changes nothing.
        let again = fold_over(&a.runs, None, &names).unwrap();
        assert!(again.notes[0].contains("removes no aliasing"));
    }

    /// Fold on one factor: that factor and its two-factor interactions are
    /// clear of the main effects and of the other two-factor interactions.
    #[test]
    fn fold_over_on_one_factor() {
        let ff = fractional_factorial(7, 4).unwrap();
        let names = default_factor_names(7);
        let f = 2; // C
        let a = fold_over(&ff.design.runs, Some(f), &names).unwrap();
        assert_eq!(a.runs.len(), 16);
        for i in 0..8 {
            for j in 0..7 {
                let expected = if j == f { -a.runs[i][j] } else { a.runs[i][j] };
                assert_eq!(a.runs[8 + i][j], expected);
            }
        }
        for x in 0..7 {
            for y in x + 1..7 {
                if x != f && y != f {
                    assert_eq!(inner(&a.runs, &[f, x, y]), 0.0, "C with {x}{y}");
                }
            }
        }
        for g in (0..7).filter(|&g| g != f) {
            // C×g against every main effect and every interaction without C or g.
            for j in (0..7).filter(|&j| j != f && j != g) {
                assert_eq!(inner(&a.runs, &[f, g, j]), 0.0);
                for l in (j + 1..7).filter(|&l| l != f && l != g) {
                    assert_eq!(inner(&a.runs, &[f, g, j, l]), 0.0, "C{g} with {j}{l}");
                }
            }
        }
        assert!(a.description.starts_with("Fold-over on C"));
        assert!(a.notes.iter().any(|n| n.starts_with("C and its two-factor interactions are clear")), "{:?}", a.notes);
        assert!(a.notes.iter().any(|n| n.contains("remain")), "other main effects stay aliased");
    }

    #[test]
    fn fold_over_errors_and_center_runs() {
        let names = default_factor_names(2);
        let mut runs = two_level_factorial(2).unwrap().runs;
        runs.push(vec![0.0, 0.0]);
        let a = fold_over(&runs, None, &names).unwrap();
        assert_eq!(a.added().len(), 4, "the center run is not repeated");
        assert!(a.notes.iter().any(|n| n.contains("1 center run(s) were not repeated")));
        runs.push(vec![0.5, 1.0]);
        assert!(fold_over(&runs, None, &names).unwrap_err().to_string().contains("run 6"));
        assert!(fold_over(&runs[..4], Some(2), &names).is_err());
        assert!(fold_over(&[], None, &names).is_err());
        assert!(fold_over(&runs[..4], None, &names[..1]).is_err());
    }

    /// 2² factorial + face-centerd axial runs + 3 center runs is the
    /// face-centerd central composite design.
    #[test]
    fn axial_and_center_runs_make_a_central_composite() {
        let fact = two_level_factorial(2).unwrap().runs;
        let a = add_axial_runs(&fact, 1.0, 3).unwrap();
        let ccd = central_composite(
            2,
            &CcdOptions {
                alpha: AxialDistance::FaceCentered,
                inscribed: false,
                center_points: Some(3),
                fraction: None,
            },
        )
        .unwrap();
        let sorted = |mut r: Vec<Vec<f64>>| {
            r.sort_by(|a, b| a.partial_cmp(b).unwrap());
            r
        };
        assert_eq!(sorted(a.runs.clone()), sorted(ccd.runs));
        assert_eq!(a.added_types.iter().filter(|t| **t == PointType::Axial).count(), 4);
        assert_eq!(a.added_types.iter().filter(|t| **t == PointType::Center).count(), 3);
        assert_eq!(a.blocks, [1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2]);
        assert!(a.notes[0].contains("face-centerd"));
        assert!(a.notes[1].contains("α = 1.4142"), "{:?}", a.notes);
        let r = add_axial_runs(&fact, 1.5, 0).unwrap();
        assert_eq!(r.added()[1], [1.5, 0.0]);
        assert!(r.notes[0].contains("beyond"));
        assert!(add_axial_runs(&fact, 0.0, 1).is_err());
        let c = add_center_runs(&fact, 2).unwrap();
        assert_eq!(c.added(), [[0.0, 0.0], [0.0, 0.0]]);
        assert!(add_center_runs(&fact, 0).is_err());
    }

    /// The 2^(3−1) half fraction (I = ABC) augmented by 4 runs for the model
    /// with all two-factor interactions: the complementary half fraction,
    /// det(XᵀX) = 8⁷ (the full 2³ factorial).
    #[test]
    fn optimal_augmentation_completes_the_half_fraction() {
        let half: Vec<Vec<f64>> =
            two_level_factorial(3).unwrap().runs.into_iter().filter(|r| r[0] * r[1] * r[2] > 0.0).collect();
        assert_eq!(half.len(), 4);
        let f = ModelFormula::interactions(3);
        let names = default_factor_names(3);
        let a = augment_optimal(&half, &f, &Region::cube(3), &OptimalOptions::new(4), &names).unwrap();
        let ld = log_det_information(&f, &a.runs);
        assert!((ld - 7.0 * 8f64.ln()).abs() < 1e-12, "{ld}");
        for r in a.added() {
            assert!(r[0] * r[1] * r[2] < 0.0, "{r:?} is in the other half");
        }
        assert_eq!(a.runs[..4], half[..]);
        assert!(a.notes[1].starts_with("The 4 existing runs cannot estimate every term"), "{:?}", a.notes);
        assert!(a.notes[1].contains("100.00%"));
    }

    /// 2² factorial + center run, 3 runs added for the full quadratic model:
    /// the exhaustive optimum over the 3² grid is ln det = 7.742402021815783
    /// (`tests/data/optimal_reference.py`, case 2).
    #[test]
    fn optimal_augmentation_matches_exhaustive_search() {
        let mut fixed = two_level_factorial(2).unwrap().runs;
        fixed.push(vec![0.0, 0.0]);
        let f = ModelFormula::quadratic(2);
        let names = default_factor_names(2);
        let a = augment_optimal(&fixed, &f, &Region::cube(2), &OptimalOptions::new(3), &names).unwrap();
        let ld = log_det_information(&f, &a.runs);
        assert!((ld - 7.742_402_021_815_783).abs() < 1e-9, "{ld}");
        assert!(a.notes[1].starts_with("The 5 existing runs cannot estimate"), "the 2² cannot estimate both squares");
        // With a constraint the added runs respect it.
        let mut region = Region::cube(2);
        region.add_linear(vec![1.0, 1.0], 0.5, "A + B ≤ 0.5").unwrap();
        let c = augment_optimal(&fixed, &f, &region, &OptimalOptions::new(4), &names).unwrap();
        assert!(c.added().iter().all(|r| region.contains(r)));
        assert!(c.notes.iter().any(|n| n.contains("1 existing run(s) lie outside")), "(+1, +1)");
        // Errors.
        assert!(augment_optimal(&fixed, &f, &Region::cube(2), &OptimalOptions::new(0), &names).is_err());
        assert!(augment_optimal(&fixed[..2], &f, &Region::cube(2), &OptimalOptions::new(3), &names).is_err());
        assert!(
            augment_optimal(&fixed, &ModelFormula::quadratic(3), &Region::cube(3), &OptimalOptions::new(3), &names)
                .is_err()
        );
    }
}
