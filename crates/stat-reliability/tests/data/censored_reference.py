"""Independent reference values for censored life-distribution fits.

Run with:  python3 censored_reference.py   (writes censored_reference.json)

Everything here is written from the textbook definitions with SciPy's
distribution objects (scipy.stats), SciPy optimizers and NumPy, independently
of the Rust implementation:

* censored log-likelihoods: sum of w*log f (exact), w*log S (right),
  w*log F (left), w*log(F(u) - F(l)) (interval) [Meeker & Escobar 1998, ch. 2, 8];
* maximum likelihood by repeated Nelder-Mead with tight tolerances, polished by
  BFGS; threshold forms by bounded scalar search over the threshold of the
  profile likelihood, then a joint polish;
* standard errors from a 4th-order central-difference Hessian of the
  log-likelihood in (mu, log sigma) / (log k, log theta) / (log theta)
  coordinates (threshold last), mapped by the delta method;
* likelihood-ratio (profile) limits by brentq on 2(l_hat - l_profile) = chi2_1(0.95);
* Turnbull's estimate by plain self-consistency (EM) iterations to 1e-14;
* the censored Anderson-Darling distance by numerical quadrature (scipy.integrate.quad)
  of W * int (Fhat - F)^2 / (F (1 - F)) dF over the range where Fhat is determined,
  Fhat linear in F inside innermost intervals;
* Weibull with known shape: chi-square limits (Nelson 1985).

All datasets are invented for the tests.
"""
import json, math
import numpy as np
from scipy import stats, optimize, integrate

INF = math.inf
LEVEL = 0.95
CRIT = stats.chi2.ppf(LEVEL, 1)

# ---------------------------------------------------------------- data ---
def E(t, w=1): return (t, t, w)
def R(t, w=1): return (t, INF, w)
def L(t, w=1): return (-INF, t, w)
def I(a, b, w=1): return (a, b, w)

DATASETS = {
    # Exact failures and Type I right censoring at 1000 (seven survivors as one frequency row).
    'right': [E(145), E(210), E(265), E(330), E(372), E(418), E(480), E(545), E(610), E(690),
              E(760), E(845), E(930), R(1000, 7), R(520), R(700)],
    # Left censoring below a first look at 50 hours, exact failures, survivors at 400.
    'left': [L(50, 3), E(62), E(75), E(88), E(104), E(118), E(135), E(160), E(190), E(230), E(290), R(400, 2)],
    # Inspections every 100 hours; failures found at each inspection, survivors at 600.
    'interval': [I(0, 100, 3), I(100, 200, 7), I(200, 300, 9), I(300, 400, 6), I(400, 500, 4), I(500, 600, 2),
                 R(600, 9)],
    # Everything at once, with overlapping intervals.
    'mixed': [E(55), E(92), E(140), E(141, 2), I(60, 120), I(80, 150), I(100, 200, 3), I(150, 250),
              L(40, 2), L(70), R(180, 3), R(260, 2), R(90), E(205), I(210, 320, 2)],
    # A clear failure-free period (threshold forms).
    'threshold': [E(325), E(344), E(358), E(370), E(380), E(391), E(400), E(410), I(415, 445, 2), E(440),
                  E(451), E(463), E(477), E(492), R(500, 3)],
}

# ------------------------------------------------------------- families ---
def frozen(key, th):
    g = th[-1] if key.endswith('threshold') else 0.0
    if key in ('normal',): return stats.norm(loc=th[0], scale=math.exp(th[1]))
    if key == 'logistic': return stats.logistic(loc=th[0], scale=math.exp(th[1]))
    if key == 'smallest-extreme-value': return stats.gumbel_l(loc=th[0], scale=math.exp(th[1]))
    if key == 'largest-extreme-value': return stats.gumbel_r(loc=th[0], scale=math.exp(th[1]))
    if key.startswith('lognormal'): return stats.lognorm(s=math.exp(th[1]), scale=math.exp(th[0]), loc=g)
    if key.startswith('log-logistic'): return stats.fisk(c=math.exp(-th[1]), scale=math.exp(th[0]), loc=g)
    if key.startswith('weibull'): return stats.weibull_min(c=math.exp(-th[1]), scale=math.exp(th[0]), loc=g)
    if key.startswith('exponential'): return stats.expon(scale=math.exp(th[0]), loc=g)
    if key.startswith('gamma'): return stats.gamma(a=math.exp(th[0]), scale=math.exp(th[1]), loc=g)
    raise KeyError(key)

def reported(key, th):
    """Parameters in their usual form, in the Rust report order."""
    base = key.replace('-threshold', '')
    if base in ('normal', 'logistic', 'smallest-extreme-value', 'largest-extreme-value', 'lognormal', 'log-logistic'):
        out = [th[0], math.exp(th[1])]
    elif base == 'weibull':
        out = [math.exp(-th[1]), math.exp(th[0])]
    elif base == 'exponential':
        out = [math.exp(th[0])]
    else:
        out = [math.exp(th[0]), math.exp(th[1])]
    if key.endswith('threshold'):
        out.append(th[-1])
    return out

# (coordinate, transform) of each reported parameter: 'id', 'exp', 'expneg'
def reported_map(key):
    base = key.replace('-threshold', '')
    if base == 'weibull': m = [(1, 'expneg'), (0, 'exp')]
    elif base == 'exponential': m = [(0, 'exp')]
    elif base == 'gamma': m = [(0, 'exp'), (1, 'exp')]
    else: m = [(0, 'id'), (1, 'exp')]
    if key.endswith('threshold'):
        m.append((len(m) if base != 'exponential' else 1, 'id'))
    return m

def tr(kind, x):
    return x if kind == 'id' else (math.exp(x) if kind == 'exp' else math.exp(-x))

_SPLIT = {}

def split(obs):
    k = id(obs)
    if k not in _SPLIT:
        a = lambda rows: np.array(rows, float).reshape(-1, 3)
        ex = a([o for o in obs if o[0] == o[1]])
        ri = a([o for o in obs if o[1] == INF and o[0] != o[1]])
        le = a([o for o in obs if o[0] == -INF])
        iv = a([o for o in obs if o[0] != o[1] and np.isfinite(o[0]) and np.isfinite(o[1])])
        _SPLIT[k] = (ex, ri, le, iv)
    return _SPLIT[k]

def loglik(key, th, obs):
    if not all(np.isfinite(th)):
        return -INF
    try:
        d = frozen(key, th)
    except (ValueError, OverflowError):
        return -INF
    ex, ri, le, iv = split(obs)
    with np.errstate(all='ignore'):
        parts = [ex[:, 2] * d.logpdf(ex[:, 0]), ri[:, 2] * d.logsf(ri[:, 0]), le[:, 2] * d.logcdf(le[:, 1])]
        if len(iv):
            fl, fu = d.cdf(iv[:, 0]), d.cdf(iv[:, 1])
            sl, su = d.sf(iv[:, 0]), d.sf(iv[:, 1])
            diff = np.where(fu < 0.5, fu - fl, sl - su)
            parts.append(iv[:, 2] * np.log(diff))
        total = sum(float(np.sum(p)) for p in parts)
    return total if np.isfinite(total) else -INF

def pseudo(obs, positive):
    vals = []
    for lo, hi, w in obs:
        if lo == hi or hi == INF: v = lo
        elif lo == -INF: v = hi / 2 if positive else hi - 50
        else: v = math.sqrt(max(lo, 1e-3) * hi) if positive else (lo + hi) / 2
        vals.append((v, w))
    return vals

def start(key, obs, gamma=0.0):
    base = key.replace('-threshold', '')
    positive = base not in ('normal', 'logistic', 'smallest-extreme-value', 'largest-extreme-value')
    v = np.array([p[0] - gamma for p in pseudo(obs, positive)])
    w = np.array([p[1] for p in pseudo(obs, positive)], float)
    if positive:
        v = np.maximum(v, 1e-6)
    if base in ('exponential',):
        return [math.log(np.average(v, weights=w))]
    if base == 'gamma':
        m = np.average(v, weights=w); s2 = np.average((v - m) ** 2, weights=w)
        return [math.log(m * m / s2), math.log(s2 / m)]
    y = np.log(v) if positive else v
    m = np.average(y, weights=w); s = math.sqrt(np.average((y - m) ** 2, weights=w))
    return [m, math.log(s)]

def newton_polish(f, x):
    """Newton steps with central-difference derivatives, halving until f rises."""
    x = np.array(x, float)
    n = len(x)
    fx = f(x)
    for _ in range(60):
        h = 1e-4 * (1 + np.abs(x))
        g = np.zeros(n); H = np.zeros((n, n))
        for i in range(n):
            e = np.zeros(n); e[i] = h[i]
            fp, fm = f(x + e), f(x - e)
            H[i, i] = (fp - 2 * fx + fm) / h[i] ** 2
            eg = np.zeros(n); eg[i] = 1e-6 * (1 + abs(x[i]))
            g[i] = (f(x + eg) - f(x - eg)) / (2 * eg[i])
            for j in range(i):
                ej = np.zeros(n); ej[j] = h[j]
                H[i, j] = H[j, i] = (f(x + e + ej) - f(x + e - ej) - f(x - e + ej) + f(x - e - ej)) / (4 * h[i] * h[j])
        if not (np.all(np.isfinite(H)) and np.all(np.isfinite(g))):
            break
        try:
            step = -np.linalg.solve(H, g)
        except np.linalg.LinAlgError:
            break
        if not np.all(np.linalg.eigvalsh(H) < 0):
            step = g * 1e-3
        t = 1.0
        while t > 1e-8:
            fn = f(x + t * step)
            if np.isfinite(fn) and fn >= fx - 1e-12:
                break
            t /= 2
        else:
            break
        x = x + t * step; fx = f(x)
        if np.max(np.abs(t * step) / (1 + np.abs(x))) < 1e-12:
            break
    return x, fx

def maximize(f, x0, simplex=True):
    neg = lambda x: (-f(x) if np.isfinite(f(x)) else 1e300)
    x = np.array(x0, float)
    if simplex:
        for _ in range(2):
            r = optimize.minimize(neg, x, method='Nelder-Mead',
                                  options=dict(xatol=1e-9, fatol=1e-12, maxiter=6000, maxfev=6000))
            x = r.x
    else:
        r = optimize.minimize(neg, x, method='BFGS', options=dict(gtol=1e-8))
        if r.fun < neg(x):
            x = r.x
    return newton_polish(f, x)

def fit(key, obs):
    f = lambda th: loglik(key, th, obs)
    if not key.endswith('threshold'):
        return maximize(f, start(key, obs))
    m = min(hi for lo, hi, w in obs if hi < INF)
    lo_all = min(v for o in obs for v in o[:2] if np.isfinite(v))
    hi_all = max(v for o in obs for v in o[:2] if np.isfinite(v))
    rng = hi_all - lo_all
    def prof(t):
        g = m - rng * math.exp(t)
        x, v = maximize(lambda z: f(list(z) + [g]), start(key, obs, g))
        return -v, x, g
    ts = np.arange(5.0, -11.01, -0.5)
    vals = [prof(t)[0] for t in ts]
    # The largest interior local maximum of the profile likelihood (the
    # likelihood may rise without bound as the threshold nears the earliest failure).
    interior = [j for j in range(1, len(ts) - 1) if vals[j] <= vals[j - 1] and vals[j] <= vals[j + 1]]
    i = min(interior, key=lambda j: vals[j])
    r = optimize.minimize_scalar(lambda t: prof(t)[0], bounds=(ts[i + 1], ts[i - 1]), method='bounded',
                                 options=dict(xatol=1e-10))
    _, x, g = prof(r.x)
    return maximize(f, list(x) + [g])

def hessian(f, x, h=1e-3):
    n = len(x); H = np.zeros((n, n))
    def at(d):
        return f(x + d)
    for i in range(n):
        for j in range(n):
            hi = h * (1 + abs(x[i])) if i < 1 else h
            hj = h * (1 + abs(x[j])) if j < 1 else h
            ei = np.zeros(n); ej = np.zeros(n); ei[i] = hi; ej[j] = hj
            if i == j:
                H[i, i] = (-at(2 * ei) + 16 * at(ei) - 30 * f(x) + 16 * at(-ei) - at(-2 * ei)) / (12 * hi * hi)
            else:
                H[i, j] = (at(ei + ej) - at(ei - ej) - at(-ei + ej) + at(-ei - ej)) / (4 * hi * hj)
                # Richardson on the mixed difference.
                H2 = (at(2 * ei + 2 * ej) - at(2 * ei - 2 * ej) - at(-2 * ei + 2 * ej) + at(-2 * ei - 2 * ej)) / (16 * hi * hj)
                H[i, j] = (4 * H[i, j] - H2) / 3
    return H

def quantile_constraint(key, th, p, t):
    """Solve the location (or gamma scale) so that F(t) = p."""
    th = list(th)
    base = key.replace('-threshold', '')
    g = th[-1] if key.endswith('threshold') else 0.0
    if base in ('normal', 'logistic', 'smallest-extreme-value', 'largest-extreme-value'):
        std = {'normal': stats.norm, 'logistic': stats.logistic, 'smallest-extreme-value': stats.gumbel_l,
               'largest-extreme-value': stats.gumbel_r}[base]
        th[0] = t - math.exp(th[1]) * std.ppf(p)
    elif base in ('lognormal', 'log-logistic', 'weibull'):
        if t <= g: return None
        std = {'lognormal': stats.norm, 'log-logistic': stats.logistic, 'weibull': stats.gumbel_l}[base]
        th[0] = math.log(t - g) - math.exp(th[1]) * std.ppf(p)
    elif base == 'exponential':
        if t <= g: return None
        th[0] = math.log((t - g) / -math.log1p(-p))
    else:
        if t <= g: return None
        th[1] = math.log((t - g) / stats.gamma.ppf(p, math.exp(th[0])))
    return th

def profile_limits(f_prof, s_hat, ll_hat, se):
    h = lambda s: 2 * (ll_hat - f_prof(s)) - CRIT
    out = []
    for d in (-1, 1):
        step = se
        a = s_hat
        b = None
        for _ in range(60):
            s = s_hat + d * step
            if h(s) > 0:
                b = s; break
            a = s; step *= 1.6
        out.append(optimize.brentq(h, a, b, xtol=1e-12, rtol=1e-12) if b is not None else d * INF)
    return out

def free_max(f, th, fixed):
    """Maximize f over the coordinates not in `fixed`."""
    idx = [i for i in range(len(th)) if i not in fixed]
    def g(z):
        x = list(th)
        for k, i in enumerate(idx): x[i] = z[k]
        return f(x)
    if not idx:
        return f(th), th
    z, v = maximize(g, [th[i] for i in idx], simplex=False)
    x = list(th)
    for k, i in enumerate(idx): x[i] = z[k]
    return v, x

def analyse(key, obs, ps, ts, lr=True):
    th, ll = fit(key, obs)
    f = lambda x: loglik(key, x, obs)
    H = hessian(f, th)
    V = np.linalg.inv(-H)
    out = dict(internal=list(th), loglik=ll, params=reported(key, th))
    m = reported_map(key)
    out['se'] = [abs(tr(t, th[c])) * math.sqrt(V[c, c]) if t != 'id' else math.sqrt(V[c, c]) for c, t in m]
    if not lr:
        return out
    lims = []
    for c, t in m:
        state = {'x': list(th)}
        def prof(s, c=c):
            x = list(state['x']); x[c] = s
            v, x = free_max(lambda y: f(y[:c] + [s] + y[c + 1:]), x, {c})
            state['x'] = x
            return v
        lo, hi = profile_limits(prof, th[c], ll, math.sqrt(V[c, c]))
        a, b = tr(t, lo), tr(t, hi)
        lims.append(sorted([a, b]))
    out['param_lr'] = lims
    elim = 1 if key.replace('-threshold', '') == 'gamma' else 0
    positive = key in ('lognormal', 'log-logistic', 'weibull', 'exponential', 'gamma')
    def grad(g):
        e = np.zeros(len(th)); gr = []
        for i in range(len(th)):
            hh = 1e-6
            e[:] = 0; e[i] = hh
            gr.append((g(th + e) - g(th - e)) / (2 * hh))
        gr = np.array(gr)
        return math.sqrt(gr @ V @ gr)
    qs = {}
    for p in ps:
        tq = frozen(key, th).ppf(p)
        to_s = (lambda t: math.log(t)) if positive else (lambda t: t)
        from_s = (lambda s: math.exp(s)) if positive else (lambda s: s)
        se = grad(lambda x: to_s(frozen(key, x).ppf(p)))
        def prof(s):
            t = from_s(s)
            def cons(y):
                z = quantile_constraint(key, y, p, t)
                return f(z) if z is not None else -INF
            v, x = free_max(cons, state['x'], {elim})
            state['x'] = x
            return v
        state = {'x': list(th)}
        lo, hi = profile_limits(prof, to_s(tq), ll, se)
        qs[repr(p)] = [tq, from_s(lo), from_s(hi)]
    out['quantiles'] = qs
    fs = {}
    for t in ts:
        F = frozen(key, th).cdf(t)
        logit = lambda q: math.log(q / (1 - q))
        se = grad(lambda x: logit(frozen(key, x).cdf(t)))
        def prof(s):
            p = 1 / (1 + math.exp(-s))
            def cons(y):
                z = quantile_constraint(key, y, p, t)
                return f(z) if z is not None else -INF
            v, x = free_max(cons, state['x'], {elim})
            state['x'] = x
            return v
        state = {'x': list(th)}
        lo, hi = profile_limits(prof, logit(F), ll, se)
        ex = lambda s: 0.0 if s == -INF else (1.0 if s == INF else 1 / (1 + math.exp(-s)))
        fs[repr(float(t))] = [F, ex(lo), ex(hi)]
    out['probabilities'] = fs
    return out

# ------------------------------------------------------------- Turnbull ---
def turnbull(obs):
    ends = []
    for lo, hi, w in obs:
        ends.append((lo, 0 if lo == hi else 2, 'L'))
        ends.append((hi, 1, 'R'))
    ends.sort(key=lambda e: (e[0], e[1]))
    inner = [(ends[k], ends[k + 1]) for k in range(len(ends) - 1) if ends[k][2] == 'L' and ends[k + 1][2] == 'R']
    A = np.zeros((len(obs), len(inner)))
    for i, (lo, hi, w) in enumerate(obs):
        lk = (lo, 0 if lo == hi else 2)
        for j, (q, p) in enumerate(inner):
            if (q[0], q[1]) >= lk and p[0] <= hi:
                A[i, j] = 1
    w = np.array([o[2] for o in obs], float); w /= w.sum()
    s = np.full(len(inner), 1 / len(inner))
    for _ in range(2_000_000):
        d = A @ s
        new = s * (A.T @ (w / d))
        if np.max(np.abs(new - s)) < 1e-15:
            s = new; break
        s = new
    return [[q[0], p[0], q[1] == 0, float(m)] for (q, p), m in zip(inner, s)], s

def censored_ad(obs, inner, key, th):
    d = frozen(key, th)
    W = sum(o[2] for o in obs)
    finite = [v for o in obs for v in o[:2] if np.isfinite(v)]
    complete = sum(m for q, p, c, m in inner if p == INF) < 1e-12
    u = lambda t: 0.0 if t == -INF else (1.0 if t == INF else d.cdf(t))
    end = 1.0 if complete else u(max(finite))
    total, level, uprev = 0.0, 0.0, 0.0
    for q, p, closed, m in inner:
        if p == INF: break
        ua, ub = min(u(q), end), min(u(p), end)
        start = max(ua, uprev)
        if start > uprev:
            total += integrate.quad(lambda x: (level - x) ** 2 / (x * (1 - x)), uprev, start, epsabs=1e-13, epsrel=1e-12, limit=200)[0]
        if ub > start:
            a, b = level, level + m
            total += integrate.quad(lambda x: ((a + (b - a) * (x - start) / (ub - start)) - x) ** 2 / (x * (1 - x)),
                                    start, ub, epsabs=1e-13, epsrel=1e-12, limit=200)[0]
        level = min(level + m, 1.0)
        uprev = max(ub, uprev)
    if end > uprev:
        total += integrate.quad(lambda x: (level - x) ** 2 / (x * (1 - x)), uprev, end, epsabs=1e-13, epsrel=1e-12, limit=200)[0]
    return W * total


# ---------------------------------------------------------------- main ---
TWO = ['exponential', 'weibull', 'lognormal', 'log-logistic', 'gamma', 'normal', 'logistic',
       'smallest-extreme-value', 'largest-extreme-value']
out = {}
for name, obs in DATASETS.items():
    case = dict(obs=[[None if lo == -INF else lo, None if hi == INF else hi, w] for lo, hi, w in obs], fits={})
    keys = ['weibull-threshold', 'lognormal-threshold', 'log-logistic-threshold'] if name == 'threshold' else TWO
    for key in keys:
        lr = key in ('weibull', 'lognormal') or (
            name == 'right' and key in ('exponential', 'gamma', 'normal', 'log-logistic'))
        ts = [300.0] if name != 'threshold' else [400.0]
        case['fits'][key] = analyse(key, obs, [0.1, 0.5], ts, lr=lr)
        print(name, key, case['fits'][key]['loglik'], case['fits'][key]['params'], flush=True)
    inner, s = turnbull(obs)
    case['turnbull'] = [[None if q == -INF else q, None if p == INF else p, c, m] for q, p, c, m in inner]
    if name != 'threshold':
        th = np.array(case['fits']['weibull']['internal'])
        case['ad_weibull'] = censored_ad(obs, inner, 'weibull', th)
    out[name] = case

# Exponential with a threshold at its boundary (earliest time an exact failure):
# gamma = m, theta = sum w (t - m) / r over failures and censored units.
obs = DATASETS['threshold']
m = min(hi for lo, hi, w in obs if hi < INF)
r = sum(w for lo, hi, w in obs if lo == hi)
# (The interval observation is treated through the likelihood, so maximise theta numerically.)
th, ll = maximize(lambda z: loglik('exponential-threshold', [z[0], m], obs), [math.log(200.0)])
out['threshold']['exponential_boundary'] = dict(threshold=m, scale=math.exp(th[0]), loglik=ll)

# Weibull with known shape (Nelson 1985).
known = []
for times, events, beta in [([500.0] * 10, [0] * 10, 2.0),
                            ([150.0, 420.0, 500.0, 500.0, 500.0, 500.0, 500.0, 500.0], [1, 1, 0, 0, 0, 0, 0, 0], 1.5)]:
    T = sum(t ** beta for t in times); rr = sum(events)
    lower = (2 * T / stats.chi2.ppf(0.975, 2 * rr + 2)) ** (1 / beta)
    upper = (2 * T / stats.chi2.ppf(0.025, 2 * rr)) ** (1 / beta) if rr > 0 else None
    bound = (2 * T / stats.chi2.ppf(0.95, 2 * rr + 2)) ** (1 / beta)
    known.append(dict(times=times, events=events, shape=beta, scale=(T / rr) ** (1 / beta) if rr else None,
                      lower=lower, upper=upper, bound=bound,
                      b10_bound=bound * (-math.log(0.9)) ** (1 / beta),
                      reliability_bound_400=math.exp(-(400 / bound) ** beta)))
out['known_shape'] = known

with open(__file__.replace('.py', '.json'), 'w') as fh:
    json.dump(out, fh, indent=1)
