//! Nested comparison of groups: subjects nested in groups, replicate
//! measurements on each subject (a two-level hierarchical design).
//!
//! The model is `y_gsr = μ + α_g + b_gs + e_gsr` with subject effects
//! `b_gs ~ N(0, σ²_s)` and residuals `e_gsr ~ N(0, σ²)`. It is fitted by REML
//! ([`fit_mixed`]); the test of equal group means is an F test with
//! Satterthwaite's denominator degrees of freedom, and group means and
//! pairwise differences get t tests and intervals on Satterthwaite degrees
//! of freedom (Kuznetsova, Brockhoff & Christensen 2017). Treating the
//! replicates as independent observations would understate every standard
//! error; the subjects, not the measurements, are the experimental units.
//!
//! With equal replicates per subject and equal subjects per group the
//! classical nested ANOVA (Sokal & Rohlf, *Biometry*, 4th ed., ch. 10) gives
//! the same group test, `F = MS_groups / MS_subjects(groups)` on
//! `(a − 1, S − a)` degrees of freedom whenever the REML estimate of `σ²_s`
//! is positive; it is reported alongside for such balanced data.

use stat_core::distributions::{ContinuousDistribution, FisherSnedecor, StudentsT};
use stat_core::{Matrix, Result, StatError};

use crate::family::MixedFamily;
use crate::fit::{ContrastTest, MixedFit, MixedModel, MixedOptions, RandomTerm, fit_mixed};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// An estimate with its standard error, Satterthwaite degrees of freedom,
/// t test against zero and confidence interval.
#[derive(Debug, Clone, PartialEq)]
pub struct NestedEstimate {
    /// Estimate.
    pub estimate: f64,
    /// Standard error.
    pub std_error: f64,
    /// Satterthwaite degrees of freedom.
    pub df: f64,
    /// t statistic (estimate / standard error).
    pub t: f64,
    /// Two-sided p-value.
    pub p_value: f64,
    /// Lower confidence limit.
    pub lower: f64,
    /// Upper confidence limit.
    pub upper: f64,
}

/// The classical nested ANOVA table (balanced data).
#[derive(Debug, Clone, PartialEq)]
pub struct NestedAnova {
    /// Groups sum of squares and degrees of freedom.
    pub ss_groups: f64,
    /// `a − 1`.
    pub df_groups: f64,
    /// Subjects-within-groups sum of squares.
    pub ss_subjects: f64,
    /// `S − a`.
    pub df_subjects: f64,
    /// Replicates (residual) sum of squares.
    pub ss_residual: f64,
    /// `N − S`.
    pub df_residual: f64,
    /// `MS_groups / MS_subjects`.
    pub f_groups: f64,
    /// p-value of `f_groups`.
    pub p_groups: f64,
    /// `MS_subjects / MS_residual` (do subjects differ within groups?).
    pub f_subjects: f64,
    /// p-value of `f_subjects`.
    pub p_subjects: f64,
}

/// Result of a nested comparison.
#[derive(Debug, Clone)]
pub struct NestedComparison {
    /// Group names.
    pub groups: Vec<String>,
    /// Subjects in each group.
    pub subjects_per_group: Vec<usize>,
    /// Measurements in each group.
    pub observations_per_group: Vec<usize>,
    /// Model-based mean of each group.
    pub means: Vec<NestedEstimate>,
    /// F test of equal group means (Satterthwaite denominator df).
    pub group_test: ContrastTest,
    /// Differences `first − second` for every pair `first < second`.
    pub differences: Vec<((usize, usize), NestedEstimate)>,
    /// REML estimate of the between-subject variance `σ²_s`.
    pub subject_variance: f64,
    /// REML estimate of the residual (replicate) variance `σ²`.
    pub residual_variance: f64,
    /// Intraclass correlation `σ²_s / (σ²_s + σ²)`: the share of variance
    /// between subjects.
    pub icc: f64,
    /// Every subject has the same number of replicates and every group the
    /// same number of subjects.
    pub balanced: bool,
    /// The classical nested ANOVA (always computed; exact only when
    /// `balanced`).
    pub anova: NestedAnova,
    /// The REML fit.
    pub fit: MixedFit,
}

/// Compare groups with subjects nested in groups. `groups[i]` and
/// `subjects[i]` are row `i`'s group and subject (subject codes identify a
/// subject across the whole data, so the same label in two groups must be
/// given two codes); `level` is the confidence level.
///
/// # Errors
/// Mismatched lengths, non-finite responses, fewer than two groups, a group
/// with fewer than two subjects, a subject in two groups, no replicates at
/// all, or a failed fit.
pub fn nested_comparison(
    y: &[f64],
    groups: &[usize],
    group_names: &[String],
    subjects: &[usize],
    subject_names: &[String],
    level: f64,
) -> Result<NestedComparison> {
    let n = y.len();
    if groups.len() != n || subjects.len() != n {
        return Err(invalid("one group and one subject per measurement"));
    }
    if y.iter().any(|v| !v.is_finite()) {
        return Err(invalid("measurements must be finite"));
    }
    let a = group_names.len();
    let s = subject_names.len();
    if a < 2 {
        return Err(invalid("a nested comparison needs at least two groups"));
    }
    if groups.iter().any(|&g| g >= a) || subjects.iter().any(|&j| j >= s) {
        return Err(invalid("group or subject code out of range"));
    }
    let mut subject_group = vec![usize::MAX; s];
    let mut reps = vec![0usize; s];
    for i in 0..n {
        let j = subjects[i];
        if subject_group[j] != usize::MAX && subject_group[j] != groups[i] {
            return Err(invalid(format!("subject {:?} appears in two groups", subject_names[j])));
        }
        subject_group[j] = groups[i];
        reps[j] += 1;
    }
    if subject_group.contains(&usize::MAX) {
        return Err(invalid("every subject needs at least one measurement"));
    }
    let subjects_per_group: Vec<usize> = (0..a).map(|g| subject_group.iter().filter(|&&x| x == g).count()).collect();
    if subjects_per_group.iter().any(|&c| c < 2) {
        return Err(invalid("every group needs at least two subjects"));
    }
    if n <= s {
        return Err(invalid("no replicate measurements: each subject has a single value"));
    }
    let observations_per_group: Vec<usize> = (0..a).map(|g| groups.iter().filter(|&&x| x == g).count()).collect();
    let balanced = reps.iter().all(|&r| r == reps[0]) && subjects_per_group.iter().all(|&c| c == subjects_per_group[0]);
    let anova = classical(y, groups, subjects, a, s, &subject_group);

    // REML fit: intercept plus treatment coding against the first group.
    let mut cols: Vec<Vec<f64>> = vec![vec![1.0; n]];
    for g in 1..a {
        cols.push(groups.iter().map(|&x| f64::from(u8::from(x == g))).collect());
    }
    let x = Matrix::from_columns(&cols)?;
    let mut x_names = vec!["Intercept".to_owned()];
    x_names.extend(group_names[1..].iter().cloned());
    let model = MixedModel {
        y: y.to_vec(),
        x,
        x_names,
        weights: Vec::new(),
        offset: Vec::new(),
        random: vec![RandomTerm {
            group: "Subject".to_owned(),
            levels: subject_names.to_vec(),
            codes: subjects.to_vec(),
            effects: vec!["Intercept".to_owned()],
            z: Matrix::from_columns(&[vec![1.0; n]])?,
            correlated: false,
        }],
        family: MixedFamily::Normal,
        residual_groups: None,
        categories: Vec::new(),
    };
    let fit = fit_mixed(&model, &MixedOptions { confidence_level: level, ..MixedOptions::default() })?;
    let row = |coef: &[(usize, f64)]| {
        let mut l = vec![0.0; a];
        for &(i, w) in coef {
            l[i] += w;
        }
        l
    };
    let estimate = |l: Vec<f64>| -> Result<NestedEstimate> {
        let beta: Vec<f64> = fit.fixed.iter().map(|f| f.estimate).collect();
        let est: f64 = l.iter().zip(&beta).map(|(w, b)| w * b).sum();
        let c = &fit.fixed_covariance;
        let var: f64 = (0..a).map(|i| (0..a).map(|j| l[i] * c.col(j)[i] * l[j]).sum::<f64>()).sum();
        let se = var.max(0.0).sqrt();
        let df = fit.test_contrast(std::slice::from_ref(&l))?.den_df.unwrap_or(f64::NAN);
        let t = est / se;
        let dist = StudentsT::new(df)?;
        let p = 2.0 * dist.sf(t.abs())?;
        let q = dist.quantile(0.5 + level / 2.0)?;
        Ok(NestedEstimate {
            estimate: est,
            std_error: se,
            df,
            t,
            p_value: p.min(1.0),
            lower: est - q * se,
            upper: est + q * se,
        })
    };
    // Group g's mean: intercept (+ its coefficient).
    let coefs = |g: usize| if g == 0 { vec![(0, 1.0)] } else { vec![(0, 1.0), (g, 1.0)] };
    let means = (0..a).map(|g| estimate(row(&coefs(g)))).collect::<Result<Vec<_>>>()?;
    let mut differences = Vec::new();
    for i in 0..a {
        for j in i + 1..a {
            let mut c = coefs(i);
            c.extend(coefs(j).into_iter().map(|(k, w)| (k, -w)));
            differences.push(((i, j), estimate(row(&c))?));
        }
    }
    let l: Vec<Vec<f64>> = (1..a).map(|g| row(&[(g, 1.0)])).collect();
    let group_test = fit.test_contrast(&l)?;
    let subject_variance = fit.variance_components.first().map_or(0.0, |v| v.variance);
    let residual_variance = fit.residual.as_ref().map_or(f64::NAN, |v| v.variance);
    Ok(NestedComparison {
        groups: group_names.to_vec(),
        subjects_per_group,
        observations_per_group,
        means,
        group_test,
        differences,
        subject_variance,
        residual_variance,
        icc: subject_variance / (subject_variance + residual_variance),
        balanced,
        anova,
        fit,
    })
}

fn classical(
    y: &[f64],
    groups: &[usize],
    subjects: &[usize],
    a: usize,
    s: usize,
    subject_group: &[usize],
) -> NestedAnova {
    let n = y.len();
    let mean = |sel: &dyn Fn(usize) -> bool| {
        let (sum, c) = (0..n).filter(|&i| sel(i)).fold((0.0, 0usize), |(t, c), i| (t + y[i], c + 1));
        (sum / c as f64, c)
    };
    let (grand, _) = mean(&|_| true);
    let group_means: Vec<(f64, usize)> = (0..a).map(|g| mean(&|i| groups[i] == g)).collect();
    let subject_means: Vec<(f64, usize)> = (0..s).map(|j| mean(&|i| subjects[i] == j)).collect();
    let ss_groups: f64 = group_means.iter().map(|&(m, c)| c as f64 * (m - grand).powi(2)).sum();
    let ss_subjects: f64 = (0..s)
        .map(|j| subject_means[j].1 as f64 * (subject_means[j].0 - group_means[subject_group[j]].0).powi(2))
        .sum();
    let ss_residual: f64 = (0..n).map(|i| (y[i] - subject_means[subjects[i]].0).powi(2)).sum();
    let (df_g, df_s, df_r) = ((a - 1) as f64, (s - a) as f64, (n - s) as f64);
    let f_groups = (ss_groups / df_g) / (ss_subjects / df_s);
    let f_subjects = (ss_subjects / df_s) / (ss_residual / df_r);
    let tail = |f: f64, d1: f64, d2: f64| FisherSnedecor::new(d1, d2).and_then(|d| d.sf(f)).unwrap_or(f64::NAN);
    NestedAnova {
        ss_groups,
        df_groups: df_g,
        ss_subjects,
        df_subjects: df_s,
        ss_residual,
        df_residual: df_r,
        f_groups,
        p_groups: tail(f_groups, df_g, df_s),
        f_subjects,
        p_subjects: tail(f_subjects, df_s, df_r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1e-300), "{a} vs {b}");
    }

    /// Simulated in R (`tests/data/nested_reference.R`, seed 17): three
    /// groups of four subjects, three replicates each.
    const Y: [f64; 36] = [
        9.8, 8.9, 10.1, 9.9, 10.6, 10.0, 10.2, 10.2, 10.3, 8.5, 8.4, 10.4, 11.9, 13.4, 12.5, 11.5, 11.6, 10.7, 11.7,
        11.9, 12.0, 10.7, 12.4, 13.2, 12.7, 12.6, 13.3, 12.9, 13.6, 13.2, 14.3, 15.4, 15.9, 13.8, 15.2, 13.6,
    ];

    /// Responses, group codes and names, subject codes and names.
    type Design = (Vec<f64>, Vec<usize>, Vec<String>, Vec<usize>, Vec<String>);

    fn design(keep: &[usize]) -> Design {
        let names: Vec<String> = ["Control", "Low", "High"].map(str::to_owned).to_vec();
        let subj: Vec<String> = (0..12).map(|j| format!("S{j}")).collect();
        let y = keep.iter().map(|&i| Y[i]).collect();
        let g = keep.iter().map(|&i| i / 12).collect();
        let s = keep.iter().map(|&i| i / 3).collect();
        (y, g, names, s, subj)
    }

    /// Balanced data: R `aov(y ~ group + Error(group:subject))` gives
    /// F = 29.95288 on (2, 9), p = 0.00010518, MS subjects 1.6860185185,
    /// residual SS 11.94 on 24; `lmerTest` the same F with Satterthwaite
    /// df 9, σ²_s = 0.396172882855604, σ² = 0.49749998704348, and group
    /// differences 2.18333 and 4.1 with SE 0.530097260825831.
    #[test]
    fn balanced_matches_aov_and_lmer_test() {
        let all: Vec<usize> = (0..36).collect();
        let (y, g, names, s, subj) = design(&all);
        let r = nested_comparison(&y, &g, &names, &s, &subj, 0.95).unwrap();
        assert!(r.balanced);
        close(r.anova.ss_groups, 101.002_222_222_222_2, 1e-12);
        close(r.anova.ss_subjects, 15.174_166_666_666_7, 1e-11);
        close(r.anova.ss_residual, 11.94, 1e-11);
        close(r.anova.f_groups, 29.952_88, 1e-6);
        close(r.anova.p_groups, 0.000_105_18, 1e-4);
        close(r.group_test.statistic, r.anova.f_groups, 1e-6);
        close(r.group_test.den_df.unwrap(), 9.0, 1e-5);
        close(r.subject_variance, 0.396_172_882_855_604, 1e-5);
        close(r.residual_variance, 0.497_499_987_043_48, 1e-6);
        let (pair, d) = &r.differences[0];
        assert_eq!(*pair, (0, 1));
        close(d.estimate, -2.183_333_333_333_34, 1e-9);
        close(d.std_error, 0.530_097_260_825_831, 1e-5);
        close(d.p_value, 2.602_827_589_660_18e-3, 1e-4);
        close(r.means[0].estimate, 9.775, 1e-10);
        close(r.means[0].std_error, 0.374_835_367_818_359, 1e-5);
    }

    /// Two groups: the nested t test. R `lmerTest`: difference 2.18333,
    /// SE 0.383876426406631, t 5.68759419214917 on 6 df,
    /// p = 0.00127450871089226.
    #[test]
    fn nested_t_test() {
        let keep: Vec<usize> = (0..24).collect();
        let (y, g, mut names, s, mut subj) = design(&keep);
        names.truncate(2);
        subj.truncate(8);
        let r = nested_comparison(&y, &g, &names, &s, &subj, 0.95).unwrap();
        let d = &r.differences[0].1;
        close(d.estimate, -2.183_333_333_333_33, 1e-9);
        close(d.std_error, 0.383_876_426_406_631, 1e-5);
        close(d.t, -5.687_594_192_149_17, 1e-5);
        close(d.df, 6.0, 1e-5);
        close(d.p_value, 1.274_508_710_892_26e-3, 1e-4);
        close(r.group_test.statistic, d.t * d.t, 1e-9);
    }

    /// Unbalanced (rows 2, 9, 10, 20 and 33 dropped): R `lmerTest`
    /// F = 33.2230787367312; for the hypothesis rows used here (each
    /// group against the first), `contestMD` gives Satterthwaite df
    /// 9.05382552930883 and p = 6.76384986453946e-5 (its Type III table
    /// combines differently scaled rows, df 9.0755). Emmeans group
    /// means 9.94155073059543, 11.95908532250862, 13.73732971041198 with df
    /// 10.30992295885647 and 8.50699966904339; Low − High −1.77824438790335,
    /// SE 0.45398564911608, p = 0.00393515490325014.
    #[test]
    fn unbalanced_matches_lmer_test() {
        let keep: Vec<usize> = (0..36).filter(|i| ![1, 8, 9, 19, 32].contains(i)).collect();
        let (y, g, names, s, subj) = design(&keep);
        let r = nested_comparison(&y, &g, &names, &s, &subj, 0.95).unwrap();
        assert!(!r.balanced);
        close(r.group_test.statistic, 33.223_08, 1e-5);
        close(r.group_test.den_df.unwrap(), 9.053_825_529_308_83, 1e-6);
        close(r.group_test.p_value, 6.763_849_864_539_46e-5, 1e-5);
        close(r.subject_variance, 0.220_389_371_569_592, 1e-4);
        close(r.residual_variance, 0.519_690_620_818_204, 1e-5);
        close(r.means[0].estimate, 9.941_550_730_595_43, 1e-7);
        close(r.means[1].estimate, 11.959_085_322_508_62, 1e-7);
        close(r.means[2].estimate, 13.737_329_710_411_98, 1e-7);
        close(r.means[0].df, 10.309_922_958_856_47, 1e-4);
        close(r.means[2].df, 8.506_999_669_043_39, 1e-4);
        let d = &r.differences[2].1;
        close(d.estimate, -1.778_244_387_903_35, 1e-7);
        close(d.std_error, 0.453_985_649_116_08, 1e-5);
        close(d.p_value, 0.003_935_154_903_250_14, 1e-3);
    }

    #[test]
    fn invalid_designs() {
        let all: Vec<usize> = (0..36).collect();
        let (y, g, names, mut s, subj) = design(&all);
        // A subject in two groups.
        s[0] = 5;
        assert!(nested_comparison(&y, &g, &names, &s, &subj, 0.95).is_err());
        // No replicates.
        let one: Vec<usize> = (0..36).step_by(3).collect();
        let (y, g, names, s, subj) = design(&one);
        assert!(nested_comparison(&y, &g, &names, &s, &subj, 0.95).is_err());
    }
}
