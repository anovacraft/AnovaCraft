"""Independent reference values for crates/stat-multivariate/tests/imputation_reference.rs.

Written separately from the Rust code with NumPy only (whole-matrix
operations, `numpy.linalg.solve` for the conditional regressions and
`numpy.linalg.svd` for the low-rank fits), following the published
algorithms:

* EM for the multivariate normal with missing values (Dempster, Laird &
  Rubin 1977; Little & Rubin 2019, §11.2): E-step conditional means and
  covariances, M-step mean and ML covariance; run to a change of 1e-14.
* Iterative low-rank SVD imputation (Troyanskaya et al. 2001): start at
  the observed column means, recentre each iteration, refit the missing
  cells from the leading k singular triples of the standardized matrix.
* Regularized iterative PCA (Josse & Husson 2012, J. SFdS 153(2)): as
  above, but each kept singular value s is shrunk to s - n*sigma2/s, with
  sigma2 = n p / min(p, n-1) * sum(s_discarded**2 / n)
           / ((n-1) p - (n-1) k - p k + k**2),
  capped at s_{k+1}**2 / n. Checked on the 24 x 4 table and on a 40 x 4
  standard-normal table (20% missing) where the unregularized rank-2
  iteration drifts and never settles.

Run: python3 imputation_reference.py  (prints the Rust constants).
"""
import numpy as np

rng = np.random.default_rng(20261016)
n, p = 24, 4
cov = np.array([[4.0, 2.4, 1.0, -1.2], [2.4, 9.0, 3.0, 0.5], [1.0, 3.0, 2.25, 0.3], [-1.2, 0.5, 0.3, 1.0]])
x = rng.multivariate_normal([10.0, 50.0, 3.0, -2.0], cov, size=n).round(2)
mask = rng.random((n, p)) < 0.2
mask[:, 0] &= np.arange(n) % 3 != 0  # keep column 0 mostly observed
mask[mask.all(axis=1), 0] = False
data = np.where(mask, np.nan, x)


def em(data, tol=1e-14, max_iter=100000):
    obs = ~np.isnan(data)
    mu = np.nanmean(data, axis=0)
    sigma = np.diag(np.nanvar(data, axis=0))
    used = obs.any(axis=1)
    d = data[used]
    o_all = obs[used]
    m = d.shape[0]
    for _ in range(max_iter):
        t1 = np.zeros(p)
        t2 = np.zeros((p, p))
        for row, o in zip(d, o_all):
            mi = ~o
            xh = row.copy()
            c = np.zeros((p, p))
            if mi.any():
                s_oo = sigma[np.ix_(o, o)]
                s_mo = sigma[np.ix_(mi, o)]
                coef = np.linalg.solve(s_oo, s_mo.T).T
                xh[mi] = mu[mi] + coef @ (row[o] - mu[o])
                c[np.ix_(mi, mi)] = sigma[np.ix_(mi, mi)] - coef @ s_mo.T
            t1 += xh
            t2 += np.outer(xh, xh) + c
        new_mu = t1 / m
        new_sigma = t2 / m - np.outer(new_mu, new_mu)
        done = np.max(np.abs(new_sigma - sigma)) < tol and np.max(np.abs(new_mu - mu)) < tol
        mu, sigma = new_mu, new_sigma
        if done:
            break
    filled = data.copy()
    for i in range(n):
        o = obs[i]
        mi = ~o
        if mi.any():
            coef = np.linalg.solve(sigma[np.ix_(o, o)], sigma[np.ix_(mi, o)].T).T
            filled[i, mi] = mu[mi] + coef @ (data[i, o] - mu[o])
    return mu, sigma, filled


def svd_impute(data, k, tol=1e-28, max_iter=200000, regularize=False):
    n, p = data.shape
    miss = np.isnan(data)
    sd = np.nanstd(data, axis=0, ddof=1)
    z = np.where(miss, np.nanmean(data, axis=0), data) / sd
    for _ in range(max_iter):
        means = z.mean(axis=0)
        u, s, vt = np.linalg.svd(z - means, full_matrices=False)
        d = s[:k]
        if regularize:
            df = (n - 1) * p - (n - 1) * k - p * k + k * k
            sigma2 = n * p / min(p, n - 1) * np.sum(s[k:] ** 2 / n) / df
            sigma2 = min(sigma2, s[k] ** 2 / n)
            d = d - n * sigma2 / d
        fit = means + (u[:, :k] * d) @ vt[:k]
        change = np.sum((fit[miss] - z[miss]) ** 2) / np.sum(z**2)
        z[miss] = fit[miss]
        if change < tol:
            return z * sd, True
    return z * sd, False


def rust(name, a):
    a = np.atleast_2d(a)
    rows = ",\n    ".join("[" + ", ".join("NAN" if np.isnan(v) else repr(float(v)) for v in r) + "]" for r in a)
    return f"const {name}: [[f64; {a.shape[1]}]; {a.shape[0]}] = [\n    {rows},\n];"


mu, sigma, filled = em(data)
svd2, _ = svd_impute(data, 2)
svd2_reg, _ = svd_impute(data, 2, regularize=True)

# A table on which the unregularized rank-2 iteration drifts.
rng2 = np.random.default_rng(3)
wild = rng2.normal(size=(40, 4)).round(2)
wild_mask = rng2.random((40, 4)) < 0.2
wild_mask[wild_mask.all(axis=1), 0] = False
wild = np.where(wild_mask, np.nan, wild)
hard, hard_ok = svd_impute(wild, 2, tol=1e-8, max_iter=1000)
assert not hard_ok, "the unregularized fit was expected to drift on this table"
print(f"// unregularized: largest |imputed| {np.nanmax(np.abs(hard)):.1f}, data within +-{np.nanmax(np.abs(wild)):.2f}")
wild_reg, wild_ok = svd_impute(wild, 2, regularize=True)
assert wild_ok
print(rust("DATA", data))
print(rust("EM_MEAN", mu))
print(rust("EM_COV", sigma))
print(rust("EM_FILLED", filled))
print(rust("SVD2_FILLED", svd2))
print(rust("SVD2_REG_FILLED", svd2_reg))
print(rust("WILD", wild))
print(rust("WILD_REG_FILLED", wild_reg))
