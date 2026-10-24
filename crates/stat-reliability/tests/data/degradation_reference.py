"""Independent reference values for degradation analysis.

Run with:  python3 degradation_reference.py   (writes degradation_reference.json)

Independently of the Rust implementation:

* per-unit linear paths by numpy.polyfit on the transformed scales; pseudo
  failure times by solving the fitted line for the threshold
  (Meeker & Escobar 1998, ch. 13 sections 13.6-13.7);
* per-unit nonlinear paths D(t) = Dinf (1 - exp(-k t)) and D(t) = a t^b by
  scipy.optimize.curve_fit (Levenberg-Marquardt), crossing times in closed form;
* life distributions of the pseudo failure times by maximum likelihood with
  scipy.stats.weibull_min / lognorm and Nelder-Mead (right censoring for
  units whose path never reaches the threshold);
* the random-effects path model by statsmodels MixedLM (REML, random
  intercept and slope per unit, unstructured covariance) and the implied
  F(t) = Phi((b0 + b1 h(t) - Df) / sqrt(v(t))) [Lu & Meeker 1993];
* destructive degradation: maximum likelihood of a normal location-scale
  regression of ln(strength) on sqrt(time) with an Arrhenius term in the rate,
  scipy Nelder-Mead + BFGS, Hessian by central differences; time-to-threshold
  quantiles and probabilities with delta-method Wald limits.

The laser data are Meeker & Escobar's (1998, Table C.17) percent increase in
operating current of 15 GaAs lasers at 80 degC, every 250 hours to 4000
hours, as distributed with the open-source R package IGPFrailty (dataset
`laser`). The other datasets are invented for the tests.
"""
import json, math
import numpy as np
from scipy import stats, optimize
import warnings
warnings.filterwarnings('ignore')

Z = stats.norm.ppf(0.975)
INV_K = 1.0 / 8.617333262e-5

LASER = [
    [0.4741, 0.9255, 2.1147, 2.7168, 3.511, 4.3415, 4.9076, 5.4782, 5.9925, 6.7224, 7.1303, 8.0006, 8.9193, 9.494, 9.8675, 10.9446],
    [0.7078, 1.2175, 1.8955, 2.2954, 2.865, 3.7454, 4.4192, 4.9894, 5.5148, 6.0668, 6.6385, 7.1615, 7.7778, 8.4242, 8.9092, 9.2834],
    [0.7074, 1.1651, 1.7253, 1.9888, 2.5325, 2.9695, 3.2977, 3.9354, 4.1613, 4.4459, 4.889, 5.2696, 5.6913, 6.0216, 6.4485, 6.8849],
    [0.3634, 0.6161, 1.3597, 1.9525, 2.2995, 2.9468, 3.3871, 3.7863, 4.1116, 4.5013, 4.7209, 4.9766, 5.2812, 5.615, 5.9503, 6.1438],
    [0.2707, 0.6144, 1.1093, 1.773, 2.0625, 2.5835, 2.9863, 3.3765, 4.0491, 4.626, 5.237, 5.6191, 6.0449, 6.3167, 7.0952, 7.5941],
    [0.3584, 1.3922, 1.9541, 2.8617, 3.4552, 3.8117, 4.533, 5.3541, 5.9217, 6.7078, 7.7027, 8.6053, 9.1477, 9.951, 10.4857, 11.0096],
    [0.3637, 0.9237, 1.2098, 1.4591, 1.9323, 2.3862, 2.6799, 2.9397, 3.4183, 4.0857, 4.5843, 4.8402, 5.1145, 5.5691, 6.114, 7.1723],
    [0.458, 1.0678, 1.4163, 1.7659, 2.1061, 2.3969, 2.78, 3.0187, 3.289, 3.7542, 4.1595, 4.7575, 5.1597, 5.4591, 5.8085, 6.245],
    [0.5098, 0.9308, 1.5713, 1.9576, 2.5871, 3.2892, 3.6075, 4.1148, 4.5964, 4.913, 5.3408, 5.8418, 6.3961, 6.8441, 7.2037, 7.8848],
    [0.4136, 1.488, 2.381, 2.995, 3.835, 4.501, 5.251, 6.256, 7.051, 7.803, 8.321, 8.93, 9.554, 10.45, 11.28, 12.21],
    [0.4435, 0.9978, 1.5738, 1.9584, 2.5093, 2.8436, 3.4682, 4.007, 4.5124, 4.8018, 5.2042, 5.6556, 6.1951, 6.5405, 6.9626, 7.4238],
    [0.3864, 0.7988, 1.3521, 1.741, 2.979, 3.5871, 4.0315, 4.4393, 4.7906, 5.2205, 5.4848, 5.9639, 6.2276, 6.9877, 7.3671, 7.8841],
    [0.3034, 0.7448, 1.5225, 1.8548, 2.3897, 2.9473, 3.5104, 3.9207, 5.0316, 5.4708, 5.8362, 6.4962, 6.9372, 7.3883, 7.8472, 8.087],
    [0.438, 0.6979, 1.0515, 1.3526, 1.7998, 2.5514, 2.8274, 3.3908, 3.7159, 4.0906, 4.8296, 5.4145, 5.759, 6.1419, 6.5086, 6.8823],
    [0.5056, 0.8349, 1.2864, 1.5191, 1.9148, 2.2697, 2.7763, 3.4153, 3.7814, 4.1053, 4.3754, 4.6295, 5.3831, 5.8391, 6.1618, 6.6247],
]
LASER_TIMES = [250.0 * (i + 1) for i in range(16)]

def tr(kind, v):
    return v if kind == 'none' else math.log(v) if kind == 'log' else math.sqrt(v)

def inv(kind, u):
    return u if kind == 'none' else math.exp(u) if kind == 'log' else (u * u if u >= 0 else float('nan'))

def linear_unit(times, values, g, h, df, rising):
    x = np.array([tr(h, t) for t in times]); y = np.array([tr(g, v) for v in values])
    b, a = np.polyfit(x, y, 1)
    sse = float(np.sum((y - a - b * x) ** 2))
    sign = 1 if rising else -1
    if b * sign <= 0: return [a, b], sse, ('never', max(times))
    t = inv(h, (tr(g, df) - a) / b)
    if not (t > 0): return [a, b], sse, ('before', min(times))
    return [a, b], sse, ('at', t)

def nonlinear_unit(times, values, form, df, rising):
    t = np.array(times); y = np.array(values)
    if form == 'asymptotic':
        f = lambda t, d, k: d * (1 - np.exp(-k * t))
        p0 = [y.max() * 1.5, 1.0 / t.max()]
    else:
        f = lambda t, a, b: a * t ** b
        p0 = [y[-1] / t[-1], 1.0]
    p, _ = optimize.curve_fit(f, t, y, p0=p0, maxfev=200000, xtol=1e-15, ftol=1e-15)
    sse = float(np.sum((y - f(t, *p)) ** 2))
    c, s = p
    sign = 1 if rising else -1
    if c * s * sign <= 0: return list(p), sse, ('never', max(times))
    r = df / c
    if form == 'asymptotic':
        cross = -math.log1p(-r) / s if 0 < r < 1 else None
    else:
        cross = r ** (1 / s) if r > 0 else None
    if cross is None: return list(p), sse, ('never', max(times))
    return list(p), sse, ('at', cross)

def life_fit(crossings, family):
    """Weibull or lognormal MLE with right censoring ('never' units)."""
    ex = np.array([c[1] for c in crossings if c[0] == 'at'])
    rc = np.array([c[1] for c in crossings if c[0] == 'never'])
    def nll(th):
        mu, ls = th; s = math.exp(ls)
        d = stats.weibull_min(c=1 / s, scale=math.exp(mu)) if family == 'weibull' else \
            stats.lognorm(s=s, scale=math.exp(mu))
        return -(np.sum(d.logpdf(ex)) + np.sum(d.logsf(rc)))
    lt = np.log(np.concatenate([ex, rc]))
    r = optimize.minimize(nll, [lt.mean(), math.log(lt.std() + 0.1)], method='Nelder-Mead',
                          options=dict(xatol=1e-12, fatol=1e-13, maxiter=50000))
    r = optimize.minimize(nll, r.x, method='Nelder-Mead', options=dict(xatol=1e-12, fatol=1e-13, maxiter=50000))
    mu, ls = r.x
    return {'mu': mu, 'sigma': math.exp(ls), 'loglik': -r.fun}

def unit_case(units, times_of, model, g, h, df, rising, families):
    out = {'units': [{'times': times_of(i), 'values': u} for i, u in enumerate(units)], 'model': model,
           'response': g, 'time': h, 'threshold': df, 'rising': rising, 'paths': []}
    crossings = []
    for i, u in enumerate(units):
        if model == 'linear':
            p, sse, cr = linear_unit(times_of(i), u, g, h, df, rising)
        else:
            p, sse, cr = nonlinear_unit(times_of(i), u, model, df, rising)
        crossings.append(cr)
        out['paths'].append({'parameters': p, 'sse': sse, 'kind': cr[0], 'time': cr[1]})
    out['life'] = {f: life_fit(crossings, f) for f in families}
    return out

cases = {}
cases['laser_linear'] = unit_case(LASER, lambda i: LASER_TIMES, 'linear', 'none', 'none', 10.0, True,
                                  ['weibull', 'lognormal'])

# Invented: asymptotic growth towards a limit, threshold 0.5 (two units level off below it).
rng = np.random.default_rng(7)
asym_times = [0, 50, 100, 200, 400, 700, 1000, 1500]
asym = []
for d, k in [(0.9, 0.0021), (0.7, 0.0030), (1.1, 0.0012), (0.45, 0.004), (0.8, 0.0009), (0.6, 0.0025), (0.42, 0.002)]:
    asym.append([round(d * (1 - math.exp(-k * t)) + 0.01 * rng.standard_normal(), 4) if t > 0 else 0.0
                 for t in asym_times])
cases['asymptotic'] = unit_case(asym, lambda i: asym_times, 'asymptotic', 'none', 'none', 0.5, True, ['lognormal'])

# Invented: power-law wear, threshold 2.0.
pow_times = [10, 20, 40, 80, 160, 320]
powr = []
for a, b in [(0.05, 0.62), (0.08, 0.55), (0.03, 0.70), (0.06, 0.58), (0.04, 0.66), (0.07, 0.60)]:
    powr.append([round(a * t ** b * (1 + 0.02 * rng.standard_normal()), 4) for t in pow_times])
cases['power'] = unit_case(powr, lambda i: pow_times, 'power', 'none', 'none', 2.0, True, ['weibull'])

# Invented: light output falling, log response on sqrt(hours), failure at 70% of initial.
lo_times = [0, 250, 500, 1000, 2000, 3000, 4000]
lo = []
for r in [0.0045, 0.0038, 0.0052, 0.0029, 0.0061, 0.0041, 0.0035, 0.0049]:
    lo.append([round(100 * math.exp(-r * math.sqrt(t)) * (1 + 0.004 * rng.standard_normal()), 3) for t in lo_times])
cases['light_output'] = unit_case(lo, lambda i: lo_times, 'linear', 'log', 'sqrt', 70.0, False,
                                  ['weibull', 'lognormal'])

# Random-effects path model (statsmodels MixedLM, REML).
import statsmodels.formula.api as smf
import pandas as pd
re_times = [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0]
re_units = []
for i in range(10):
    b0 = 1.0 + 0.3 * rng.standard_normal(); b1 = 1.5 + 0.35 * rng.standard_normal()
    re_units.append([round(b0 + b1 * t + 0.15 * rng.standard_normal(), 4) for t in re_times])
rows = [(i, t, v) for i, u in enumerate(re_units) for t, v in zip(re_times, u)]
df = pd.DataFrame(rows, columns=['unit', 't', 'y'])
m = smf.mixedlm('y ~ t', df, groups=df['unit'], re_formula='~t').fit(reml=True, method=['lbfgs', 'bfgs', 'nm'],
                                                                   maxiter=20000)
beta = [float(m.fe_params['Intercept']), float(m.fe_params['t'])]
cov = np.array(m.cov_re)
DF = 8.0
def F(t):
    mean = beta[0] + beta[1] * t - DF
    v = cov[0, 0] + 2 * cov[0, 1] * t + cov[1, 1] * t * t
    return float(stats.norm.cdf(mean / math.sqrt(v)))
grid = [2.0, 3.0, 4.0, 5.0, 7.0]
def quant(p):
    return optimize.brentq(lambda t: F(t) - p, 0.01, 1e3, xtol=1e-12)
cases['random_effects'] = {
    'units': [{'times': re_times, 'values': u} for u in re_units], 'threshold': DF, 'rising': True,
    'beta': beta, 'cov_re': [cov[0, 0], cov[0, 1], cov[1, 1]], 'scale': float(m.scale),
    'probabilities': [{'t': t, 'F': F(t)} for t in grid],
    'quantiles': [{'p': p, 't': quant(p)} for p in [0.1, 0.5]],
}

# Destructive degradation: adhesive bond strength (newtons), one unit per measurement.
# ln(strength) falls linearly in sqrt(weeks) at a rate b1*exp(b2*(x - x_use)), x = 1/kT
# (Escobar, Meeker, Kugler & Kramer 2003; Meeker & Escobar 1998, sec. 13.9).
DFD, USE = 50.0, 30.0
x_of = lambda temp: INV_K / (temp + 273.15)
XU = x_of(USE)
dd = []
for temp in [50.0, 60.0, 70.0]:
    rate = -0.02 * math.exp(-0.7 * (x_of(temp) - XU))
    for wk in [0, 2, 4, 8, 16, 32]:
        for rep in range(3):
            v = 90.0 * math.exp(rate * math.sqrt(wk) + 0.04 * rng.standard_normal())
            dd.append((float(wk), round(v, 2), temp))
def mu_of(th, wk, temp):
    return th[0] + th[1] * math.exp(th[2] * (x_of(temp) - XU)) * math.sqrt(wk)
DD_W = np.array([d[0] for d in dd]); DD_Y = np.log([d[1] for d in dd]); DD_X = np.array([x_of(d[2]) - XU for d in dd])
def nll(th):
    mu = th[0] + th[1] * np.exp(th[2] * DD_X) * np.sqrt(DD_W)
    return -float(np.sum(stats.norm.logpdf(DD_Y, mu, math.exp(th[3]))))
r = optimize.minimize(nll, [math.log(90.0), -0.05, -0.5, math.log(0.1)], method='Nelder-Mead',
                      options=dict(xatol=1e-12, fatol=1e-13, maxiter=200000, maxfev=200000))
for _ in range(4):
    r = optimize.minimize(nll, r.x, method='Nelder-Mead',
                          options=dict(xatol=1e-12, fatol=1e-13, maxiter=200000, maxfev=200000))
r2 = optimize.minimize(nll, r.x, method='BFGS', options=dict(gtol=1e-10))
if r2.fun < r.fun: r = r2
th = r.x
def hess(f, x, h=1e-4):
    n = len(x); H = np.zeros((n, n))
    for i in range(n):
        for j in range(n):
            ei = np.zeros(n); ej = np.zeros(n); ei[i] = h; ej[j] = h
            H[i, j] = (f(x + ei + ej) - f(x + ei - ej) - f(x - ei + ej) + f(x - ei - ej)) / (4 * h * h)
    return H
covd = np.linalg.inv(hess(nll, th, 1e-5))
lg = math.log(DFD)
def tq(t, p):
    rate = t[1]  # the rate at use conditions
    hval = (lg - math.exp(t[3]) * stats.norm.ppf(p) - t[0]) / rate
    return hval * hval if hval > 0 else float('nan')
def tp(t, wk):
    return stats.norm.cdf((lg - mu_of(t, wk, USE)) / math.exp(t[3]))
def dse(g):
    gr = np.zeros(4)
    for i in range(4):
        e = np.zeros(4); e[i] = 1e-6
        gr[i] = (g(th + e) - g(th - e)) / 2e-6
    return math.sqrt(gr @ covd @ gr)
qs = []
for p in [0.1, 0.5]:
    est = tq(th, p); se = dse(lambda t: math.log(tq(t, p)))
    qs.append({'p': p, 'estimate': est, 'wald': [est * math.exp(-Z * se), est * math.exp(Z * se)]})
fs = []
for wk in [500.0, 1000.0]:
    est = tp(th, wk); lgt = lambda v: math.log(v / (1 - v)); se = dse(lambda t: lgt(tp(t, wk)))
    ex = lambda v: 1 / (1 + math.exp(-v))
    fs.append({'t': wk, 'estimate': est, 'wald': [ex(lgt(est) - Z * se), ex(lgt(est) + Z * se)]})
cases['destructive'] = {
    'times': [d[0] for d in dd], 'values': [d[1] for d in dd], 'stresses': [d[2] for d in dd],
    'threshold': DFD, 'use': USE, 'theta': list(th), 'se': list(np.sqrt(np.diag(covd))), 'loglik': -r.fun,
    'quantiles': qs, 'probabilities': fs,
}

with open(__file__.replace('.py', '.json'), 'w') as fh:
    json.dump(cases, fh, indent=1)
print('wrote', len(cases), 'cases')
for k, v in cases.items():
    if 'life' in v: print(k, v['life'], [p['kind'] for p in v['paths']])
print(cases['random_effects']['beta'], cases['random_effects']['cov_re'], cases['random_effects']['quantiles'])
print(cases['destructive']['theta'], cases['destructive']['quantiles'])
