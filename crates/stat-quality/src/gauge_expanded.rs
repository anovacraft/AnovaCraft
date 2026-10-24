//! Expanded gauge R&R: studies with factors beyond parts and operators
//! (gauges, fixtures, locations, days), each fixed or random, crossed with or
//! nested in the others, balanced or not, fitted by REML.
//!
//! # Model
//! The measurement is modelled as a linear mixed model
//!
//! ```text
//! y = Xβ + Σₜ Zₜ uₜ + ε,   uₜ ~ N(0, σ²ₜ I),   ε ~ N(0, σ² I),
//! ```
//!
//! with one random intercept term `t` for every random factor and every
//! two-factor interaction that involves a random factor, and fixed effects
//! `β` for the intercept, the fixed factors and their interactions
//! (Burdick, Borror & Montgomery 2005, *Design and Analysis of Gauge R&R
//! Studies*, ch. 8 on studies with more than two factors and ch. 9 on nested
//! designs; Searle, Casella & McCulloch 1992, *Variance Components*, §6).
//!
//! * **Terms.** Each factor contributes its main term. A factor nested in
//!   others is identified by its own level together with the levels of every
//!   factor it is nested in (written `Location(Operator)`). With
//!   [`ExpandedGaugeOptions::interactions`], every pair of factors where
//!   neither is nested in the other contributes a two-factor interaction
//!   (`Part × Operator`). A term is random if any factor in it is random.
//!   A fixed factor may not be nested in a random one.
//! * **Fixed effects** use sum-to-zero (effect) coding — within each parent
//!   cell for a nested factor — and products of those columns for fixed
//!   interactions. Columns that are linear combinations of earlier ones
//!   (missing cells) are dropped by Gram–Schmidt with a relative tolerance of
//!   10⁻⁹. Each fixed term is tested by `stat_mixed`'s F test of its
//!   coefficients with Satterthwaite's denominator degrees of freedom; when
//!   some of a term's columns were dropped, its test covers only the
//!   columns kept and a warning says so.
//! * **Estimation.** Restricted maximum likelihood through
//!   [`stat_mixed::fit_mixed`], which keeps variance estimates non-negative.
//!   For balanced data whose ANOVA estimates are all positive, REML and the
//!   ANOVA (expected-mean-square) estimators coincide (Searle, Casella &
//!   McCulloch 1992, §6.2), so a crossed study gives the components of
//!   [`crate::gauge::gauge_rr_anova`] and a nested one those of
//!   [`crate::gauge_nested::gauge_rr_nested`].
//! * **Intervals.** Each variance gets Satterthwaite's χ² limits from its
//!   large-sample standard error ([`stat_mixed::satterthwaite_limits`]). The
//!   totals use the same limits with the standard error `√(1ᵀ C 1)` of the
//!   sum, `C` being the large-sample covariance of the summed variances
//!   ([`stat_mixed::MixedFit::variance_covariance`]).
//! * **Gauge R&R totals** come from the random components only: the part's
//!   main term is part-to-part variation; the main term of a random factor
//!   nested within parts (directly or through another factor, e.g.
//!   `Location(Part)`) is within-part variation, which is product variation,
//!   not measurement error, and is added to part-to-part variation
//!   (AIAG, *Measurement Systems Analysis*, 4th ed., 2010, on within-part
//!   variation; Burdick, Borror & Montgomery 2005, ch. 9); the residual is
//!   repeatability; every other random component (operators, gauges,
//!   interactions, including interactions with a within-part factor) is
//!   reproducibility. Fixed factors shift the mean only; they are reported
//!   as tests and do not enter the totals. The summary measures are those
//!   of the crossed study, with part-to-part variation (`σ²_P`) including
//!   the within-part components.
//!
//! Terms that cannot be separated from the residual (one observation per
//! level) or from an earlier term (the same grouping of the observations)
//! are left out and listed in [`ExpandedGaugeStudy::warnings`].

use stat_core::{Matrix, Result, StatError};
use stat_mixed::{MixedFamily, MixedFit, MixedModel, MixedOptions, RandomTerm, fit_mixed, satterthwaite_limits};

use crate::gauge::{
    GaugeOptions, VarianceComponent, distinct_categories, invalid, summarize_component, validate_options,
};

/// One factor of an expanded study.
#[derive(Debug, Clone, PartialEq)]
pub struct GaugeFactor {
    /// Name, e.g. `"Operator"` or `"Gauge"`.
    pub name: String,
    /// Level labels.
    pub levels: Vec<String>,
    /// Level of each measurement (an index into `levels`).
    pub codes: Vec<usize>,
    /// Random (levels sampled from a population, contributing a variance
    /// component) or fixed (the levels of interest, contributing mean shifts).
    pub random: bool,
    /// Indices (into the factor list) of the factors this one is nested in.
    /// Empty when it is crossed with all others.
    pub nested_in: Vec<usize>,
}

/// Options for [`gauge_rr_expanded`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExpandedGaugeOptions {
    /// Study multiplier and tolerance (the pooling option does not apply).
    pub gauge: GaugeOptions,
    /// Confidence level of the variance intervals. Default 0.95.
    pub confidence_level: f64,
    /// Include two-factor interactions of factors not nested in one another.
    /// Default `true`.
    pub interactions: bool,
}

impl Default for ExpandedGaugeOptions {
    fn default() -> Self {
        Self { gauge: GaugeOptions::default(), confidence_level: 0.95, interactions: true }
    }
}

/// What a variance component counts towards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentRole {
    /// Part-to-part variation (the part's main term).
    Part,
    /// Within-part variation: the main term of a random factor nested in
    /// the part (e.g. `Location(Part)`). Product variation, counted with
    /// part-to-part variation.
    WithinPart,
    /// Reproducibility: every other random term.
    Reproducibility,
    /// Repeatability: the residual.
    Repeatability,
}

/// One estimated variance component.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedComponent {
    /// Source label, e.g. `"Operator"`, `"Part × Gauge"`, `"Repeatability"`.
    pub source: String,
    /// What the component counts towards.
    pub role: ComponentRole,
    /// Number of levels of the term (observations for the residual).
    pub levels: usize,
    /// Large-sample standard error of the variance, when available.
    pub std_error: Option<f64>,
    /// Satterthwaite confidence limits of the variance, when available.
    pub interval: Option<(f64, f64)>,
    /// The variance with its summary measures.
    pub summary: VarianceComponent,
}

/// A total built from several components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExpandedTotal {
    /// The summed variance with its summary measures.
    pub summary: VarianceComponent,
    /// Large-sample standard error of the sum, when available.
    pub std_error: Option<f64>,
    /// Satterthwaite confidence limits of the sum, when available.
    pub interval: Option<(f64, f64)>,
}

/// F test of one fixed term.
#[derive(Debug, Clone, PartialEq)]
pub struct FixedTermTest {
    /// Term label.
    pub source: String,
    /// Numerator degrees of freedom (estimable coefficients of the term).
    pub num_df: usize,
    /// Satterthwaite denominator degrees of freedom.
    pub den_df: Option<f64>,
    /// F statistic.
    pub f_statistic: f64,
    /// p-value.
    pub p_value: f64,
}

/// Result of [`gauge_rr_expanded`].
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedGaugeStudy {
    /// Measurements used.
    pub n_obs: usize,
    /// Random components in term order, then repeatability.
    pub components: Vec<ExpandedComponent>,
    /// Repeatability + reproducibility.
    pub gauge_rr: ExpandedTotal,
    /// The residual variance.
    pub repeatability: ExpandedTotal,
    /// Sum of the reproducibility components.
    pub reproducibility: ExpandedTotal,
    /// Part-to-part (product) variation: the part's variance plus the
    /// within-part components.
    pub part: ExpandedTotal,
    /// Sum of the within-part components (zero, with no standard error, when
    /// no random factor is nested in the part).
    pub within_part: ExpandedTotal,
    /// Gauge R&R + part.
    pub total: ExpandedTotal,
    /// Tests of the fixed terms.
    pub fixed_tests: Vec<FixedTermTest>,
    /// Number of distinct categories `⌊√2 σ̂_P / σ̂_gauge⌋`.
    pub distinct_categories: Option<u64>,
    /// REML criterion (−2 restricted log likelihood).
    pub minus_two_log_likelihood: f64,
    /// The optimizer converged.
    pub converged: bool,
    /// Left-out terms, singular fits and other notes.
    pub warnings: Vec<String>,
}

/// A model term: the factors in it (with their ancestors) and its label.
struct Term {
    label: String,
    factors: Vec<usize>,
    members: Vec<usize>,
    random: bool,
}

/// Fit an expanded gauge study by REML.
///
/// `measurements` holds one value per observation; each factor's `codes`
/// give the observation's levels; `part` is the index of the part factor,
/// which must be random.
///
/// # Errors
/// * [`StatError::InvalidArgument`] for no factors, a part index out of
///   range or a fixed part factor, codes of the wrong length or out of range,
///   bad nesting (self-nesting, cycles, unknown factors, a fixed factor in a
///   random one), a factor with fewer than two observed levels, or invalid
///   options; and from the mixed-model fit (for example too few
///   observations).
/// * [`StatError::NonFiniteInput`] for NaN or infinite measurements.
pub fn gauge_rr_expanded(
    measurements: &[f64],
    factors: &[GaugeFactor],
    part: usize,
    options: &ExpandedGaugeOptions,
) -> Result<ExpandedGaugeStudy> {
    validate_options(&options.gauge)?;
    if !(options.confidence_level > 0.0 && options.confidence_level < 1.0) {
        return Err(invalid("the confidence level must lie in (0, 1)"));
    }
    let n = measurements.len();
    if measurements.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "gauge study measurements" });
    }
    if factors.is_empty() || part >= factors.len() {
        return Err(invalid("an expanded gauge study needs a part factor"));
    }
    if !factors[part].random {
        return Err(invalid("the part factor must be random"));
    }
    for f in factors {
        if f.codes.len() != n {
            return Err(StatError::DimensionMismatch {
                context: "factor codes vs. measurements",
                expected: n,
                found: f.codes.len(),
            });
        }
        if f.codes.iter().any(|&c| c >= f.levels.len()) {
            return Err(invalid(format!("level codes of {} are out of range", f.name)));
        }
        let mut seen = vec![false; f.levels.len()];
        f.codes.iter().for_each(|&c| seen[c] = true);
        if seen.iter().filter(|s| **s).count() < 2 {
            return Err(invalid(format!("{} needs at least two levels", f.name)));
        }
    }
    let ancestors = ancestors(factors)?;
    for (i, f) in factors.iter().enumerate() {
        if !f.random && ancestors[i].iter().any(|&a| factors[a].random) {
            return Err(invalid(format!("the fixed factor {} cannot be nested in a random factor", f.name)));
        }
    }

    let main_label = |i: usize| {
        if ancestors[i].is_empty() {
            factors[i].name.clone()
        } else {
            let parents: Vec<&str> = ancestors[i].iter().map(|&a| factors[a].name.as_str()).collect();
            format!("{}({})", factors[i].name, parents.join(", "))
        }
    };
    let members_of = |fs: &[usize]| {
        let mut m: Vec<usize> = fs.iter().flat_map(|&f| ancestors[f].iter().copied().chain([f])).collect();
        m.sort_unstable();
        m.dedup();
        m
    };
    let mut terms: Vec<Term> = (0..factors.len())
        .map(|i| Term { label: main_label(i), factors: vec![i], members: members_of(&[i]), random: factors[i].random })
        .collect();
    if options.interactions {
        for a in 0..factors.len() {
            for b in a + 1..factors.len() {
                if ancestors[a].contains(&b) || ancestors[b].contains(&a) {
                    continue;
                }
                terms.push(Term {
                    label: format!("{} × {}", main_label(a), main_label(b)),
                    factors: vec![a, b],
                    members: members_of(&[a, b]),
                    random: factors[a].random || factors[b].random,
                });
            }
        }
    }

    let mut warnings = Vec::new();
    // Random terms: groupings of the observations by the members' levels.
    let mut random: Vec<RandomTerm> = Vec::new();
    let mut random_terms: Vec<usize> = Vec::new();
    for (t, term) in terms.iter().enumerate().filter(|(_, t)| t.random) {
        let (codes, levels) = grouping(factors, &term.members, n);
        if levels.len() == n {
            warnings.push(format!(
                "{} is left out: every measurement is its own level, so it cannot be separated from repeatability.",
                term.label
            ));
            continue;
        }
        if let Some(k) = random.iter().position(|r| r.codes == codes) {
            warnings.push(format!(
                "{} is left out: it groups the measurements exactly as {} does.",
                term.label, random[k].group
            ));
            continue;
        }
        random.push(RandomTerm {
            group: term.label.clone(),
            levels,
            codes,
            effects: vec!["Intercept".to_owned()],
            z: Matrix::from_col_major(n, 1, vec![1.0; n])?,
            correlated: false,
        });
        random_terms.push(t);
    }

    // Fixed effects: intercept plus effect-coded fixed terms, aliased columns dropped.
    let mut columns: Vec<Vec<f64>> = vec![vec![1.0; n]];
    let mut names = vec!["Intercept".to_owned()];
    let mut basis: Vec<Vec<f64>> = Vec::new();
    keep_if_independent(&mut basis, &columns[0]);
    let mut fixed_terms: Vec<(String, Vec<usize>)> = Vec::new();
    for term in terms.iter().filter(|t| !t.random) {
        let candidates = match term.factors.as_slice() {
            [f] => main_columns(factors, *f, &ancestors[*f], n),
            [a, b] => {
                let (ca, cb) =
                    (main_columns(factors, *a, &ancestors[*a], n), main_columns(factors, *b, &ancestors[*b], n));
                ca.iter().flat_map(|x| cb.iter().map(move |y| x.iter().zip(y).map(|(u, v)| u * v).collect())).collect()
            }
            _ => Vec::new(),
        };
        let mut kept = Vec::new();
        let n_candidates = candidates.len();
        for (k, c) in candidates.into_iter().enumerate() {
            if keep_if_independent(&mut basis, &c) {
                kept.push(columns.len());
                columns.push(c);
                names.push(format!("{} [{}]", term.label, k + 1));
            }
        }
        if kept.is_empty() {
            warnings.push(format!("{} is left out: its effects are confounded with earlier terms.", term.label));
        } else {
            if kept.len() < n_candidates {
                warnings.push(format!(
                    "{}: only {} of its {n_candidates} effect columns can be estimated (missing cells, or \
                     effects confounded with earlier terms), so its F test covers only the {} kept.",
                    term.label,
                    kept.len(),
                    if kept.len() == 1 { "column".to_owned() } else { format!("{} columns", kept.len()) }
                ));
            }
            fixed_terms.push((term.label.clone(), kept));
        }
    }

    if random.is_empty() {
        return Err(invalid("no random term can be estimated from these data"));
    }
    let x = Matrix::from_columns(&columns)?;
    let model = MixedModel {
        y: measurements.to_vec(),
        x,
        x_names: names,
        weights: Vec::new(),
        offset: Vec::new(),
        random,
        family: MixedFamily::Normal,
        residual_groups: None,
        categories: Vec::new(),
    };
    let fit = fit_mixed(&model, &MixedOptions { confidence_level: options.confidence_level, ..Default::default() })?;
    warnings.extend(fit.warnings.iter().cloned());

    let p = columns.len();
    let mut fixed_tests = Vec::with_capacity(fixed_terms.len());
    for (label, cols) in &fixed_terms {
        let l: Vec<Vec<f64>> = cols
            .iter()
            .map(|&c| {
                let mut r = vec![0.0; p];
                r[c] = 1.0;
                r
            })
            .collect();
        let t = fit.test_contrast(&l)?;
        fixed_tests.push(FixedTermTest {
            source: label.clone(),
            num_df: t.num_df,
            den_df: t.den_df,
            f_statistic: t.statistic,
            p_value: t.p_value,
        });
    }

    summarize(&fit, &model, &terms, &random_terms, part, options, fixed_tests, warnings)
}

/// A component before summary: variance, standard error, interval, role, label, levels.
type RawComponent = (f64, Option<f64>, Option<(f64, f64)>, ComponentRole, String, usize);

/// Components, totals and summary measures from a fit.
#[allow(clippy::too_many_arguments)]
fn summarize(
    fit: &MixedFit,
    model: &MixedModel,
    terms: &[Term],
    random_terms: &[usize],
    part: usize,
    options: &ExpandedGaugeOptions,
    fixed_tests: Vec<FixedTermTest>,
    warnings: Vec<String>,
) -> Result<ExpandedGaugeStudy> {
    let residual = fit.residual.as_ref().ok_or_else(|| invalid("the fit has no residual variance"))?;
    // (variance, se, interval, role, label, levels) — random terms, then the residual.
    let mut raw: Vec<RawComponent> = Vec::new();
    for (k, &t) in random_terms.iter().enumerate() {
        let vc = &fit.variance_components[k];
        let term = &terms[t];
        let role = if term.factors == [part] {
            ComponentRole::Part
        } else if term.factors.len() == 1 && term.members.contains(&part) {
            // A factor nested (directly or not) in the part.
            ComponentRole::WithinPart
        } else {
            ComponentRole::Reproducibility
        };
        raw.push((vc.variance, vc.std_error, vc.interval, role, terms[t].label.clone(), model.random[k].levels.len()));
    }
    raw.push((
        residual.variance,
        residual.std_error,
        residual.interval,
        ComponentRole::Repeatability,
        "Repeatability".to_owned(),
        fit.n_obs,
    ));
    let sum_of = |roles: &[ComponentRole]| -> (f64, Option<f64>) {
        let idx: Vec<usize> = (0..raw.len()).filter(|&i| roles.contains(&raw[i].3)).collect();
        let v: f64 = idx.iter().map(|&i| raw[i].0).sum();
        let se = fit.variance_covariance.as_ref().map(|c| {
            let mut s = 0.0;
            for &i in &idx {
                for &j in &idx {
                    s += c[(i, j)];
                }
            }
            s.max(0.0).sqrt()
        });
        (v, if idx.is_empty() { None } else { se })
    };
    use ComponentRole::{Part, Repeatability, Reproducibility, WithinPart};
    let parts = [
        sum_of(&[Repeatability, Reproducibility]),
        sum_of(&[Repeatability]),
        sum_of(&[Reproducibility]),
        sum_of(&[Part, WithinPart]),
        sum_of(&[Repeatability, Reproducibility, Part, WithinPart]),
        sum_of(&[WithinPart]),
    ];
    let var_total = parts[4].0;
    let total_of = |(v, se): (f64, Option<f64>)| ExpandedTotal {
        summary: summarize_component(v, var_total, &options.gauge),
        std_error: se,
        interval: se.and_then(|s| satterthwaite_limits(v, s, options.confidence_level)),
    };
    let components = raw
        .iter()
        .map(|(v, se, interval, role, label, levels)| ExpandedComponent {
            source: label.clone(),
            role: *role,
            levels: *levels,
            std_error: *se,
            interval: *interval,
            summary: summarize_component(*v, var_total, &options.gauge),
        })
        .collect();
    Ok(ExpandedGaugeStudy {
        n_obs: fit.n_obs,
        components,
        gauge_rr: total_of(parts[0]),
        repeatability: total_of(parts[1]),
        reproducibility: total_of(parts[2]),
        part: total_of(parts[3]),
        within_part: total_of(parts[5]),
        total: total_of(parts[4]),
        fixed_tests,
        distinct_categories: distinct_categories(parts[3].0, parts[0].0),
        minus_two_log_likelihood: fit.minus_two_log_likelihood,
        converged: fit.converged,
        warnings,
    })
}

/// All factors each factor is nested in, directly or through its parents,
/// in factor order.
fn ancestors(factors: &[GaugeFactor]) -> Result<Vec<Vec<usize>>> {
    let k = factors.len();
    let mut out = Vec::with_capacity(k);
    for i in 0..k {
        let mut seen = vec![false; k];
        let mut stack: Vec<usize> = factors[i].nested_in.clone();
        while let Some(a) = stack.pop() {
            if a >= k {
                return Err(invalid(format!("{} is nested in an unknown factor", factors[i].name)));
            }
            if a == i {
                return Err(invalid(format!("{} cannot be nested in itself", factors[i].name)));
            }
            if !seen[a] {
                seen[a] = true;
                stack.extend(factors[a].nested_in.iter().copied());
            }
        }
        out.push((0..k).filter(|&a| seen[a]).collect());
    }
    Ok(out)
}

/// Codes (by first appearance) and labels of the combinations of `members`' levels.
fn grouping(factors: &[GaugeFactor], members: &[usize], n: usize) -> (Vec<usize>, Vec<String>) {
    let mut keys: Vec<Vec<usize>> = Vec::new();
    let mut codes = Vec::with_capacity(n);
    for r in 0..n {
        let key: Vec<usize> = members.iter().map(|&m| factors[m].codes[r]).collect();
        let c = keys.iter().position(|k| *k == key).unwrap_or_else(|| {
            keys.push(key);
            keys.len() - 1
        });
        codes.push(c);
    }
    let labels = keys
        .iter()
        .map(|k| members.iter().zip(k).map(|(&m, &c)| factors[m].levels[c].as_str()).collect::<Vec<_>>().join(", "))
        .collect();
    (codes, labels)
}

/// Effect-coded columns of factor `f`'s main term: within each cell of its
/// ancestors, `1[level k] − 1[last level]` for every level but the last.
fn main_columns(factors: &[GaugeFactor], f: usize, ancestors: &[usize], n: usize) -> Vec<Vec<f64>> {
    let (cells, _) = grouping(factors, ancestors, n);
    let n_cells = cells.iter().max().map_or(0, |m| m + 1);
    let codes = &factors[f].codes;
    let mut out = Vec::new();
    for cell in 0..n_cells {
        let mut present: Vec<usize> = (0..n).filter(|&r| cells[r] == cell).map(|r| codes[r]).collect();
        present.sort_unstable();
        present.dedup();
        let Some((&last, rest)) = present.split_last() else { continue };
        for &level in rest {
            out.push(
                (0..n)
                    .map(|r| match (cells[r] == cell, codes[r]) {
                        (true, c) if c == level => 1.0,
                        (true, c) if c == last => -1.0,
                        _ => 0.0,
                    })
                    .collect(),
            );
        }
    }
    out
}

/// Modified Gram–Schmidt: add `c` to the orthonormal `basis` and return
/// `true` unless its residual is below 10⁻⁹ of its norm.
fn keep_if_independent(basis: &mut Vec<Vec<f64>>, c: &[f64]) -> bool {
    let norm = c.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm == 0.0 {
        return false;
    }
    let mut r = c.to_vec();
    for q in basis.iter() {
        let d: f64 = q.iter().zip(&r).map(|(a, b)| a * b).sum();
        r.iter_mut().zip(q).for_each(|(x, qv)| *x -= d * qv);
    }
    let rn = r.iter().map(|v| v * v).sum::<f64>().sqrt();
    if rn <= 1e-9 * norm {
        return false;
    }
    r.iter_mut().for_each(|x| *x /= rn);
    basis.push(r);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn factor(name: &str, codes: Vec<usize>, random: bool, nested_in: Vec<usize>) -> GaugeFactor {
        let k = codes.iter().max().map_or(0, |m| m + 1);
        GaugeFactor {
            name: name.to_owned(),
            levels: (1..=k).map(|i| i.to_string()).collect(),
            codes,
            random,
            nested_in,
        }
    }

    #[test]
    fn labels_and_left_out_terms() {
        // 3 operators × 2 parts each (nested), 2 repeats: P×O is not generated,
        // and the part term is labelled with its parent.
        let ops: Vec<usize> = (0..12).map(|i| i / 4).collect();
        let parts: Vec<usize> = (0..12).map(|i| (i / 2) % 2).collect();
        let y: Vec<f64> = (0..12).map(|i| (i as f64 * 0.37).sin() * 3.0 + (i / 2) as f64).collect();
        let fs = vec![factor("Part", parts, true, vec![1]), factor("Operator", ops, true, vec![])];
        let s = gauge_rr_expanded(&y, &fs, 0, &ExpandedGaugeOptions::default()).unwrap();
        let labels: Vec<&str> = s.components.iter().map(|c| c.source.as_str()).collect();
        assert_eq!(labels, ["Part(Operator)", "Operator", "Repeatability"]);
        assert_eq!(s.components[0].levels, 6);
        assert_eq!(s.components[0].role, ComponentRole::Part);
        assert!(s.fixed_tests.is_empty());
    }

    #[test]
    fn fixed_factor_is_tested_not_counted() {
        // 4 parts × 2 operators × 2 gauges (fixed), 1 reading each: the
        // three-way cell is unique, so P×O×G is the residual.
        let mut y = Vec::new();
        let (mut p, mut o, mut g) = (Vec::new(), Vec::new(), Vec::new());
        for part in 0..4 {
            for op in 0..2 {
                for gauge in 0..2 {
                    p.push(part);
                    o.push(op);
                    g.push(gauge);
                    let noise = ((part * 7 + op * 3 + gauge * 5) as f64).sin() * 0.2;
                    y.push(10.0 + part as f64 + 0.3 * op as f64 + 0.8 * gauge as f64 + noise);
                }
            }
        }
        let fs = vec![
            factor("Part", p, true, vec![]),
            factor("Operator", o, true, vec![]),
            factor("Gauge", g, false, vec![]),
        ];
        let s = gauge_rr_expanded(&y, &fs, 0, &ExpandedGaugeOptions::default()).unwrap();
        assert_eq!(s.fixed_tests.len(), 1);
        assert_eq!(s.fixed_tests[0].source, "Gauge");
        assert_eq!(s.fixed_tests[0].num_df, 1);
        // Balanced: the gauge effect is tested against Operator × Gauge, on (2 − 1)(2 − 1) = 1 df.
        assert!((s.fixed_tests[0].den_df.unwrap() - 1.0).abs() < 0.01, "{:?}", s.fixed_tests);
        assert!(s.fixed_tests[0].f_statistic > 10.0, "{:?}", s.fixed_tests);
        assert!(s.components.iter().all(|c| c.source != "Gauge"));
        let sum: f64 = s.components.iter().map(|c| c.summary.variance).sum();
        assert!((sum - s.total.summary.variance).abs() < 1e-9);
        assert!((s.gauge_rr.summary.variance + s.part.summary.variance - s.total.summary.variance).abs() < 1e-12);
    }

    #[test]
    fn fixed_term_with_a_missing_cell_says_its_test_is_partial() {
        // Parts × operators (random) × a fixed three-level gauge, with every
        // reading of operator 2 on gauge 3 missing: Operator × Gauge keeps
        // its columns only through the other cells. Gauge × Operator is
        // random (operator random), so make a second fixed factor instead.
        let (mut y, mut p, mut g, mut h) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for part in 0..4 {
            for gauge in 0..3 {
                for head in 0..2 {
                    if gauge == 2 && head == 1 {
                        continue; // the missing cell
                    }
                    for rep in 0..2 {
                        p.push(part);
                        g.push(gauge);
                        h.push(head);
                        let noise = ((part * 7 + gauge * 3 + head * 5 + rep * 11) as f64).sin() * 0.3;
                        y.push(10.0 + part as f64 + 0.5 * gauge as f64 + 0.2 * head as f64 + noise);
                    }
                }
            }
        }
        let fs =
            vec![factor("Part", p, true, vec![]), factor("Gauge", g, false, vec![]), factor("Head", h, false, vec![])];
        let s = gauge_rr_expanded(&y, &fs, 0, &ExpandedGaugeOptions::default()).unwrap();
        let gh = s.fixed_tests.iter().find(|t| t.source == "Gauge × Head").unwrap();
        // (3 − 1)(2 − 1) = 2 interaction columns, one estimable.
        assert_eq!(gh.num_df, 1);
        assert!(
            s.warnings.iter().any(|w| w.starts_with("Gauge × Head: only 1 of its 2 effect columns")),
            "{:?}",
            s.warnings
        );
        assert!(!s.warnings.iter().any(|w| w.starts_with("Gauge:") || w.starts_with("Head:")));
    }

    #[test]
    fn within_part_variation_counts_as_product_variation() {
        // 4 parts × 3 locations on each part × 2 operators, 2 readings.
        let (mut y, mut p, mut l, mut o) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for part in 0..4 {
            for loc in 0..3 {
                for op in 0..2 {
                    for rep in 0..2 {
                        p.push(part);
                        l.push(loc);
                        o.push(op);
                        let noise = ((part * 13 + loc * 7 + op * 3 + rep * 5) as f64).sin() * 0.2;
                        y.push(
                            10.0 + 2.0 * part as f64
                                + [0.0, 0.9, -0.6, 0.4][part] * loc as f64
                                + 0.3 * op as f64
                                + noise,
                        );
                    }
                }
            }
        }
        let fs = vec![
            factor("Part", p, true, vec![]),
            factor("Operator", o, true, vec![]),
            factor("Location", l, true, vec![0]),
        ];
        let s = gauge_rr_expanded(&y, &fs, 0, &ExpandedGaugeOptions::default()).unwrap();
        let role = |name: &str| s.components.iter().find(|c| c.source == name).unwrap().role;
        assert_eq!(role("Location(Part)"), ComponentRole::WithinPart);
        assert_eq!(role("Operator × Location(Part)"), ComponentRole::Reproducibility);
        let var =
            |r: ComponentRole| -> f64 { s.components.iter().filter(|c| c.role == r).map(|c| c.summary.variance).sum() };
        let within = var(ComponentRole::WithinPart);
        assert!(within > 0.0);
        assert!((s.within_part.summary.variance - within).abs() < 1e-12);
        assert!((s.part.summary.variance - var(ComponentRole::Part) - within).abs() < 1e-12);
        let grr = var(ComponentRole::Repeatability) + var(ComponentRole::Reproducibility);
        assert!((s.gauge_rr.summary.variance - grr).abs() < 1e-12);
        assert!((s.gauge_rr.summary.variance + s.part.summary.variance - s.total.summary.variance).abs() < 1e-12);
    }

    #[test]
    fn validation() {
        let y = vec![1.0, 2.0, 3.0, 4.0];
        let a = factor("Part", vec![0, 0, 1, 1], true, vec![]);
        let fixed_part = GaugeFactor { random: false, ..a.clone() };
        let o = ExpandedGaugeOptions::default();
        assert!(gauge_rr_expanded(&y, &[fixed_part], 0, &o).is_err());
        assert!(gauge_rr_expanded(&y, std::slice::from_ref(&a), 1, &o).is_err());
        let cyc = vec![factor("A", vec![0, 0, 1, 1], true, vec![1]), factor("B", vec![0, 1, 0, 1], true, vec![0])];
        assert!(gauge_rr_expanded(&y, &cyc, 0, &o).unwrap_err().to_string().contains("itself"));
        let bad_nest = vec![a.clone(), factor("G", vec![0, 1, 0, 1], false, vec![0])];
        assert!(gauge_rr_expanded(&y, &bad_nest, 0, &o).unwrap_err().to_string().contains("fixed factor"));
        let one_level = vec![a.clone(), factor("G", vec![0, 0, 0, 0], true, vec![])];
        assert!(gauge_rr_expanded(&y, &one_level, 0, &o).is_err());
        assert!(gauge_rr_expanded(&[1.0, f64::NAN, 3.0, 4.0], &[a], 0, &o).is_err());
    }

    #[test]
    fn gram_schmidt_drops_dependent_columns() {
        let mut b = Vec::new();
        assert!(keep_if_independent(&mut b, &[1.0, 1.0, 1.0]));
        assert!(keep_if_independent(&mut b, &[1.0, -1.0, 0.0]));
        assert!(!keep_if_independent(&mut b, &[2.0, 0.0, 1.0]));
        assert!(!keep_if_independent(&mut b, &[0.0, 0.0, 0.0]));
    }
}
