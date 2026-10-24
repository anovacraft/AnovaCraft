"""Independent reference values for parametric (accelerated failure time)
survival regression.

Run with:  python3 survival_regression_reference.py
(writes survival_regression_reference.json next to this file)

Sources, all independent of the Rust code:

* lifelines WeibullAFTFitter, LogNormalAFTFitter and LogLogisticAFTFitter:
  `fit` for right-censored data and `fit_interval_censoring` (with frequency
  weights) for inspection data -- coefficients, standard errors, scale and
  log-likelihood;
* an independent SciPy maximization of the censored log-likelihood written
  from the definitions (log T = x'b + sigma W with W smallest extreme value,
  normal or logistic; exact: log f(z) - log sigma - log t; right: log S(z);
  left: log F(z); interval: log(F(zu) - F(zl)), using scipy.stats
  distribution objects), by Nelder-Mead and BFGS polished with Newton steps on
  a numerical Hessian, for all four distributions (exponential: sigma = 1);
  standard errors from a 4th-order central-difference Hessian; profile
  likelihood limits by brentq; quantile and failure-probability predictions
  with delta-method Wald limits (Meeker & Escobar 1998, sec. 8.4, 17.3);
  likelihood-ratio tests of dropping a covariate; Cox-Snell residuals.

Data: the Rossi et al. (1980) recidivism data shipped with lifelines (right
censoring), an invented capacitor life test inspected at fixed times
(interval, left and right censoring with unit counts; this is also the
application's sample), and a small invented dataset mixing exact, left,
interval and right-censored times.
"""
import json
import sys
import time, math, os
import numpy as np
import pandas as pd
from scipy import stats, optimize
from lifelines import WeibullAFTFitter, LogNormalAFTFitter, LogLogisticAFTFitter
from lifelines.datasets import load_rossi

LEVEL = 0.95
CRIT = stats.chi2.ppf(LEVEL, 1)
Z = stats.norm.ppf(0.5 + LEVEL / 2)
INF = math.inf

STD = {
    'weibull': stats.gumbel_l,       # smallest extreme value
    'exponential': stats.gumbel_l,
    'lognormal': stats.norm,
    'loglogistic': stats.logistic,
}


def tolist(a):
    return [float(v) for v in np.asarray(a, float).ravel()]


# ------------------------------------------------------------------- data ---
def capacitor_test():
    """Invented accelerated life test: 3 temperatures x 2 lots x 20 units,
    Weibull lives (shape 2) with log scale 8.3 - 0.045 (T - 85) + 0.25 [lot B],
    inspected at fixed times; units still working at 2000 h are censored."""
    rng = np.random.default_rng(20261016)
    inspections = [0, 168, 336, 504, 672, 1000, 1500, 2000]
    rows = []
    for temp in [85, 105, 125]:
        for lot in ['A', 'B']:
            mu = 8.3 - 0.045 * (temp - 85) + (0.25 if lot == 'B' else 0.0)
            t = np.exp(mu + 0.5 * stats.gumbel_l.rvs(size=20, random_state=rng))
            counts = {}
            for v in t:
                if v > inspections[-1]:
                    key = (inspections[-1], None)
                else:
                    k = next(i for i in range(1, len(inspections)) if v <= inspections[i])
                    key = (inspections[k - 1], inspections[k])
                counts[key] = counts.get(key, 0) + 1
            for (a, b), n in sorted(counts.items(), key=lambda q: (q[0][0], q[0][1] or INF)):
                rows.append({'temperature': temp, 'lot': lot, 'start': a, 'end': b, 'units': n})
    return rows


def mixed_data():
    """Invented: exact, left, interval and right-censored times with two covariates."""
    rows = [
        # (lower, upper, x1, x2, weight); lower = -inf left, upper = inf right
        (55, 55, 0.0, 1.2, 1), (92, 92, 0.0, 0.4, 1), (140, 140, 1.0, 0.8, 1), (141, 141, 1.0, 1.9, 2),
        (60, 120, 0.0, 1.0, 1), (80, 150, 1.0, 0.3, 1), (100, 200, 0.0, 2.2, 3), (150, 250, 1.0, 1.4, 1),
        (-INF, 40, 1.0, 2.5, 2), (-INF, 70, 0.0, 1.7, 1), (180, INF, 0.0, 0.2, 3), (260, INF, 1.0, 0.1, 2),
        (90, INF, 1.0, 0.9, 1), (205, 205, 0.0, 0.6, 1), (210, 320, 0.0, 0.5, 2), (35, 35, 1.0, 2.8, 1),
        (120, 120, 1.0, 1.1, 1), (300, INF, 0.0, 0.7, 2),
    ]
    return rows


# ---------------------------------------------------------- SciPy likelihood ---
def loglik(theta, dist, lo, hi, X, w):
    D = STD[dist]
    k = X.shape[1]
    beta = np.asarray(theta[:k], float)
    sigma = 1.0 if dist == 'exponential' else math.exp(theta[k])
    mu = X @ beta
    with np.errstate(divide='ignore'):
        la = np.where(lo > 0, np.log(np.where(lo > 0, lo, 1.0)), -INF)
        lb = np.where(hi < INF, np.log(np.where(hi < INF, hi, 1.0)), INF)
    exact = lo == hi
    right = (~exact) & (hi == INF) & (la > -INF)
    left = (~exact) & (la == -INF) & (hi < INF)
    inter = (~exact) & (la > -INF) & (hi < INF)
    ll = 0.0
    z = (la[exact] - mu[exact]) / sigma
    ll += np.sum(w[exact] * (D.logpdf(z) - math.log(sigma) - la[exact]))
    ll += np.sum(w[right] * D.logsf((la[right] - mu[right]) / sigma))
    ll += np.sum(w[left] * D.logcdf((lb[left] - mu[left]) / sigma))
    zl = (la[inter] - mu[inter]) / sigma
    zu = (lb[inter] - mu[inter]) / sigma
    p = np.where(zl > 0, D.sf(zl) - D.sf(zu), D.cdf(zu) - D.cdf(zl))
    with np.errstate(divide='ignore', invalid='ignore'):
        ll += np.sum(w[inter] * np.log(p))
    return float(ll) if np.isfinite(ll) else -INF


def hessian(f, x, h=1e-4):
    """4th-order difference Hessian; `h` is one step or one per parameter."""
    hs = np.broadcast_to(np.asarray(h, float), (len(x),))
    n = len(x)
    H = np.zeros((n, n))
    for i in range(n):
        for j in range(i, n):
            def fe(di, dj):
                y = np.array(x, float)
                y[i] += di * hs[i]
                y[j] += dj * hs[j]
                return f(y)
            if i == j:
                v = (-fe(2, 0) + 16 * fe(1, 0) - 30 * f(np.array(x, float)) + 16 * fe(-1, 0) - fe(-2, 0)) / (12 * hs[i] * hs[i])
            else:
                v = (fe(1, 1) - fe(1, -1) - fe(-1, 1) + fe(-1, -1)) / (4 * hs[i] * hs[j])
            H[i, j] = H[j, i] = v
    return H


def gradient(f, x, h=1e-5):
    g = np.zeros(len(x))
    for i in range(len(x)):
        y1 = np.array(x, float); y1[i] += h
        y2 = np.array(x, float); y2[i] -= h
        y3 = np.array(x, float); y3[i] += 2 * h
        y4 = np.array(x, float); y4[i] -= 2 * h
        g[i] = (8 * (f(y1) - f(y2)) - (f(y3) - f(y4))) / (12 * h)
    return g


def maximize(f, x0, fixed=None, warm=False, scale=None):
    """Maximize f over theta with an optional (index, value) held fixed.
    `warm`: x0 is already close (a profile step), so go straight to Newton."""
    free = [i for i in range(len(x0)) if fixed is None or i != fixed[0]]

    def full(v):
        y = np.array(x0, float)
        y[free] = v
        if fixed is not None:
            y[fixed[0]] = fixed[1]
        return y

    g = lambda v: -f(full(v))
    v = np.array(x0, float)[free]
    if len(free) == 0:
        return full(v), f(full(v))
    if warm:
        res = optimize.OptimizeResult(x=v, fun=g(v))
    else:
        res = optimize.minimize(g, v, method='BFGS', options={'gtol': 1e-9, 'maxiter': 2000})
    stalled = not warm and (not res.success or np.max(np.abs(gradient(lambda u: -g(u), res.x))) > 1e-4)
    if stalled or not np.isfinite(res.fun) or np.max(np.abs(res.x - v)) > 50:
        if np.isfinite(res.fun) and np.max(np.abs(res.x - v)) <= 50:
            v = res.x
        for _ in range(2):
            v = optimize.minimize(g, v, method='Nelder-Mead',
                                  options={'xatol': 1e-10, 'fatol': 1e-12, 'maxiter': 40000, 'maxfev': 80000}).x
        res = optimize.minimize(g, v, method='BFGS', options={'gtol': 1e-9})
    v = res.x
    for _ in range(12 if warm else 6):
        ff = lambda u: f(full(u))
        H = hessian(ff, v, h=1e-3 * (np.ones(len(x0)) if scale is None else scale)[free])
        gr = gradient(ff, v)
        try:
            step = np.linalg.solve(H, gr)
        except np.linalg.LinAlgError:
            break
        t = 1.0
        while t > 1e-6 and ff(v - t * step) < ff(v) - 1e-12:
            t /= 2
        v = v - t * step
        if np.max(np.abs(t * step)) < 1e-11:
            break
    return full(v), f(full(v))


def root(g, a, b):
    """Root of g in [a, b]: Brent's method, or bisection if optimizer noise
    stops it from converging."""
    try:
        return optimize.brentq(g, a, b, xtol=1e-11, maxiter=200)
    except RuntimeError:
        pass
    ga = g(a)
    for _ in range(60):
        m = 0.5 * (a + b)
        gm = g(m)
        if (gm < 0) == (ga < 0):
            a, ga = m, gm
        else:
            b = m
        if abs(b - a) < 1e-10 * max(1.0, abs(m)):
            break
    return 0.5 * (a + b)


def scipy_fit(dist, lo, hi, X, w, x0_rows, probs, times, drop, profiles=False):
    started = time.time()
    try:
        return scipy_fit_inner(dist, lo, hi, X, w, x0_rows, probs, times, drop, profiles)
    finally:
        print(f'scipy {dist}: {time.time() - started:.1f} s', file=sys.stderr, flush=True)


def scipy_fit_inner(dist, lo, hi, X, w, x0_rows, probs, times, drop, profiles):
    k = X.shape[1]
    f = lambda th: loglik(th, dist, lo, hi, X, w)
    # Start: least squares on representative log times.
    y = np.array([0.5 * (math.log(a) + math.log(b)) if (a > 0 and b < INF) else (math.log(a) if a > 0 else math.log(b))
                  for a, b in zip(lo, hi)])
    b0 = np.linalg.lstsq(X, y, rcond=None)[0]
    start = list(b0) + ([] if dist == 'exponential' else [math.log(max(np.std(y - X @ b0), 0.2))])
    # Steps scaled to each predictor's size (temperatures near 100 need
    # coefficient steps 100 times smaller than a 0/1 lot indicator).
    scale = np.array([1.0 / max(1.0, float(np.sqrt(np.mean(X[:, c] ** 2)))) for c in range(k)]
                     + ([] if dist == 'exponential' else [1.0]))
    theta, ll = maximize(f, start, scale=scale)
    H = hessian(f, theta, h=2e-4 * scale)  # balances truncation and rounding of the 4th-order formula
    cov = np.linalg.inv(-H)
    se = np.sqrt(np.diag(cov))
    # Profile-likelihood limits are slow to compute this way; they are
    # checked on the right-censored Weibull and lognormal fits only.
    prof = [] if profiles else None
    for j in (range(len(theta)) if profiles else []):
        def gfun(c, j=j):
            # Warm start: the other parameters moved along their regression on
            # parameter j (from the covariance), which is close to the profile.
            start = np.array(theta) + cov[:, j] / cov[j, j] * (c - theta[j])
            return 2 * (ll - maximize(f, start, fixed=(j, c), warm=True, scale=scale)[1]) - CRIT
        a = theta[j] - se[j]
        while gfun(a) < 0:
            a -= se[j]
        b = theta[j] + se[j]
        while gfun(b) < 0:
            b += se[j]
        prof.append([root(gfun, a, theta[j]), root(gfun, theta[j], b)])
    D = STD[dist]
    sigma = 1.0 if dist == 'exponential' else math.exp(theta[k])
    preds = []
    for xr in x0_rows:
        xd = np.array([1.0] + list(xr))
        mu = xd @ theta[:k]
        qs = []
        for p in probs:
            wp = D.ppf(p)
            grad = list(xd) + ([] if dist == 'exponential' else [sigma * wp])
            grad = np.array(grad)
            s = math.sqrt(grad @ cov @ grad)
            lq = mu + sigma * wp
            qs.append({'p': p, 'estimate': math.exp(lq), 'lower': math.exp(lq - Z * s), 'upper': math.exp(lq + Z * s)})
        fs = []
        for t in times:
            zt = (math.log(t) - mu) / sigma
            grad = np.array(list(-xd / sigma) + ([] if dist == 'exponential' else [-zt]))
            s = math.sqrt(grad @ cov @ grad)
            fs.append({'t': t, 'estimate': D.cdf(zt), 'lower': D.cdf(zt - Z * s), 'upper': D.cdf(zt + Z * s)})
        preds.append({'x': list(xr), 'quantiles': qs, 'probabilities': fs})
    # Likelihood-ratio test of dropping covariate `drop` (column drop + 1 of X).
    keep = [c for c in range(k) if c != drop + 1]
    f2 = lambda th: loglik(th, dist, lo, hi, X[:, keep], w)
    start2 = [theta[c] for c in keep] + ([] if dist == 'exponential' else [theta[k]])
    _, ll2 = maximize(f2, start2, scale=np.concatenate([scale[keep], scale[k:]]))
    # Cox-Snell residual intervals.
    mu_all = X @ theta[:k]
    def r(t, m):
        if t <= 0:
            return 0.0
        if t == INF:
            return INF
        return -D.logsf((math.log(t) - m) / sigma)
    cs = [[r(a, m), r(b, m)] for a, b, m in zip(lo, hi, mu_all)]
    return {'theta': tolist(theta), 'se': tolist(se), 'loglik': float(ll), 'profile': prof, 'predictions': preds,
            'drop': {'covariate': drop, 'statistic': float(2 * (ll - ll2))},
            'cox_snell': [[c if c != INF else None for c in pair] for pair in cs]}


# ------------------------------------------------------------------ lifelines ---
FITTERS = {'weibull': WeibullAFTFitter, 'lognormal': LogNormalAFTFitter, 'loglogistic': LogLogisticAFTFitter}
PARAMS = {'weibull': ('lambda_', 'rho_'), 'lognormal': ('mu_', 'sigma_'), 'loglogistic': ('alpha_', 'beta_')}


def lifelines_fit(dist, df, covs, interval, weights=None):
    m = FITTERS[dist]()
    cols = covs + (['lower', 'upper'] if interval else ['time', 'event']) + ([weights] if weights else [])
    d = df[cols]
    if interval:
        m.fit_interval_censoring(d, 'lower', 'upper', weights_col=weights)
    else:
        m.fit(d, 'time', 'event', weights_col=weights)
    loc, sc = PARAMS[dist]
    coef = [m.params_[(loc, 'Intercept')]] + [m.params_[(loc, c)] for c in covs]
    se = [m.standard_errors_[(loc, 'Intercept')]] + [m.standard_errors_[(loc, c)] for c in covs]
    g = m.params_[(sc, 'Intercept')]
    log_sigma = g if dist == 'lognormal' else -g
    return {'coef': tolist(coef), 'se': tolist(se), 'log_sigma': float(log_sigma),
            'log_sigma_se': float(m.standard_errors_[(sc, 'Intercept')]), 'loglik': float(m.log_likelihood_)}


def main():
    out = {}
    # 1. Rossi, right censoring.
    rossi = load_rossi()
    covs = ['fin', 'age', 'prio']
    df = pd.DataFrame({'time': rossi['week'].astype(float), 'event': rossi['arrest'].astype(int)})
    for c in covs:
        df[c] = rossi[c].astype(float)
    lo = df['time'].values.astype(float)
    hi = np.where(df['event'].values == 1, lo, INF)
    X = np.column_stack([np.ones(len(df))] + [df[c].values for c in covs])
    w = np.ones(len(df))
    case = {'lower': tolist(lo), 'upper': [v if v != INF else None for v in hi], 'x': [tolist(r) for r in X[:, 1:]],
            'weight': tolist(w), 'names': covs, 'lifelines': {}, 'scipy': {}}
    for dist in FITTERS:
        case['lifelines'][dist] = lifelines_fit(dist, df, covs, False)
    for dist in STD:
        case['scipy'][dist] = scipy_fit(dist, lo, hi, X, w, [[1.0, 25.0, 2.0]], [0.1, 0.5], [20.0, 52.0], 0,
                                        profiles=dist in ('weibull', 'lognormal'))
    out['rossi'] = case
    print(f'rossi done', file=sys.stderr, flush=True)
    # 2. Capacitor inspections: interval, left (start 0) and right censoring, unit counts.
    rows = capacitor_test()
    lo = np.array([float(r['start']) for r in rows])
    hi = np.array([float(r['end']) if r['end'] is not None else INF for r in rows])
    lot = np.array([1.0 if r['lot'] == 'B' else 0.0 for r in rows])
    temp = np.array([float(r['temperature']) for r in rows])
    w = np.array([float(r['units']) for r in rows])
    X = np.column_stack([np.ones(len(rows)), temp, lot])
    # lifelines is given the temperature centred at 105 (its optimizer does not
    # start well from raw temperatures); the intercept is then mu at 105 C, lot A.
    # A first inspection interval starting at 0 is given to lifelines as starting at
    # 1e-6 h (its starting values take logs of the bounds); the survival there is 1
    # to double precision, so the likelihood is unchanged.
    df = pd.DataFrame({'lower': np.maximum(lo, 1e-6), 'upper': hi, 'temperature': temp - 105.0, 'lot': lot,
                       'units': w})
    case = {'rows': rows, 'lower': tolist(lo), 'upper': [v if v != INF else None for v in hi],
            'x': [tolist(r) for r in X[:, 1:]], 'weight': tolist(w), 'names': ['temperature', 'lot'],
            'lifelines': {}, 'scipy': {}}
    for dist in FITTERS:
        case['lifelines'][dist] = lifelines_fit(dist, df, ['temperature', 'lot'], True, 'units')
    for dist in STD:
        case['scipy'][dist] = scipy_fit(dist, lo, hi, X, w, [[85.0, 0.0], [55.0, 1.0]], [0.01, 0.1, 0.5],
                                        [1000.0, 20000.0], 1)
    out['capacitor'] = case
    print(f'capacitor done', file=sys.stderr, flush=True)
    # 3. Mixed censoring.
    rows = mixed_data()
    lo = np.array([r[0] for r in rows], float)
    hi = np.array([r[1] for r in rows], float)
    X = np.column_stack([np.ones(len(rows)), [r[2] for r in rows], [r[3] for r in rows]])
    w = np.array([r[4] for r in rows], float)
    case = {'lower': [v if v != -INF else None for v in lo], 'upper': [v if v != INF else None for v in hi],
            'x': [tolist(r) for r in X[:, 1:]], 'weight': tolist(w), 'names': ['x1', 'x2'], 'scipy': {}}
    for dist in STD:
        case['scipy'][dist] = scipy_fit(dist, lo, hi, X, w, [[0.0, 1.0], [1.0, 2.0]], [0.1, 0.5], [100.0, 250.0], 1)
    out['mixed'] = case
    print(f'mixed done', file=sys.stderr, flush=True)
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, 'survival_regression_reference.json'), 'w') as f:
        json.dump(out, f, indent=1)


if __name__ == '__main__':
    main()
