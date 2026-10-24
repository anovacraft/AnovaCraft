//! The mixed-model extensions checked against R (`tests/data/extensions_reference.R`):
//! prior weights and offsets (lme4), CR2 cluster-robust standard errors with
//! Bell–McCaffrey degrees of freedom and the HTZ test (clubSandwich), residual
//! variances by group and between–within degrees of freedom (nlme), the
//! cumulative logit mixed model (ordinal::clmm) and shelf life with random
//! batches (lme4 and lmerTest with hand-computed bounds).

use stat_core::Matrix;
use stat_data::{JsonValue, parse_json};
use stat_mixed::{
    DfMethod, MixedFamily, MixedModel, MixedOptions, RandomBatchOptions, RandomTerm, ResidualGroups, fit_mixed,
    random_batch_shelf_life,
};

fn reference() -> JsonValue {
    parse_json(include_str!("data/extensions_reference.json")).unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn get(v: &JsonValue, path: &str) -> JsonValue {
    path.split('.').fold(v.clone(), |acc, k| acc.get(k).unwrap_or_else(|| panic!("missing {path}")).clone())
}

fn vec_of(v: &JsonValue, path: &str) -> Vec<f64> {
    nums(&get(v, path))
}

fn num(v: &JsonValue, path: &str) -> f64 {
    get(v, path).as_f64().unwrap()
}

fn close(a: f64, b: f64, rel: f64, what: &str) {
    assert!((a - b).abs() <= rel * b.abs().max(1.0), "{what}: {a} vs {b}");
}

fn intercepts(group: &str, codes: Vec<usize>, g: usize) -> RandomTerm {
    let n = codes.len();
    RandomTerm {
        group: group.to_owned(),
        levels: (0..g).map(|i| format!("{group}{i}")).collect(),
        codes,
        effects: vec!["Intercept".to_owned()],
        z: Matrix::from_columns(&[vec![1.0; n]]).unwrap(),
        correlated: true,
    }
}

/// Orthodont: `distance ~ age * Sex` with random intercepts by subject.
fn orthodont(slope: bool) -> MixedModel {
    let r = reference();
    let y = vec_of(&r, "orthodont.distance");
    let age = vec_of(&r, "orthodont.age");
    let female = vec_of(&r, "orthodont.female");
    let codes: Vec<usize> = vec_of(&r, "orthodont.subject").iter().map(|&c| c as usize).collect();
    let n = y.len();
    let inter: Vec<f64> = age.iter().zip(&female).map(|(a, f)| a * f).collect();
    let mut term = intercepts("Subject", codes, num(&r, "orthodont.n_subjects") as usize);
    if slope {
        term.effects.push("age".to_owned());
        term.z = Matrix::from_columns(&[vec![1.0; n], age.clone()]).unwrap();
    }
    MixedModel {
        y,
        x: Matrix::from_columns(&[vec![1.0; n], age, female, inter]).unwrap(),
        x_names: ["Intercept", "age", "SexFemale", "age:SexFemale"].map(str::to_owned).to_vec(),
        weights: Vec::new(),
        offset: Vec::new(),
        random: vec![term],
        family: MixedFamily::Normal,
        residual_groups: None,
        categories: Vec::new(),
    }
}

#[test]
fn between_within_degrees_of_freedom_match_nlme() {
    let r = reference();
    let options = MixedOptions { df_method: DfMethod::BetweenWithin, ..MixedOptions::default() };
    let fit = fit_mixed(&orthodont(false), &options).unwrap();
    assert_eq!(fit.df_label, "between–within");
    let df: Vec<f64> = fit.fixed.iter().map(|f| f.df.unwrap()).collect();
    assert_eq!(df, vec_of(&r, "between_within.df"));
    for (f, t) in fit.fixed.iter().zip(vec_of(&r, "between_within.t")) {
        close(f.statistic, t, 1e-5, &f.name);
    }
    // Effect test of Sex uses its 25 df; an interaction-and-sex test the smaller.
    let t = fit.test_contrast(&[vec![0.0, 0.0, 1.0, 0.0], vec![0.0, 0.0, 0.0, 1.0]]).unwrap();
    assert_eq!(t.den_df, Some(25.0));
}

#[test]
fn residual_variances_by_group_match_nlme_var_ident() {
    let r = reference();
    let mut model = orthodont(false);
    let female = vec_of(&r, "orthodont.female");
    model.residual_groups = Some(ResidualGroups {
        name: "Sex".to_owned(),
        levels: vec!["Male".to_owned(), "Female".to_owned()],
        codes: female.iter().map(|&f| f as usize).collect(),
    });
    let fit = fit_mixed(&model, &MixedOptions::default()).unwrap();
    close(fit.minus_two_log_likelihood, num(&r, "var_ident.minus_two_ll"), 1e-8, "−2 log L (REML)");
    let sds: Vec<f64> = fit.residual_by_group.iter().map(|c| c.std_dev).collect();
    close(sds[0], num(&r, "var_ident.sd_male"), 1e-5, "male residual SD");
    close(sds[1], num(&r, "var_ident.sd_female"), 1e-5, "female residual SD");
    close(fit.variance_components[0].std_dev, num(&r, "var_ident.subject_sd"), 1e-5, "subject SD");
    for ((f, e), s) in fit.fixed.iter().zip(vec_of(&r, "var_ident.estimate")).zip(vec_of(&r, "var_ident.std_error")) {
        close(f.estimate, e, 1e-6, &f.name);
        close(f.std_error, s, 1e-5, &f.name);
    }
    let lr = fit.equal_variance_test.as_ref().unwrap();
    assert_eq!(lr.df, 1);
    close(lr.statistic, num(&r, "var_ident.lr"), 1e-6, "likelihood ratio");
    assert!(lr.p_value < 1e-4);
    assert_eq!(fit.residual.as_ref().unwrap().effect, "Male");
    assert!(fit.residual_by_group.iter().all(|c| c.interval.is_some()));
}

#[test]
fn cr2_robust_errors_match_club_sandwich() {
    let r = reference();
    for (slope, key) in [(false, "cr2"), (true, "cr2_slope")] {
        let options = MixedOptions { robust: true, ..MixedOptions::default() };
        let fit = fit_mixed(&orthodont(slope), &options).unwrap();
        let rob = fit.robust.as_ref().unwrap();
        assert_eq!((rob.cluster.as_str(), rob.n_clusters), ("Subject", 27));
        assert_eq!(fit.df_label, "Bell–McCaffrey (CR2)");
        for ((f, se), df) in
            fit.fixed.iter().zip(vec_of(&r, &format!("{key}.std_error"))).zip(vec_of(&r, &format!("{key}.df")))
        {
            close(f.std_error, se, 1e-5, &format!("{key} {} SE", f.name));
            close(f.df.unwrap(), df, 1e-4, &format!("{key} {} df", f.name));
        }
        if !slope {
            let t = fit.test_contrast(&[vec![0.0, 0.0, 1.0, 0.0], vec![0.0, 0.0, 0.0, 1.0]]).unwrap();
            close(t.statistic, num(&r, "cr2.htz_f"), 1e-5, "HTZ F");
            close(t.den_df.unwrap(), num(&r, "cr2.htz_df"), 1e-4, "HTZ df");
            close(t.p_value, num(&r, "cr2.htz_p"), 1e-4, "HTZ p");
        }
    }
    // Crossed groupings have no outermost cluster: model-based errors, with a note.
    let mut crossed = orthodont(false);
    let n = crossed.y.len();
    crossed.random.push(intercepts("Age", (0..n).map(|i| i % 4).collect(), 4));
    let fit = fit_mixed(&crossed, &MixedOptions { robust: true, ..MixedOptions::default() }).unwrap();
    assert!(fit.robust.is_none() && fit.warnings.iter().any(|w| w.contains("Robust standard errors need")));
}

#[test]
fn prior_weights_match_lme4() {
    let r = reference();
    let mut model = orthodont(false);
    let age = vec_of(&r, "orthodont.age");
    let n = age.len();
    model.x = Matrix::from_columns(&[vec![1.0; n], age]).unwrap();
    model.x_names = vec!["Intercept".to_owned(), "age".to_owned()];
    model.weights = vec_of(&r, "weighted_lmm.w");
    let fit = fit_mixed(&model, &MixedOptions::default()).unwrap();
    close(fit.minus_two_log_likelihood, num(&r, "weighted_lmm.reml"), 1e-8, "REML criterion");
    for ((f, e), s) in
        fit.fixed.iter().zip(vec_of(&r, "weighted_lmm.estimate")).zip(vec_of(&r, "weighted_lmm.std_error"))
    {
        close(f.estimate, e, 1e-6, &f.name);
        close(f.std_error, s, 1e-5, &f.name);
    }
    close(fit.variance_components[0].variance, num(&r, "weighted_lmm.subject_var"), 1e-5, "subject variance");
    close(fit.residual.as_ref().unwrap().std_dev, num(&r, "weighted_lmm.sigma"), 1e-6, "σ");
}

#[test]
fn poisson_weights_and_offset_match_lme4() {
    let r = reference();
    let y = vec_of(&r, "poisson.y");
    let n = y.len();
    let codes: Vec<usize> = vec_of(&r, "poisson.g").iter().map(|&c| c as usize).collect();
    let model = MixedModel {
        y,
        x: Matrix::from_columns(&[vec![1.0; n], vec_of(&r, "poisson.x")]).unwrap(),
        x_names: vec!["Intercept".to_owned(), "x".to_owned()],
        weights: vec_of(&r, "poisson.w"),
        offset: vec_of(&r, "poisson.exposure").iter().map(|e| e.ln()).collect(),
        random: vec![intercepts("g", codes, 10)],
        family: MixedFamily::Poisson,
        residual_groups: None,
        categories: Vec::new(),
    };
    let fit = fit_mixed(&model, &MixedOptions::default()).unwrap();
    close(fit.minus_two_log_likelihood, num(&r, "poisson.minus_two_ll"), 1e-6, "−2 log L");
    for ((f, e), s) in fit.fixed.iter().zip(vec_of(&r, "poisson.estimate")).zip(vec_of(&r, "poisson.std_error")) {
        close(f.estimate, e, 1e-4, &f.name);
        close(f.std_error, s, 1e-3, &f.name);
    }
    close(fit.variance_components[0].variance, num(&r, "poisson.g_var"), 1e-3, "group variance");
}

fn wine(slope: bool) -> MixedModel {
    let r = reference();
    let y = vec_of(&r, "wine.rating");
    let warm = vec_of(&r, "wine.warm");
    let contact = vec_of(&r, "wine.contact");
    let n = y.len();
    let codes: Vec<usize> = vec_of(&r, "wine.judge").iter().map(|&c| c as usize).collect();
    let mut term = intercepts("judge", codes, 9);
    if slope {
        term.effects.push("contact".to_owned());
        term.z = Matrix::from_columns(&[vec![1.0; n], contact.clone()]).unwrap();
    }
    MixedModel {
        y,
        x: Matrix::from_columns(&[warm, contact]).unwrap(),
        x_names: vec!["tempwarm".to_owned(), "contactyes".to_owned()],
        weights: Vec::new(),
        offset: Vec::new(),
        random: vec![term],
        family: MixedFamily::Ordinal,
        residual_groups: None,
        categories: (1..=5).map(|k| k.to_string()).collect(),
    }
}

#[test]
fn ordinal_mixed_model_matches_clmm() {
    let r = reference();
    let fit = fit_mixed(&wine(false), &MixedOptions::default()).unwrap();
    close(fit.minus_two_log_likelihood, num(&r, "wine.laplace.minus_two_ll"), 1e-7, "−2 log L (Laplace)");
    let coef: Vec<f64> = fit.thresholds.iter().chain(&fit.fixed).map(|c| c.estimate).collect();
    let se: Vec<f64> = fit.thresholds.iter().chain(&fit.fixed).map(|c| c.std_error).collect();
    for (k, (a, b)) in coef.iter().zip(vec_of(&r, "wine.laplace.coef")).enumerate() {
        close(*a, b, 1e-4, &format!("coefficient {k}"));
    }
    for (k, (a, b)) in se.iter().zip(vec_of(&r, "wine.laplace.std_error")).enumerate() {
        close(*a, b, 2e-3, &format!("std error {k}"));
    }
    close(fit.variance_components[0].variance, num(&r, "wine.laplace.judge_var"), 1e-3, "judge variance");
    assert_eq!(fit.thresholds[0].name, "1|2");
    // Probabilities: each row's sum to 1.
    assert!(fit.category_probabilities.iter().all(|p| (p.iter().sum::<f64>() - 1.0).abs() < 1e-12));

    let agq = fit_mixed(&wine(false), &MixedOptions { quadrature_points: 10, ..MixedOptions::default() }).unwrap();
    close(agq.minus_two_log_likelihood, num(&r, "wine.agq10.minus_two_ll"), 1e-7, "−2 log L (AGQ 10)");
    let coef: Vec<f64> = agq.thresholds.iter().chain(&agq.fixed).map(|c| c.estimate).collect();
    for (k, (a, b)) in coef.iter().zip(vec_of(&r, "wine.agq10.coef")).enumerate() {
        close(*a, b, 1e-4, &format!("AGQ coefficient {k}"));
    }
    close(agq.variance_components[0].variance, num(&r, "wine.agq10.judge_var"), 1e-3, "AGQ judge variance");

    let slope = fit_mixed(&wine(true), &MixedOptions::default()).unwrap();
    close(slope.minus_two_log_likelihood, num(&r, "wine_slope.minus_two_ll"), 1e-6, "−2 log L (slope)");
    let coef: Vec<f64> = slope.thresholds.iter().chain(&slope.fixed).map(|c| c.estimate).collect();
    for (k, (a, b)) in coef.iter().zip(vec_of(&r, "wine_slope.coef")).enumerate() {
        close(*a, b, 1e-3, &format!("slope-model coefficient {k}"));
    }
    for (c, v) in slope.variance_components.iter().zip(vec_of(&r, "wine_slope.var")) {
        close(c.variance, v, 1e-2, &c.effect);
    }
    // An intercept column is refused.
    let mut bad = wine(false);
    bad.x = Matrix::from_columns(&[vec![1.0; 72], vec_of(&r, "wine.warm")]).unwrap();
    assert!(fit_mixed(&bad, &MixedOptions::default()).is_err());
}

#[test]
fn random_batch_shelf_life_matches_hand_computed_bounds() {
    let r = reference();
    let time = vec_of(&r, "stability.time");
    let assay = vec_of(&r, "stability.assay");
    let batch: Vec<String> = vec_of(&r, "stability.batch").iter().map(|b| format!("B{b}")).collect();
    let options =
        RandomBatchOptions { lower_limit: Some(95.0), upper_limit: None, confidence_level: 0.95, horizon: Some(60.0) };
    let s = random_batch_shelf_life(&time, &assay, &batch, &options).unwrap();
    for (f, e) in s.fit.fixed.iter().zip(vec_of(&r, "stability.estimate")) {
        close(f.estimate, e, 1e-6, &f.name);
    }
    let at = vec_of(&r, "stability.at");
    for (k, &t) in at.iter().enumerate() {
        let (_, lo, _) = s.bounds(t, false).unwrap();
        close(lo, vec_of(&r, "stability.mean_bound")[k], 1e-4, "population-mean bound");
        let (_, lo, _) = s.bounds(t, true).unwrap();
        close(lo, vec_of(&r, "stability.future_bound")[k], 1e-4, "future-batch bound");
        let x = [1.0, t];
        let (_, df) = s.fit.prediction_variance(&x, None).unwrap();
        close(df.unwrap(), vec_of(&r, "stability.mean_df")[k], 1e-3, "mean df");
        let (_, df) = s.fit.prediction_variance(&x, Some((0, &x))).unwrap();
        close(df.unwrap(), vec_of(&r, "stability.future_df")[k], 1e-3, "future df");
    }
    close(s.mean_shelf_life.unwrap(), num(&r, "stability.mean_life"), 1e-4, "population-mean shelf life");
    close(s.batch_shelf_life.unwrap(), num(&r, "stability.future_life"), 1e-4, "future-batch shelf life");
    assert!(s.batch_shelf_life < s.mean_shelf_life);
    assert_eq!(s.batch_lines.len(), 6);
    // Two batches are too few.
    let two: Vec<String> = batch.iter().map(|b| if b == "B0" || b == "B1" { "X".into() } else { "Y".into() }).collect();
    assert!(random_batch_shelf_life(&time, &assay, &two, &options).is_err());
}
