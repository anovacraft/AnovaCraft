"""Independent reference values for life-stress (accelerated life test) models.

Run with:  python3 life_stress_reference.py   (writes life_stress_reference.json)

Written from the textbook definitions with SciPy and NumPy, independently of
the Rust implementation (which works on log time with the standardized
error distributions and its own Newton iterations):

* likelihoods on the time scale, vectorized with NumPy (z = (ln t - mu)/sigma;
  Weibull/exponential S = exp(-e^z), lognormal via scipy.stats.norm,
  log-logistic S = 1/(1 + e^z); densities on the time scale include the
  1/(sigma t) Jacobian): w*log f (exact), w*log S (right), w*log F (left),
  w*log(F(u) - F(l)) (interval) [Meeker & Escobar 1998, ch. 17, 19;
  Nelson 1990, ch. 5]; SciPy's frozen distributions (weibull_min, lognorm,
  fisk, expon) give the failure probabilities at use conditions;
* the scale parameter is sigma = 1/shape (Weibull, log-logistic) or the
  lognormal's sigma; log sigma is optionally linear in the stresses;
* maximum likelihood by Nelder-Mead from several starts, polished by BFGS;
* standard errors from a central-difference Hessian in (beta, log sigma);
* likelihood-ratio limits by brentq on 2(l_hat - l_profile) = chi2_1(0.95),
  each profile re-maximized by Nelder-Mead + BFGS;
* a second, unrelated implementation: lifelines' WeibullAFTFitter /
  LogNormalAFTFitter / LogLogisticAFTFitter with covariate 11604.518/(degC + 273.15)
  and frequency weights (an Arrhenius model is an AFT model in that covariate).

Datasets:
* Device-A (Meeker & Escobar 1998, Table C.10, from Hooper & Amster 1990):
  hours to failure at 10, 40, 60 and 80 degC with survivors at 5000 hours.
* Class-B insulation (Nelson 1990, Accelerated Testing, ch. 3 Table 4.1):
  motorettes at 150, 170, 190, 220 degC.
* GAB insulation (Escobar & Meeker 2006, Statistical Science 21, Table 1):
  thousands of hours at voltage stresses 170-220 V/mm, survivors at 6.480.
* 'peck' and 'eyring' are invented two-stress tests with left, interval and
  right censoring.
"""
import json, math
import numpy as np
from scipy import stats, optimize
import warnings
warnings.filterwarnings('ignore')

INF = math.inf
LEVEL = 0.95
CRIT = stats.chi2.ppf(LEVEL, 1)
Z = stats.norm.ppf(0.5 + LEVEL / 2)
INV_K = 1.0 / 8.617333262e-5

def E(t, w=1): return (t, t, w)
def R(t, w=1): return (t, INF, w)
def L(t, w=1): return (-INF, t, w)
def I(a, b, w=1): return (a, b, w)

# ------------------------------------------------------------------ data --
def device_a():
    rows = []
    rows.append((R(5000, 30), [10.0]))
    for t in [1298, 1390, 3187, 3241, 3261, 3313, 4501, 4568, 4841, 4982]:
        rows.append((E(t), [40.0]))
    rows.append((R(5000, 90), [40.0]))
    for t in [581, 925, 1432, 1586, 2452, 2734, 2772, 4106, 4674]:
        rows.append((E(t), [60.0]))
    rows.append((R(5000, 11), [60.0]))
    for t in [283, 361, 515, 638, 854, 1024, 1030, 1045, 1767, 1777, 1856, 1951, 1964, 2884]:
        rows.append((E(t), [80.0]))
    rows.append((R(5000, 1), [80.0]))
    return rows

def class_b():
    rows = [(R(8064, 10), [150.0])]
    rows += [(E(t), [170.0]) for t in [1764, 2772, 3444, 3542, 3780, 4860, 5196]] + [(R(5448, 3), [170.0])]
    rows += [(E(t), [190.0]) for t in [408, 408, 1344, 1344, 1440]] + [(R(1680, 5), [190.0])]
    rows += [(E(t), [220.0]) for t in [408, 408, 504, 504, 504]] + [(R(528, 5), [220.0])]
    return rows

def gab():
    table = {170: ([], 15), 190: ([3.248, 4.052, 5.304], 12),
             200: ([1.759, 3.645, 3.706, 3.726, 3.990, 5.153, 6.368], 8),
             210: ([1.401, 2.829, 2.941, 2.991, 3.311, 3.364, 3.474, 4.902, 5.639, 6.021, 6.456], 4),
             220: ([0.401, 1.297, 1.342, 1.999, 2.075, 2.196, 2.885, 3.019, 3.550, 3.566, 3.610, 3.659, 3.687,
                    4.152, 5.572], 0)}
    rows = []
    for v, (fails, cens) in table.items():
        rows += [(E(t), [float(v)]) for t in fails]
        if cens:
            rows.append((R(6.480, cens), [float(v)]))
    return rows

def peck():
    # Temperature (degC) and relative humidity (%); inspections and survivors.
    return [
        (L(100, 2), [85.0, 85.0]), (I(100, 200, 3), [85.0, 85.0]), (I(200, 400, 4), [85.0, 85.0]),
        (E(250), [85.0, 85.0]), (E(310), [85.0, 85.0]), (R(500, 3), [85.0, 85.0]),
        (I(100, 200, 1), [85.0, 60.0]), (I(200, 400, 3), [85.0, 60.0]), (E(450), [85.0, 60.0]),
        (E(520), [85.0, 60.0]), (R(600, 6), [85.0, 60.0]),
        (I(200, 400, 2), [70.0, 85.0]), (E(380), [70.0, 85.0]), (E(560), [70.0, 85.0]), (E(705), [70.0, 85.0]),
        (I(600, 800, 2), [70.0, 85.0]), (R(900, 5), [70.0, 85.0]),
        (E(640), [70.0, 60.0]), (I(700, 900, 2), [70.0, 60.0]), (R(1000, 9), [70.0, 60.0]),
    ]

def eyring():
    # Temperature (degC) and voltage; exact failures and survivors.
    data = {
        (120.0, 10.0): [55, 80, 96, 120, 151, 170, 210], (120.0, 6.0): [140, 185, 230, 262, 330, 410],
        (100.0, 10.0): [190, 260, 315, 390, 470], (100.0, 6.0): [420, 610, 780, 905],
        (80.0, 10.0): [700, 940, 1300],
    }
    survivors = {(120.0, 6.0): (450, 2), (100.0, 10.0): (600, 3), (100.0, 6.0): (1000, 4), (80.0, 10.0): (1500, 5)}
    rows = []
    for k, ts in data.items():
        rows += [(E(t), list(k)) for t in ts]
        if k in survivors:
            rows.append((R(*survivors[k]), list(k)))
    return rows

# ------------------------------------------------------------- model ------
def transform(kind, s):
    if kind in ('arrhenius', 'eyring'): return INV_K / (s + 273.15)
    if kind == 'inverse-power': return math.log(s)
    return s

def offset(kinds, s):
    return sum(-math.log(v + 273.15) for k, v in zip(kinds, s) if k == 'eyring')

def design(kinds, interaction, s):
    x = [transform(k, v) for k, v in zip(kinds, s)]
    out = [1.0] + x
    if interaction: out.append(x[0] * x[1])
    return out

def frozen(family, mu, sigma):
    if family == 'weibull': return stats.weibull_min(c=1 / sigma, scale=np.exp(mu))
    if family == 'lognormal': return stats.lognorm(s=sigma, scale=np.exp(mu))
    if family == 'loglogistic': return stats.fisk(c=1 / sigma, scale=np.exp(mu))
    if family == 'exponential': return stats.expon(scale=np.exp(mu))
    raise KeyError(family)

def std_quantile(family, p):
    if family in ('weibull', 'exponential'): return math.log(-math.log1p(-p))
    if family == 'lognormal': return stats.norm.ppf(p)
    return math.log(p / (1 - p))

class Model:
    def __init__(self, rows, family, kinds, interaction=False, scale_stress=False):
        self.rows, self.family, self.kinds = rows, family, kinds
        self.interaction, self.scale_stress = interaction, scale_stress
        self.X = [design(kinds, interaction, s) for _, s in rows]
        self.O = [offset(kinds, s) for _, s in rows]
        self.Zs = [[1.0] + [transform(k, v) for k, v in zip(kinds, s)] if scale_stress else [1.0] for _, s in rows]
        self.p = len(self.X[0])
        self.q = 0 if family == 'exponential' else len(self.Zs[0])

    def mu_sigma(self, th, x, z, o):
        mu = o + sum(a * b for a, b in zip(x, th[:self.p]))
        sigma = 1.0 if self.q == 0 else math.exp(sum(a * b for a, b in zip(z, th[self.p:])))
        return mu, sigma

    def arrays(self):
        if not hasattr(self, '_arr'):
            lo = np.array([r[0][0] for r in self.rows], float)
            hi = np.array([r[0][1] for r in self.rows], float)
            w = np.array([r[0][2] for r in self.rows], float)
            self._arr = (lo, hi, w, np.array(self.X), np.array(self.Zs), np.array(self.O))
        return self._arr

    def ll(self, th):
        lo, hi, w, X, Zs, O = self.arrays()
        th = np.asarray(th, float)
        mu = O + X @ th[:self.p]
        sigma = np.ones_like(mu) if self.q == 0 else np.exp(Zs[:, :self.q] @ th[self.p:])
        if not (np.all(np.isfinite(mu)) and np.all(np.isfinite(sigma))): return -INF
        exact = lo == hi; right = (hi == INF) & ~exact; left = (lo == -INF) & ~exact
        inter = ~(exact | right | left)
        fam = self.family
        with np.errstate(all='ignore'):
            def z(t): return (np.log(t) - mu) / sigma
            def log_sf(v):
                if fam in ('weibull', 'exponential'): return -np.exp(v)
                if fam == 'lognormal': return stats.norm.logsf(v)
                return -np.logaddexp(0.0, v)
            def log_cdf(v):
                if fam in ('weibull', 'exponential'): return np.log(-np.expm1(-np.exp(v)))
                if fam == 'lognormal': return stats.norm.logcdf(v)
                return v - np.logaddexp(0.0, v)
            def log_density(v):
                if fam in ('weibull', 'exponential'): return v - np.exp(v)
                if fam == 'lognormal': return stats.norm.logpdf(v)
                return v - 2.0 * np.logaddexp(0.0, v)
            c = np.zeros_like(mu)
            te = np.where(exact, lo, 1.0)
            c[exact] = (log_density(z(te)) - np.log(sigma) - np.log(te))[exact]
            c[right] = log_sf(z(np.where(right, lo, 1.0)))[right]
            c[left] = log_cdf(z(np.where(left, hi, 1.0)))[left]
            if inter.any():
                za = z(np.where(inter, lo, 1.0)); zb = z(np.where(inter, hi, 2.0))
                cdf_b = np.exp(log_cdf(zb))
                lower_tail = cdf_b < 0.5
                diff = np.where(lower_tail, cdf_b - np.exp(log_cdf(za)), np.exp(log_sf(za)) - np.exp(log_sf(zb)))
                c[inter] = np.log(np.maximum(diff, 1e-300))[inter]
        total = float(np.sum(w * c))
        return total if np.isfinite(total) else -INF

    def fit(self):
        # Least-squares start on log representative times.
        y = []
        for (lo, hi, w), _ in self.rows:
            y.append(math.log(lo if hi == INF else hi if lo == -INF else math.sqrt(lo * hi) if lo > 0 else hi))
        X = np.array(self.X); W = np.array([r[0][2] for r in self.rows], float)
        b = np.linalg.lstsq(X * np.sqrt(W)[:, None], (np.array(y) - np.array(self.O)) * np.sqrt(W), rcond=None)[0]
        best = None
        for shift in [0.0, 1.0, 2.0]:
            for ls in [math.log(0.5), 0.0, math.log(1.5)]:
                th0 = list(b) + ([ls] + [0.0] * (self.q - 1) if self.q else [])
                th0[0] += shift
                r = self.maximize(lambda t: self.ll(t), th0)
                if best is None or r[1] > best[1]: best = r
        self.th, self.ll_hat = best
        self.cov = np.linalg.inv(-hessian(self.ll, self.th))
        return self

    @staticmethod
    def maximize(f, th0, restarts=3):
        def neg(t):
            v = f(t)
            return -v if np.isfinite(v) else 1e300
        if restarts == 0:
            # Profiles start next to the optimum: quasi-Newton, polished by a simplex.
            r = optimize.minimize(neg, th0, method='BFGS', options=dict(gtol=1e-9))
            r2 = optimize.minimize(neg, r.x, method='Nelder-Mead',
                                   options=dict(xatol=1e-10, fatol=1e-12, maxiter=20000, maxfev=20000))
            if r2.fun <= r.fun: r = r2
            return list(r.x), -r.fun
        r = optimize.minimize(neg, th0, method='Nelder-Mead',
                              options=dict(xatol=1e-12, fatol=1e-13, maxiter=200000, maxfev=200000))
        for _ in range(restarts):
            r = optimize.minimize(neg, r.x, method='Nelder-Mead',
                                  options=dict(xatol=1e-12, fatol=1e-13, maxiter=200000, maxfev=200000))
        r2 = optimize.minimize(neg, r.x, method='BFGS', options=dict(gtol=1e-10))
        if r2.fun <= r.fun: r = r2
        return list(r.x), -r.fun

    def point(self, s):
        x = design(self.kinds, self.interaction, s)
        z = [1.0] + [transform(k, v) for k, v in zip(self.kinds, s)] if self.scale_stress else [1.0]
        return x, z, offset(self.kinds, s)

    def profile(self, constrain, k):
        """max over th without coordinate k, th[k] = constrain(th)."""
        def inner(rest):
            th = list(rest[:k]) + [0.0] + list(rest[k:])
            th[k] = constrain(th)
            return self.ll(th) if np.isfinite(th[k]) else -INF
        return inner

    def lr(self, s_hat, se, make_constraint, k):
        warm = [list(self.th[:k]) + list(self.th[k + 1:])]
        def h(s):
            f = self.profile(make_constraint(s), k)
            # Quasi-Newton from the previous point, and simplex restarts from
            # it and from the unconstrained estimates (ridges in models with a
            # stress-dependent scale defeat a single local search).
            x, v = self.maximize(f, warm[0], restarts=0)
            for start in (warm[0], list(self.th[:k]) + list(self.th[k + 1:])):
                x2, v2 = self.maximize(f, start, restarts=1)
                if v2 > v: x, v = x2, v2
            warm[0] = x
            return 2 * (self.ll_hat - v) - CRIT
        out = []
        for d in (-1, 1):
            warm[0] = list(self.th[:k]) + list(self.th[k + 1:])
            inner, step = s_hat, se
            while True:
                b = s_hat + d * step
                if h(b) > 0: break
                inner, step = b, step * 1.6
            out.append(optimize.brentq(h, min(inner, b), max(inner, b), xtol=1e-12, rtol=1e-12))
        return sorted(out)

def hessian(f, th, h=1e-4):
    n = len(th); H = np.zeros((n, n)); th = np.array(th, float)
    def at(d): return f(list(th + d))
    for i in range(n):
        for j in range(i, n):
            ei = np.zeros(n); ej = np.zeros(n); ei[i] = h; ej[j] = h
            v = (at(ei + ej) - at(ei - ej) - at(-ei + ej) + at(-ei - ej)) / (4 * h * h)
            H[i, j] = H[j, i] = v
    return H

def grad_se(m, g):
    th = np.array(m.th); n = len(th); gr = np.zeros(n); h = 1e-6
    for i in range(n):
        e = np.zeros(n); e[i] = h
        gr[i] = (g(list(th + e)) - g(list(th - e))) / (2 * h)
    return math.sqrt(gr @ m.cov @ gr)

def analyse(m, use, other, p_values, times, coefficient_lr=None):
    m.fit()
    out = {'family': m.family, 'kinds': m.kinds, 'interaction': m.interaction, 'scale_stress': m.scale_stress,
           'obs': [[r[0][0] if r[0][0] > -INF else None, r[0][1] if r[0][1] < INF else None, r[0][2]] for r in m.rows],
           'stresses': [r[1] for r in m.rows], 'use': use, 'other': other,
           'theta': m.th, 'loglik': m.ll_hat, 'se': list(np.sqrt(np.diag(m.cov)))}
    xu, zu, ou = m.point(use)
    def y_p(th, p):
        mu, sg = m.mu_sigma(th, xu, zu, ou)
        return mu + std_quantile(m.family, p) * sg
    qs = []
    for p in p_values:
        s_hat = y_p(m.th, p); se = grad_se(m, lambda th: y_p(th, p))
        w = std_quantile(m.family, p)
        def cons(s, w=w):
            def c(th):
                th = list(th); th[0] = 0.0
                mu0, sg = m.mu_sigma(th, xu, zu, ou)
                return s - w * sg - mu0
            return c
        lo, hi = m.lr(s_hat, se, cons, 0)
        qs.append({'p': p, 'estimate': math.exp(s_hat), 'wald': [math.exp(s_hat - Z * se), math.exp(s_hat + Z * se)],
                   'lr': [math.exp(lo), math.exp(hi)]})
    out['quantiles'] = qs
    fs = []
    for t in times:
        def F(th):
            mu, sg = m.mu_sigma(th, xu, zu, ou)
            return frozen(m.family, mu, sg).cdf(t)
        f = F(m.th); logit = lambda v: math.log(v / (1 - v))
        se = grad_se(m, lambda th: logit(F(th)))
        expit = lambda v: 1 / (1 + math.exp(-v))
        def cons(s):
            pr = expit(s); w = std_quantile(m.family, pr)
            def c(th):
                th = list(th); th[0] = 0.0
                mu0, sg = m.mu_sigma(th, xu, zu, ou)
                return math.log(t) - w * sg - mu0
            return c
        lo, hi = m.lr(logit(f), se, cons, 0)
        fs.append({'t': t, 'estimate': f, 'wald': [expit(logit(f) - Z * se), expit(logit(f) + Z * se)],
                   'lr': [expit(lo), expit(hi)]})
    out['probabilities'] = fs
    # Acceleration factor of `other` relative to `use`: exp(mu(use) - mu(other)).
    xo, zo, oo = m.point(other)
    lin = [a - b for a, b in zip(xu, xo)]
    c0 = ou - oo
    est = c0 + sum(a * b for a, b in zip(lin, m.th[:m.p]))
    se = math.sqrt(np.array(lin) @ m.cov[:m.p, :m.p] @ np.array(lin))
    k = max(range(1, m.p), key=lambda j: abs(lin[j]))
    def cons(s):
        def c(th):
            rest = c0 + sum(lin[j] * th[j] for j in range(m.p) if j != k)
            return (s - rest) / lin[k]
        return c
    lo, hi = m.lr(est, se, cons, k)
    out['acceleration'] = {'estimate': math.exp(est), 'wald': [math.exp(est - Z * se), math.exp(est + Z * se)],
                           'lr': [math.exp(lo), math.exp(hi)]}
    # Profile limits of each stress coefficient.
    coef = []
    for j in range(1, m.p):
        def cons(s, j=j):
            return lambda th: s
        lo, hi = m.lr(m.th[j], math.sqrt(m.cov[j, j]), cons, j)
        coef.append({'estimate': m.th[j], 'se': math.sqrt(m.cov[j, j]),
                     'wald': [m.th[j] - Z * math.sqrt(m.cov[j, j]), m.th[j] + Z * math.sqrt(m.cov[j, j])],
                     'lr': [lo, hi]})
    out['coefficients'] = coef
    return out

def individual_loglik(rows, family):
    """Sum of separate fits per level (levels where a maximum exists)."""
    levels = {}
    for r in rows: levels.setdefault(tuple(r[1]), []).append(r)
    total, used = 0.0, []
    for k, rs in sorted(levels.items()):
        if all(r[0][1] == INF for r in rs): continue
        m = Model([(r[0], [0.0]) for r in rs], family, [])
        m.X = [[1.0] for _ in rs]; m.O = [0.0] * len(rs); m.Zs = [[1.0] for _ in rs]; m.p = 1
        m.q = 0 if family == 'exponential' else 1
        th0 = [math.log(np.mean([r[0][0] for r in rs])), 0.0][:m.p + m.q]
        best = None
        for ls in [-0.7, 0.0, 0.7]:
            r = Model.maximize(m.ll, [th0[0] + 0.5, ls][:m.p + m.q])
            if best is None or r[1] > best[1]: best = r
        total += best[1]; used.append(list(k))
    return total, used

def common_scale_loglik(rows, family, used):
    keep = [r for r in rows if r[1] in used]
    L = len(used)
    m = Model(keep, family, [])
    m.X = [[1.0] + [1.0 if used.index(r[1]) == j else 0.0 for j in range(1, L)] for r in keep]
    m.O = [0.0] * len(keep); m.Zs = [[1.0] for _ in keep]; m.p = L; m.q = 1
    best = None
    for start in [6.0, 8.0, 10.0]:
        r = Model.maximize(m.ll, [start] + [0.0] * (L - 1) + [0.0])
        if best is None or r[1] > best[1]: best = r
    return best[1]

def lifelines_fit(rows, family):
    import pandas as pd
    from lifelines import WeibullAFTFitter, LogNormalAFTFitter, LogLogisticAFTFitter
    df = pd.DataFrame({'T': [r[0][0] for r in rows], 'E': [1 if r[0][0] == r[0][1] else 0 for r in rows],
                       'w': [r[0][2] for r in rows], 'x': [INV_K / (r[1][0] + 273.15) for r in rows]})
    cls = {'weibull': WeibullAFTFitter, 'lognormal': LogNormalAFTFitter, 'loglogistic': LogLogisticAFTFitter}[family]
    f = cls().fit(df, 'T', 'E', weights_col='w')
    p = f.params_
    first = p.index.get_level_values(0)[0]
    mu_part = p.loc[first]
    other = p.loc[[k for k in p.index.get_level_values(0).unique() if k != first][0]]
    # lifelines: Weibull lambda_ = exp(b0 + b1 x), rho_ = exp(c) (shape); lognormal mu_ = b0 + b1 x, sigma_ = exp(c).
    intercept, slope = float(mu_part['Intercept']), float(mu_part['x'])
    s = float(np.exp(other['Intercept']))
    sigma = 1 / s if family in ('weibull', 'loglogistic') else s
    return {'intercept': intercept, 'slope': slope, 'sigma': sigma, 'loglik': float(f.log_likelihood_)}

cases = {}
rows = device_a()
for fam in ['lognormal', 'weibull', 'loglogistic', 'exponential']:
    cases['device_a_' + fam] = analyse(Model(rows, fam, ['arrhenius']), [10.0], [80.0], [0.01, 0.1, 0.5], [30000.0])
    if fam != 'exponential':
        cases['device_a_' + fam]['lifelines'] = lifelines_fit(rows, fam)
ind, used = individual_loglik(rows, 'lognormal')
cases['device_a_lognormal']['levels'] = {
    'individual': ind, 'used': used, 'common_scale': common_scale_loglik(rows, 'lognormal', used),
    'life_stress': Model([r for r in rows if r[1] in used], 'lognormal', ['arrhenius']).fit().ll_hat}
cases['class_b_lognormal'] = analyse(Model(class_b(), 'lognormal', ['arrhenius']), [130.0], [220.0], [0.1, 0.5],
                                     [20000.0])
g = gab()
cases['gab_weibull'] = analyse(Model(g, 'weibull', ['inverse-power']), [120.0], [170.0], [0.1], [50.0])
cases['gab_weibull_scale'] = analyse(Model(g, 'weibull', ['inverse-power'], scale_stress=True), [120.0], [220.0],
                                     [0.1], [50.0])
cases['peck_weibull'] = analyse(Model(peck(), 'weibull', ['arrhenius', 'inverse-power']), [40.0, 50.0],
                                [85.0, 85.0], [0.1, 0.5], [5000.0])
cases['eyring_lognormal'] = analyse(Model(eyring(), 'lognormal', ['eyring', 'exponential'], interaction=True),
                                    [50.0, 5.0], [120.0, 10.0], [0.1], [10000.0])

with open(__file__.replace('.py', '.json'), 'w') as fh:
    json.dump(cases, fh, indent=1)
print('wrote', len(cases), 'cases')
for k, v in cases.items():
    print(k, v['theta'], v['loglik'])
