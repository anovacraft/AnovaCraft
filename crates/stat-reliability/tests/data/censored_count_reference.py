"""Independent reference values for count distributions fitted to censored
counts (below a detection limit, too numerous to count, or within a range).

Run with:  python3 censored_count_reference.py   (writes censored_count_reference.json)

Written from the definitions with SciPy, independently of the Rust
implementation (which sums probabilities itself and uses its own incomplete
gamma and beta functions and Newton iterations):

* Poisson(lambda) via scipy.stats.poisson; negative binomial with mean mu
  and size k (variance mu + mu^2/k) via scipy.stats.nbinom(n=k, p=k/(k+mu));
* log-likelihood sum w log P(lower <= X <= upper): logpmf for exact counts,
  logsumexp of logpmf over a finite range, logsf(lower - 1) for X >= lower
  [Helsel 2012, ch. 6; Cameron & Trivedi 2013, ch. 4];
* maximum likelihood over the log parameters by Nelder-Mead from several
  starts, BFGS and Newton steps; standard errors from a Richardson-
  extrapolated central-difference Hessian on the log scale; Wald limits
  exp(log est -/+ z se); likelihood-ratio limits by brentq on
  2(l_hat - l_profile) = chi2_1(0.95), the other parameter re-maximized.

The data are invented: indicator-organism counts per 100 mL in water
samples, with a detection limit and an upper counting limit.
"""
import json, math
import numpy as np
from scipy import stats, optimize, special
import warnings
warnings.filterwarnings('ignore')

LEVEL = 0.95
CRIT = stats.chi2.ppf(LEVEL, 1)
Z = stats.norm.ppf(0.5 + LEVEL / 2)
INF = math.inf

# (lower, upper, weight): exact counts, "< 3" as [0, 2], ">= 40" as [40, inf].
WATER = [(c, c, 1) for c in [3, 4, 4, 6, 6, 7, 9, 9, 10, 11, 12, 12, 14, 15, 17, 19, 22, 25, 28, 33]] + [
    (5, 5, 2), (8, 8, 2), (0, 2, 6), (0, 2, 1), (40, INF, 3), (10, 19, 2), (20, 29, 1)]
# Low counts with a detection limit of 2 ([0, 1]) and frequencies.
LOW = [(0, 1, 14), (2, 2, 6), (3, 3, 4), (4, 4, 3), (5, 5, 1), (7, 7, 1), (6, INF, 1)]


def dist(family, th):
    if family == 'poisson':
        return stats.poisson(math.exp(th[0]))
    mu, k = math.exp(th[0]), math.exp(th[1])
    return stats.nbinom(k, k / (k + mu))


def loglik(data, family, th):
    d = dist(family, th)
    total = 0.0
    for lo, hi, w in data:
        if lo == hi:
            c = d.logpmf(lo)
        elif hi == INF:
            c = d.logsf(lo - 1)
        else:
            c = special.logsumexp(d.logpmf(np.arange(lo, hi + 1)))
        total += w * c
    return total if np.isfinite(total) else -1e300


def hessian(f, x, h=2e-4):
    x = np.asarray(x, float)

    def hess(h):
        d = len(x)
        H = np.zeros((d, d))
        f0 = f(x)
        for i in range(d):
            ei = np.zeros(d); ei[i] = h
            H[i, i] = (f(x + ei) - 2 * f0 + f(x - ei)) / h ** 2
            for j in range(i):
                ej = np.zeros(d); ej[j] = h
                H[i, j] = H[j, i] = (f(x + ei + ej) - f(x + ei - ej) - f(x - ei + ej) + f(x - ei - ej)) / (4 * h * h)
        return H
    return (4 * hess(h / 2) - hess(h)) / 3


def gradient(f, x, h=1e-5):
    x = np.asarray(x, float)
    g = np.zeros(len(x))
    for i in range(len(x)):
        e = np.zeros(len(x)); e[i] = h
        g[i] = (8 * (f(x + e / 2) - f(x - e / 2)) - (f(x + e) - f(x - e))) / (6 * h)
    return g


def maximize(f, starts):
    best = None
    for x0 in starts:
        r = optimize.minimize(lambda t: -f(t), x0, method='Nelder-Mead',
                              options={'xatol': 1e-11, 'fatol': 1e-13, 'maxiter': 20000})
        r = optimize.minimize(lambda t: -f(t), r.x, method='BFGS', options={'gtol': 1e-10})
        x = r.x
        for _ in range(20):
            step = np.linalg.solve(-hessian(f, x), gradient(f, x))
            x = x + step
            if np.max(np.abs(step)) < 1e-13:
                break
        if best is None or f(x) > f(best):
            best = x
    return best


def case(name, data, family):
    f = lambda th: loglik(data, family, th)
    n = sum(w for _, _, w in data)
    if family == 'poisson':
        starts = [np.array([math.log(8.0)]), np.array([0.0]), np.array([3.0])]
    else:
        starts = [np.array([math.log(8.0), 0.0]), np.array([1.0, -1.0]), np.array([2.5, 1.5])]
    th = maximize(f, starts)
    lhat = f(th)
    cov = np.linalg.inv(-hessian(f, th))
    names = ['mean'] if family == 'poisson' else ['mean', 'size']
    params = []
    for i, nm in enumerate(names):
        se = math.sqrt(cov[i, i])
        others = [j for j in range(len(th)) if j != i]

        def profile(s):
            if not others:
                return f(np.array([s]))
            def g(u):
                t = np.array(th, float); t[i] = s; t[others[0]] = u[0]
                return -f(t)
            r = optimize.minimize(g, [th[others[0]]], method='BFGS', options={'gtol': 1e-10})
            r = optimize.minimize(g, r.x, method='Nelder-Mead', options={'xatol': 1e-12, 'fatol': 1e-14})
            return -r.fun
        h = lambda s: 2 * (lhat - profile(s)) - CRIT
        # A limit more than e^20 away is reported as unbounded (None): the
        # profile of the size tends to the Poisson likelihood as k -> inf.
        lr = []
        for sign in (-1, 1):
            b = th[i] + sign * se
            while h(b) < 0 and abs(b - th[i]) <= 20:
                b += sign * se
            if h(b) < 0:
                lr.append(None)
            else:
                a, c = sorted([b, th[i]])
                lr.append(math.exp(optimize.brentq(h, a, c, xtol=1e-13)))
        est = math.exp(th[i])
        params.append({'name': nm, 'estimate': est, 'std_error': est * se,
                       'wald': [math.exp(th[i] - Z * se), math.exp(th[i] + Z * se)], 'likelihood_ratio': lr})
    d = dist(family, th)
    m = len(th)
    return {'name': name, 'family': family, 'data': [[lo, None if hi == INF else hi, w] for lo, hi, w in data],
            'parameters': params, 'log_likelihood': lhat, 'aic': -2 * lhat + 2 * m, 'bic': -2 * lhat + m * math.log(n),
            'n': n, 'pmf': [float(d.pmf(k)) for k in range(6)], 'cdf': {'3': float(d.cdf(3)), '10': float(d.cdf(10))}}


cases = [case('water-poisson', WATER, 'poisson'), case('water-negative-binomial', WATER, 'negative-binomial'),
         case('low-poisson', LOW, 'poisson'), case('low-negative-binomial', LOW, 'negative-binomial')]
with open(__file__.replace('.py', '.json'), 'w') as fh:
    json.dump({'level': LEVEL, 'cases': cases}, fh, indent=1)
for c in cases:
    print(c['name'], [(p['estimate'], p['std_error'], p['likelihood_ratio']) for p in c['parameters']], c['log_likelihood'])
