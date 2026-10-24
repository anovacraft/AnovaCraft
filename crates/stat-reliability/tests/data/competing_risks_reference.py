"""Independent reference values for the competing-risks routines.

Run with:  python3 -I competing_risks_reference.py   (writes competing_risks_reference.json)

Written from the published definitions, independently of the Rust code:

* Aalen-Johansen cumulative incidence, its delta-method (Greenwood-type)
  variance and log(-log) limits: lifelines 0.30 AalenJohansenFitter (the data
  have no tied failure times across causes, so lifelines does not jitter them).
* Gray's (1988) K-sample test: the scores z_k = sum_t [d1_k - R_k d1. / R.] are
  computed directly from the counts; their covariance is J V J^T, where J is
  the Jacobian of z with respect to every group's cause-specific hazard
  increments (d1/Y, d2/Y) at every time, with the weights R_k held at their
  estimates (Gray 1988, sec. 2), found by complex-step differentiation of the
  plain product-limit formulas, and V is the multinomial covariance of the
  increments.  This is a different algorithm from the Rust code's closed-form
  backward recursion.
* Fine-Gray (1999) regression: beta solves the weighted score equation
  U(beta) = 0 (scipy.optimize.root); the information is -dU/dbeta by complex
  step; each unit's influence on the score is dU/df_i, the complex-step
  derivative with respect to its frequency (which re-estimates the censoring
  Kaplan-Meier, so it includes Fine & Gray's censoring term psi_i); the robust
  covariance is I^-1 (sum f_i IF_i IF_i^T) I^-1.  With no censoring the fit is
  cross-checked against lifelines CoxPHFitter(robust=True) on the
  subdistribution risk set (competing failures kept at risk past the end).
* Failure modes of a series system: a Weibull per mode fitted by maximizing
  the censored likelihood (scipy.optimize), covariance from a central
  difference Hessian, F_sys = 1 - prod(1 - F_j) and its delta-method standard
  error (Meeker & Escobar 1998, ch. 15).

All datasets are invented for the tests (seeded NumPy draws).
"""
import json
import math
import warnings

import numpy as np
import pandas as pd
from scipy import optimize, stats
from lifelines import AalenJohansenFitter, CoxPHFitter

warnings.filterwarnings("ignore")
LEVEL = 0.95
Z = stats.norm.ppf(0.5 + LEVEL / 2)


def simulate(seed, n, rates, censor_rate, groups=1, group_effect=None, decimals=3):
    """Exponential latent times per cause; cause index 0.. or -1 censored."""
    rng = np.random.default_rng(seed)
    rows = []
    for g in range(groups):
        mult = 1.0 if group_effect is None else group_effect[g]
        for _ in range(n):
            lat = [rng.exponential(1 / (r * (mult if j == 0 else 1.0))) for j, r in enumerate(rates)]
            c = rng.exponential(1 / censor_rate) if censor_rate > 0 else math.inf
            t = min(min(lat), c)
            cause = -1 if t == c else int(np.argmin(lat))
            rows.append([round(t, decimals), cause, g])
    return rows


def no_cross_ties(rows):
    seen = {}
    for t, c, _ in rows:
        if c < 0:
            continue
        if t in seen and seen[t] != c:
            return False
        seen[t] = c
    return True


# ------------------------------------------------------------ Aalen-Johansen
def aj_case(rows, weights, n_causes):
    t = np.array([r[0] for r in rows], float)
    e = np.array([r[1] + 1 for r in rows], int)  # lifelines: 0 censored, 1.. causes
    w = np.array(weights, float)
    out = {"times": [], "incidence": [], "variance": [], "lower": [], "upper": []}
    per = []
    for j in range(n_causes):
        f = AalenJohansenFitter(calculate_variance=True, seed=0)
        f.fit(t, e, event_of_interest=j + 1, weights=w)
        per.append(f)
    # Report at every distinct failure time (any cause).
    times = sorted(set(t[e > 0].tolist()))
    for s in times:
        out["times"].append(s)
        inc, var, lo, hi = [], [], [], []
        for j, f in enumerate(per):
            cif = f.cumulative_density_.loc[:s].iloc[-1, 0]
            v = f.variance_.loc[:s].iloc[-1]
            ci = f.confidence_interval_.loc[:s].iloc[-1].values
            inc.append(float(cif))
            var.append(float(v))
            lo.append(float(min(ci)) if cif > 0 else 0.0)
            hi.append(float(max(ci)) if cif > 0 else 0.0)
        out["incidence"].append(inc)
        out["variance"].append(var)
        out["lower"].append(lo)
        out["upper"].append(hi)
    return out


# ------------------------------------------------------------ Gray's test
def gray(rows, weights, cause, n_groups):
    t = np.array([r[0] for r in rows], float)
    c = np.array([r[1] for r in rows], int)
    g = np.array([r[2] for r in rows], int)
    w = np.array(weights, float)
    times = np.unique(t[c >= 0])
    m = len(times)
    Y = np.zeros((n_groups, m))
    D1 = np.zeros((n_groups, m))
    D2 = np.zeros((n_groups, m))
    for k in range(n_groups):
        sel = g == k
        for i, s in enumerate(times):
            Y[k, i] = w[sel & (t >= s)].sum()
            D1[k, i] = w[sel & (t == s) & (c == cause)].sum()
            D2[k, i] = w[sel & (t == s) & (c >= 0) & (c != cause)].sum()

    def paths(h1, h2):
        # S(t-), F1(t-) from hazard increments (works with complex numbers).
        s_b = np.ones(m, dtype=h1.dtype)
        f_b = np.zeros(m, dtype=h1.dtype)
        s, f = 1.0 + 0j * h1[0], 0.0 + 0j * h1[0]
        for i in range(m):
            s_b[i], f_b[i] = s, f
            f = f + s * h1[i]
            s = s * (1 - h1[i] - h2[i])
        return s_b, f_b

    with np.errstate(divide="ignore", invalid="ignore"):
        H1 = np.where(Y > 0, D1 / Y, 0.0)
        H2 = np.where(Y > 0, D2 / Y, 0.0)
    R = np.zeros((n_groups, m))
    for k in range(n_groups):
        s_b, f_b = paths(H1[k].astype(complex), H2[k].astype(complex))
        with np.errstate(divide="ignore", invalid="ignore"):
            R[k] = np.where((Y[k] > 0) & (s_b.real > 0), Y[k] * (1 - f_b.real) / s_b.real, 0.0)
    Rt = R.sum(0)
    D1t = D1.sum(0)
    ok = Rt > 0
    z = np.array([(D1[k] - np.where(ok, R[k] * D1t / np.where(ok, Rt, 1), 0)).sum() for k in range(n_groups)])
    # z_a (fixed weights) = sum_l sum_t (delta_al - R_a/R.) R_l dGamma_l(t), with
    # dGamma_l = S_l(t-) h1_l(t) / (1 - F1_l(t-)).
    share = np.where(ok, R / np.where(ok, Rt, 1), 0.0)
    q = n_groups - 1

    def zfun(l, h1, h2, a):
        s_b, f_b = paths(h1, h2)
        dgam = s_b * h1 / (1 - f_b)
        wgt = ((1.0 if a == l else 0.0) - share[a]) * R[l]
        wgt = np.where(ok, wgt, 0.0)
        return (wgt * dgam).sum()

    cov = np.zeros((q, q))
    hstep = 1e-30
    for l in range(n_groups):
        # Jacobian rows: d z_a / d h1_l(t), d z_a / d h2_l(t).
        J1 = np.zeros((q, m))
        J2 = np.zeros((q, m))
        for i in range(m):
            if Y[l, i] <= 0:
                continue
            for a in range(q):
                h1 = H1[l].astype(complex)
                h1[i] += 1j * hstep
                J1[a, i] = zfun(l, h1, H2[l].astype(complex), a).imag / hstep
                h2 = H2[l].astype(complex)
                h2[i] += 1j * hstep
                J2[a, i] = zfun(l, H1[l].astype(complex), h2, a).imag / hstep
        for i in range(m):
            y = Y[l, i]
            if y <= 0:
                continue
            p1, p2 = H1[l, i], H2[l, i]
            # Multinomial covariance of the increments d/Y.
            v11 = p1 * (1 - p1) / y
            v22 = p2 * (1 - p2) / y
            v12 = -p1 * p2 / y
            for a in range(q):
                for b in range(q):
                    cov[a, b] += (J1[a, i] * J1[b, i] * v11 + J2[a, i] * J2[b, i] * v22
                                  + (J1[a, i] * J2[b, i] + J2[a, i] * J1[b, i]) * v12)
    stat = float(z[:q] @ np.linalg.solve(cov, z[:q]))
    return {"scores": z.tolist(), "covariance": cov.tolist(), "statistic": stat, "df": q,
            "p_value": float(stats.chi2.sf(stat, q))}


# ------------------------------------------------------------ Fine-Gray
def fg_score(beta, f, t, c, X, cause):
    """U(beta) for frequencies f (complex allowed in beta or f)."""
    dtype = complex if (np.iscomplexobj(beta) or np.iscomplexobj(f)) else float
    n = len(t)
    # Censoring Kaplan-Meier G(s-) from the frequencies (at risk: time >= u).
    cens_times = np.unique(t[c < 0])
    def g_before(s):
        val = 1.0 + 0j if dtype is complex else 1.0
        for u in cens_times:
            if u >= s:
                break
            y = f[t >= u].sum()
            d = f[(t == u) & (c < 0)].sum()
            val = val * (1 - d / y)
        return val
    ev_times = np.unique(t[c == cause])
    g_unit = np.array([g_before(s) for s in t], dtype=dtype)
    eta = np.exp(X @ beta)
    U = np.zeros(X.shape[1], dtype=dtype)
    comp = (c >= 0) & (c != cause)
    for s in ev_times:
        gs = g_before(s)
        r = np.where(t >= s, 1.0, 0.0).astype(dtype)
        cm = comp & (t < s)
        r[cm] = gs / g_unit[cm]
        wexp = f * r * eta
        s0 = wexp.sum()
        s1 = (wexp[:, None] * X).sum(0)
        dmask = (t == s) & (c == cause)
        d = f[dmask].sum()
        U = U + (f[dmask][:, None] * X[dmask]).sum(0) - d * s1 / s0
    return U


def fg_baseline(beta, f, t, c, X, cause):
    cens_times = np.unique(t[c < 0])
    def g_before(s):
        val = 1.0
        for u in cens_times:
            if u >= s:
                break
            val *= 1 - f[(t == u) & (c < 0)].sum() / f[t >= u].sum()
        return val
    comp = (c >= 0) & (c != cause)
    g_unit = np.array([g_before(s) for s in t])
    eta = np.exp(X @ beta)
    out = []
    cum = 0.0
    for s in np.unique(t[c == cause]):
        r = np.where(t >= s, 1.0, 0.0)
        cm = comp & (t < s)
        r[cm] = g_before(s) / g_unit[cm]
        cum += f[(t == s) & (c == cause)].sum() / (f * r * eta).sum()
        out.append([float(s), float(cum)])
    return out


def fine_gray_ref(rows, X, weights, cause):
    t = np.array([r[0] for r in rows], float)
    c = np.array([r[1] for r in rows], int)
    X = np.array(X, float)
    f = np.array(weights, float)
    p = X.shape[1]
    sol = optimize.root(lambda b: fg_score(b, f, t, c, X, cause), np.zeros(p), method="hybr", tol=1e-14)
    beta = sol.x
    h = 1e-30
    info = np.zeros((p, p))
    for a in range(p):
        b = beta.astype(complex)
        b[a] += 1j * h
        info[:, a] = -fg_score(b, f, t, c, X, cause).imag / h
    info = 0.5 * (info + info.T)
    IF = np.zeros((len(t), p))
    for i in range(len(t)):
        ff = f.astype(complex)
        ff[i] += 1j * h
        IF[i] = fg_score(beta.astype(complex), ff, t, c, X, cause).imag / h
    meat = (f[:, None, None] * IF[:, :, None] * IF[:, None, :]).sum(0)
    ainv = np.linalg.inv(info)
    robust = ainv @ meat @ ainv
    return {"beta": beta.tolist(), "robust_se": np.sqrt(np.diag(robust)).tolist(),
            "naive_se": np.sqrt(np.diag(ainv)).tolist(), "baseline": fg_baseline(beta, f, t, c, X, cause)}


def cox_on_subdistribution(rows, X, cause):
    """No censoring: Cox on the subdistribution risk set (lifelines, robust)."""
    t = np.array([r[0] for r in rows], float)
    c = np.array([r[1] for r in rows], int)
    end = t.max() + 1.0
    time = np.where((c >= 0) & (c != cause), end, t)
    event = (c == cause).astype(int)
    df = pd.DataFrame(np.array(X, float), columns=[f"x{i}" for i in range(len(X[0]))])
    df["T"] = time
    df["E"] = event
    cph = CoxPHFitter()
    cph.fit(df, "T", "E", robust=True)
    return {"beta": cph.params_.values.tolist(), "robust_se": cph.standard_errors_.values.tolist()}


# ------------------------------------------------------------ series system
def weibull_fit(t, failed, w):
    def nll(th):
        mu, ls = th
        sig = math.exp(ls)
        y = (np.log(t) - mu) / sig
        ll = np.where(failed, y - np.exp(y) - np.log(sig) - np.log(t), -np.exp(y))
        return -(w * ll).sum()
    best = None
    for start in ([np.log(t).mean() + 1, 0.0], [np.log(t).mean() + 2, -0.5], [np.log(t).mean(), 0.5]):
        r = optimize.minimize(nll, start, method="Nelder-Mead", options={"xatol": 1e-12, "fatol": 1e-14, "maxiter": 20000})
        r = optimize.minimize(nll, r.x, method="BFGS", options={"gtol": 1e-11})
        if best is None or r.fun < best.fun:
            best = r
    th = best.x
    hstep = 1e-4
    H = np.zeros((2, 2))
    for i in range(2):
        for j in range(2):
            ei = np.eye(2)[i] * hstep
            ej = np.eye(2)[j] * hstep
            H[i, j] = (nll(th + ei + ej) - nll(th + ei - ej) - nll(th - ei + ej) + nll(th - ei - ej)) / (4 * hstep * hstep)
    return th, np.linalg.inv(H)


def series(rows, weights, n_modes, eval_times):
    t = np.array([r[0] for r in rows], float)
    c = np.array([r[1] for r in rows], int)
    w = np.array(weights, float)
    fits = [weibull_fit(t, c == j, w) for j in range(n_modes)]
    def F(th, s):
        mu, ls = th
        return 1 - math.exp(-math.exp((math.log(s) - mu) / math.exp(ls)))
    def grad(th, s):
        e = 1e-6
        return np.array([(F(th + np.eye(2)[i] * e, s) - F(th - np.eye(2)[i] * e, s)) / (2 * e) for i in range(2)])
    out = {"modes": [], "times": eval_times, "system": [], "system_se": []}
    for th, V in fits:
        out["modes"].append({"shape": float(math.exp(-th[1])), "scale": float(math.exp(th[0]))})
    for s in eval_times:
        fs = [F(th, s) for th, _ in fits]
        sys_f = 1 - np.prod([1 - x for x in fs])
        var = 0.0
        for j, (th, V) in enumerate(fits):
            gj = grad(th, s)
            others = np.prod([1 - fs[k] for k in range(n_modes) if k != j])
            var += others ** 2 * float(gj @ V @ gj)
        out["system"].append(float(sys_f))
        out["system_se"].append(math.sqrt(var))
    return out


def main():
    cases = {}
    # Aalen-Johansen: three causes with censoring; then the same data with frequencies.
    rows = simulate(11, 45, [0.08, 0.05, 0.03], 0.04)
    assert no_cross_ties(rows)
    # End on a censored unit so the last at-risk set is not emptied by failures.
    rows.append([round(max(r[0] for r in rows) + 1.0, 3), -1, 0])
    cases["aj"] = {"rows": rows, "weights": [1] * len(rows), "n_causes": 3,
                   "result": aj_case(rows, [1] * len(rows), 3)}
    wts = [1 + (i % 3) for i in range(len(rows))]
    cases["aj_weighted"] = {"rows": rows, "weights": wts, "n_causes": 3, "result": aj_case(rows, wts, 3)}

    # Gray's test: two groups (a real difference in cause 0), three groups with ties and frequencies.
    rows2 = simulate(23, 50, [0.10, 0.06], 0.05, groups=2, group_effect=[1.0, 1.8], decimals=2)
    cases["gray2"] = {"rows": rows2, "weights": [1] * len(rows2),
                      "tests": [gray(rows2, [1] * len(rows2), k, 2) for k in range(2)]}
    rows3 = simulate(37, 30, [0.10, 0.06], 0.05, groups=3, group_effect=[1.0, 1.3, 0.7], decimals=1)
    w3 = [1 + (i % 2) for i in range(len(rows3))]
    cases["gray3"] = {"rows": rows3, "weights": w3, "tests": [gray(rows3, w3, k, 3) for k in range(2)]}

    # Fine-Gray with censoring: a continuous and a binary covariate.
    rng = np.random.default_rng(5)
    rows4 = simulate(41, 60, [0.10, 0.07], 0.05, decimals=4)
    X4 = [[float(round(rng.normal(), 3)), float(i % 2)] for i in range(len(rows4))]
    w4 = [1 + (i % 3 == 0) for i in range(len(rows4))]
    cases["fine_gray"] = {"rows": rows4, "x": X4, "weights": w4, "cause": 0,
                          "result": fine_gray_ref(rows4, X4, w4, 0)}
    # No censoring: also Cox on the subdistribution risk set.
    rows5 = simulate(43, 50, [0.10, 0.07], 0.0, decimals=5)
    X5 = [[float(round(rng.normal(), 3))] for _ in rows5]
    cases["fine_gray_uncensored"] = {"rows": rows5, "x": X5, "weights": [1] * len(rows5), "cause": 0,
                                     "result": fine_gray_ref(rows5, X5, [1] * len(rows5), 0),
                                     "cox": cox_on_subdistribution(rows5, X5, 0)}

    # Series system of two Weibull modes.
    rng = np.random.default_rng(77)
    rows6 = []
    for _ in range(40):
        a = 1000 * rng.weibull(2.5)
        b = 1500 * rng.weibull(1.2)
        cens = 1400.0
        tt = min(a, b, cens)
        rows6.append([round(float(tt), 2), -1 if tt == cens else (0 if tt == a else 1), 0])
    cases["series"] = {"rows": rows6, "weights": [1] * len(rows6),
                       "result": series(rows6, [1] * len(rows6), 2, [100.0, 300.0, 600.0, 1000.0])}

    with open("competing_risks_reference.json", "w") as fh:
        json.dump(cases, fh, indent=1)
    for k in ("gray2", "gray3"):
        print(k, [round(x["statistic"], 4) for x in cases[k]["tests"]])
    print("fine-gray", cases["fine_gray"]["result"]["beta"], cases["fine_gray"]["result"]["robust_se"])
    print("uncensored fg", cases["fine_gray_uncensored"]["result"], "cox", cases["fine_gray_uncensored"]["cox"])


if __name__ == "__main__":
    main()
