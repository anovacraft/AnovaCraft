"""Reference values for the multidimensional scaling tests (src/mds/tests.rs).

The dissimilarities are city-block (L1) distances between 10 points in 3-D,
rounded to 3 decimals: not Euclidean, so B has negative eigenvalues.

* Classical scaling: eigenvalues of B = -1/2 J D^2 J by numpy.linalg.eigh,
  and the 2-D coordinates' pairwise distances (sign-free).
* Metric SMACOF: scikit-learn's manifold.smacof (metric, raw stress) started
  from the classical 2-D solution, 5000 iterations; its raw stress and the
  pairwise distances of its configuration.

Run: python3 mds_reference.py   (numpy, scikit-learn)
"""
import numpy as np
from sklearn.manifold import smacof

rng = np.random.default_rng(7)
P = rng.normal(size=(10, 3)) * [3, 2, 1]
D = np.round(np.abs(P[:, None, :] - P[None, :, :]).sum(-1), 3)
n = len(D)
print("// dissimilarities")
for r in D:
    print("vec![" + ", ".join(f"{v:.3f}" for v in r) + "],")
J = np.eye(n) - 1 / n
B = -0.5 * J @ (D ** 2) @ J
w, V = np.linalg.eigh(B)
w, V = w[::-1], V[:, ::-1]
print("// eigenvalues", [repr(float(v)) for v in w])
X = V[:, :2] * np.sqrt(w[:2])
def pd(X):
    return [repr(float(np.linalg.norm(X[i] - X[j]))) for i in range(n) for j in range(i)]
print("// classical distances (lower triangle by rows)", pd(X))
Y, stress = smacof(D, metric=True, n_components=2, init=X, n_init=1, max_iter=5000, eps=1e-15, normalized_stress=False)
print("// smacof raw stress", repr(float(stress)))
print("// smacof distances", pd(Y))
