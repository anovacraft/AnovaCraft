//! Model hierarchy (effect heredity): an interaction or power should come
//! with the lower-order effects it is built from — `A*B` with `A` and `B`,
//! `x^2` with `x`, `A*x^2` with `A*x`, `x^2`, `A` and `x`.
//!
//! Without them the higher-order effect's estimate and test change with the
//! origin of continuous predictors and the coding of categorical ones (Peixoto,
//! "Hierarchical variable selection in polynomial regression models", *The
//! American Statistician* 41(4), 1987; Nelder, "A reformulation of linear
//! models", *JRSS A* 140, 1977). In designed experiments the same idea appears
//! as **effect heredity**: an interaction is rarely active unless its parent
//! factors are (Hamada & Wu, *J. Quality Technology* 24, 1992; Chipman,
//! *Canadian J. Statistics* 24, 1996). *Strong* heredity asks for every parent,
//! *weak* heredity for at least one.
//!
//! An effect is a product of predictors raised to powers, `[(predictor,
//! power), …]`; its **ancestors** are its proper divisors (every effect whose
//! factors it contains to at least the same powers) and its **parents** the
//! ancestors one step lower: drop one first-power factor or lower one power by
//! one.

/// An effect: `(predictor, power)` factors.
pub type Effect = Vec<(usize, u8)>;

/// Factors in predictor order, merging repeats.
fn normal(e: &[(usize, u8)]) -> Effect {
    let mut out: Effect = Vec::new();
    for &(i, p) in e {
        match out.iter_mut().find(|(j, _)| *j == i) {
            Some(f) => f.1 = f.1.saturating_add(p),
            None => out.push((i, p)),
        }
    }
    out.retain(|f| f.1 > 0);
    out.sort_unstable();
    out
}

/// The parents of `effect` (empty for a main effect).
#[must_use]
pub fn parents(effect: &[(usize, u8)]) -> Vec<Effect> {
    let e = normal(effect);
    let mut out: Vec<Effect> = Vec::new();
    for k in 0..e.len() {
        let mut p = e.clone();
        if p[k].1 > 1 {
            p[k].1 -= 1;
        } else {
            p.remove(k);
        }
        if !p.is_empty() && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// `true` when `a` is an ancestor (proper divisor) of `d`.
#[must_use]
pub fn is_ancestor(a: &[(usize, u8)], d: &[(usize, u8)]) -> bool {
    let (a, d) = (normal(a), normal(d));
    !a.is_empty() && a != d && a.iter().all(|&(i, p)| d.iter().any(|&(j, q)| i == j && q >= p))
}

/// Every ancestor of `effect`, lowest order first.
#[must_use]
pub fn ancestors(effect: &[(usize, u8)]) -> Vec<Effect> {
    let mut out: Vec<Effect> = Vec::new();
    let mut frontier = parents(effect);
    while let Some(p) = frontier.pop() {
        if !out.contains(&p) {
            frontier.extend(parents(&p));
            out.push(p);
        }
    }
    out.sort_by_key(|e| (e.iter().map(|f| u32::from(f.1)).sum::<u32>(), e.clone()));
    out
}

/// `effects` with every missing ancestor added before the effects that need
/// it (existing effects keep their order).
#[must_use]
pub fn hierarchical_closure(effects: &[Effect]) -> Vec<Effect> {
    let mut out: Vec<Effect> = Vec::new();
    for e in effects {
        for a in ancestors(e) {
            if !out.contains(&a) && !effects.iter().any(|x| normal(x) == a) {
                out.push(a);
            }
        }
    }
    // Added ancestors first (lowest order first), then the given effects.
    out.sort_by_key(|e| (e.iter().map(|f| u32::from(f.1)).sum::<u32>(), e.clone()));
    let mut all = out;
    for e in effects {
        let e = normal(e);
        if !all.contains(&e) {
            all.push(e);
        }
    }
    all
}

/// Effects of `effects` whose parents are not all present, each with its
/// missing parents (empty when the model is hierarchical).
#[must_use]
pub fn hierarchy_violations(effects: &[Effect]) -> Vec<(Effect, Vec<Effect>)> {
    let present: Vec<Effect> = effects.iter().map(|e| normal(e)).collect();
    present
        .iter()
        .filter_map(|e| {
            let missing: Vec<Effect> = ancestors(e).into_iter().filter(|a| !present.contains(a)).collect();
            (!missing.is_empty()).then(|| (e.clone(), missing))
        })
        .collect()
}

/// For each effect, the indices (into `effects`) of its parents that are in
/// the list.
#[must_use]
pub fn parent_indices(effects: &[Effect]) -> Vec<Vec<usize>> {
    let present: Vec<Effect> = effects.iter().map(|e| normal(e)).collect();
    present.iter().map(|e| parents(e).iter().filter_map(|p| present.iter().position(|x| x == p)).collect()).collect()
}

/// Turn `effect` on (adding its ancestors when `keep_hierarchy`) or off
/// (removing its descendants when `keep_hierarchy`) in `effects`.
pub fn set_effect(effects: &mut Vec<Effect>, effect: &[(usize, u8)], on: bool, keep_hierarchy: bool) {
    let e = normal(effect);
    if on {
        if keep_hierarchy {
            for a in ancestors(&e) {
                if !effects.iter().any(|x| normal(x) == a) {
                    effects.push(a);
                }
            }
        }
        if !effects.iter().any(|x| normal(x) == e) {
            effects.push(e);
        }
    } else {
        effects.retain(|x| {
            let x = normal(x);
            x != e && !(keep_hierarchy && is_ancestor(&e, &x))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: usize = 0;
    const B: usize = 1;
    const X: usize = 2;

    #[test]
    fn parents_and_ancestors() {
        assert!(parents(&[(A, 1)]).is_empty());
        assert_eq!(parents(&[(A, 1), (B, 1)]), [vec![(B, 1)], vec![(A, 1)]]);
        assert_eq!(parents(&[(X, 3)]), [vec![(X, 2)]]);
        assert_eq!(parents(&[(X, 2), (A, 1)]), [vec![(X, 2)], vec![(A, 1), (X, 1)]]);
        assert_eq!(ancestors(&[(A, 1), (X, 2)]), [vec![(A, 1)], vec![(X, 1)], vec![(A, 1), (X, 1)], vec![(X, 2)]]);
        assert!(is_ancestor(&[(X, 1)], &[(A, 1), (X, 2)]));
        assert!(!is_ancestor(&[(X, 2)], &[(X, 2)]));
        assert!(!is_ancestor(&[(B, 1)], &[(A, 1), (X, 2)]));
    }

    #[test]
    fn closure_violations_and_toggling() {
        let model = vec![vec![(A, 1), (B, 1)], vec![(X, 2)], vec![(A, 1)]];
        let v = hierarchy_violations(&model);
        assert_eq!(v, [(vec![(A, 1), (B, 1)], vec![vec![(B, 1)]]), (vec![(X, 2)], vec![vec![(X, 1)]])]);
        let closed = hierarchical_closure(&model);
        assert_eq!(closed, [vec![(B, 1)], vec![(X, 1)], vec![(A, 1), (B, 1)], vec![(X, 2)], vec![(A, 1)]]);
        assert!(hierarchy_violations(&closed).is_empty());
        assert_eq!(parent_indices(&closed), [vec![], vec![], vec![0, 4], vec![1], vec![]]);
        // Toggling.
        let mut m: Vec<Effect> = Vec::new();
        set_effect(&mut m, &[(A, 1), (X, 2)], true, true);
        assert_eq!(m.len(), 5);
        assert!(hierarchy_violations(&m).is_empty());
        set_effect(&mut m, &[(X, 1)], false, true);
        assert_eq!(m, [vec![(A, 1)]], "x and everything built on it leave");
        set_effect(&mut m, &[(B, 1), (A, 1)], true, false);
        assert_eq!(hierarchy_violations(&m).len(), 1, "allowed when not keeping hierarchy");
        set_effect(&mut m, &[(A, 1)], false, false);
        assert_eq!(m, [vec![(A, 1), (B, 1)]]);
    }
}
