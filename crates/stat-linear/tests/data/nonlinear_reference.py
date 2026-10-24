"""Independent reference fits for stat-linear's nonlinear regression tests.

Every case is fitted with SciPy's `scipy.optimize.least_squares` (MINPACK
Levenberg-Marquardt, `method='lm'`, or the trust-region reflective method
for bounded cases) with tolerances at machine precision, from starting
values near the truth (the Rust tests start from the library's automatic
starting values instead). Then, independently of the Rust code:

* standard errors s*sqrt(diag((J'J)^-1)) with s^2 = SSE/(n-P), J the
  analytic Jacobian at the solution, inverted by numpy's SVD-based pinv;
* the correlation matrix of the estimates;
* profile t intervals (Bates & Watts 1988, section 6.1): for each parameter,
  the root of |tau(v)| = t(n-P, 0.975), tau(v) = sign(v - est) *
  sqrt(S(v) - S_min)/s with S(v) the constrained minimum over the other
  parameters (least_squares again), found by brentq;
* delta-method 95% confidence limits of the mean and prediction limits at a
  few predictor values.

Data: generated here from the stated models with a fixed seed (invented),
plus the Puromycin (treated) enzyme-kinetics data of Treloar (1974) as
listed by Bates & Watts (1988, Appendix A1.3).

Run:  python3 nonlinear_reference.py   (writes nonlinear_reference.json)
"""
import json
import numpy as np
from scipy import optimize, stats

rng = np.random.default_rng(20261016)


def forms():
    f = {}
    f['exponential'] = (lambda x, t: t[0] * np.exp(t[1] * x),
                        lambda x, t: np.column_stack([np.exp(t[1] * x), t[0] * x * np.exp(t[1] * x)]))
    f['exponential_asymptote'] = (
        lambda x, t: t[0] + t[1] * np.exp(t[2] * x),
        lambda x, t: np.column_stack([np.ones_like(x), np.exp(t[2] * x), t[1] * x * np.exp(t[2] * x)]))

    def l3(x, t):
        return t[0] / (1 + np.exp(-t[1] * (x - t[2])))

    def l3j(x, t):
        u = np.exp(-t[1] * (x - t[2]))
        q = 1 / (1 + u)
        return np.column_stack([q, t[0] * q * q * u * (x - t[2]), -t[0] * q * q * u * t[1]])
    f['logistic3'] = (l3, l3j)

    def l4(x, t):
        return t[0] + (t[1] - t[0]) / (1 + np.exp(-t[2] * (x - t[3])))

    def l4j(x, t):
        u = np.exp(-t[2] * (x - t[3]))
        q = 1 / (1 + u)
        s = t[1] - t[0]
        return np.column_stack([1 - q, q, s * q * q * u * (x - t[3]), -s * q * q * u * t[2]])
    f['logistic4'] = (l4, l4j)

    def gz(x, t):
        return t[0] * np.exp(-np.exp(-t[1] * (x - t[2])))

    def gzj(x, t):
        w = np.exp(-t[1] * (x - t[2]))
        e = np.exp(-w)
        return np.column_stack([e, t[0] * e * w * (x - t[2]), -t[0] * e * w * t[1]])
    f['gompertz'] = (gz, gzj)
    f['michaelis_menten'] = (lambda x, t: t[0] * x / (t[1] + x),
                             lambda x, t: np.column_stack([x / (t[1] + x), -t[0] * x / (t[1] + x) ** 2]))
    f['power'] = (lambda x, t: t[0] * x ** t[1],
                  lambda x, t: np.column_stack([x ** t[1], t[0] * x ** t[1] * np.log(x)]))
    f['rational'] = (lambda x, t: (t[0] + t[1] * x) / (1 + t[2] * x),
                     lambda x, t: np.column_stack([1 / (1 + t[2] * x), x / (1 + t[2] * x),
                                                   -x * (t[0] + t[1] * x) / (1 + t[2] * x) ** 2]))
    return f


FORMS = forms()
TIGHT = dict(xtol=2.3e-16, ftol=2.3e-16, gtol=2.3e-16, max_nfev=200000)


def fit(form, x, y, start, lower=None, upper=None):
    f, jac = FORMS[form]
    res = lambda t: f(x, t) - y
    jj = lambda t: jac(x, t)
    if lower is None:
        r = optimize.least_squares(res, start, jac=jj, method='lm', **TIGHT)
    else:
        r = optimize.least_squares(res, start, jac=jj, method='trf', bounds=(lower, upper), **TIGHT)
    return r.x


def constrained(form, x, y, j, v, warm):
    f, jac = FORMS[form]
    p = len(warm)
    others = [k for k in range(p) if k != j]

    def full(u):
        t = np.empty(p)
        t[j] = v
        t[others] = u
        return t
    r = optimize.least_squares(lambda u: f(x, full(u)) - y, warm[others],
                               jac=lambda u: jac(x, full(u))[:, others], method='lm', **TIGHT)
    return float(np.sum(r.fun ** 2)), full(r.x)


def case(name, form, x, y, start, x0, lower=None, upper=None, profile=True):
    f, jac = FORMS[form]
    x = np.asarray(x, float)
    y = np.asarray(y, float)
    t = fit(form, x, y, np.asarray(start, float), lower, upper)
    p = len(t)
    free = list(range(p))
    if lower is not None:
        # Parameters that end on a bound are set exactly there and the others
        # re-estimated with them fixed.
        tol = lambda b: 0.0 if np.isinf(b) else 1e-7 * (1 + abs(b))
        free = [k for k in range(p) if t[k] - lower[k] > tol(lower[k]) and upper[k] - t[k] > tol(upper[k])]
        for k in range(p):
            if k not in free:
                t[k] = lower[k] if t[k] - lower[k] <= tol(lower[k]) else upper[k]
        fixed = [k for k in range(p) if k not in free]
        assert len(fixed) == 1
        _, t = constrained(form, x, y, fixed[0], t[fixed[0]], t)
    n = len(y)
    sse = float(np.sum((f(x, t) - y) ** 2))
    df = n - p
    s = np.sqrt(sse / df)
    J = jac(x, t)[:, free]
    inv = np.linalg.pinv(J.T @ J)
    se = np.full(p, np.nan)
    se[free] = s * np.sqrt(np.diag(inv))
    corr = np.full((p, p), np.nan)
    d = np.sqrt(np.diag(inv))
    corr[np.ix_(free, free)] = inv / np.outer(d, d)
    tc = stats.t.ppf(0.975, df)
    prof = []
    for j in range(p):
        if not profile or j not in free:
            prof.append(None)
            continue
        lims = []
        for side in (-1, 1):
            warm = t.copy()

            def g(c):
                nonlocal warm
                st, warm = constrained(form, x, y, j, t[j] + side * c * se[j], warm)
                return np.sqrt(max(st - sse, 0)) / s - tc
            a, c = 0.0, 1.0
            while g(c) < 0:
                a, c = c, c * 1.5
                warm = t.copy()
            warm = t.copy()
            root = optimize.brentq(g, a, c, xtol=1e-14, rtol=1e-14)
            lims.append(t[j] + side * root * se[j])
        prof.append(lims)
    bands = []
    for xv in x0:
        val = float(f(np.array([xv]), t)[0])
        gv = jac(np.array([xv]), t)[0][free]
        sm = s * np.sqrt(gv @ inv @ gv)
        bands.append(dict(x=xv, fit=val, mean=[val - tc * sm, val + tc * sm],
                          prediction=[val - tc * np.sqrt(sm ** 2 + s ** 2), val + tc * np.sqrt(sm ** 2 + s ** 2)]))
    out = dict(name=name, form=form, x=x.tolist(), y=y.tolist(), estimates=t.tolist(),
               se=[None if np.isnan(v) else v for v in se], sse=sse, df=df, residual_se=s,
               correlation=[[None if np.isnan(v) else v for v in row] for row in corr],
               profile=prof, bands=bands)
    if lower is not None:
        out['lower'] = [None if np.isinf(v) else v for v in lower]
        out['upper'] = [None if np.isinf(v) else v for v in upper]
    return out


cases = []
# Puromycin, treated (Treloar 1974; Bates & Watts 1988, A1.3).
conc = [0.02, 0.02, 0.06, 0.06, 0.11, 0.11, 0.22, 0.22, 0.56, 0.56, 1.10, 1.10]
rate = [76, 47, 97, 107, 123, 139, 159, 152, 191, 201, 207, 200]
cases.append(case('puromycin', 'michaelis_menten', conc, rate, [200, 0.1], [0.05, 0.5, 1.0]))


def noisy(form, x, truth, sd):
    f, _ = FORMS[form]
    x = np.asarray(x, float)
    return (f(x, np.asarray(truth, float)) + rng.normal(0, sd, size=x.size)).tolist()


x = np.linspace(0, 10, 15)
cases.append(case('decay', 'exponential', x, noisy('exponential', x, [8, -0.35], 0.15), [7, -0.3], [0, 5, 12]))
x = np.linspace(0, 24, 18)
cases.append(case('asymptote', 'exponential_asymptote', x,
                  noisy('exponential_asymptote', x, [20, -15, -0.2], 0.6), [18, -12, -0.15], [1, 12]))
x = np.arange(0, 42, 3.0)
cases.append(case('growth3', 'logistic3', x, noisy('logistic3', x, [55, 0.25, 18], 1.5), [50, 0.2, 20], [10, 18, 40]))
x = np.linspace(-3, 3, 20)
cases.append(case('logistic4', 'logistic4', x, noisy('logistic4', x, [5, 95, 1.6, 0.4], 3.0),
                  [10, 90, 1.5, 0.3], [-1, 0, 2]))
x = np.arange(1, 30, 2.0)
cases.append(case('gompertz', 'gompertz', x, noisy('gompertz', x, [40, 0.18, 10], 1.0), [35, 0.2, 9], [5, 15]))
x = np.linspace(0.5, 12, 16)
cases.append(case('power', 'power', x, noisy('power', x, [2.5, 0.6], 0.2), [2, 0.5], [1, 6]))
x = np.linspace(0, 8, 17)
cases.append(case('rational', 'rational', x, noisy('rational', x, [1.0, 3.0, 0.4], 0.12), [1, 2, 0.3], [0.5, 4]))
# Bounded: the asymptote is forced to stay at or above 22 (it binds).
x = np.linspace(0, 24, 18)
yb = noisy('exponential_asymptote', x, [20, -15, -0.2], 0.6)
cases.append(case('bounded', 'exponential_asymptote', x, yb, [23, -12, -0.15], [6],
                  lower=np.array([22.0, -np.inf, -np.inf]), upper=np.array([np.inf, np.inf, np.inf]), profile=False))

with open(__file__.replace('.py', '.json'), 'w') as fh:
    json.dump(dict(source='nonlinear_reference.py (SciPy %s)' % __import__('scipy').__version__, cases=cases),
              fh, indent=1)
print('wrote', len(cases), 'cases')
