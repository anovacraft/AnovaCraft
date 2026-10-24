"""Independent reference values for warranty-return analysis.

Run with:  python3 -I warranty_reference.py   (writes warranty_reference.json)

Written from the definitions (Kalbfleisch, Lawless & Robinson 1991;
Meeker & Escobar 1998, sec. 12.7), independently of the Rust code:

* the likelihood is built directly from the counts: a unit shipped in period s
  and returned in period r contributes F(r - s + 1) - F(r - s); a unit still in
  service contributes 1 - F(min(P - s, W));
* Weibull and lognormal maximum likelihood by Nelder-Mead and BFGS on
  (log scale, log shape) / (mu, log sigma), covariance from a central-difference
  Hessian;
* expected future returns sum, over cohorts, the units still in service times
  the conditional probability [F(a + k) - F(a + k - 1)] / [1 - F(a)] (ages
  capped at W), plus planned shipments times F(k - m + 1) - F(k - m);
* the prediction standard deviation combines the binomial variance
  sum n p (1 - p) with the delta-method variance of the expected count
  (central-difference gradient).

The counts are invented for the tests.
"""
import json
import math

import numpy as np
from scipy import optimize, stats

LEVEL = 0.95
Z = stats.norm.ppf(0.5 + LEVEL / 2)

# Monthly shipments and returns by month of return (row: month shipped).
SHIPPED = [820, 1150, 1300, 980, 1420, 1600, 1250, 900]
RETURNS = [
    [2, 4, 6, 7, 9, 9, 10, 11],
    [3, 5, 8, 11, 12, 13, 14],
    [2, 6, 9, 12, 14, 15],
    [1, 4, 7, 9, 10],
    [3, 7, 10, 13],
    [3, 8, 12],
    [2, 6],
    [1],
]
P = 8
PLANNED = [1000, 1000, 1200]
HORIZON = 6


def cdf(family, th, x):
    x = np.asarray(x, float)
    out = np.zeros_like(x)
    pos = x > 0
    if family == "weibull":
        scale, shape = math.exp(th[0]), math.exp(th[1])
        out[pos] = 1 - np.exp(-(x[pos] / scale) ** shape)
    else:
        mu, sigma = th[0], math.exp(th[1])
        out[pos] = stats.norm.cdf((np.log(x[pos]) - mu) / sigma)
    return out


def cap(a, w):
    return a if w is None else min(a, w)


def seen(w):
    """Returns as observed under warranty length w (none at ages past it)."""
    return [[r for a, r in enumerate(rets) if w is None or a < w] for rets in RETURNS]


def nll(th, family, w):
    total = 0.0
    for s, (n, rets) in enumerate(zip(SHIPPED, seen(w))):
        for a, r in enumerate(rets):
            if r > 0:
                p = cdf(family, th, [a + 1])[0] - cdf(family, th, [a])[0]
                if p <= 0:
                    return 1e300
                total += r * math.log(p)
        left = n - sum(rets)
        sv = 1 - cdf(family, th, [cap(P - s, w)])[0]
        total += left * math.log(sv) if sv > 0 else -1e300
    return -total


def fit(family, w):
    best = None
    starts = [[3.5, 0.0], [5.0, 0.3], [4.0, -0.3]] if family == "weibull" else [[4.0, 0.3], [5.0, 0.6], [3.5, 0.0]]
    for st in starts:
        r = optimize.minimize(nll, st, args=(family, w), method="Nelder-Mead",
                              options={"xatol": 1e-12, "fatol": 1e-13, "maxiter": 4000})
        r = optimize.minimize(nll, r.x, args=(family, w), method="BFGS", options={"gtol": 1e-10})
        if best is None or r.fun < best.fun:
            best = r
    th = best.x
    h = 1e-4
    H = np.zeros((2, 2))
    for i in range(2):
        for j in range(2):
            ei, ej = np.eye(2)[i] * h, np.eye(2)[j] * h
            H[i, j] = (nll(th + ei + ej, family, w) - nll(th + ei - ej, family, w)
                       - nll(th - ei + ej, family, w) + nll(th - ei - ej, family, w)) / (4 * h * h)
    return th, np.linalg.inv(H), -best.fun


def exposures(k_from, k_to, w):
    out = []
    for s, (n, rets) in enumerate(zip(SHIPPED, seen(w))):
        left = n - sum(rets)
        age = P - s
        out.append((left, cap(age + k_from - 1, w), cap(age + k_to, w), cap(age, w)))
    for m0, units in enumerate(PLANNED):
        m = m0 + 1
        if m > k_to:
            continue
        lo = max(k_from, m) - m
        out.append((units, cap(lo, w), cap(k_to - m + 1, w), 0.0))
    return out


def expected(family, th, exps):
    mu, var = 0.0, 0.0
    for n, lo, hi, cond in exps:
        if hi <= lo:
            continue
        s = 1 - cdf(family, th, [cond])[0]
        p = (cdf(family, th, [hi])[0] - cdf(family, th, [lo])[0]) / s
        mu += n * p
        var += n * p * (1 - p)
    return mu, var


def forecast(family, th, V, w):
    rows = {"periods": [], "cumulative": []}
    for key, rng in (("periods", lambda k: (k, k)), ("cumulative", lambda k: (1, k))):
        for k in range(1, HORIZON + 1):
            a, b = rng(k)
            exps = exposures(a, b, w)
            mu, var = expected(family, th, exps)
            e = 1e-6
            g = np.array([(expected(family, th + np.eye(2)[i] * e, exps)[0]
                           - expected(family, th - np.eye(2)[i] * e, exps)[0]) / (2 * e) for i in range(2)])
            pse = math.sqrt(float(g @ V @ g))
            sd = math.sqrt(var + pse ** 2)
            rows[key].append({"expected": mu, "parameter_se": pse, "std_error": sd,
                              "lower": max(mu - Z * sd, 0.0), "upper": mu + Z * sd})
    return rows


def main():
    out = {"shipped": SHIPPED, "returns": RETURNS, "periods": P, "planned": PLANNED, "horizon": HORIZON, "cases": {}}
    for family, w in (("weibull", None), ("lognormal", None), ("weibull", 7.0)):
        th, V, ll = fit(family, w)
        name = f"{family}" + ("" if w is None else f"_w{int(w)}")
        params = ({"shape": math.exp(th[1]), "scale": math.exp(th[0])} if family == "weibull"
                  else {"mu": th[0], "sigma": math.exp(th[1])})
        out["cases"][name] = {"family": family, "warranty": w, "returns": seen(w), "log_likelihood": ll, "params": params,
                              "forecast": forecast(family, th, V, w)}
        print(name, params, ll, out["cases"][name]["forecast"]["periods"][0])
    with open("warranty_reference.json", "w") as fh:
        json.dump(out, fh, indent=1)


if __name__ == "__main__":
    main()
