"""Independent reference values for stress-life (S-N) fatigue curves.

Run with:  python3 fatigue_reference.py   (writes fatigue_reference.json)

Written from the textbook definitions with SciPy and NumPy, independently of
the Rust implementation (which standardizes covariates and maximizes with its
own damped Newton iterations through `life_regression`):

* cycles to failure N; run-outs (unfailed specimens) are right-censored;
* log life Y = ln N = mu(S) + sigma(S) * eps, eps standard normal (lognormal),
  smallest extreme value (Weibull) or logistic (log-logistic)
  [ASTM E739-10; Meeker & Escobar 1998, ch. 17; Pascual & Meeker 1999];
* curves: Basquin mu = b0 + b1 ln S; semi-log mu = b0 + b1 S; quadratic
  mu = b0 + b1 ln S + b2 (ln S)^2; endurance limit (Stromeyer)
  mu = b0 + b1 ln(S - E), 0 <= E < min S; optionally ln sigma = g0 + g1 ln S;
* log-likelihood on the cycles scale: w log f(N) for failures (density of N,
  including the 1/(sigma N) Jacobian), w log S(N) for run-outs;
* maximum likelihood by Nelder-Mead from several starts, then BFGS, then
  Newton steps with a central-difference Hessian; standard errors from the
  inverse of the negative central-difference Hessian in (beta, gamma), with
  curvature-scaled steps and Richardson extrapolation, taken in coordinates
  orthonormalized by the QR decomposition of each design matrix;
* the endurance limit by profile likelihood: a fine grid over E in
  [0, min S), bounded Brent minimization of the negative profile around the
  best grid point, then a joint Newton polish over (E, beta, gamma);
  likelihood-ratio limits by brentq on 2(l_hat - l_p(E)) = chi2_1(0.95);
  standard errors and quantile intervals of the endurance form are
  conditional on E-hat (E held fixed);
* stresses at a given life by brentq on the quantile curve.

The data are invented (a realistic rotating-bending test of a steel: 32
specimens at five stress amplitudes, 200-400 MPa, run-outs at 10^7 cycles).
They are also the application's sample data set.
"""
import json, math
import numpy as np
from scipy import stats, optimize
import warnings
warnings.filterwarnings('ignore')

LEVEL = 0.95
CRIT = stats.chi2.ppf(LEVEL, 1)
Z = stats.norm.ppf(0.5 + LEVEL / 2)

# (stress MPa, cycles, failed)
DATA = [
    (200, 8990000, 1), (200, 10000000, 0), (200, 7950000, 1), (200, 6020000, 1),
    (200, 7330000, 1), (200, 5750000, 1), (200, 9240000, 1), (200, 10000000, 0),
    (250, 992000, 1), (250, 937000, 1), (250, 1540000, 1), (250, 1450000, 1),
    (250, 1300000, 1), (250, 815000, 1),
    (300, 383000, 1), (300, 531000, 1), (300, 212000, 1), (300, 316000, 1),
    (300, 165000, 1), (300, 217000, 1),
    (350, 74500, 1), (350, 153000, 1), (350, 96400, 1), (350, 193000, 1),
    (350, 183000, 1), (350, 157000, 1),
    (400, 29000, 1), (400, 70700, 1), (400, 88200, 1), (400, 94800, 1),
    (400, 45300, 1), (400, 72700, 1),
]
S = np.array([d[0] for d in DATA], float)
N = np.array([d[1] for d in DATA], float)
FAILED = np.array([d[2] == 1 for d in DATA])
Y = np.log(N)
MIN_S = S.min()


def location_design(form, s, e=0.0):
    s = np.asarray(s, float)
    one = np.ones_like(s)
    if form == 'basquin':
        return np.column_stack([one, np.log(s)])
    if form == 'semi-log':
        return np.column_stack([one, s])
    if form == 'quadratic-log':
        return np.column_stack([one, np.log(s), np.log(s) ** 2])
    if form == 'endurance-limit':
        return np.column_stack([one, np.log(s - e)])
    raise ValueError(form)


def scale_design(scale_stress, s):
    s = np.asarray(s, float)
    return np.column_stack([np.ones_like(s), np.log(s)]) if scale_stress else np.ones((len(s), 1))


def std_quantile(family, p):
    if family == 'lognormal':
        return stats.norm.ppf(p)
    if family == 'weibull':
        return math.log(-math.log1p(-p))
    if family == 'log-logistic':
        return math.log(p / (1 - p))
    raise ValueError(family)


def std_cdf(family, z):
    if family == 'lognormal':
        return stats.norm.cdf(z)
    if family == 'weibull':
        return -math.expm1(-math.exp(z))
    return 1 / (1 + math.exp(-z))


def loglik(theta, form, family, scale_stress, e=0.0):
    X = location_design(form, S, e)
    Zs = scale_design(scale_stress, S)
    p = X.shape[1]
    mu = X @ theta[:p]
    log_sigma = Zs @ theta[p:]
    sigma = np.exp(log_sigma)
    z = (Y - mu) / sigma
    if family == 'lognormal':
        lf = stats.norm.logpdf(z) - log_sigma - Y
        ls = stats.norm.logsf(z)
    elif family == 'weibull':
        ez = np.exp(z)
        lf = z - ez - log_sigma - Y
        ls = -ez
    else:
        lf = z - 2 * np.logaddexp(0, z) - log_sigma - Y
        ls = -np.logaddexp(0, z)
    v = np.where(FAILED, lf, ls).sum()
    return v if np.isfinite(v) else -1e300


def num_grad_hess(f, x, rel=1e-4):
    x = np.asarray(x, float)
    d = len(x)
    h = rel * np.maximum(1.0, np.abs(x))
    g = np.zeros(d)
    H = np.zeros((d, d))
    f0 = f(x)
    for i in range(d):
        ei = np.zeros(d); ei[i] = h[i]
        fp, fm = f(x + ei), f(x - ei)
        fp2, fm2 = f(x + ei / 2), f(x - ei / 2)
        g[i] = (4 * (fp2 - fm2) / h[i] - (fp - fm) / (2 * h[i])) / 3
        H[i, i] = (fp - 2 * f0 + fm) / h[i] ** 2
        for j in range(i):
            ej = np.zeros(d); ej[j] = h[j]
            H[i, j] = H[j, i] = (f(x + ei + ej) - f(x + ei - ej) - f(x - ei + ej) + f(x - ei - ej)) / (4 * h[i] * h[j])
    return g, H


def hessian(f, x):
    """Central-difference Hessian with steps scaled to the curvature of each
    coordinate (h_i = 2e-3 / sqrt(-H_ii)) and Richardson extrapolation over
    h and h/2, so strongly correlated coefficients do not inflate the
    truncation error."""
    x = np.asarray(x, float)
    _, H0 = num_grad_hess(f, x, 1e-5)
    h = 2e-3 / np.sqrt(np.abs(np.diag(H0)))

    def hess(h):
        d = len(x)
        H = np.zeros((d, d))
        f0 = f(x)
        for i in range(d):
            ei = np.zeros(d); ei[i] = h[i]
            H[i, i] = (f(x + ei) - 2 * f0 + f(x - ei)) / h[i] ** 2
            for j in range(i):
                ej = np.zeros(d); ej[j] = h[j]
                H[i, j] = H[j, i] = (f(x + ei + ej) - f(x + ei - ej) - f(x - ei + ej) + f(x - ei - ej)) / (4 * h[i] * h[j])
        return H
    return (4 * hess(h / 2) - hess(h)) / 3


def newton_polish(f, x, iters=30):
    x = np.asarray(x, float)
    for _ in range(iters):
        g, H = num_grad_hess(f, x)
        try:
            step = np.linalg.solve(-H, g)
        except np.linalg.LinAlgError:
            break
        t = 1.0
        while t > 1e-6 and f(x + t * step) < f(x) - 1e-12:
            t /= 2
        x = x + t * step
        if np.max(np.abs(t * step)) < 1e-12 * (1 + np.max(np.abs(x))):
            break
    return x


def maximize(f, starts):
    best = None
    for x0 in starts:
        r = optimize.minimize(lambda t: -f(t), x0, method='Nelder-Mead',
                              options={'xatol': 1e-10, 'fatol': 1e-12, 'maxiter': 40000, 'maxfev': 80000})
        r = optimize.minimize(lambda t: -f(t), r.x, method='BFGS', options={'gtol': 1e-9})
        x = newton_polish(f, r.x)
        if best is None or f(x) > f(best):
            best = x
    return best


def starts_for(form, scale_stress, e=0.0):
    X = location_design(form, S, e)
    beta = np.linalg.lstsq(X, Y, rcond=None)[0]
    r = Y - X @ beta
    ls = math.log(max(r.std(), 0.05))
    base = list(beta) + [ls] + ([0.0] if scale_stress else [])
    out = []
    for db0, dls in [(0, 0), (0.5, 0.3), (-0.5, -0.3), (0, 0.8)]:
        x = list(base)
        x[0] += db0
        x[len(beta)] += dls
        out.append(np.array(x))
    return out


def fit(form, family, scale_stress, e=0.0, starts=None):
    f = lambda t: loglik(t, form, family, scale_stress, e)
    return maximize(f, starts or starts_for(form, scale_stress, e))


def quantile_info(theta, form, family, scale_stress, e, s, p):
    p_loc = location_design(form, [s], e).shape[1]

    def yq(t):
        mu = (location_design(form, [s], e) @ t[:p_loc])[0]
        sig = math.exp((scale_design(scale_stress, [s]) @ t[p_loc:])[0])
        return mu + std_quantile(family, p) * sig
    return yq


def report(name, form, family, scale_stress, e_info=None):
    e = e_info['estimate'] if e_info else 0.0
    theta = fit(form, family, scale_stress, e)
    f = lambda t: loglik(t, form, family, scale_stress, e)
    # Hessian in orthonormalized coordinates (theta = T phi, with T from the
    # QR decompositions of the location and scale designs), where it is well
    # conditioned even for the nearly collinear (1, ln S, (ln S)^2) columns.
    blocks = []
    for X in [location_design(form, S, e), scale_design(scale_stress, S)]:
        _, R = np.linalg.qr(X)
        blocks.append(np.linalg.inv(R) * math.sqrt(len(S)))
    T = np.zeros((len(theta), len(theta)))
    T[:blocks[0].shape[0], :blocks[0].shape[0]] = blocks[0]
    T[blocks[0].shape[0]:, blocks[0].shape[0]:] = blocks[1]
    phi = np.linalg.solve(T, theta)
    cov = T @ np.linalg.inv(-hessian(lambda u: f(T @ u), phi)) @ T.T
    p_loc = location_design(form, [300.0], e).shape[1]
    quantiles = []
    for s in [225.0, 275.0, 375.0]:
        for p in [0.1, 0.5]:
            yq = quantile_info(theta, form, family, scale_stress, e, s, p)
            g, _ = num_grad_hess(yq, theta, 1e-6)
            se = math.sqrt(g @ cov @ g)
            y = yq(theta)
            quantiles.append({'stress': s, 'p': p, 'estimate': math.exp(y),
                              'wald': [math.exp(y - Z * se), math.exp(y + Z * se)]})
    fprob = []
    for s, n in [(250.0, 1e6), (350.0, 1e5)]:
        Xs = location_design(form, [s], e)
        mu = (Xs @ theta[:p_loc])[0]
        sig = math.exp((scale_design(scale_stress, [s]) @ theta[p_loc:])[0])
        fprob.append({'stress': s, 'cycles': n, 'estimate': std_cdf(family, (math.log(n) - mu) / sig)})
    at_life = []
    for n, p in [(1e6, 0.5), (1e6, 0.1), (3e5, 0.5)]:
        def h(s):
            return quantile_info(theta, form, family, scale_stress, e, s, p)(theta) - math.log(n)
        lo = e + 1e-6 * (MIN_S - e) if form == 'endurance-limit' else MIN_S / 2
        hi = 2 * S.max()
        grid = np.exp(np.linspace(math.log(lo - (e if form == 'endurance-limit' else 0)),
                                  math.log(hi - (e if form == 'endurance-limit' else 0)), 2000)) + (e if form == 'endurance-limit' else 0)
        vals = [h(s) for s in grid]
        root = None
        for i in range(len(grid) - 1, 0, -1):
            if vals[i] * vals[i - 1] <= 0:
                root = optimize.brentq(h, grid[i - 1], grid[i], xtol=1e-13, rtol=1e-15)
                break
        at_life.append({'cycles': n, 'p': p, 'stress': root})
    sigma_at = [math.exp((scale_design(scale_stress, [s]) @ theta[p_loc:])[0]) for s in [200.0, 400.0]]
    out = {
        'name': name, 'form': form, 'family': family, 'scale_stress': scale_stress,
        'location': list(theta[:p_loc]), 'scale': list(theta[p_loc:]),
        'std_errors': list(np.sqrt(np.diag(cov))),
        'log_likelihood': f(theta), 'sigma_at': {'200': sigma_at[0], '400': sigma_at[1]},
        'quantiles': quantiles, 'failure_probability': fprob, 'stress_at_life': at_life,
    }
    if e_info:
        out['endurance_limit'] = e_info
    return out


def endurance(family, scale_stress):
    form = 'endurance-limit'
    cache = {}

    def profile(e):
        key = float(e)
        if key not in cache:
            th = fit(form, family, scale_stress, e)
            cache[key] = (loglik(th, form, family, scale_stress, e), th)
        return cache[key][0]
    grid = np.linspace(0.0, MIN_S * (1 - 1e-6), 400)
    vals = np.array([profile(e) for e in grid])
    j = int(np.argmax(vals))
    lo, hi = grid[max(j - 1, 0)], grid[min(j + 1, len(grid) - 1)]
    r = optimize.minimize_scalar(lambda e: -profile(e), bounds=(lo, hi), method='bounded',
                                 options={'xatol': 1e-9})
    e0 = r.x
    th0 = cache[float(e0)][1]
    # Joint polish over (E, beta, gamma).
    joint = lambda t: loglik(t[1:], form, family, scale_stress, t[0])
    x = newton_polish(joint, np.concatenate([[e0], th0]))
    e_hat = x[0]
    l_hat = joint(x)
    h = lambda e: 2 * (l_hat - profile(e)) - CRIT
    lower = 0.0 if h(0.0) <= 0 else optimize.brentq(h, 0.0, e_hat, xtol=1e-10)
    top = MIN_S * (1 - 1e-6)
    upper = MIN_S if h(top) <= 0 else optimize.brentq(h, e_hat, top, xtol=1e-10)
    return {'estimate': e_hat, 'log_likelihood': l_hat, 'likelihood_ratio': [lower, upper],
            'profile_at': [{'e': float(e), 'log_likelihood': profile(e)} for e in [0.0, 100.0, 180.0]]}


cases = [
    report('basquin-lognormal', 'basquin', 'lognormal', False),
    report('basquin-weibull', 'basquin', 'weibull', False),
    report('semilog-lognormal', 'semi-log', 'lognormal', False),
    report('quadratic-lognormal', 'quadratic-log', 'lognormal', False),
    report('basquin-lognormal-scale', 'basquin', 'lognormal', True),
    report('basquin-loglogistic', 'basquin', 'log-logistic', False),
    report('endurance-lognormal', 'endurance-limit', 'lognormal', False, endurance('lognormal', False)),
    report('endurance-weibull', 'endurance-limit', 'weibull', False, endurance('weibull', False)),
]



def lifelines_check():
    """A second, unrelated implementation: lifelines' AFT fitters with the
    covariate ln S (a Basquin curve is an AFT model in ln S)."""
    try:
        import pandas as pd
        from lifelines import LogNormalAFTFitter, WeibullAFTFitter
    except ImportError:
        return None
    df = pd.DataFrame({'N': N, 'failed': FAILED.astype(int), 'lnS': np.log(S)})
    out = {}
    for key, cls, loc, scl in [('lognormal', LogNormalAFTFitter, 'mu_', 'sigma_'),
                               ('weibull', WeibullAFTFitter, 'lambda_', 'rho_')]:
        m = cls().fit(df, 'N', 'failed', fit_options={'precision': 1e-12})
        b0 = m.params_[(loc, 'Intercept')]
        b1 = m.params_[(loc, 'lnS')]
        g = m.params_[(scl, 'Intercept')]
        # lifelines' Weibull has shape rho = exp(g), so sigma = 1/rho.
        out[key] = {'location': [b0, b1], 'log_sigma': g if key == 'lognormal' else -g,
                    'log_likelihood': m.log_likelihood_}
    return out


out = {'data': [{'stress': d[0], 'cycles': d[1], 'failed': bool(d[2])} for d in DATA], 'level': LEVEL,
       'cases': cases,
       'lifelines': lifelines_check()}
with open(__file__.replace('.py', '.json'), 'w') as fh:
    json.dump(out, fh, indent=1)
for c in cases:
    print(c['name'], c['location'], c['scale'], c['log_likelihood'], c.get('endurance_limit', {}).get('estimate'),
          c.get('endurance_limit', {}).get('likelihood_ratio'))
