"""Reference values for the t-SNE tests (src/tsne/tests.rs) and the
trustworthiness test.

Writes tsne_data.csv: 60 points in 5-D (three simulated normal clusters,
columns x1..x5) and a fixed 2-D layout (columns y1, y2: the first two
coordinates plus noise) used for trustworthiness.

Prints:
* exact joint affinities P (perplexity 10) at a few entries, with each row's
  conditional distribution calibrated by scipy.optimize.brentq to the exact
  perplexity in float64, and checks them against scikit-learn's
  _joint_probabilities (float32 bisection, so to ~1e-4 relative);
* the sparse (nearest-neighbour) affinities for perplexity 5 at a few
  entries, built as scikit-learn's Barnes-Hut t-SNE builds them
  (k = floor(3 perplexity) + 1 neighbours) but calibrated in float64;
* sklearn.manifold.trustworthiness of the fixed layout for k = 5.

Run: python3 tsne_reference.py   (numpy, scipy, scikit-learn)
"""
import os
import numpy as np
from scipy.optimize import brentq
from scipy.spatial.distance import squareform, cdist
from sklearn.manifold import trustworthiness
from sklearn.manifold._t_sne import _joint_probabilities

here = os.path.dirname(os.path.abspath(__file__))
rng = np.random.default_rng(42)
centres = np.array([[0, 0, 0, 0, 0], [6, 0, 2, 0, 0], [0, 5, 0, 3, -2]], float)
X = np.vstack([c + rng.normal(size=(20, 5)) for c in centres])
Y = X[:, :2] + 0.8 * rng.normal(size=(60, 2))
X = np.array([[float(repr(float(v))) for v in r] for r in X])
Y = np.array([[float(repr(float(v))) for v in r] for r in Y])
with open(os.path.join(here, "tsne_data.csv"), "w") as f:
    f.write("x1,x2,x3,x4,x5,y1,y2\n")
    for a, b in zip(X, Y):
        f.write(",".join(repr(float(v)) for v in list(a) + list(b)) + "\n")
n = len(X)
D2 = cdist(X, X, "sqeuclidean")


def cond(d2, perp):
    d2 = d2 - d2.min()
    def h(logb):
        w = np.exp(-d2 * np.exp(logb))
        p = w / w.sum()
        return -(p[p > 0] * np.log(p[p > 0])).sum() - np.log(perp)
    lb = brentq(h, -30, 30, xtol=1e-15, rtol=1e-15)
    w = np.exp(-d2 * np.exp(lb))
    return w / w.sum()


C = np.zeros((n, n))
for i in range(n):
    idx = [j for j in range(n) if j != i]
    C[i, idx] = cond(D2[i, idx], 10.0)
P = (C + C.T) / (2 * n)
sk = squareform(_joint_probabilities(D2, 10.0, 0))
assert np.max(np.abs(sk - P) / np.maximum(P, 1e-12)) < 1e-3 or np.max(np.abs(sk - P)) < 1e-7
for (i, j) in [(0, 1), (0, 59), (10, 11), (30, 45), (59, 58)]:
    print(f"exact P[{i}][{j}] = {P[i, j]!r}")

k = int(3 * 5.0) + 1
Cs = np.zeros((n, n))
for i in range(n):
    order = sorted([j for j in range(n) if j != i], key=lambda j: (D2[i, j], j))[:k]
    Cs[i, order] = cond(D2[i, order], 5.0)
Ps = Cs + Cs.T
Ps /= Ps.sum()
for (i, j) in [(0, 1), (0, 59), (10, 11), (30, 45), (59, 58)]:
    print(f"sparse P[{i}][{j}] = {Ps[i, j]!r}")
print("sparse nonzeros =", int((Ps > 0).sum()))
print("trustworthiness k=5:", repr(float(trustworthiness(X, Y, n_neighbors=5))))
