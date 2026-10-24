//! Full and fractional factorial designs.
//!
//! # Full factorials
//! [`full_factorial`] crosses every level of every factor, with `L` levels coded
//! as `L` equally spaced values from −1 to +1 (two levels: −1, +1; three levels:
//! −1, 0, +1). Runs are in Yates standard order: the first factor changes
//! fastest.
//!
//! # Two-level fractional factorials `2^(k−p)`
//! The first `k − p` ("base") factors form a full `2^(k−p)` factorial and each
//! of the remaining `p` factors is set to the product of a subset of the base
//! columns, its **generator** (e.g. `E = ABCD`). Each generator gives a word of
//! the **defining relation** (`I = ABCDE`); all `2^p − 1` products of these words
//! form the complete defining relation. The **resolution** is the length of its
//! shortest word: in a resolution III design main effects are aliased with
//! two-factor interactions, in IV two-factor interactions are aliased with each
//! other, in V and above no main effect or two-factor interaction is aliased with
//! another (Box, Hunter & Hunter, *Statistics for Experimenters*, 2nd ed., ch. 6;
//! Montgomery, *Design and Analysis of Experiments*, ch. 8).
//!
//! [`fractional_factorial`] uses a table of minimum-aberration generators for
//! `k ≤ 11` (the generators tabulated in Montgomery, Table 8.14), and
//! [`fractional_factorial_with_generators`] accepts any generators. Factors are
//! named `A, B, …` (skipping `I`) in generator words.

use crate::design::{Design, Factor, PointType, default_factor_names};
use stat_core::text::superscript;
use stat_core::{Result, StatError};

/// A full factorial design with `levels[j]` levels for factor `j`.
///
/// # Errors
/// [`StatError::InvalidArgument`] if there are no factors, a factor has fewer
/// than two levels, or the design would have more than 1 000 000 runs.
pub fn full_factorial(levels: &[usize]) -> Result<Design> {
    if levels.is_empty() {
        return Err(invalid("a design needs at least one factor"));
    }
    if levels.iter().any(|&l| l < 2) {
        return Err(invalid("every factor needs at least two levels"));
    }
    let n = levels.iter().try_fold(1usize, |acc, &l| acc.checked_mul(l)).filter(|&n| n <= 1_000_000);
    let Some(n) = n else { return Err(invalid("the full factorial would have more than 1 000 000 runs")) };
    let coded = |l: usize, i: usize| if l == 1 { 0.0 } else { -1.0 + 2.0 * i as f64 / (l - 1) as f64 };
    let mut runs = Vec::with_capacity(n);
    for r in 0..n {
        let mut rest = r;
        let run: Vec<f64> = levels
            .iter()
            .map(|&l| {
                let i = rest % l;
                rest /= l;
                coded(l, i)
            })
            .collect();
        runs.push(run);
    }
    let k = levels.len();
    let description = if levels.iter().all(|&l| l == levels[0]) {
        format!("{}{} full factorial ({n} runs)", levels[0], superscript(&k.to_string()))
    } else {
        let parts: Vec<String> = levels.iter().map(usize::to_string).collect();
        format!("{} full factorial ({n} runs)", parts.join("×"))
    };
    let factors = default_factor_names(k).into_iter().map(Factor::coded).collect();
    Design::new(description, factors, runs, vec![PointType::Factorial; n])
}

/// `2^k` full factorial.
///
/// # Errors
/// As for [`full_factorial`].
pub fn two_level_factorial(k: usize) -> Result<Design> {
    full_factorial(&vec![2; k])
}

/// `3^k` full factorial (levels −1, 0, +1).
///
/// # Errors
/// As for [`full_factorial`].
pub fn three_level_factorial(k: usize) -> Result<Design> {
    full_factorial(&vec![3; k])
}

/// Minimum-aberration generators by `(k, p)`: the words for the generated
/// factors, in order, over the base factor letters.
const GENERATORS: &[(usize, usize, &[&str])] = &[
    (3, 1, &["AB"]),
    (4, 1, &["ABC"]),
    (5, 1, &["ABCD"]),
    (5, 2, &["AB", "AC"]),
    (6, 1, &["ABCDE"]),
    (6, 2, &["ABC", "BCD"]),
    (6, 3, &["AB", "AC", "BC"]),
    (7, 1, &["ABCDEF"]),
    (7, 2, &["ABCD", "ABDE"]),
    (7, 3, &["ABC", "BCD", "ACD"]),
    (7, 4, &["AB", "AC", "BC", "ABC"]),
    (8, 1, &["ABCDEFG"]),
    (8, 2, &["ABCD", "ABEF"]),
    (8, 3, &["ABC", "ABD", "BCDE"]),
    (8, 4, &["BCD", "ACD", "ABC", "ABD"]),
    (9, 1, &["ABCDEFGH"]),
    (9, 2, &["ACDFG", "BCEFG"]),
    (9, 3, &["ABCD", "ACEF", "CDEF"]),
    (9, 4, &["BCDE", "ACDE", "ABDE", "ABCE"]),
    (9, 5, &["ABC", "BCD", "ACD", "ABD", "ABCD"]),
    (10, 1, &["ABCDEFGHJ"]),
    (10, 3, &["ABCG", "BCDE", "ACDF"]),
    (10, 4, &["BCDF", "ACDF", "ABDE", "ABCE"]),
    (10, 5, &["ABCD", "ABCE", "ABDE", "ACDE", "BCDE"]),
    (10, 6, &["ABC", "BCD", "ACD", "ABD", "ABCD", "AB"]),
    (11, 1, &["ABCDEFGHJK"]),
    (11, 4, &["ABCG", "BCDE", "ACDF", "ABCDEFG"]),
    (11, 5, &["CDE", "ABCD", "ABF", "BDEF", "ADEF"]),
    (11, 6, &["ABC", "BCD", "CDE", "ACD", "ADE", "BDE"]),
    (11, 7, &["ABC", "BCD", "ACD", "ABD", "ABCD", "AB", "AC"]),
];

/// The `(k, p)` pairs with tabulated generators.
#[must_use]
pub fn tabulated_fractions() -> Vec<(usize, usize)> {
    GENERATORS.iter().map(|&(k, p, _)| (k, p)).collect()
}

/// A two-level fractional factorial with its aliasing information.
#[derive(Debug, Clone, PartialEq)]
pub struct FractionalFactorial {
    /// The design (`2^(k−p)` runs).
    pub design: Design,
    /// Number of factors `k`.
    pub k: usize,
    /// Fraction exponent `p`.
    pub p: usize,
    /// Generator words as bit masks over factors (bit `j` = factor `j`); each
    /// includes its generated factor.
    pub generators: Vec<u32>,
    /// Every word of the defining relation, shortest first.
    pub defining_relation: Vec<u32>,
    /// Length of the shortest word (`usize::MAX` for a full factorial).
    pub resolution: usize,
}

/// Factor bit mask from letters such as `"ABD"` (letters as in
/// [`default_factor_names`]).
///
/// # Errors
/// [`StatError::InvalidArgument`] for an unknown letter or one at or beyond `limit`.
pub fn parse_word(word: &str, limit: usize) -> Result<u32> {
    let names = default_factor_names(limit.min(25));
    let mut mask = 0u32;
    for ch in word.chars().filter(|c| !c.is_whitespace() && *c != '*' && *c != '·') {
        let up = ch.to_ascii_uppercase().to_string();
        let j = names
            .iter()
            .position(|n| *n == up)
            .ok_or_else(|| invalid(format!("unknown factor letter {ch:?} in {word:?}")))?;
        mask ^= 1 << j;
    }
    Ok(mask)
}

/// Letters of a bit-mask word, e.g. `0b1011 → "ABD"`.
#[must_use]
pub fn word_letters(word: u32, k: usize) -> String {
    let names = default_factor_names(k);
    (0..k).filter(|j| word & (1 << j) != 0).map(|j| names[j].as_str()).collect()
}

/// Roman numeral for a resolution.
#[must_use]
pub fn resolution_label(resolution: usize) -> String {
    match resolution {
        3 => "III".into(),
        4 => "IV".into(),
        5 => "V".into(),
        6 => "VI".into(),
        7 => "VII".into(),
        8 => "VIII".into(),
        9 => "IX".into(),
        10 => "X".into(),
        usize::MAX => "full".into(),
        r => r.to_string(),
    }
}

/// `2^(k−p)` fractional factorial from the generator table.
///
/// # Errors
/// [`StatError::InvalidArgument`] if `p == 0` is not requested as a full
/// factorial with `k ≤ 20`, or `(k, p)` is not tabulated (supply generators with
/// [`fractional_factorial_with_generators`]).
pub fn fractional_factorial(k: usize, p: usize) -> Result<FractionalFactorial> {
    if p == 0 {
        return fractional_factorial_with_generators(k, &[]);
    }
    let words = GENERATORS.iter().find(|&&(kk, pp, _)| kk == k && pp == p).map(|&(_, _, w)| w).ok_or_else(|| {
        invalid(format!("no tabulated 2{} design; give generators explicitly", superscript(&format!("{k}−{p}"))))
    })?;
    fractional_factorial_with_generators(k, words)
}

/// The smallest fraction (largest `p`) of `2^k` with at least the given
/// resolution among the tabulated designs, or `0` (the full factorial).
#[must_use]
pub fn smallest_fraction(k: usize, min_resolution: usize) -> usize {
    GENERATORS
        .iter()
        .filter(|&&(kk, _, _)| kk == k)
        .filter_map(|&(_, p, words)| {
            let f = fractional_factorial_with_generators(k, words).ok()?;
            (f.resolution >= min_resolution).then_some(p)
        })
        .max()
        .unwrap_or(0)
}

/// `2^(k−p)` fractional factorial with the given generators: `generators[i]` is
/// the word (over the first `k − p` letters) defining factor `k − p + i`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `k` outside `2..=20`, more generators than
/// factors allow, a word using non-base letters, or generators that make two
/// factors identical (a word of length ≤ 1 in the defining relation).
pub fn fractional_factorial_with_generators(k: usize, generators: &[&str]) -> Result<FractionalFactorial> {
    if !(2..=20).contains(&k) {
        return Err(invalid("two-level factorials support 2 to 20 factors"));
    }
    let p = generators.len();
    if p >= k {
        return Err(invalid("there must be fewer generators than factors"));
    }
    let base = k - p;
    let mut words = Vec::with_capacity(p);
    for (i, g) in generators.iter().enumerate() {
        let w = parse_word(g, k)?;
        if w >> base != 0 {
            return Err(invalid(format!("generator {g:?} may only use the {base} base factors")));
        }
        if w.count_ones() < 2 {
            return Err(invalid(format!("generator {g:?} would make two factors identical")));
        }
        words.push(w | (1 << (base + i)));
    }
    // Complete defining relation: products (XOR) of every non-empty subset.
    let mut relation: Vec<u32> = (1u32..(1 << p))
        .map(|subset| (0..p).filter(|i| subset & (1 << i) != 0).fold(0, |acc, i| acc ^ words[i]))
        .collect();
    relation.sort_by_key(|w| (w.count_ones(), *w));
    let resolution = relation.iter().map(|w| w.count_ones() as usize).min().unwrap_or(usize::MAX);
    if resolution < 3 {
        return Err(invalid("these generators alias a main effect with another (resolution below III)"));
    }

    let full = two_level_factorial(base)?;
    let runs: Vec<Vec<f64>> = full
        .runs
        .iter()
        .map(|r| {
            let mut run = r.clone();
            for &w in &words {
                let product: f64 = (0..base).filter(|j| w & (1 << j) != 0).map(|j| r[j]).product();
                run.push(product);
            }
            run
        })
        .collect();
    let n = runs.len();
    let description = if p == 0 {
        format!("2{} full factorial ({n} runs)", superscript(&k.to_string()))
    } else {
        format!(
            "2{} fractional factorial, resolution {} ({n} runs)",
            superscript(&format!("{k}−{p}")),
            resolution_label(resolution)
        )
    };
    let factors = default_factor_names(k).into_iter().map(Factor::coded).collect();
    let mut design = Design::new(description, factors, runs, vec![PointType::Factorial; n])?;
    let mut f = FractionalFactorial {
        design: design.clone(),
        k,
        p,
        generators: words,
        defining_relation: relation,
        resolution,
    };
    design.notes = f.alias_notes();
    f.design = design;
    Ok(f)
}

impl FractionalFactorial {
    /// Effects (as bit masks) aliased with `effect`, including itself, sorted
    /// by order then letters.
    #[must_use]
    pub fn aliases(&self, effect: u32) -> Vec<u32> {
        let mut out: Vec<u32> =
            std::iter::once(effect).chain(self.defining_relation.iter().map(|w| w ^ effect)).collect();
        out.sort_by_key(|w| (w.count_ones(), *w));
        out
    }

    /// Generator, defining-relation and alias notes. Aliases list effects of up
    /// to three factors for every main effect and two-factor interaction that is
    /// aliased with another effect of order two or less.
    #[must_use]
    pub fn alias_notes(&self) -> Vec<String> {
        let k = self.k;
        let mut notes = Vec::new();
        if self.p == 0 {
            notes.push("Full factorial: no effects are aliased.".to_owned());
            return notes;
        }
        let base = k - self.p;
        let gens: Vec<String> = self
            .generators
            .iter()
            .enumerate()
            .map(|(i, &w)| {
                let generated = 1u32 << (base + i);
                format!("{} = {}", word_letters(generated, k), word_letters(w ^ generated, k))
            })
            .collect();
        notes.push(format!("Generators: {}", gens.join(", ")));
        let relation: Vec<String> = self.defining_relation.iter().map(|&w| word_letters(w, k)).collect();
        notes.push(format!("Defining relation: I = {}", relation.join(" = ")));
        notes.push(format!("Resolution {}", resolution_label(self.resolution)));
        let mut seen = std::collections::HashSet::new();
        let low_order: Vec<u32> = (0..k)
            .map(|j| 1u32 << j)
            .chain((0..k).flat_map(|a| (a + 1..k).map(move |b| (1u32 << a) | (1u32 << b))))
            .collect();
        let mut chains = Vec::new();
        for e in low_order {
            if !seen.insert(e) {
                continue;
            }
            let chain: Vec<u32> = self.aliases(e).into_iter().filter(|w| w.count_ones() <= 3).collect();
            for &w in &chain {
                seen.insert(w);
            }
            if chain.iter().filter(|w| w.count_ones() <= 2).count() > 1 || (e.count_ones() == 1 && chain.len() > 1) {
                chains.push(chain.iter().map(|&w| word_letters(w, k)).collect::<Vec<_>>().join(" = "));
            }
        }
        if chains.is_empty() {
            notes.push(
                "No main effect or two-factor interaction is aliased with an effect of three factors or fewer."
                    .to_owned(),
            );
        } else {
            notes.push("Aliases (effects of more than three factors omitted):".to_owned());
            notes.extend(chains.into_iter().map(|c| format!("  {c}")));
        }
        notes
    }
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
    fn full_factorials_in_standard_order() {
        let d = two_level_factorial(3).unwrap();
        assert_eq!(d.n_runs(), 8);
        assert_eq!(d.column(0), [-1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0], "A alternates fastest");
        assert_eq!(d.column(2), [-1.0, -1.0, -1.0, -1.0, 1.0, 1.0, 1.0, 1.0]);
        let t = three_level_factorial(2).unwrap();
        assert_eq!(t.n_runs(), 9);
        assert_eq!(t.column(0)[..3], [-1.0, 0.0, 1.0]);
        let m = full_factorial(&[2, 3, 4]).unwrap();
        assert_eq!(m.n_runs(), 24);
        assert!(m.description.contains("2×3×4"));
        assert_eq!(m.column(2)[6], -1.0 + 2.0 / 3.0);
        assert!(full_factorial(&[2, 1]).is_err());
        assert!(full_factorial(&[]).is_err());
        assert!(full_factorial(&[2; 21]).is_err(), "over a million runs");
    }

    /// Every tabulated design has the resolution given in the source table,
    /// orthogonal balanced columns, and no repeated runs.
    #[test]
    fn tabulated_designs() {
        let expected: [(usize, usize, usize); 26] = [
            (3, 1, 3),
            (4, 1, 4),
            (5, 1, 5),
            (5, 2, 3),
            (6, 1, 6),
            (6, 2, 4),
            (6, 3, 3),
            (7, 1, 7),
            (7, 2, 4),
            (7, 3, 4),
            (7, 4, 3),
            (8, 2, 5),
            (8, 3, 4),
            (8, 4, 4),
            (9, 2, 6),
            (9, 3, 4),
            (9, 4, 4),
            (9, 5, 3),
            (10, 3, 5),
            (10, 4, 4),
            (10, 5, 4),
            (10, 6, 3),
            (11, 4, 5),
            (11, 5, 4),
            (11, 6, 4),
            (11, 7, 3),
        ];
        for (k, p, res) in expected {
            let f = fractional_factorial(k, p).unwrap();
            assert_eq!(f.resolution, res, "2^({k}-{p})");
            let d = &f.design;
            assert_eq!(d.n_runs(), 1 << (k - p));
            for a in 0..k {
                let ca = d.column(a);
                assert_eq!(ca.iter().sum::<f64>(), 0.0, "balanced");
                for b in a + 1..k {
                    assert_eq!(dot(&ca, &d.column(b)), 0.0, "orthogonal {a} {b} in 2^({k}-{p})");
                }
            }
            let mut runs = d.runs.clone();
            runs.sort_by(|x, y| x.partial_cmp(y).unwrap());
            runs.dedup();
            assert_eq!(runs.len(), d.n_runs(), "distinct runs");
            assert_eq!(f.defining_relation.len(), (1 << p) - 1);
        }
        for p in [1usize] {
            for k in 3..=11 {
                assert_eq!(fractional_factorial(k, p).unwrap().resolution, k, "half fraction of 2^{k}");
            }
        }
    }

    #[test]
    fn generated_columns_and_aliases() {
        // 2^(4-1), D = ABC: I = ABCD, resolution IV.
        let f = fractional_factorial(4, 1).unwrap();
        let d = &f.design;
        for r in &d.runs {
            assert_eq!(r[3], r[0] * r[1] * r[2]);
        }
        assert_eq!(f.defining_relation, [0b1111]);
        // AB = CD.
        assert_eq!(f.aliases(0b0011), [0b0011, 0b1100]);
        let notes = d.notes.join("\n");
        assert!(notes.contains("Generators: D = ABC"));
        assert!(notes.contains("I = ABCD"));
        assert!(notes.contains("A = BCD"));
        assert!(notes.contains("AB = CD"));
        // 2^(5-2) resolution III: main effects aliased with two-factor interactions.
        let f = fractional_factorial(5, 2).unwrap();
        assert!(f.design.notes.iter().any(|n| n.trim_start().starts_with("A = BD")), "{:?}", f.design.notes);
        assert!(f.design.description.contains("resolution III"));
        // Resolution V: no low-order aliasing to report.
        let f = fractional_factorial(5, 1).unwrap();
        assert!(f.design.notes.iter().any(|n| n.starts_with("No main effect")));
    }

    #[test]
    fn custom_generators_and_errors() {
        // D = AB, E = AC: I = ABD = ACE = BCDE.
        let f = fractional_factorial_with_generators(5, &["AB", "AC"]).unwrap();
        assert_eq!(f.resolution, 3);
        assert_eq!(f.defining_relation, [0b01011, 0b10101, 0b11110]);
        assert!(fractional_factorial_with_generators(4, &["A"]).is_err(), "D = A");
        assert!(fractional_factorial_with_generators(4, &["ABD"]).is_err(), "D is not a base factor");
        assert!(fractional_factorial_with_generators(4, &["ABQ"]).is_err());
        assert!(fractional_factorial_with_generators(5, &["AB", "AB"]).is_err(), "D = E");
        assert!(fractional_factorial(12, 3).is_err(), "not tabulated");
        assert_eq!(smallest_fraction(5, 5), 1);
        assert_eq!(smallest_fraction(8, 5), 2);
        assert_eq!(smallest_fraction(4, 5), 0);
        assert_eq!(parse_word("a*b·d", 4).unwrap(), 0b1011);
        assert_eq!(word_letters(0b1011, 4), "ABD");
    }
}
