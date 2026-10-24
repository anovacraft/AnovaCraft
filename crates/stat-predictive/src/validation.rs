//! Holding rows back to judge a predictive model honestly.
//!
//! Each row gets a [`Role`]: **training** rows fit the model, **validation**
//! rows tune it (choose a tree's size, a penalty, a number of iterations),
//! and **test** rows are kept out of both so that their error estimates how
//! the chosen model does on new data (Hastie, Tibshirani & Friedman, *The
//! Elements of Statistical Learning*, 2nd ed., §7.2). Roles can be drawn at
//! random ([`random_roles`]), within strata of the response so that every
//! class is represented in each role ([`stratified_roles`]), or by whole
//! groups so that related rows (one subject's measurements) never straddle
//! roles ([`grouped_roles`]). K-fold cross-validation ([`fold_assignment`])
//! is the alternative when data are too few to hold rows back.
//!
//! Every assignment is seeded and reproducible: the same seed and inputs
//! always give the same roles.

use stat_core::rng::Rng;
use stat_core::{Result, StatError};

/// What a row is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    /// Fits the model.
    Training,
    /// Tunes the model (chooses its size or penalty).
    Validation,
    /// Kept out of fitting and tuning; estimates performance on new data.
    Test,
}

impl Role {
    /// The three roles in report order.
    pub const ALL: [Self; 3] = [Self::Training, Self::Validation, Self::Test];

    /// Label in reports and saved columns.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Training => "Training",
            Self::Validation => "Validation",
            Self::Test => "Test",
        }
    }

    /// The role a cell of a validation column names: `0`, `1`, `2`, or text
    /// starting `train`, `valid` or `test` (any case). `None` otherwise.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim().to_lowercase();
        match t.as_str() {
            "0" => Some(Self::Training),
            "1" => Some(Self::Validation),
            "2" => Some(Self::Test),
            _ if t.starts_with("train") => Some(Self::Training),
            _ if t.starts_with("valid") => Some(Self::Validation),
            _ if t.starts_with("test") => Some(Self::Test),
            _ => None,
        }
    }
}

fn check_shares(validation: f64, test: f64) -> Result<()> {
    if !(validation >= 0.0 && test >= 0.0 && validation + test < 1.0) {
        return Err(StatError::InvalidArgument {
            message: "the validation and test shares must be zero or more and leave some rows for training".into(),
        });
    }
    Ok(())
}

/// Roles for `items` things in random order: the first
/// `round(validation · items)` become validation, the next
/// `round(test · items)` test, the rest training.
fn split_shuffled(order: &[usize], validation: f64, test: f64, out: &mut [Role]) {
    let n = order.len() as f64;
    let n_valid = (validation * n).round() as usize;
    let n_test = ((test * n).round() as usize).min(order.len() - n_valid.min(order.len()));
    for (k, &i) in order.iter().enumerate() {
        out[i] = if k < n_valid {
            Role::Validation
        } else if k < n_valid + n_test {
            Role::Test
        } else {
            Role::Training
        };
    }
}

/// Random roles for `n` rows: shares `validation` and `test` of the rows
/// (rounded), the rest training.
///
/// # Errors
/// [`StatError::InvalidArgument`] unless both shares are ≥ 0 and their sum is
/// below 1.
pub fn random_roles(n: usize, validation: f64, test: f64, seed: u64) -> Result<Vec<Role>> {
    check_shares(validation, test)?;
    let mut order: Vec<usize> = (0..n).collect();
    Rng::new(seed).shuffle(&mut order);
    let mut out = vec![Role::Training; n];
    split_shuffled(&order, validation, test, &mut out);
    Ok(out)
}

/// Random roles drawn separately within each stratum (`strata[i]` is row
/// `i`'s class), so each role holds about the same share of every class.
///
/// # Errors
/// As [`random_roles`].
pub fn stratified_roles(strata: &[usize], validation: f64, test: f64, seed: u64) -> Result<Vec<Role>> {
    check_shares(validation, test)?;
    let mut rng = Rng::new(seed);
    let mut out = vec![Role::Training; strata.len()];
    let k = strata.iter().copied().max().map_or(0, |m| m + 1);
    for s in 0..k {
        let mut order: Vec<usize> = (0..strata.len()).filter(|&i| strata[i] == s).collect();
        rng.shuffle(&mut order);
        split_shuffled(&order, validation, test, &mut out);
    }
    Ok(out)
}

/// Random roles given to whole groups (`groups[i]` is row `i`'s group): the
/// shares count groups, and every row of a group gets the group's role.
///
/// # Errors
/// As [`random_roles`].
pub fn grouped_roles(groups: &[usize], validation: f64, test: f64, seed: u64) -> Result<Vec<Role>> {
    check_shares(validation, test)?;
    let k = groups.iter().copied().max().map_or(0, |m| m + 1);
    let mut order: Vec<usize> = (0..k).collect();
    Rng::new(seed).shuffle(&mut order);
    let mut group_role = vec![Role::Training; k];
    split_shuffled(&order, validation, test, &mut group_role);
    Ok(groups.iter().map(|&g| group_role[g]).collect())
}

/// Fold of each of `n` rows for `folds`-fold cross-validation: a random
/// permutation dealt round-robin, so fold sizes differ by at most one.
///
/// # Errors
/// [`StatError::InvalidArgument`] unless `2 ≤ folds ≤ n`.
pub fn fold_assignment(n: usize, folds: usize, seed: u64) -> Result<Vec<usize>> {
    if folds < 2 || folds > n {
        return Err(StatError::InvalidArgument {
            message: format!("cross-validation needs between 2 and {n} folds (one per row at most)"),
        });
    }
    let mut order: Vec<usize> = (0..n).collect();
    Rng::new(seed).shuffle(&mut order);
    let mut out = vec![0; n];
    for (k, &i) in order.iter().enumerate() {
        out[i] = k % folds;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(roles: &[Role], r: Role) -> usize {
        roles.iter().filter(|&&x| x == r).count()
    }

    #[test]
    fn shares_strata_groups_and_folds() {
        let roles = random_roles(100, 0.2, 0.1, 5).unwrap();
        assert_eq!((count(&roles, Role::Validation), count(&roles, Role::Test)), (20, 10));
        assert_eq!(roles, random_roles(100, 0.2, 0.1, 5).unwrap(), "seeded");
        assert_ne!(roles, random_roles(100, 0.2, 0.1, 6).unwrap());
        assert!(random_roles(10, 0.6, 0.4, 1).is_err());

        // 30 of class 0 and 10 of class 1: a quarter of each validates.
        let strata: Vec<usize> = (0..40).map(|i| usize::from(i >= 30)).collect();
        let roles = stratified_roles(&strata, 0.25, 0.0, 3).unwrap();
        let valid_in = |c: usize| (0..40).filter(|&i| strata[i] == c && roles[i] == Role::Validation).count();
        assert_eq!((valid_in(0), valid_in(1)), (8, 3));

        // Groups of four rows stay together.
        let groups: Vec<usize> = (0..40).map(|i| i / 4).collect();
        let roles = grouped_roles(&groups, 0.3, 0.2, 9).unwrap();
        for g in 0..10 {
            let first = roles[g * 4];
            assert!((0..4).all(|k| roles[g * 4 + k] == first));
        }
        assert_eq!(count(&roles, Role::Validation), 12);

        let folds = fold_assignment(23, 5, 2).unwrap();
        let sizes: Vec<usize> = (0..5).map(|f| folds.iter().filter(|&&x| x == f).count()).collect();
        assert!(sizes.iter().all(|&s| s == 4 || s == 5));
        assert!(fold_assignment(3, 4, 1).is_err());

        assert_eq!(Role::parse(" Validation "), Some(Role::Validation));
        assert_eq!(Role::parse("2"), Some(Role::Test));
        assert_eq!(Role::parse("TRAIN"), Some(Role::Training));
        assert_eq!(Role::parse("holdout"), None);
    }
}
