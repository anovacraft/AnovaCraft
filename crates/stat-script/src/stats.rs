//! Statistical functions on arrays of numbers, for methods that have no
//! analysis dialog yet: tolerance intervals, outlier tests, comparisons with
//! a control, comparisons with the best, the median and runs tests, and
//! best-subsets regression, and the adjustment and combination of p-values.
//! Each returns its result as a map. Missing values
//! (`()`) are left out (for best subsets, rows with any missing value).
//! Also the distribution functions — density, distribution function,
//! quantile and seeded random draws — for every family of
//! [`stat_core::catalog`].

use crate::value::{ScriptResult, as_number, fail, number, script_err};
use rhai::{Array, Dynamic, Engine, FLOAT, Map};
use stat_core::Matrix;
use stat_core::catalog::{Distribution, Family};
use stat_inference::Alternative;
use stat_inference::best::{BestDirection, comparisons_with_best};
use stat_inference::control::dunnett_test;
use stat_inference::outliers::{DixonRatio, dixon_test, generalized_esd_test, grubbs_test};
use stat_inference::pvalues::{
    AdjustMethod, CombinedTest, adjust_p_values, fisher_combined, storey_q_values, stouffer_combined,
};
use stat_inference::runs::{MedianTies, median_test, runs_test, up_down_runs_test};
use stat_inference::tolerance::{ToleranceSides, nonparametric_tolerance_interval, normal_tolerance_interval};
use stat_linear::{BestSubsetsOptions, Heredity, best_subsets};

/// Register the functions.
pub(crate) fn register(engine: &mut Engine) {
    engine.register_fn("tolerance_interval", |x: Array| tolerance(&x, &Map::new()));
    engine.register_fn("tolerance_interval", |x: Array, o: Map| tolerance(&x, &o));
    engine.register_fn("grubbs_test", |x: Array| grubbs(&x, &Map::new()));
    engine.register_fn("grubbs_test", |x: Array, o: Map| grubbs(&x, &o));
    engine.register_fn("dixon_test", |x: Array| dixon(&x, &Map::new()));
    engine.register_fn("dixon_test", |x: Array, o: Map| dixon(&x, &o));
    engine.register_fn("esd_test", |x: Array| esd(&x, &Map::new()));
    engine.register_fn("esd_test", |x: Array, o: Map| esd(&x, &o));
    engine.register_fn("dunnett_test", |c: Array, g: Array| dunnett(&c, &g, &Map::new()));
    engine.register_fn("dunnett_test", |c: Array, g: Array, o: Map| dunnett(&c, &g, &o));
    engine.register_fn("best_test", |g: Array| best(&g, &Map::new()));
    engine.register_fn("best_test", |g: Array, o: Map| best(&g, &o));
    engine.register_fn("dist_density", |name: &str, p: Array, x: Dynamic| at(name, &p, &x, Distribution::density));
    engine.register_fn("dist_cdf", |name: &str, p: Array, x: Dynamic| at(name, &p, &x, Distribution::cdf));
    engine.register_fn("dist_quantile", |name: &str, p: Array, q: Dynamic| quantile(name, &p, &q));
    engine.register_fn("random_values", |name: &str, p: Array, n: rhai::INT, seed: rhai::INT| {
        random_values(name, &p, n, seed)
    });
    engine.register_fn("median_test", |g: Array| median(&g, &Map::new()));
    engine.register_fn("median_test", |g: Array, o: Map| median(&g, &o));
    engine.register_fn("runs_test", |x: Array| runs(&x, &Map::new()));
    engine.register_fn("runs_test", |x: Array, o: Map| runs(&x, &o));
    engine.register_fn("up_down_runs_test", |x: Array| up_down(&x, &Map::new()));
    engine.register_fn("up_down_runs_test", |x: Array, o: Map| up_down(&x, &o));
    engine.register_fn("p_adjust", |p: Array, method: &str| p_adjust(&p, method));
    engine.register_fn("ranks", |x: Array| ranks(&x, "average"));
    engine.register_fn("ranks", |x: Array, ties: &str| ranks(&x, ties));
    engine.register_fn("q_values", |p: Array| q_values(&p, &Map::new()));
    engine.register_fn("q_values", |p: Array, o: Map| q_values(&p, &o));
    engine.register_fn("combine_p_values", |p: Array| combine(&p, &Map::new()));
    engine.register_fn("combine_p_values", |p: Array, o: Map| combine(&p, &o));
    engine.register_fn("best_subsets", |y: Array, x: Map| subsets(&y, &x, &Map::new()));
    engine.register_fn("best_subsets", |y: Array, x: Map, o: Map| subsets(&y, &x, &o));
}

/// Non-missing numbers of an array.
fn values(a: &Array, what: &str) -> ScriptResult<Vec<f64>> {
    let mut out = Vec::with_capacity(a.len());
    for v in a {
        match as_number(v) {
            Some(x) if x.is_nan() => {}
            Some(x) => out.push(x),
            None => return fail(format!("{what} must be numbers (found {})", v.type_name())),
        }
    }
    Ok(out)
}

fn opt_number(o: &Map, key: &str, default: f64) -> ScriptResult<f64> {
    match o.get(key) {
        None => Ok(default),
        Some(v) => match as_number(v) {
            Some(x) if !x.is_nan() => Ok(x),
            _ => fail(format!("`{key}` must be a number")),
        },
    }
}

fn opt_text(o: &Map, key: &str, default: &str) -> String {
    o.get(key).map_or_else(|| default.to_owned(), |v| v.to_string())
}

fn alternative(o: &Map, key: &str) -> ScriptResult<Alternative> {
    match opt_text(o, key, "two-sided").as_str() {
        "two-sided" => Ok(Alternative::TwoSided),
        "greater" | "high" | "above" => Ok(Alternative::Greater),
        "less" | "low" | "below" => Ok(Alternative::Less),
        other => fail(format!("unknown `{key}` \"{other}\" (use \"two-sided\", \"greater\" or \"less\")")),
    }
}

/// A float result (`()` for NaN, meaning not available).
fn f(x: f64) -> Dynamic {
    if x.is_nan() { Dynamic::UNIT } else { (x as FLOAT).into() }
}

fn tolerance(x: &Array, o: &Map) -> ScriptResult<Map> {
    let x = values(x, "values")?;
    let p = opt_number(o, "proportion", 0.90)?;
    let g = opt_number(o, "confidence", 0.95)?;
    let sides = match opt_text(o, "sides", "two-sided").as_str() {
        "two-sided" => ToleranceSides::TwoSided,
        "lower" => ToleranceSides::Lower,
        "upper" => ToleranceSides::Upper,
        other => return fail(format!("unknown `sides` \"{other}\" (use \"two-sided\", \"lower\" or \"upper\")")),
    };
    let t = match opt_text(o, "method", "normal").as_str() {
        "normal" => normal_tolerance_interval(&x, p, g, sides),
        "distribution-free" => nonparametric_tolerance_interval(&x, p, g, sides),
        other => return fail(format!("unknown `method` \"{other}\" (use \"normal\" or \"distribution-free\")")),
    }
    .map_err(script_err)?;
    let mut m = Map::new();
    m.insert("method".into(), t.method.into());
    m.insert("n".into(), number(t.n as f64));
    m.insert("lower".into(), f(t.lower));
    m.insert("upper".into(), f(t.upper));
    m.insert("proportion".into(), f(t.proportion));
    m.insert("confidence".into(), f(t.confidence));
    m.insert("achieved_confidence".into(), f(t.achieved_confidence));
    m.insert("mean".into(), f(t.mean));
    m.insert("std_dev".into(), f(t.std_dev));
    m.insert("factor".into(), f(t.factor));
    Ok(m)
}

/// Positions (0-based) of the non-missing values in the original array.
fn positions(a: &Array) -> Vec<usize> {
    a.iter().enumerate().filter(|(_, v)| as_number(v).is_some_and(|x| !x.is_nan())).map(|(i, _)| i).collect()
}

fn grubbs(a: &Array, o: &Map) -> ScriptResult<Map> {
    let x = values(a, "values")?;
    let t = grubbs_test(&x, alternative(o, "side")?, opt_number(o, "alpha", 0.05)?).map_err(script_err)?;
    let mut m = Map::new();
    m.insert("statistic".into(), f(t.statistic));
    m.insert("p_value".into(), f(t.p_value));
    m.insert("critical_value".into(), f(t.critical_value));
    m.insert("value".into(), f(t.value));
    m.insert("index".into(), number(positions(a)[t.index] as f64));
    m.insert("mean".into(), f(t.mean));
    m.insert("std_dev".into(), f(t.std_dev));
    Ok(m)
}

fn dixon(a: &Array, o: &Map) -> ScriptResult<Map> {
    let x = values(a, "values")?;
    let ratio = match o.get("ratio").map(ToString::to_string) {
        None => None,
        Some(r) => match DixonRatio::ALL.into_iter().find(|d| d.label() == r) {
            Some(d) => Some(d),
            None => return fail(format!("unknown `ratio` \"{r}\" (use \"r10\", \"r11\", \"r21\" or \"r22\")")),
        },
    };
    let t = dixon_test(&x, ratio, alternative(o, "side")?, opt_number(o, "alpha", 0.05)?).map_err(script_err)?;
    let mut m = Map::new();
    m.insert("ratio".into(), t.ratio.label().into());
    m.insert("statistic".into(), f(t.statistic));
    m.insert("p_value".into(), f(t.p_value));
    m.insert("critical_value".into(), f(t.critical_value));
    m.insert("value".into(), f(t.value));
    m.insert("index".into(), number(positions(a)[t.index] as f64));
    Ok(m)
}

fn esd(a: &Array, o: &Map) -> ScriptResult<Map> {
    let x = values(a, "values")?;
    let max = opt_number(o, "max_outliers", 1.0_f64.max((x.len() as f64 / 10.0).floor()))?;
    if max < 1.0 || max.fract() != 0.0 {
        return fail("`max_outliers` must be a positive whole number");
    }
    let t = generalized_esd_test(&x, max as usize, opt_number(o, "alpha", 0.05)?).map_err(script_err)?;
    let pos = positions(a);
    let steps: Array = t
        .steps
        .iter()
        .map(|s| {
            let mut m = Map::new();
            m.insert("index".into(), number(pos[s.index] as f64));
            m.insert("value".into(), f(s.value));
            m.insert("statistic".into(), f(s.statistic));
            m.insert("critical_value".into(), f(s.critical_value));
            Dynamic::from_map(m)
        })
        .collect();
    let mut m = Map::new();
    m.insert("outliers".into(), number(t.outliers as f64));
    m.insert(
        "indices".into(),
        Dynamic::from_array(t.outlier_indices().iter().map(|&i| number(pos[i] as f64)).collect()),
    );
    m.insert("steps".into(), Dynamic::from_array(steps));
    Ok(m)
}

fn groups(g: &Array) -> ScriptResult<Vec<Vec<f64>>> {
    g.iter()
        .map(|v| match v.clone().try_cast::<Array>() {
            Some(a) => values(&a, "group values"),
            None => fail("give the groups as an array of arrays"),
        })
        .collect()
}

fn best(g: &Array, o: &Map) -> ScriptResult<Map> {
    let groups = groups(g)?;
    let direction = match opt_text(o, "best", "largest").as_str() {
        "largest" | "larger" | "high" => BestDirection::Largest,
        "smallest" | "smaller" | "low" => BestDirection::Smallest,
        other => return fail(format!("unknown `best` \"{other}\" (use \"largest\" or \"smallest\")")),
    };
    let t = comparisons_with_best(&groups, opt_number(o, "level", 0.95)?, direction).map_err(script_err)?;
    let rows: Array = t
        .comparisons
        .iter()
        .map(|c| {
            let mut m = Map::new();
            m.insert("group".into(), number(c.group as f64));
            m.insert("mean".into(), f(c.mean));
            m.insert("difference".into(), f(c.difference));
            m.insert("critical_value".into(), f(c.critical_value));
            m.insert("lower".into(), f(c.lower));
            m.insert("upper".into(), f(c.upper));
            m.insert("conclusion".into(), c.verdict.label().into());
            Dynamic::from_map(m)
        })
        .collect();
    let mut m = Map::new();
    m.insert("df".into(), number(t.df));
    m.insert("mse".into(), f(t.mse));
    m.insert("comparisons".into(), Dynamic::from_array(rows));
    Ok(m)
}

/// The distribution named `name` (a family key such as `"weibull"`) with
/// parameters `p` in the catalogue's order.
fn distribution(name: &str, p: &Array) -> ScriptResult<Distribution> {
    let Some(family) = Family::from_key(name) else {
        let keys: Vec<&str> = Family::ALL.iter().map(|f| f.key()).collect();
        return fail(format!("unknown distribution \"{name}\" (use one of: {})", keys.join(", ")));
    };
    let params = values(p, "parameters")?;
    Distribution::new(family, &params).map_err(script_err)
}

/// `g` applied to a number, or to each number of an array.
fn map_numbers(x: &Dynamic, g: impl Fn(f64) -> ScriptResult<f64>) -> ScriptResult<Dynamic> {
    if let Some(a) = x.clone().try_cast::<Array>() {
        let out: ScriptResult<Array> = a
            .iter()
            .map(|v| match as_number(v) {
                Some(v) if v.is_nan() => Ok(Dynamic::UNIT),
                Some(v) => g(v).map(f),
                None => fail("expected numbers"),
            })
            .collect();
        return out.map(Dynamic::from_array);
    }
    match as_number(x) {
        Some(v) if v.is_nan() => Ok(Dynamic::UNIT),
        Some(v) => g(v).map(f),
        None => fail("expected a number or an array of numbers"),
    }
}

fn at(name: &str, p: &Array, x: &Dynamic, g: fn(&Distribution, f64) -> f64) -> ScriptResult<Dynamic> {
    let d = distribution(name, p)?;
    map_numbers(x, |v| Ok(g(&d, v)))
}

fn quantile(name: &str, p: &Array, q: &Dynamic) -> ScriptResult<Dynamic> {
    let d = distribution(name, p)?;
    map_numbers(q, |v| d.quantile(v).map_err(script_err))
}

fn random_values(name: &str, p: &Array, n: rhai::INT, seed: rhai::INT) -> ScriptResult<Array> {
    let d = distribution(name, p)?;
    let Ok(n) = usize::try_from(n) else { return fail("the number of values cannot be negative") };
    if n > 10_000_000 {
        return fail("at most 10 000 000 values at a time");
    }
    Ok(d.draws(n, seed as u64).into_iter().map(number).collect())
}

fn dunnett(c: &Array, g: &Array, o: &Map) -> ScriptResult<Map> {
    let control = values(c, "control values")?;
    let treatments = groups(g)?;
    let t = dunnett_test(&control, &treatments, opt_number(o, "level", 0.95)?, alternative(o, "alternative")?)
        .map_err(script_err)?;
    let rows: Array = t
        .comparisons
        .iter()
        .map(|c| {
            let mut m = Map::new();
            m.insert("group".into(), number(c.treatment as f64));
            m.insert("difference".into(), f(c.difference));
            m.insert("std_error".into(), f(c.std_error));
            m.insert("t".into(), f(c.statistic));
            m.insert("p_value".into(), f(c.p_value));
            m.insert("lower".into(), f(c.confidence_interval.0));
            m.insert("upper".into(), f(c.confidence_interval.1));
            Dynamic::from_map(m)
        })
        .collect();
    let mut m = Map::new();
    m.insert("critical_value".into(), f(t.critical_value));
    m.insert("df".into(), number(t.df));
    m.insert("mse".into(), f(t.mse));
    m.insert("comparisons".into(), Dynamic::from_array(rows));
    Ok(m)
}

/// Numbers of an array with missing values (`()`) kept in place as NaN, so
/// results line up with the input.
fn aligned(a: &Array, what: &str) -> ScriptResult<Vec<f64>> {
    a.iter()
        .map(|v| match as_number(v) {
            Some(x) => Ok(x),
            None if v.is_unit() => Ok(f64::NAN),
            None => fail(format!("{what} must be numbers (found {})", v.type_name())),
        })
        .collect()
}

fn p_adjust(p: &Array, method: &str) -> ScriptResult<Array> {
    let p = aligned(p, "p-values")?;
    let out = if method == "q" {
        storey_q_values(&p, 0.5).map_err(script_err)?.q_values
    } else {
        let Some(m) = AdjustMethod::from_key(method) else {
            return fail(format!(
                "unknown method \"{method}\" (use \"bonferroni\", \"sidak\", \"holm\", \"hochberg\", \"bh\", \"by\" or \"q\")"
            ));
        };
        adjust_p_values(&p, m).map_err(script_err)?
    };
    Ok(out.into_iter().map(f).collect())
}

/// Ranks of `x` from 1 under a tie rule ([`stat_core::ranking`]); missing
/// values stay `()`.
fn ranks(x: &Array, ties: &str) -> ScriptResult<Array> {
    let Some(rule) = stat_core::ranking::TieRule::from_key(ties) else {
        return fail(format!(
            "unknown tie rule \"{ties}\" (use \"average\", \"min\", \"max\", \"dense\" or \"first\")"
        ));
    };
    let x = aligned(x, "values")?;
    Ok(stat_core::ranking::ranks_with_ties(&x, rule).into_iter().map(f).collect())
}

fn q_values(p: &Array, o: &Map) -> ScriptResult<Map> {
    let p = aligned(p, "p-values")?;
    let q = storey_q_values(&p, opt_number(o, "lambda", 0.5)?).map_err(script_err)?;
    let mut m = Map::new();
    m.insert("pi0".into(), f(q.pi0));
    m.insert("lambda".into(), f(q.lambda));
    m.insert("q_values".into(), Dynamic::from_array(q.q_values.into_iter().map(f).collect()));
    Ok(m)
}

fn combine(p: &Array, o: &Map) -> ScriptResult<Map> {
    let p = aligned(p, "p-values")?;
    let weights = match o.get("weights") {
        None => None,
        Some(w) => match w.clone().try_cast::<Array>() {
            Some(a) => Some(aligned(&a, "weights")?),
            None => return fail("`weights` must be an array of numbers"),
        },
    };
    let as_map = |t: CombinedTest| {
        let mut m = Map::new();
        m.insert("statistic".into(), f(t.statistic));
        m.insert("df".into(), f(t.df));
        m.insert("p_value".into(), f(t.p_value));
        m.insert("k".into(), number(t.k as f64));
        Dynamic::from_map(m)
    };
    let mut m = Map::new();
    m.insert("fisher".into(), as_map(fisher_combined(&p).map_err(script_err)?));
    m.insert("stouffer".into(), as_map(stouffer_combined(&p, weights.as_deref()).map_err(script_err)?));
    Ok(m)
}

fn median(g: &Array, o: &Map) -> ScriptResult<Map> {
    let groups = groups(g)?;
    let ties = match opt_text(o, "ties", "below").as_str() {
        "below" => MedianTies::Below,
        "above" => MedianTies::Above,
        "ignore" => MedianTies::Ignore,
        other => return fail(format!("unknown `ties` \"{other}\" (use \"below\", \"above\" or \"ignore\")")),
    };
    let t = median_test(&groups, ties).map_err(script_err)?;
    let counts = |v: &[usize]| Dynamic::from_array(v.iter().map(|&c| number(c as f64)).collect());
    let mut m = Map::new();
    m.insert("median".into(), f(t.grand_median));
    m.insert("above".into(), counts(&t.above));
    m.insert("below".into(), counts(&t.below));
    m.insert("chi_square".into(), f(t.test.statistic));
    m.insert("df".into(), number(t.test.df));
    m.insert("p_value".into(), f(t.test.p_value));
    m.insert("min_expected".into(), f(t.min_expected));
    Ok(m)
}

fn runs_map(t: &stat_inference::runs::RunsTest) -> Map {
    let mut m = Map::new();
    m.insert("runs".into(), number(t.runs as f64));
    m.insert("n".into(), number(t.n as f64));
    m.insert("expected".into(), f(t.expected));
    m.insert("std_dev".into(), f(t.std_dev));
    m.insert("p_fewer".into(), f(t.p_fewer));
    m.insert("p_more".into(), f(t.p_more));
    m.insert("p_value".into(), f(t.p_value));
    m.insert("exact".into(), t.exact.into());
    m
}

fn runs(a: &Array, o: &Map) -> ScriptResult<Map> {
    let x = values(a, "values")?;
    let reference = match o.get("reference") {
        None => None,
        Some(_) => Some(opt_number(o, "reference", 0.0)?),
    };
    let t = runs_test(&x, reference, alternative(o, "alternative")?).map_err(script_err)?;
    let mut m = runs_map(&t);
    m.insert("reference".into(), f(t.reference));
    m.insert("above".into(), number(t.above as f64));
    m.insert("below".into(), number(t.below as f64));
    Ok(m)
}

fn up_down(a: &Array, o: &Map) -> ScriptResult<Map> {
    let x = values(a, "values")?;
    let t = up_down_runs_test(&x, alternative(o, "alternative")?).map_err(script_err)?;
    Ok(runs_map(&t))
}

fn subsets(y: &Array, x: &Map, o: &Map) -> ScriptResult<Array> {
    if x.is_empty() {
        return fail("give at least one predictor");
    }
    let names: Vec<String> = x.keys().map(ToString::to_string).collect();
    let mut columns = Vec::with_capacity(names.len());
    for (name, v) in x {
        let Some(a) = v.clone().try_cast::<Array>() else {
            return fail(format!("predictor `{name}` must be an array"));
        };
        if a.len() != y.len() {
            return fail(format!("predictor `{name}` has {} values; the response has {}", a.len(), y.len()));
        }
        columns.push(a);
    }
    let num = |v: &Dynamic, what: &str| -> ScriptResult<f64> {
        as_number(v).map_or_else(|| fail(format!("{what} must be numbers")), Ok)
    };
    // Complete rows only.
    let mut yy = Vec::new();
    let mut xx: Vec<Vec<f64>> = vec![Vec::new(); columns.len()];
    for i in 0..y.len() {
        let yi = num(&y[i], "response values")?;
        let row: Vec<f64> = columns.iter().map(|c| num(&c[i], "predictor values")).collect::<ScriptResult<_>>()?;
        if yi.is_nan() || row.iter().any(|v| v.is_nan()) {
            continue;
        }
        yy.push(yi);
        for (col, v) in xx.iter_mut().zip(row) {
            col.push(v);
        }
    }
    let forced = match o.get("forced") {
        None => Vec::new(),
        Some(v) => {
            let Some(a) = v.clone().try_cast::<Array>() else { return fail("`forced` must be an array of names") };
            a.iter()
                .map(|n| {
                    let n = n.to_string();
                    names.iter().position(|m| *m == n).map_or_else(|| fail(format!("no predictor `{n}`")), Ok)
                })
                .collect::<ScriptResult<_>>()?
        }
    };
    let per = opt_number(o, "models_per_size", 3.0)?;
    let max = o.get("max_size").map(|_| opt_number(o, "max_size", 0.0)).transpose()?;
    let options = BestSubsetsOptions {
        models_per_size: per.max(0.0) as usize,
        max_size: max.map(|m| m.max(0.0) as usize),
        forced,
        heredity: Heredity::None,
    };
    let matrix = Matrix::from_columns(&xx).map_err(script_err)?;
    let effects: Vec<_> = (0..names.len()).map(|j| j..j + 1).collect();
    let r = best_subsets(&matrix, &yy, &effects, &vec![Vec::new(); names.len()], &options).map_err(script_err)?;
    Ok(r.models
        .iter()
        .map(|s| {
            let mut m = Map::new();
            let terms: Array = s.effects.iter().map(|&e| names[e].clone().into()).collect();
            m.insert("predictors".into(), Dynamic::from_array(terms));
            m.insert("size".into(), number(s.size() as f64));
            m.insert("rss".into(), f(s.rss));
            m.insert("r_squared".into(), f(s.r_squared));
            m.insert("adj_r_squared".into(), f(s.adj_r_squared));
            m.insert("cp".into(), f(s.cp));
            m.insert("aicc".into(), f(s.aicc));
            m.insert("bic".into(), f(s.bic));
            Dynamic::from_map(m)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use crate::{RunOptions, run_script};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn run(source: &str) -> Vec<String> {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&lines);
        run_script(source, &RunOptions::default(), move |s| sink.borrow_mut().push(s.to_owned())).unwrap();
        lines.take()
    }

    /// Comparisons with the best and the distribution functions (engine
    /// values are checked in `stat_inference::best` and `stat_core::catalog`;
    /// Φ(1.96) = 0.9750021048517795, the Poisson(3) mass at 2 = 4.5e⁻³ =
    /// 0.22404180765538775 and the Weibull(2, 1) median √ln 2 =
    /// 0.8325546111576977, from SciPy 1.18).
    #[test]
    fn best_test_and_distribution_functions() {
        let out = run(r#"
            let b = best_test([[10.0, 11.0, 12.0], [14.0, 15.0, 16.0], [20.0, 21.0, 22.0]]);
            print(b.comparisons[2].conclusion); print(b.comparisons[0].conclusion); print(b.df);
            let s = best_test([[10.0, 11.0, 12.0], [14.0, 15.0, 16.0]], #{ best: "smallest", level: 0.9 });
            print(s.comparisons[0].conclusion);
            print(round(dist_cdf("normal", [0, 1], 1.96) * 1e10));
            print(round(dist_density("poisson", [3], 2) * 1e10));
            print(round(dist_quantile("weibull", [2, 1], [0.5])[0] * 1e10));
            let r = random_values("gamma", [2, 3], 5, 7);
            print(r.len()); print(r == random_values("gamma", [2, 3], 5, 7));
        "#);
        assert_eq!(
            out,
            ["the best", "not the best", "6", "the best", "9750021049.0", "2240418077.0", "8325546112.0", "5", "true"]
        );
    }

    #[test]
    fn functions_return_maps_matching_the_engine() {
        let out = run(r#"
            let x = [9.8, 10.2, 10.1, 9.7, 10.4, 10.0, 9.9, 10.3, 10.0, 9.6, ()];
            let t = tolerance_interval(x, #{ proportion: 0.9, confidence: 0.95, sides: "upper" });
            print(t.n); print(t.lower); print(round(t.factor * 1e6));
            let g = grubbs_test([1.0, 1.1, 0.9, 1.05, 0.95, 3.0], #{ side: "greater" });
            print(g.index); print(g.p_value < 0.05);
            let d = dixon_test([0.189, 0.167, 0.187, 0.183, 0.186, 0.182, 0.181, 0.184, 0.181, 0.177]);
            print(d.ratio); print(d.index);
            let e = esd_test([1.0, 1.2, 0.9, 1.1, 1.0, 0.95, 1.05, 8.0, 9.0, 1.02, 0.98, 1.01], #{ max_outliers: 3 });
            print(e.outliers); print(e.indices);
            let c = dunnett_test([7.40, 8.50, 7.20, 8.24, 9.84, 8.32], [[9.76, 8.80, 7.68, 9.36], [12.80, 9.68, 12.16, 9.20, 10.55]]);
            print(c.df); print(c.comparisons[1].p_value < 0.01);
            let m = median_test([[1, 2, 3, 4], [5, 6, 7, 8]]);
            print(m.above);
            let r = runs_test([1, 9, 2, 8, 3, 7, 4, 6], #{ alternative: "greater" });
            print(r.runs);
            print(up_down_runs_test([1, 2, 3, 4, 5]).runs);
            let s = best_subsets([1.0, 2.1, 2.9, 4.2, 5.0, 5.9, 7.1], #{ a: [1, 2, 3, 4, 5, 6, 7], b: [3, 1, 4, 1, 5, 9, 2] });
            print(s.len()); print(s[1].predictors);
        "#);
        assert_eq!(
            out,
            [
                "10",
                "-inf",
                "2354640.0",
                "5",
                "true",
                "r11",
                "1",
                "2",
                "[8, 7]",
                "12",
                "true",
                "[0, 4]",
                "8",
                "1",
                "4",
                "[\"a\"]",
            ]
        );
    }

    /// pandas `Series([2, 1, 2, NaN, 3]).rank(method=…)`.
    #[test]
    fn ranks_with_tie_rules() {
        let out = run(r#"
            let x = [2, 1, 2, (), 3];
            print(ranks(x)); print(ranks(x, "min")); print(ranks(x, "max")); print(ranks(x, "dense")); print(ranks(x, "first"));
        "#);
        assert_eq!(
            out,
            [
                "[2.5, 1.0, 2.5, (), 4.0]",
                "[2.0, 1.0, 2.0, (), 4.0]",
                "[3.0, 1.0, 3.0, (), 4.0]",
                "[2.0, 1.0, 2.0, (), 3.0]",
                "[2.0, 1.0, 3.0, (), 4.0]",
            ]
        );
    }

    /// R: p.adjust(c(0.01, 0.04, 0.03, 0.005), "BH") = 0.02, 0.04, 0.04, 0.02;
    /// "holm" = 0.03, 0.06, 0.06, 0.02; Fisher's χ² = −2 Σ ln p = 33.25784 on 8 DF;
    /// Storey's π̂₀ at λ = 0.02 is 2 / (4 · 0.98).
    #[test]
    fn p_value_functions_match_r() {
        let out = run(r#"
            let p = [0.01, 0.04, (), 0.03, 0.005];
            print(p_adjust(p, "bh").map(|x| if x == () { -1 } else { round(x * 1e6) / 1e6 }));
            print(p_adjust(p, "holm").map(|x| if x == () { -1 } else { round(x * 1e6) / 1e6 }));
            let q = q_values(p, #{ lambda: 0.02 });
            print(q.pi0);
            let c = combine_p_values(p);
            print(round(c.fisher.statistic * 1e5) / 1e5); print(c.fisher.df); print(c.stouffer.k);
        "#);
        assert_eq!(
            out,
            [
                "[0.02, 0.04, -1, 0.04, 0.02]",
                "[0.03, 0.06, -1, 0.06, 0.02]",
                "0.5102040816326531",
                "33.25784",
                "8.0",
                "4",
            ]
        );
        assert!(run_script("p_adjust([0.1, 0.2], \"fdr\")", &RunOptions::default(), |_| {}).is_err());
        assert!(run_script("p_adjust([0.1, 1.2], \"bh\")", &RunOptions::default(), |_| {}).is_err());
    }

    /// The example in docs/scripting.md.
    #[test]
    fn documented_best_subsets_example_runs() {
        let out = run(r#"
            let t = open_sample("Hald cement");
            let x = #{};
            for name in t.columns() {
                if name != "Heat (cal/g)" { x[name] = t.column(name); }
            }
            for m in best_subsets(t.column("Heat (cal/g)"), x, #{ models_per_size: 2 }) {
                print(`${m.predictors}: Cp = ${m.cp}`);
            }
        "#);
        assert_eq!(out.len(), 8);
        assert!(
            out[3].starts_with("[\"Tricalcium aluminate (%)\", \"Tricalcium silicate (%)\"]: Cp = 2.67"),
            "{}",
            out[3]
        );
    }

    #[test]
    fn bad_options_stop_the_script() {
        for source in [
            "tolerance_interval([1, 2, 3], #{ sides: \"both\" })",
            "grubbs_test([1, \"a\", 3])",
            "dixon_test([1, 2, 3, 4], #{ ratio: \"r99\" })",
            "median_test([1, 2])",
            "best_subsets([1, 2, 3], #{ a: [1, 2] })",
            "esd_test([1, 2, 3, 4, 5], #{ max_outliers: 0 })",
        ] {
            assert!(run_script(source, &RunOptions::default(), |_| {}).is_err(), "{source}");
        }
    }
}
