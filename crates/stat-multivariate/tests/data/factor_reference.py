"""Reference values for the factor-analysis tests (crates/stat-multivariate/src/factor.rs).

Run with Python 3, NumPy, SciPy and statsmodels:

    python3 crates/stat-multivariate/tests/data/factor_reference.py

The data are simulated (seed 20261016): six ability tests driven by two
correlated latent factors, n = 300. The script prints the correlation matrix
(pasted into the Rust test with 17 significant digits) and, computed from it:

* maximum-likelihood loadings and uniquenesses from statsmodels
  (`Factor(method="ml")`, which maximises the likelihood jointly over loadings
  and uniquenesses and returns the Lawley-Maxwell canonical form with
  Λᵀ Ψ⁻¹ Λ diagonal), cross-checked against a direct minimisation of the
  concentrated discrepancy over the uniquenesses (scipy L-BFGS-B);
* the Bartlett-corrected chi-square test of fit;
* principal-axis loadings with iterated communalities from statsmodels
  (`Factor(method="pa", smc=True)`);
* varimax without normalisation from statsmodels (`rotate_factors`), and
  varimax with Kaiser normalisation and promax (power 4) following
  Kaiser (1958) and Hendrickson & White (1964) written out in NumPy
  (statsmodels' promax uses a different normalisation and power 2);
* regression (Thomson) and Bartlett factor scores of the first rows.

Conventions: loadings are compared up to the sign of each column.
"""

import numpy as np
from scipy.optimize import minimize
from scipy.stats import chi2
from statsmodels.multivariate.factor import Factor
from statsmodels.multivariate.factor_rotation import rotate_factors

np.set_printoptions(precision=12, suppress=False, linewidth=160)
rng = np.random.default_rng(20261016)
n, p, m = 300, 6, 2
phi = np.array([[1.0, 0.4], [0.4, 1.0]])
lam = np.array([[0.8, 0.0], [0.7, 0.1], [0.75, 0.0], [0.0, 0.8], [0.1, 0.65], [0.0, 0.7]])
f = rng.multivariate_normal(np.zeros(2), phi, size=n)
x = f @ lam.T + rng.normal(size=(n, p)) * np.sqrt(1 - np.sum((lam @ phi) * lam, axis=1))
x = np.round(10 * x + 50, 1)
r = np.corrcoef(x, rowvar=False)
# The data themselves, read by the Rust tests.
np.savetxt(__file__.replace("factor_reference.py", "factor_abilities.csv"), x, fmt="%.1f", delimiter=",",
           header="t1,t2,t3,t4,t5,t6", comments="")


def fmt(a):
    return np.array2string(np.asarray(a), separator=", ", precision=17, floatmode="maxprec")


print("data (first 5 rows):\n", fmt(x[:5]))
print("means:", fmt(x.mean(0)))
print("sds:", fmt(x.std(0, ddof=1)))
print("R =\n", fmt(r))
print("eigenvalues of R:", fmt(np.linalg.eigvalsh(r)[::-1]))

# --- maximum likelihood ---------------------------------------------------
ml = Factor(corr=r, n_factor=m, method="ml", nobs=n).fit(opt={"gtol": 1e-12, "maxiter": 10000})
print("statsmodels ML loadings:\n", fmt(ml.loadings))
print("statsmodels ML uniquenesses:", fmt(ml.uniqueness))


def concentrated(psi):
    d = 1 / np.sqrt(psi)
    e = np.linalg.eigvalsh(r * np.outer(d, d))[::-1][m:]
    return np.sum(e - np.log(e) - 1)


start = (1 - 0.5 * m / p) / np.diag(np.linalg.inv(r))
opt = minimize(concentrated, start, method="L-BFGS-B", bounds=[(0.005, 1)] * p,
               options={"ftol": 1e-15, "gtol": 1e-12, "maxiter": 10000})
print("direct minimisation uniquenesses:", fmt(opt.x), "F =", opt.fun)
fmin = concentrated(ml.uniqueness)
stat = (n - 1 - (2 * p + 5) / 6 - 2 * m / 3) * fmin
df = ((p - m) ** 2 - p - m) / 2
print("ML F =", repr(fmin), "chi2 =", repr(stat), "df =", df, "p =", repr(chi2.sf(stat, df)))

# --- principal axis ---------------------------------------------------------
pa = Factor(corr=r, n_factor=m, method="pa", smc=True, nobs=n).fit(maxiter=10000, tol=1e-14)
print("statsmodels PA loadings:\n", fmt(pa.loadings))
print("statsmodels PA communalities:", fmt(pa.communality))

# --- rotations --------------------------------------------------------------
a = np.real(ml.loadings)
v_raw, _ = rotate_factors(a, "varimax")
print("statsmodels varimax (no normalisation):\n", fmt(v_raw))


def varimax(x, normalize=True, eps=1e-14):
    x = np.real(np.asarray(x)).astype(float)
    k = x.shape[1]
    sc = np.sqrt((x ** 2).sum(1)) if normalize else np.ones(x.shape[0])
    x = x / sc[:, None]
    t = np.eye(k)
    d = 0
    for _ in range(10000):
        z = x @ t
        b = x.T @ (z ** 3 - z @ np.diag((z ** 2).sum(0)) / x.shape[0])
        u, s, vt = np.linalg.svd(b)
        t = u @ vt
        dpast, d = d, s.sum()
        if d < dpast * (1 + eps):
            break
    return (x @ t) * sc[:, None], t


def promax(x, power=4):
    v, t = varimax(x)
    q = v * np.abs(v) ** (power - 1)
    u = np.linalg.lstsq(v, q, rcond=None)[0]
    d = np.diag(np.linalg.inv(u.T @ u))
    u = u @ np.diag(np.sqrt(d))
    pattern = v @ u
    rot = t @ u
    tinv = np.linalg.inv(rot)
    return pattern, tinv @ tinv.T


vk, _ = varimax(a)
print("varimax (Kaiser normalisation):\n", fmt(vk))
pm, phi_hat = promax(a)
print("promax pattern (power 4):\n", fmt(pm))
print("promax factor correlations:\n", fmt(phi_hat))
print("promax structure:\n", fmt(pm @ phi_hat))

# --- factor scores ------------------------------------------------------------
z = (x - x.mean(0)) / x.std(0, ddof=1)
reg = z @ np.linalg.solve(r, vk)
print("regression scores (varimax), first 3 rows:\n", fmt(reg[:3]))
psi_inv = np.diag(1 / ml.uniqueness)
bart = z @ psi_inv @ vk @ np.linalg.inv(vk.T @ psi_inv @ vk)
print("Bartlett scores (varimax), first 3 rows:\n", fmt(bart[:3]))
reg_obl = z @ np.linalg.solve(r, pm @ phi_hat)
print("regression scores (promax), first 3 rows:\n", fmt(reg_obl[:3]))
