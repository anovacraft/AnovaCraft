"""Reference values for the Gaussian mixture tests (stat-multivariate/src/mixture/tests.rs).

Writes mixture_data.csv (150 simulated points in 2-D from three elongated,
unequal normal clusters, with the generating cluster as a starting
partition) and prints, for each covariance structure, the converged EM
optimum reached from that partition:

* VII, VVI, EEE, VVV: scikit-learn's GaussianMixture ('spherical', 'diag',
  'tied', 'full'; reg_covar = 0), started from the M-step of the partition
  and iterated to tol 1e-13.
* EII, EEI (not in scikit-learn): a direct NumPy EM written here from the
  Celeux & Govaert (1995) M-step formulas.

Run: python3 mixture_reference.py   (numpy, scipy, scikit-learn)
"""
import os
import numpy as np
from scipy.stats import multivariate_normal
from sklearn.mixture import GaussianMixture
from sklearn.mixture._gaussian_mixture import _estimate_gaussian_parameters

here = os.path.dirname(os.path.abspath(__file__))
rng = np.random.default_rng(20261016)
specs = [
    (60, [0.0, 0.0], [[4.0, 3.0], [3.0, 3.5]]),
    (50, [7.0, 1.0], [[1.0, -0.6], [-0.6, 0.8]]),
    (40, [2.0, 8.0], [[0.5, 0.0], [0.0, 2.5]]),
]
X, lab = [], []
for g, (m, mu, cov) in enumerate(specs):
    X.append(rng.multivariate_normal(mu, cov, size=m))
    lab += [g] * m
X = np.vstack(X)
lab = np.array(lab)
with open(os.path.join(here, "mixture_data.csv"), "w") as f:
    f.write("x,y,start\n")
    for (a, b), g in zip(X, lab):
        f.write(f"{float(a)!r},{float(b)!r},{g}\n")
X = np.array([[float(repr(float(v))) for v in r] for r in X])
n, p = X.shape
K = 3
Z0 = np.eye(K)[lab]


def mstep(X, Z, kind):
    nk = Z.sum(0)
    mu = (Z.T @ X) / nk[:, None]
    W = [((Z[:, k:k + 1] * (X - mu[k])).T @ (X - mu[k])) for k in range(K)]
    Wt = sum(W)
    if kind == "EII":
        cov = [np.eye(p) * np.trace(Wt) / (n * p)] * K
    elif kind == "EEI":
        cov = [np.diag(np.diag(Wt) / n)] * K
    return nk / n, mu, cov


def em(kind):
    Z = Z0
    ll_old = -np.inf
    for it in range(100000):
        w, mu, cov = mstep(X, Z, kind)
        dens = np.column_stack([w[k] * multivariate_normal(mu[k], cov[k]).pdf(X) for k in range(K)])
        ll = np.log(dens.sum(1)).sum()
        Z = dens / dens.sum(1, keepdims=True)
        if abs(ll - ll_old) < 1e-14 * abs(ll):
            break
        ll_old = ll
    return ll, w, mu, cov


def show(kind, ll, w, mu, cov, m):
    print(f"// {kind}: log-likelihood, BIC (m = {m}), weights, means, covariance of each component")
    print(f"{kind}: ll = {float(ll)!r}, bic = {float(-2 * ll + m * np.log(n))!r}")
    print("  weights", [repr(float(v)) for v in w])
    print("  means", [[repr(float(v)) for v in r] for r in mu])
    print("  cov", [[[repr(float(v)) for v in r] for r in c] for c in cov])


for kind in ["EII", "EEI"]:
    ll, w, mu, cov = em(kind)
    m = (K - 1) + K * p + (1 if kind == "EII" else p)
    show(kind, ll, w, mu, cov, m)

for kind, sk in [("VII", "spherical"), ("VVI", "diag"), ("EEE", "tied"), ("VVV", "full")]:
    nk, mu0, cov0 = _estimate_gaussian_parameters(X, Z0, 0.0, sk)
    prec0 = 1.0 / cov0 if sk in ("spherical", "diag") else np.linalg.inv(cov0)
    gm = GaussianMixture(
        K, covariance_type=sk, reg_covar=0.0, tol=1e-13, max_iter=100000,
        weights_init=nk / n, means_init=mu0, precisions_init=prec0,
    ).fit(X)
    ll = gm.score(X) * n
    if sk == "spherical":
        cov = [np.eye(p) * v for v in gm.covariances_]
    elif sk == "diag":
        cov = [np.diag(v) for v in gm.covariances_]
    elif sk == "tied":
        cov = [gm.covariances_] * K
    else:
        cov = list(gm.covariances_)
    show(kind, ll, gm.weights_, gm.means_, cov, gm._n_parameters())
    assert abs(gm.bic(X) - (-2 * ll + gm._n_parameters() * np.log(n))) < 1e-8
