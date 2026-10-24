//! Trend fits, forecasts and accuracy measures compared with R 4.3:
//! `lm` + `predict(interval = "prediction")` for the linear, quadratic and
//! (on `log y`) exponential trends, `nls` with `SSlogis` for the logistic
//! curve, and the accuracy measures and moving averages computed directly
//! (`tests/data/trend_reference.R` writes `trend_reference.json`).

use stat_data::{JsonValue, parse_json};
use stat_timeseries::trend::{TrendModel, fit_trend, moving_average_forecast};

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

fn nums(v: &JsonValue, key: &str) -> Vec<f64> {
    v.get(key).unwrap_or_else(|| panic!("missing {key}")).as_array().unwrap().iter().map(num).collect()
}

fn field(v: &JsonValue, key: &str) -> f64 {
    num(v.get(key).unwrap_or_else(|| panic!("missing {key}")))
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    let scale = want.abs().max(1e-300);
    assert!((got - want).abs() <= rel * scale, "{what}: got {got}, reference {want}");
}

fn close_all(got: &[f64], want: &[f64], rel: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what} length");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        close(*g, *w, rel, &format!("{what}[{i}]"));
    }
}

fn check_accuracy(acc: &stat_timeseries::trend::AccuracyMeasures, want: &JsonValue, rel: f64, what: &str) {
    // The mean error of a least-squares fit with an intercept is zero up to
    // rounding, so it is compared on the scale of the MAD.
    let me = field(want, "me");
    assert!((acc.mean_error - me).abs() <= rel * field(want, "mad").max(me.abs()), "{what} ME");
    close(acc.mad, field(want, "mad"), rel, &format!("{what} MAD"));
    close(acc.msd, field(want, "msd"), rel, &format!("{what} MSD"));
    close(acc.rmse, field(want, "rmse"), rel, &format!("{what} RMSE"));
    close(acc.mape.unwrap(), field(want, "mape"), rel, &format!("{what} MAPE"));
}

fn data() -> JsonValue {
    parse_json(include_str!("data/trend_reference.json")).unwrap()
}

#[test]
fn least_squares_trends_match_lm() {
    let all = data();
    let h = field(&all, "horizon") as usize;
    for c in all.get("least_squares").unwrap().as_array().unwrap() {
        let name = c.get("series").unwrap().as_str().unwrap();
        let y = nums(all.get("series").unwrap(), name);
        let model = match c.get("model").unwrap().as_str().unwrap() {
            "linear" => TrendModel::Linear,
            "quadratic" => TrendModel::Quadratic,
            _ => TrendModel::Exponential,
        };
        let what = format!("{name} {model:?}");
        let fit = fit_trend(&y, model).unwrap();
        close_all(&fit.coefficients, &nums(c, "coefficients"), 1e-9, &format!("{what} coefficients"));
        close_all(&fit.std_errors, &nums(c, "std_errors"), 1e-9, &format!("{what} standard errors"));
        close(fit.sigma, field(c, "sigma"), 1e-9, &format!("{what} sigma"));
        close_all(&fit.fitted, &nums(c, "fitted"), 1e-9, &format!("{what} fitted"));
        check_accuracy(&fit.accuracy, c.get("accuracy").unwrap(), 1e-9, &what);
        let f = fit.forecast(h, 0.95).unwrap();
        close_all(&f.mean, &nums(c, "forecast"), 1e-9, &format!("{what} forecast"));
        close_all(f.lower.as_ref().unwrap(), &nums(c, "lower"), 1e-9, &format!("{what} lower"));
        close_all(f.upper.as_ref().unwrap(), &nums(c, "upper"), 1e-9, &format!("{what} upper"));
    }
}

#[test]
fn logistic_trend_matches_nls() {
    let all = data();
    let h = field(&all, "horizon") as usize;
    for c in all.get("logistic").unwrap().as_array().unwrap() {
        let name = c.get("series").unwrap().as_str().unwrap();
        let y = nums(all.get("series").unwrap(), name);
        let fit = fit_trend(&y, TrendModel::Logistic).unwrap();
        // SSlogis: Asym / (1 + exp((xmid − t)/scal)), so c = Asym,
        // a = exp(xmid/scal), b = 1/scal. nls stops at its default relative
        // offset 1e-5, so parameters agree to about 1e-6 and the SSE (flat at
        // the minimum) to about 1e-10.
        let (asym, xmid, scal) = (field(c, "asym"), field(c, "xmid"), field(c, "scal"));
        let coef = &fit.coefficients;
        close(coef[0], asym, 1e-5, &format!("{name} asymptote"));
        close(coef[2], 1.0 / scal, 1e-5, &format!("{name} rate"));
        close(coef[1].ln() / coef[2], xmid, 1e-5, &format!("{name} midpoint"));
        let sse: f64 = fit.residuals.iter().map(|r| r * r).sum();
        assert!(sse <= field(c, "sse") * (1.0 + 1e-9), "{name} SSE {sse} vs {}", field(c, "sse"));
        close(sse, field(c, "sse"), 1e-8, &format!("{name} SSE"));
        close_all(&fit.fitted, &nums(c, "fitted"), 1e-5, &format!("{name} fitted"));
        check_accuracy(&fit.accuracy, c.get("accuracy").unwrap(), 1e-4, name);
        let f = fit.forecast(h, 0.95).unwrap();
        assert!(f.lower.is_none() && f.upper.is_none());
        close_all(&f.mean, &nums(c, "forecast"), 1e-5, &format!("{name} forecast"));
    }
}

#[test]
fn moving_average_forecasts_match_direct_computation() {
    let all = data();
    let y = nums(all.get("series").unwrap(), "air");
    for c in all.get("moving_average").unwrap().as_array().unwrap() {
        let k = field(c, "span") as usize;
        let ma = moving_average_forecast(&y, k).unwrap();
        let fitted: Vec<f64> = ma.fitted.iter().flatten().copied().collect();
        assert!(ma.fitted[..k].iter().all(Option::is_none));
        close_all(&fitted, &nums(c, "fitted"), 1e-12, &format!("span {k} fitted"));
        close(ma.forecast, field(c, "forecast"), 1e-12, &format!("span {k} forecast"));
        check_accuracy(&ma.accuracy, c.get("accuracy").unwrap(), 1e-12, &format!("span {k}"));
    }
}
