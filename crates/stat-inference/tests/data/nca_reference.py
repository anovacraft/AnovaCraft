"""Reference values for non-compartmental analysis (tests/nca_reference.rs).

Run from the repository root:

    python3 crates/stat-inference/tests/data/nca_reference.py

It writes nca_reference.json beside this script. Requires NumPy.

Contents:

* ``profiles``: an invented data set ("PK profiles") of 12 subjects x 2
  formulations (R = reference, T = test) after a 100 mg oral dose, sampled at
  0, 0.25, 0.5, 1, 1.5, 2, 3, 4, 6, 8, 12 and 24 h. Each profile is a
  one-compartment model with first-order absorption,
  C(t) = F D ka / (V (ka - ke)) (exp(-ke t) - exp(-ka t)),
  with subject-level log-normal variation of ka, ke, V and F, 8 %
  multiplicative assay noise, values rounded to 3 decimals, and values below
  the quantification limit 0.05 mg/L reported as 0. The same rows are the
  app's sample data.
* ``results``: for every profile and both area methods, the parameters from an
  independent NumPy implementation written from the textbook formulas
  (Gibaldi & Perrier 1982, ch. 11; Gabrielsson & Weiner 2016, ch. 2): linear
  and log trapezoids, the terminal slope by numpy.polyfit on ln C with the
  "adjusted R-squared within 1e-4, then most points" rule over the last
  k >= 3 positive concentrations after Cmax, and the extrapolations.
* ``hand``: a hand-worked example whose values have closed forms (stated with
  the derivation below), independent of any implementation.

PKNCA (R) was not available in the build environment (not packaged for the
distribution, and CRAN unreachable), so no PKNCA values are included.
"""

import json
import math
import os

import numpy as np

TIMES = [0, 0.25, 0.5, 1, 1.5, 2, 3, 4, 6, 8, 12, 24]
DOSE = 100.0
LLOQ = 0.05
TOL = 1e-4


def simulate():
    rng = np.random.default_rng(20261019)
    rows = []
    for subject in range(1, 13):
        # Subject-level parameters shared by both formulations.
        ke = 0.17 * math.exp(rng.normal(0, 0.25))  # 1/h, t1/2 about 4 h
        v = 30.0 * math.exp(rng.normal(0, 0.2))  # L
        for form in ("R", "T"):
            ka = (1.6 if form == "R" else 1.2) * math.exp(rng.normal(0, 0.3))
            f = (0.85 if form == "R" else 0.80) * math.exp(rng.normal(0, 0.1))
            if abs(ka - ke) < 1e-3:
                ka += 0.05
            for t in TIMES:
                c = f * DOSE * ka / (v * (ka - ke)) * (math.exp(-ke * t) - math.exp(-ka * t))
                c *= math.exp(rng.normal(0, 0.08)) if t > 0 else 1.0
                c = round(c, 3)
                if c < LLOQ:
                    c = 0.0
                rows.append({"subject": subject, "formulation": form, "time": t, "conc": c})
    return rows


def segment(t1, c1, t2, c2, log_down):
    dt = t2 - t1
    if log_down and c1 > c2 > 0:
        # C(t) = c1 exp(-k (t - t1)); integrate C and t C exactly.
        k = (math.log(c1) - math.log(c2)) / dt
        auc = (c1 - c2) / k
        aumc = (t1 * c1 - t2 * c2) / k + (c1 - c2) / k ** 2
        return auc, aumc
    return dt * (c1 + c2) / 2, dt * (t1 * c1 + t2 * c2) / 2


def terminal(t, c):
    """Best-fit terminal phase from the points after Cmax."""
    imax = int(np.argmax(c))
    tt = np.array([x for x, y in zip(t[imax + 1:], c[imax + 1:]) if y > 0])
    cc = np.array([y for y in c[imax + 1:] if y > 0])
    fits = []
    for k in range(3, len(tt) + 1):
        x, y = tt[-k:], np.log(cc[-k:])
        slope, intercept = np.polyfit(x, y, 1)
        if slope >= 0:
            continue
        r2 = np.corrcoef(x, y)[0, 1] ** 2
        adj = 1 - (1 - r2) * (k - 1) / (k - 2)
        fits.append(dict(lambda_z=-slope, intercept=intercept, n=k, r2=r2, adj_r2=adj, start=x[0], end=x[-1]))
    if not fits:
        return None
    best = max(f["adj_r2"] for f in fits)
    near = [f for f in fits if f["adj_r2"] >= best - TOL]
    return max(near, key=lambda f: f["n"])


def nca(t, c, log_down, dose):
    t, c = list(t), list(c)
    imax = int(np.argmax(c))
    last = max(i for i, y in enumerate(c) if y > 0)
    tl, cl = t[last], c[last]
    ct, cc = t[: last + 1], c[: last + 1]
    if ct[0] > 0:
        ct, cc = [0.0] + ct, [0.0] + cc
    auc = aumc = 0.0
    for i in range(len(ct) - 1):
        a, m = segment(ct[i], cc[i], ct[i + 1], cc[i + 1], log_down)
        auc += a
        aumc += m
    out = dict(cmax=c[imax], tmax=t[imax], clast=cl, tlast=tl, auc_last=auc, aumc_last=aumc)
    fit = terminal(t, c)
    if fit is None:
        out["lambda_z"] = None
        return out
    lz = fit["lambda_z"]
    auc_inf = auc + cl / lz
    aumc_inf = aumc + tl * cl / lz + cl / lz ** 2
    out.update(
        lambda_z=lz,
        intercept=fit["intercept"],
        n_lambda=fit["n"],
        r_squared=fit["r2"],
        adj_r_squared=fit["adj_r2"],
        lambda_start=fit["start"],
        lambda_end=fit["end"],
        half_life=math.log(2) / lz,
        auc_inf=auc_inf,
        auc_pct_extrapolated=100 * (cl / lz) / auc_inf,
        aumc_inf=aumc_inf,
        mrt=aumc_inf / auc_inf,
        clearance=dose / auc_inf,
        volume_z=dose / (lz * auc_inf),
    )
    return out


def geometric(values):
    v = np.asarray(values, float)
    lv = np.log(v[v > 0])
    s2 = np.var(lv, ddof=1)
    return dict(
        geometric_mean=float(np.exp(lv.mean())),
        geometric_cv=float(math.sqrt(math.expm1(s2))),
        arithmetic_mean=float(v.mean()),
        sd=float(np.std(v, ddof=1)),
        median=float(np.median(v)),
        min=float(v.min()),
        max=float(v.max()),
    )


def hand():
    """Extravascular profile t = 0, 1, 2, 4, 6 h; C = 0, 4, 2, 1, 0.5 mg/L.

    After Cmax (4 at 1 h) the points (2, 2), (4, 1), (6, 0.5) halve every 2 h,
    so lambda_z = ln 2 / 2 exactly with R^2 = 1 and t1/2 = 2 h.
    Linear trapezoid: AUClast = 2 + 3 + 3 + 1.5 = 9.5;
    AUMClast = (0 + 4)/2 + (4 + 4)/2 + 2 (4 + 4)/2 + 2 (4 + 3)/2 = 2 + 4 + 8 + 7 = 21.
    Linear-up/log-down: AUClast = 2 + (4 - 2)/ln 2 + 2 (2 - 1)/ln 2
    + 2 (1 - 0.5)/ln 2 = 2 + 5/ln 2. Extrapolated area Clast/lambda_z =
    0.5 / (ln 2 / 2) = 1/ln 2, so AUCinf = 2 + 6/ln 2 (log down) and
    9.5 + 1/ln 2 (linear).
    """
    ln2 = math.log(2)
    return dict(
        times=[0, 1, 2, 4, 6],
        conc=[0, 4, 2, 1, 0.5],
        lambda_z=ln2 / 2,
        half_life=2.0,
        linear=dict(auc_last=9.5, aumc_last=21.0, auc_inf=9.5 + 1 / ln2),
        log_down=dict(auc_last=2 + 5 / ln2, auc_inf=2 + 6 / ln2),
    )


def main():
    rows = simulate()
    results = []
    by_param = {}
    for subject in range(1, 13):
        for form in ("R", "T"):
            prof = [r for r in rows if r["subject"] == subject and r["formulation"] == form]
            t = [r["time"] for r in prof]
            c = [r["conc"] for r in prof]
            for method, log_down in (("linear", False), ("linear-up-log-down", True)):
                res = nca(t, c, log_down, DOSE)
                res.update(subject=subject, formulation=form, method=method)
                results.append(res)
                if log_down and res["lambda_z"] is not None:
                    by_param.setdefault((form, "auc_inf"), []).append(res["auc_inf"])
                    by_param.setdefault((form, "cmax"), []).append(res["cmax"])
    summaries = [dict(formulation=f, parameter=p, values=v, **geometric(v)) for (f, p), v in sorted(by_param.items())]
    out = dict(dose=DOSE, lloq=LLOQ, profiles=rows, results=results, summaries=summaries, hand=hand(),
               numpy=np.__version__)
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "nca_reference.json")
    with open(path, "w") as fh:
        json.dump(out, fh, indent=1, default=float)
    n_lambda = sum(1 for r in results if r["lambda_z"] is not None)
    zeros = sum(1 for r in rows if r["conc"] == 0 and r["time"] > 0)
    print(f"wrote {path}: {len(results)} results, {n_lambda} with lambda_z, {zeros} BLQ zeros after 0 h")


if __name__ == "__main__":
    main()
