"""Reference values for the discriminant-analysis tests
(crates/stat-multivariate/src/discriminant.rs).

Run with Python 3, NumPy, SciPy and scikit-learn:

    python3 crates/stat-multivariate/tests/data/discriminant_reference.py

Data: Fisher's iris measurements (Fisher 1936, *Annals of Eugenics* 7;
public domain), as distributed with scikit-learn, written to iris.csv
(sepal length, sepal width, petal length, petal width, species 0/1/2).

Printed:
* canonical discriminant analysis from the generalized eigenproblem
  B v = λ W v (scipy.linalg.eigh) with W the within-group and B the
  between-group SSCP matrices: eigenvalues of W⁻¹B, canonical correlations,
  Wilks' Λ with Bartlett's chi-square for functions k..r, raw coefficients
  scaled to unit pooled within-group variance (vᵀ S_w v = 1 with
  S_w = W/(n − g)), standardized coefficients and structure correlations;
  cross-checked against scikit-learn's explained_variance_ratio_;
* linear discriminant posteriors (pooled covariance S_w) and quadratic
  posteriors (each group's covariance, divisor n_k − 1) from
  scipy.stats.multivariate_normal. (scikit-learn's discriminant classes
  estimate the covariances with other divisors — n for the common
  covariance, n_k for the group covariances in recent versions — so they
  are not used for the posteriors.)

Conventions: canonical coefficient vectors are compared up to sign.
"""

import numpy as np
from scipy.linalg import eigh
from scipy.stats import chi2, multivariate_normal
from sklearn.datasets import load_iris
from sklearn.discriminant_analysis import LinearDiscriminantAnalysis

np.set_printoptions(precision=15, linewidth=160)
iris = load_iris()
x, y = iris.data, iris.target
out = np.column_stack([x, y])
np.savetxt(__file__.replace("discriminant_reference.py", "iris.csv"), out, fmt=["%.1f"] * 4 + ["%d"],
           delimiter=",", header="sepal_length,sepal_width,petal_length,petal_width,species", comments="")
n, p = x.shape
g = 3
means = np.array([x[y == k].mean(0) for k in range(g)])
grand = x.mean(0)
W = sum((x[y == k] - means[k]).T @ (x[y == k] - means[k]) for k in range(g))
B = sum((y == k).sum() * np.outer(means[k] - grand, means[k] - grand) for k in range(g))
Sw = W / (n - g)
vals, vecs = eigh(B, Sw)
order = np.argsort(vals)[::-1]
vals, vecs = vals[order], vecs[:, order]
lam = vals / (n - g)  # eigenvalues of W^-1 B
r = min(p, g - 1)
lam, vecs = lam[:r], vecs[:, :r]
print("eigenvalues of W^-1 B:", repr(lam))
print("proportions:", repr(lam / lam.sum()))
print("canonical correlations:", repr(np.sqrt(lam / (1 + lam))))
for k in range(r):
    wl = np.prod(1 / (1 + lam[k:]))
    stat = -(n - 1 - (p + g) / 2) * np.log(wl)
    df = (p - k) * (g - k - 1)
    print(f"Wilks k={k}: lambda={wl!r} chi2={stat!r} df={df} p={chi2.sf(stat, df)!r}")
print("raw coefficients (columns, v' Sw v = 1):\n", repr(vecs))
print("check v' Sw v:", np.diag(vecs.T @ Sw @ vecs))
print("standardized coefficients:\n", repr(vecs * np.sqrt(np.diag(Sw))[:, None]))
print("structure correlations:\n", repr((Sw @ vecs) / np.sqrt(np.diag(Sw))[:, None]))
print("group centroids on the canonical functions (centred at the grand mean):\n", repr((means - grand) @ vecs))
lda = LinearDiscriminantAnalysis(solver="eigen").fit(x, y)
print("sklearn explained_variance_ratio_:", lda.explained_variance_ratio_)

rows = [0, 50, 70, 83, 100, 133]
prior_prop = np.bincount(y) / n


def posteriors(cov_of, priors):
    dens = np.column_stack([multivariate_normal(means[k], cov_of(k)).logpdf(x) + np.log(priors[k]) for k in range(g)])
    dens -= dens.max(1, keepdims=True)
    post = np.exp(dens)
    return post / post.sum(1, keepdims=True)


lin = posteriors(lambda k: Sw, prior_prop)
print("linear posteriors (proportional priors), rows", rows, ":\n", repr(lin[rows]))
print("linear resubstitution errors:", int((lin.argmax(1) != y).sum()))
quad = posteriors(lambda k: np.cov(x[y == k], rowvar=False), [1 / 3] * 3)
print("quadratic posteriors (equal priors), rows", rows, ":\n", repr(quad[rows]))
print("quadratic resubstitution errors:", int((quad.argmax(1) != y).sum()))
