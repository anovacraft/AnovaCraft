"""Reference values for stat_discrete::treatment_effects, computed
independently of the Rust code:

* propensity and outcome models with statsmodels GLM;
* inverse-probability weighting and augmented IPW from their formulas
  (Lunceford & Davidian 2004), with the stacked-equation sandwich whose
  Jacobian is taken by complex-step differentiation (exact to rounding;
  the Rust code uses central differences), and for the IPW ATE also the
  closed-form influence function of Lunceford & Davidian (2004) eq. (9);
* ATE matching with replacement (Abadie & Imbens 2006) by brute force;
* covariate balance (Austin 2009 formulas) with NumPy.

Matching for the ATT is checked against R MatchIt in
treatment_effects_matchit.R. Data: app-model/tests/data/treatment_study.csv
(an invented study). Run: python3 treatment_effects_reference.py
(numpy 2, statsmodels 0.14); writes treatment_effects_reference.json.
"""
import csv
import json
import pathlib

import numpy as np
import statsmodels.api as sm

here = pathlib.Path(__file__).parent
rows = list(csv.DictReader(open(here / "../../../app-model/tests/data/treatment_study.csv")))
names = ["Age", "Sex = M", "Severity", "Smoker = Yes", "Region = North", "Region = South"]
X = np.array([[float(r["Age"]), r["Sex"] == "M", float(r["Severity"]), r["Smoker"] == "Yes",
               r["Region"] == "North", r["Region"] == "South"] for r in rows], dtype=float)
t = np.array([r["Programme"] == "Enrolled" for r in rows], dtype=float)
outcomes = {
    "continuous": np.array([float(r["Fitness gain"]) for r in rows]),
    "binary": np.array([float(r["Readmitted"]) for r in rows]),
    "count": np.array([float(r["Clinic visits"]) for r in rows]),
}
families = {"continuous": sm.families.Gaussian(), "binary": sm.families.Binomial(), "count": sm.families.Poisson()}
inv = {"continuous": lambda z: z, "binary": lambda z: 1 / (1 + np.exp(-z)), "count": np.exp}
n, p = X.shape
Xt = np.column_stack([np.ones(n), X])
q = p + 1

ps = sm.GLM(t, Xt, family=sm.families.Binomial()).fit(tol=1e-14)
beta = ps.params
e = ps.fittedvalues


def psi(theta, y, kind, method, estimand, keep):
    b = theta[:q]
    at = q
    ee = 1 / (1 + np.exp(-(Xt @ b)))
    cols = [Xt * (t - ee)[:, None]]
    p1 = p0 = None
    if method == "dr" and estimand == "ate":
        p1 = inv[kind](Xt @ theta[at:at + q]); at += q
        cols.append(Xt * (keep * t * (y - p1))[:, None])
    if method == "dr":
        p0 = inv[kind](Xt @ theta[at:at + q]); at += q
        cols.append(Xt * (keep * (1 - t) * (y - p0))[:, None])
    m1, m0 = theta[at], theta[at + 1]
    if method == "dr" and estimand == "ate":
        f1 = t * y / ee - (t - ee) * p1 / ee - m1
        f0 = (1 - t) * y / (1 - ee) + (t - ee) * p0 / (1 - ee) - m0
    elif method == "dr":
        f1 = t * (y - m1)
        f0 = t * (p0 - m0) + (1 - t) * ee / (1 - ee) * (y - p0)
    elif estimand == "ate":
        f1 = t * (y - m1) / ee
        f0 = (1 - t) * (y - m0) / (1 - ee)
    else:
        f1 = t * (y - m1)
        f0 = (1 - t) * ee / (1 - ee) * (y - m0)
    cols.append((keep * f1)[:, None])
    cols.append((keep * f0)[:, None])
    return np.hstack(cols)


def sandwich(theta, *args):
    d = len(theta)
    A = np.zeros((d, d))
    h = 1e-20
    for j in range(d):
        th = theta.astype(complex)
        th[j] += 1j * h
        A[:, j] = psi(th, *args).sum(0).imag / h
    P = psi(theta, *args)
    B = P.T @ P
    Ai = np.linalg.inv(A)
    return (Ai @ B @ Ai.T)[-2:, -2:]


def effects(m1, m0, V, kind):
    out = {"difference": [m1 - m0, float(np.sqrt(V[0, 0] + V[1, 1] - 2 * V[0, 1]))]}
    if kind != "continuous":
        g = np.array([1 / m1, -1 / m0])
        out["log_ratio"] = [float(np.log(m1 / m0)), float(np.sqrt(g @ V @ g))]
    if kind == "binary":
        g = np.array([1 / (m1 * (1 - m1)), -1 / (m0 * (1 - m0))])
        lo = lambda m: np.log(m / (1 - m))
        out["log_odds_ratio"] = [float(lo(m1) - lo(m0)), float(np.sqrt(g @ V @ g))]
    return out


def estimate(y, kind, method, estimand, trim):
    keep = ((e >= trim) & (e <= 1 - trim)).astype(float)
    theta = list(beta)
    if method == "ipw":
        w = keep * (t / e + (1 - t) / (1 - e) if estimand == "ate" else t + (1 - t) * e / (1 - e))
        m1 = np.sum(w * t * y) / np.sum(w * t)
        m0 = np.sum(w * (1 - t) * y) / np.sum(w * (1 - t))
    else:
        k = keep.astype(bool)
        fit = lambda g: sm.GLM(y[k & (t == g)], Xt[k & (t == g)], family=families[kind]).fit(tol=1e-14).params
        a0 = fit(0)
        p0 = inv[kind](Xt @ a0)
        if estimand == "ate":
            a1 = fit(1)
            p1 = inv[kind](Xt @ a1)
            m1 = np.sum(keep * (t * y / e - (t - e) * p1 / e)) / keep.sum()
            m0 = np.sum(keep * ((1 - t) * y / (1 - e) + (t - e) * p0 / (1 - e))) / keep.sum()
            theta += list(a1)
        else:
            m1 = np.sum(keep * t * y) / np.sum(keep * t)
            m0 = np.sum(keep * (t * p0 + (1 - t) * e / (1 - e) * (y - p0))) / np.sum(keep * t)
        theta += list(a0)
    theta = np.array(theta + [m1, m0])
    V = sandwich(theta, y, kind, method, estimand, keep)
    return {"kind": kind, "method": method, "estimand": estimand, "trim": trim, "m1": float(m1), "m0": float(m0),
            "cov": V.tolist(), "effects": effects(m1, m0, V, kind), "n_trimmed": int(n - keep.sum())}


cases = []
for kind, y in outcomes.items():
    for method in ["ipw", "dr"]:
        for estimand in ["ate", "att"]:
            cases.append(estimate(y, kind, method, estimand, 0.0))
cases.append(estimate(outcomes["continuous"], "continuous", "ipw", "ate", 0.1))
cases.append(estimate(outcomes["binary"], "binary", "dr", "ate", 0.05))

# Lunceford & Davidian (2004) eq. (9): IPW ATE with estimated scores.
y = outcomes["continuous"]
c = cases[0]
m1, m0 = c["m1"], c["m0"]
E = (Xt * (e * (1 - e))[:, None]).T @ Xt / n
H = (Xt * (t * (y - m1) * (1 - e) / e + (1 - t) * (y - m0) * e / (1 - e))[:, None]).mean(0)
I = t * (y - m1) / e - (1 - t) * (y - m0) / (1 - e) - (Xt * (t - e)[:, None]) @ np.linalg.solve(E, H)
ld_se = float(np.sqrt(np.sum(I ** 2)) / n)

# ATE matching with replacement (k = 1, 2), no caliper, on the logit.
d = np.log(e / (1 - e))
ate_matching = []
for k in [1, 2]:
    y = outcomes["continuous"]
    imputed = np.zeros(n)
    for i in range(n):
        others = np.where(t != t[i])[0]
        order = others[np.lexsort((others, np.abs(d[others] - d[i])))][:k]
        imputed[i] = y[order].mean()
    tau = np.mean(np.where(t == 1, y - imputed, imputed - y))
    ate_matching.append({"ratio": k, "ate": float(tau)})

# Balance (ATE denominators) with unstabilized ATE weights, and ATT.
w_ate = t / e + (1 - t) / (1 - e)
w_att = t + (1 - t) * e / (1 - e)


def wstats(v, w, g, binary):
    m = t == g
    sw, sw2 = w[m].sum(), (w[m] ** 2).sum()
    mean = np.sum(w[m] * v[m]) / sw
    var = mean * (1 - mean) if binary else sw / (sw ** 2 - sw2) * np.sum(w[m] * (v[m] - mean) ** 2)
    return mean, var


balance = []
for j, name in enumerate(names):
    v = X[:, j]
    binary = bool(np.all((v == 0) | (v == 1)))
    for estimand, w in [("ate", w_ate), ("att", w_att)]:
        u1, u0 = wstats(v, np.ones(n), 1, binary), wstats(v, np.ones(n), 0, binary)
        a1, a0 = wstats(v, w, 1, binary), wstats(v, w, 0, binary)
        sd = np.sqrt((u1[1] + u0[1]) / 2) if estimand == "ate" else np.sqrt(u1[1])
        balance.append({"name": name, "estimand": estimand, "smd": float((u1[0] - u0[0]) / sd),
                        "adjusted_smd": float((a1[0] - a0[0]) / sd),
                        "variance_ratio": None if binary else float(u1[1] / u0[1]),
                        "adjusted_variance_ratio": None if binary else float(a1[1] / a0[1])})

json.dump({"names": names, "x": X.tolist(), "t": t.tolist(), "y": {k: v.tolist() for k, v in outcomes.items()},
           "propensity": beta.tolist(), "cases": cases, "ld_ipw_ate_se": ld_se, "ate_matching": ate_matching,
           "balance": balance}, open(here / "treatment_effects_reference.json", "w"), indent=1)
print("cases", len(cases), "LD se", ld_se, "sandwich se", cases[0]["effects"]["difference"][1])
