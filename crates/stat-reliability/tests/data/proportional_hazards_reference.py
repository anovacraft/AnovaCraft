"""Independent reference values for proportional hazards (Cox) regression.

Run with:  python3 proportional_hazards_reference.py
(writes proportional_hazards_reference.json next to this file)

Sources of the reference values, all independent of the Rust code:

* statsmodels PHReg (Efron and Breslow ties, strata): coefficients, standard
  errors, log partial likelihoods at the estimate and at zero, score and
  information at zero (score test), and constrained refits with an offset for
  the profile-likelihood limits (brentq on 2(l_hat - l_p(b)) = chi2_1(0.95));
* lifelines CoxPHFitter (Efron ties only, strata): coefficients, standard
  errors, partial log-likelihood, Breslow baseline cumulative hazard at the
  covariate means, Schoenfeld, martingale and deviance residuals, and the
  Grambsch-Therneau per-covariate tests (time transforms km, rank, identity,
  log);
* a direct NumPy implementation written from the textbook formulas, which
  forms every risk set by masking (O(n^2), unlike the Rust accumulation): the
  Breslow baseline at the means, the predicted survival of a covariate row with
  the Tsiatis / Klein & Moeschberger (2003, sec. 8.8) variance of the
  cumulative hazard and log-transformed limits, Schoenfeld residuals with
  Efron- or Breslow-averaged risk-set means, martingale and deviance
  residuals, and the global Grambsch-Therneau (1994) test.

Data: the leukemia remission data (Freireich et al. 1963, Blood 21:699, with
the log white-cell counts and sex as tabulated by Kleinbaum & Klein, Survival
Analysis: A Self-Learning Text) and the Rossi et al. (1980) recidivism data, as
shipped with lifelines (lifelines.datasets).
"""
import json, math, os
import numpy as np
import pandas as pd
from scipy import stats, optimize
from statsmodels.duration.hazard_regression import PHReg
from lifelines import CoxPHFitter, KaplanMeierFitter
from lifelines.datasets import load_leukemia, load_rossi
from lifelines.statistics import proportional_hazard_test

LEVEL = 0.95
CRIT = stats.chi2.ppf(LEVEL, 1)
Z = stats.norm.ppf(0.5 + LEVEL / 2)


def tolist(a):
    return [float(v) for v in np.asarray(a, float).ravel()]


# ------------------------------------------------------------- direct NumPy ---
def risk_terms(t, e, xc, beta, strata, ties):
    """Per distinct failure time (stratum, time, deaths, s0, s1, xbar_efron) by masking."""
    r = np.exp(xc @ beta)
    out = []
    for s in np.unique(strata):
        ins = strata == s
        for tj in np.unique(t[ins & (e == 1)]):
            R = ins & (t >= tj)
            D = ins & (t == tj) & (e == 1)
            d = int(D.sum())
            s0 = r[R].sum(); s1 = (r[R, None] * xc[R]).sum(0)
            d0 = r[D].sum(); d1 = (r[D, None] * xc[D]).sum(0)
            if ties == 'efron':
                xbar = sum((s1 - l / d * d1) / (s0 - l / d * d0) for l in range(d)) / d
            else:
                xbar = s1 / s0
            out.append((s, tj, d, s0, s1, xbar, D))
    return out


def direct(t, e, x, strata, beta, cov, ties, x0, times0):
    means = x.mean(0)
    xc = x - means
    terms = risk_terms(t, e, xc, beta, strata, ties)
    res = {}
    # Breslow baseline at the means, per stratum.
    base = {}
    for (s, tj, d, s0, s1, _, _) in terms:
        base.setdefault(int(s), []).append((tj, d / s0, d / s0 ** 2, s1 / s0 * d / s0))
    res['baseline'] = []
    for s in sorted(base):
        steps = sorted(base[s], key=lambda q: q[0])
        res['baseline'].append({'times': [float(q[0]) for q in steps],
                                'cumulative_hazard': tolist(np.cumsum([q[1] for q in steps]))})
    # Predicted survival of x0 in stratum 0 at times0.
    z0 = np.asarray(x0, float) - means
    lp = float(z0 @ beta)
    steps = sorted(base[0], key=lambda q: q[0])
    pred = []
    for tt in times0:
        use = [q for q in steps if q[0] <= tt]
        H0 = sum(q[1] for q in use); q1 = sum(q[2] for q in use)
        a = sum((q[3] for q in use), np.zeros(len(beta)))
        q = z0 * H0 - a
        H = H0 * math.exp(lp)
        var = math.exp(2 * lp) * (q1 + q @ cov @ q)
        se = math.sqrt(var)
        f = math.exp(Z * se / H)
        pred.append({'time': tt, 'survival': math.exp(-H), 'std_error': se,
                     'lower': math.exp(-H * f), 'upper': math.exp(-H / f)})
    res['prediction'] = {'x': tolist(x0), 'points': pred}
    # Schoenfeld residuals (failure order: time, then data order).
    sch = []
    for (s, tj, d, s0, s1, xbar, D) in terms:
        for i in np.flatnonzero(D):
            sch.append((tj, i, xc[i] - xbar))
    sch.sort(key=lambda q: (q[0], q[1]))
    res['schoenfeld'] = [{'time': float(q[0]), 'unit': int(q[1]), 'residual': tolist(q[2])} for q in sch]
    # Martingale and deviance residuals.
    H_at = np.zeros(len(t))
    for i in range(len(t)):
        steps_s = base[int(strata[i])]
        H_at[i] = sum(q[1] for q in steps_s if q[0] <= t[i])
    m = e - H_at * np.exp(xc @ beta)
    dev = np.sign(m) * np.sqrt(-2 * (m + np.where(e == 1, e * np.log(np.where(e == 1, e - m, 1.0)), 0.0)))
    res['martingale'] = tolist(m)
    res['deviance'] = tolist(dev)
    # Grambsch-Therneau tests.
    times = np.array([q[0] for q in sch]); rr = np.array([q[2] for q in sch])
    km = KaplanMeierFitter().fit(t, e)
    gs = {'km': 1 - km.survival_function_at_times(times).values,
          'rank': stats.rankdata(times), 'identity': times, 'log': np.log(times)}
    D = e.sum()
    res['ph_tests'] = {}
    for key, g in gs.items():
        g = np.asarray(g, float)
        gc = g - g.mean()
        u = gc @ rr
        vu = cov @ u
        per = D * vu ** 2 / (np.diag(cov) * (gc ** 2).sum())
        glob = D * u @ cov @ u / (gc ** 2).sum()
        res['ph_tests'][key] = {'covariates': tolist(per), 'global': float(glob),
                                'global_p': float(stats.chi2.sf(glob, len(beta)))}
    return res


# ---------------------------------------------------------------- statsmodels ---
def sm_fit(t, e, x, strata, ties):
    kw = {'status': e, 'ties': ties}
    if strata is not None:
        kw['strata'] = strata
    mod = PHReg(t, x, **kw)
    fit = mod.fit(maxiter=500)
    beta = fit.params
    # Polish with Newton steps on the exact score and Hessian.
    for _ in range(20):
        g = mod.score(beta); h = mod.hessian(beta)
        beta = beta - np.linalg.solve(h, g)
    cov = np.linalg.inv(-mod.hessian(beta))
    ll = mod.loglike(beta)
    b0 = np.zeros(x.shape[1])
    ll0 = mod.loglike(b0)
    u0 = mod.score(b0); i0 = -mod.hessian(b0)
    out = {'coef': tolist(beta), 'se': tolist(np.sqrt(np.diag(cov))), 'loglik': float(ll), 'null_loglik': float(ll0),
           'lr': float(2 * (ll - ll0)), 'wald': float(beta @ np.linalg.solve(cov, beta)),
           'score': float(u0 @ np.linalg.solve(i0, u0))}
    # Profile-likelihood limits with offset refits.
    prof = []
    for j in range(x.shape[1]):
        others = [k for k in range(x.shape[1]) if k != j]

        def lp(b):
            if not others:
                return mod.loglike(np.array([b]))
            kw2 = dict(kw)
            kw2['offset'] = b * x[:, j]
            m2 = PHReg(t, x[:, others], **kw2)
            f2 = m2.fit(maxiter=500)
            bb = f2.params
            for _ in range(10):
                bb = bb - np.linalg.solve(m2.hessian(bb), m2.score(bb))
            return m2.loglike(bb)

        g = lambda b: 2 * (ll - lp(b)) - CRIT
        se = math.sqrt(cov[j, j])
        lo_br = beta[j] - se
        while g(lo_br) < 0:
            lo_br -= se
        hi_br = beta[j] + se
        while g(hi_br) < 0:
            hi_br += se
        prof.append([optimize.brentq(g, lo_br, beta[j], xtol=1e-12), optimize.brentq(g, beta[j], hi_br, xtol=1e-12)])
    out['profile'] = prof
    return out, beta, cov


# ------------------------------------------------------------------ lifelines ---
def ll_fit(df, time, event, covs, strata):
    cph = CoxPHFitter()
    cols = [time, event] + covs + ([strata] if strata else [])
    cph.fit(df[cols], time, event, strata=[strata] if strata else None)
    out = {'coef': tolist(cph.params_[covs]), 'se': tolist(cph.standard_errors_[covs]),
           'loglik': float(cph.log_likelihood_)}
    if strata is None:
        bch = cph.baseline_cumulative_hazard_
        out['baseline'] = {'times': tolist(bch.index.values), 'cumulative_hazard': tolist(bch.iloc[:, 0].values)}
    sch = cph.compute_residuals(df[cols], 'schoenfeld')
    out['schoenfeld'] = [{'unit': int(i), 'residual': tolist(row[covs].values)} for i, row in sch.iterrows()]
    mart = cph.compute_residuals(df[cols], 'martingale').sort_index()
    dev = cph.compute_residuals(df[cols], 'deviance').sort_index()
    out['martingale'] = tolist(mart['martingale'].values)
    out['deviance'] = tolist(dev['deviance'].values)
    if strata is None:
        out['ph_tests'] = {}
        for tt in ['km', 'rank', 'identity', 'log']:
            s = proportional_hazard_test(cph, df[cols], time_transform=tt).summary
            out['ph_tests'][tt] = tolist(s.loc[covs, 'test_statistic'].values)
    return out


def case(df, time, event, covs, strata, x0, times0):
    t = df[time].values.astype(float)
    e = df[event].values.astype(int)
    x = df[covs].values.astype(float)
    sv = None
    if strata:
        levels = sorted(df[strata].unique())
        sv = np.array([levels.index(v) for v in df[strata]])
    out = {'time': tolist(t), 'event': [int(v) for v in e], 'x': [tolist(r) for r in x], 'names': covs,
           'strata': None if sv is None else [int(v) for v in sv]}
    for ties in ['efron', 'breslow']:
        sm, beta, cov = sm_fit(t, e, x, sv, ties)
        sm['direct'] = direct(t, e, x, sv if sv is not None else np.zeros(len(t), int), beta, cov, ties, x0, times0)
        out[ties] = sm
    out['lifelines_efron'] = ll_fit(df, time, event, covs, strata)
    return out


def main():
    leuk = load_leukemia()
    rossi = load_rossi()
    cases = {
        'leukemia': case(leuk, 't', 'status', ['sex', 'logWBC', 'Rx'], None, [1.0, 2.5, 1.0], [5.0, 10.0, 22.5]),
        'rossi_strata': case(rossi, 'week', 'arrest', ['fin', 'age', 'prio'], 'wexp', [1.0, 25.0, 2.0],
                             [10.0, 30.0, 52.0]),
    }
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, 'proportional_hazards_reference.json'), 'w') as f:
        json.dump(cases, f, indent=1)


if __name__ == '__main__':
    main()
