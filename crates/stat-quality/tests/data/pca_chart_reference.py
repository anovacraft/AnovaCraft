"""Independent reference values for the principal-components monitoring chart.

Run from this directory: python3 -I pca_chart_reference.py  (writes pca_chart_reference.json)

Computed with NumPy and SciPy from the published definitions, without the
SVD route the crate uses: the model is the eigen-decomposition (numpy.linalg.eigh)
of the reference correlation (or covariance) matrix.

* T² = sum_a t_a^2 / lambda_a; limits: ((m-1)^2/m) Beta_{1-alpha}(A/2, (m-A-1)/2)
  for reference rows and A(m+1)(m-1)/(m(m-A)) F_{1-alpha}(A, m-A) for monitored rows
  (Tracy, Young & Mason 1992).
* SPE = ||z - P P' z||^2; Jackson & Mudholkar (1979) limit, and Box's g chi2(h).
* Contributions: z_j sum_a t_a p_ja / lambda_a (T²) and e_j^2 (SPE).
* Cross-validation: element-wise EKF scheme (Bro, Kjeldahl, Smilde & Kiers 2008),
  venetian-blind folds, scores of a left-out row from the other variables by lstsq.
* Change point: Lambda_tau = n log(det(S0)/det(W_tau/n)) computed with
  determinants (Sullivan & Woodall 2000), on the retained scores.
"""
import json
import numpy as np
from scipy import stats

rng = np.random.default_rng(20261019)


def simulate(n_ref=40, n_mon=20, p=6):
    load = rng.normal(0, 1, (p, 2))
    f = rng.normal(0, 1, (n_ref + n_mon, 2))
    x = 10 + f @ load.T + rng.normal(0, 0.3, (n_ref + n_mon, p))
    x[n_ref + 8:, 2] += 1.5          # a broken correlation in one sensor (SPE)
    x[n_ref + 14:, :] += 2.0 * load[:, 0]  # a shift along the model (T²)
    return np.round(x, 4)


def model(xr, standardize):
    mean = xr.mean(axis=0)
    scale = xr.std(axis=0, ddof=1) if standardize else np.ones(xr.shape[1])
    z = (xr - mean) / scale
    c = z.T @ z / (len(xr) - 1)
    lam, vec = np.linalg.eigh(c)
    order = np.argsort(lam)[::-1]
    lam, vec = lam[order], vec[:, order]
    for k in range(vec.shape[1]):
        big = vec[np.argmax(np.abs(vec[:, k])), k]
        if big < 0:
            vec[:, k] = -vec[:, k]
    return mean, scale, lam, vec


def ekf_press(xr, standardize, folds, max_a):
    n, p = xr.shape
    press = np.zeros(max_a)
    for f in range(folds):
        train = xr[[i for i in range(n) if i % folds != f]]
        mean, scale, lam, vec = model(train, standardize)
        for i in range(f, n, folds):
            z = (xr[i] - mean) / scale
            for a in range(1, max_a + 1):
                P = vec[:, :a]
                for j in range(p):
                    keep = [k for k in range(p) if k != j]
                    t, *_ = np.linalg.lstsq(P[keep], z[keep], rcond=None)
                    press[a - 1] += (z[j] - P[j] @ t) ** 2
    return press


def jm_limit(res, alpha):
    t1, t2, t3 = (np.sum(res ** k) for k in (1, 2, 3))
    h0 = 1 - 2 * t1 * t3 / (3 * t2 ** 2)
    c = stats.norm.ppf(1 - alpha)
    return t1 * (c * np.sqrt(2 * t2 * h0 ** 2) / t1 + 1 + t2 * h0 * (h0 - 1) / t1 ** 2) ** (1 / h0)


def box_limit(res, alpha):
    t1, t2 = np.sum(res), np.sum(res ** 2)
    return t2 / t1 * stats.chi2.ppf(1 - alpha, t1 ** 2 / t2)


def change_point(y):
    n = len(y)
    s0 = np.cov(y.T, bias=True).reshape(y.shape[1], y.shape[1])
    out = []
    for tau in range(1, n):
        a, b = y[:tau], y[tau:]
        w = (a - a.mean(0)).T @ (a - a.mean(0)) + (b - b.mean(0)).T @ (b - b.mean(0))
        out.append(n * np.log(np.linalg.det(s0) / np.linalg.det(w.reshape(s0.shape) / n)))
    return np.array(out)


def case(x, m, standardize, a, alpha, folds):
    mean, scale, lam, vec = model(x[:m], standardize)
    p = x.shape[1]
    max_a = min(p - 1, m - int(np.ceil(m / folds)) - 1, m - 2)
    press = ekf_press(x[:m], standardize, folds, max_a)
    z = (x - mean) / scale
    P = vec[:, :a]
    t = z @ P
    t2 = np.sum(t ** 2 / lam[:a], axis=1)
    e = z - t @ P.T
    spe = np.sum(e ** 2, axis=1)
    beta = stats.beta.ppf(1 - alpha, a / 2, (m - a - 1) / 2)
    ref_limit = (m - 1) ** 2 / m * beta
    mon_limit = a * (m + 1) * (m - 1) / (m * (m - a)) * stats.f.ppf(1 - alpha, a, m - a)
    res = np.clip(lam[a:], 0, None)
    signals = [i for i in range(len(x)) if t2[i] > (ref_limit if i < m else mon_limit)]
    i_sig = signals[-1] if signals else len(x) - 1
    ct2 = z[i_sig] * ((t[i_sig] / lam[:a]) @ P.T)
    cspe = e[i_sig] ** 2
    cp = change_point(t)
    return {
        'x': x.tolist(), 'm': m, 'standardize': standardize, 'components': a, 'alpha': alpha, 'folds': folds,
        'eigenvalues': lam.tolist(), 'press': press.tolist(), 'best_components': int(np.argmin(press)) + 1,
        't2': t2.tolist(), 'spe': spe.tolist(), 't2_reference_limit': ref_limit, 't2_monitoring_limit': mon_limit,
        'spe_jackson_mudholkar': jm_limit(res, alpha), 'spe_box': box_limit(res, alpha),
        'contribution_point': i_sig, 't2_contributions': ct2.tolist(), 'spe_contributions': cspe.tolist(),
        'change_statistics': cp.tolist(), 'change_before': int(np.argmax(cp)) + 1,
    }


x = simulate()
cases = [case(x, 40, True, 2, 0.0027, 7), case(x, 40, False, 3, 0.01, 5)]
with open('pca_chart_reference.json', 'w') as fh:
    json.dump(cases, fh, indent=1)
print([c['best_components'] for c in cases], [c['press'] for c in cases])
print(cases[0]['spe_jackson_mudholkar'], cases[0]['spe_box'], cases[0]['change_before'])
