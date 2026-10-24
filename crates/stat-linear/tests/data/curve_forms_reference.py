"""Independent reference fits for the curve-library forms added to
stat-linear's `CurveForm` (straight line, dose-response, asymmetric
logistic, Weibull and beta growth, segmented line, two exponentials,
Gaussian and Lorentzian peaks).

Each case's data are generated here from the stated model with a fixed seed
(invented data: x on a fixed grid, y = f(x; truth) + normal noise, rounded to
4 decimals). The rounded data are printed as Rust literals and are the data
the Rust test fits.

Every case is fitted with `scipy.optimize.curve_fit` (MINPACK
Levenberg-Marquardt, method='lm', xtol = ftol = gtol = 1e-15) from the
generating parameters, with the analytic Jacobian written here independently
of the Rust code. Standard errors are curve_fit's sqrt(diag(pcov)) with
absolute_sigma=False, i.e. s * sqrt(diag((J'J)^-1)), s^2 = SSE/(n - P).
The straight line is also checked against numpy's least-squares solution.

Run:  python3 curve_forms_reference.py   (prints the Rust case table)
"""
import numpy as np
from scipy import optimize

LN10 = np.log(10.0)


def line(x, a, b):
    return a + b * x


def line_j(x, a, b):
    return np.column_stack([np.ones_like(x), x])


def drl(x, bot, top, logec50, hill):
    return bot + (top - bot) / (1 + 10 ** ((logec50 - x) * hill))


def drl_j(x, bot, top, logec50, hill):
    u = 10 ** ((logec50 - x) * hill)
    q = 1 / (1 + u)
    dq = -q * q * u * LN10  # d q / d z, z = (logec50 - x) * hill
    s = top - bot
    return np.column_stack([1 - q, q, s * dq * hill, s * dq * (logec50 - x)])


def dr(x, bot, top, ec50, hill):
    with np.errstate(divide='ignore'):
        return bot + (top - bot) / (1 + (ec50 / x) ** hill)


def dr_j(x, bot, top, ec50, hill):
    # At x = 0 the curve is at its limit and the shape derivatives vanish.
    pos = x > 0
    xs = np.where(pos, x, 1.0)
    u = (ec50 / xs) ** hill
    q = np.where(pos, 1 / (1 + u), 0.0 if hill > 0 else 1.0)
    w = np.where(pos, q * q * u, 0.0)
    s = top - bot
    return np.column_stack([1 - q, q, -s * w * hill / ec50, -s * w * np.log(ec50 / xs)])


def l5(x, bot, top, logc, hill, asym):
    return bot + (top - bot) / (1 + 10 ** ((logc - x) * hill)) ** asym


def l5_j(x, bot, top, logc, hill, asym):
    u = 10 ** ((logc - x) * hill)
    d = 1 + u
    q = d ** (-asym)
    s = top - bot
    dqz = -asym * q * u / d * LN10
    return np.column_stack([1 - q, q, s * dqz * hill, s * dqz * (logc - x), -s * q * np.log(d)])


def weib(x, a, i, r, k):
    return a - (a - i) * np.exp(-(r * x) ** k)


def weib_j(x, a, i, r, k):
    w = (r * x) ** k
    e = np.exp(-w)
    with np.errstate(divide='ignore', invalid='ignore'):
        lg = np.where(x > 0, np.log(r * x), 0.0)
    return np.column_stack([1 - e, e, (a - i) * e * k * w / r, (a - i) * e * w * lg])


def beta(x, ymax, te, tm):
    d = te - tm
    xc = np.minimum(x, te)
    return ymax * (1 + (te - xc) / d) * (xc / te) ** (te / d)


def beta_j(x, ymax, te, tm):
    # Derivatives of ln g with respect to End and Peak, times f.
    d = te - tm
    inside = x <= te
    xc = np.minimum(x, te)
    h1 = 1 + (te - xc) / d
    g = h1 * (xc / te) ** (te / d)
    with np.errstate(divide='ignore', invalid='ignore'):
        lr = np.where(xc > 0, np.log(xc / te), 0.0)
    dte = ymax * g * ((xc - tm) / (d * d * h1) - tm * lr / (d * d) - 1 / d)
    dtm = ymax * g * ((te - xc) / (d * d * h1) + te * lr / (d * d))
    dte = np.where(inside, dte, 0.0)
    dtm = np.where(inside, dtm, 0.0)
    return np.column_stack([g, dte, dtm])


def seg(x, a, s1, s2, bp):
    return a + s1 * x + (s2 - s1) * np.maximum(x - bp, 0)


def seg_j(x, a, s1, s2, bp):
    h = np.maximum(x - bp, 0)
    return np.column_stack([np.ones_like(x), x - h, h, np.where(x > bp, -(s2 - s1), 0.0)])


def two(x, p, a1, k1, a2, k2):
    return p + a1 * np.exp(-k1 * x) + a2 * np.exp(-k2 * x)


def two_j(x, p, a1, k1, a2, k2):
    e1 = np.exp(-k1 * x)
    e2 = np.exp(-k2 * x)
    return np.column_stack([np.ones_like(x), e1, -a1 * x * e1, e2, -a2 * x * e2])


def gauss(x, b, h, c, w):
    return b + h * np.exp(-(x - c) ** 2 / (2 * w * w))


def gauss_j(x, b, h, c, w):
    e = np.exp(-(x - c) ** 2 / (2 * w * w))
    return np.column_stack([np.ones_like(x), e, h * e * (x - c) / w ** 2, h * e * (x - c) ** 2 / w ** 3])


def lor(x, b, h, c, g):
    return b + h / (1 + ((x - c) / g) ** 2)


def lor_j(x, b, h, c, g):
    v = (x - c) / g
    d = 1 + v * v
    return np.column_stack([np.ones_like(x), 1 / d, h * 2 * v / (g * d * d), h * 2 * v * v / (g * d * d)])


CASES = [
    # name, key, f, jac, x, truth, noise sd, seed
    ('line', 'line', line, line_j, np.linspace(0, 10, 12), [2.5, 1.3], 0.4, 1),
    ('dose_response_log', 'dose_response_log', drl, drl_j, np.linspace(-9, -3, 19),
     [5.0, 95.0, -6.0, 1.2], 2.5, 2),
    ('inhibition_log', 'dose_response_log', drl, drl_j, np.linspace(-9, -3, 19),
     [8.0, 102.0, -5.5, -0.9], 2.5, 3),
    ('dose_response', 'dose_response', dr, dr_j,
     np.array([0, 0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500, 1000.0]),
     [3.0, 48.0, 12.0, 1.1], 1.2, 4),
    ('logistic5', 'logistic5', l5, l5_j, np.linspace(-3, 3, 25), [1.0, 20.0, 0.5, 1.0, 0.35], 0.15, 5),
    ('weibull_growth', 'weibull_growth', weib, weib_j, np.linspace(0, 20, 21), [40.0, 2.0, 0.15, 2.2], 0.6, 6),
    ('beta_growth', 'beta_growth', beta, beta_j, np.linspace(0, 100, 26), [30.0, 70.0, 42.0], 0.5, 7),
    ('segmented', 'segmented', seg, seg_j, np.linspace(0, 20, 23), [4.0, 1.5, -0.6, 11.3], 0.5, 8),
    ('two_exponentials', 'two_exponentials', two, two_j, np.linspace(0, 20, 31),
     [2.0, 30.0, 1.5, 12.0, 0.15], 0.25, 9),
    ('gaussian_peak', 'gaussian_peak', gauss, gauss_j, np.linspace(400, 600, 26), [3.0, 50.0, 497.0, 22.0], 1.0, 10),
    ('lorentzian_peak', 'lorentzian_peak', lor, lor_j, np.linspace(-10, 10, 27), [1.5, 12.0, 0.8, 1.7], 0.3, 11),
]


def rust(v):
    return '[' + ', '.join(repr(float(t)) for t in v) + ']'


def main():
    for name, key, f, jac, x, truth, sd, seed in CASES:
        rng = np.random.default_rng(20261018 + seed)
        y = np.round(f(x, *truth) + rng.normal(0, sd, x.size), 4)
        x = np.round(x, 6)
        est, cov = optimize.curve_fit(f, x, y, p0=truth, jac=lambda xx, *t: jac(xx, *t), method='lm',
                                      xtol=1e-15, ftol=1e-15, gtol=1e-15, maxfev=100000)
        # Refine with a few Gauss-Newton steps (exact to rounding).
        for _ in range(5):
            r = y - f(x, *est)
            J = jac(x, *est)
            step = np.linalg.lstsq(J, r, rcond=None)[0]
            est = est + step
        r = y - f(x, *est)
        J = jac(x, *est)
        s2 = r @ r / (x.size - len(truth))
        se = np.sqrt(np.diag(s2 * np.linalg.pinv(J.T @ J)))
        se_cf = np.sqrt(np.diag(cov))
        assert np.allclose(se, se_cf, rtol=1e-7), (name, se, se_cf)
        if key == 'line':
            ols = np.linalg.lstsq(np.column_stack([np.ones_like(x), x]), y, rcond=None)[0]
            assert np.allclose(ols, est, rtol=1e-12)
        print(f'    // {name}: sse = {float(r @ r)!r}')
        print(f'    Case {{ name: "{name}", key: "{key}",')
        print(f'        x: &{rust(x)},')
        print(f'        y: &{rust(y)},')
        print(f'        estimates: &{rust(est)},')
        print(f'        se: &{rust(se)} }},')


if __name__ == '__main__':
    main()
