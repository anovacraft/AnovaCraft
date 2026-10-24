"""Reference values for the UMAP tests (src/umap/tests.rs).

Uses the 60 five-dimensional points of tsne_data.csv. With exact nearest
neighbours (scikit-learn NearestNeighbors, each point its own first
neighbour, k = 10), umap-learn's fuzzy_simplicial_set gives each point's
rho and sigma and the symmetric fuzzy graph; find_ab_params gives the curve
parameters a, b. umap-learn computes in float32 and bisects sigma to 1e-5,
so agreement is to about 1e-5 relative.

Run: python3 umap_reference.py   (numpy, scikit-learn, umap-learn 0.5)
"""
import os
import numpy as np
from sklearn.neighbors import NearestNeighbors
import umap.umap_ as u

here = os.path.dirname(os.path.abspath(__file__))
X = np.loadtxt(os.path.join(here, "tsne_data.csv"), delimiter=",", skiprows=1)[:, :5]
k = 10
nn = NearestNeighbors(n_neighbors=k).fit(X)
d, i = nn.kneighbors(X)
G, sigmas, rhos = u.fuzzy_simplicial_set(X, k, np.random.RandomState(0), "euclidean", knn_indices=i, knn_dists=d)
G = G.toarray()
for a in [0, 25, 59]:
    print(f"rho[{a}] = {float(rhos[a])!r}, sigma[{a}] = {float(sigmas[a])!r}")
for a, b in [(0, int(i[0, 1])), (0, int(i[0, 9])), (25, int(i[25, 5])), (59, int(i[59, 3]))]:
    print(f"w[{a}][{b}] = {float(G[a, b])!r}")
print("nonzeros =", int((G > 0).sum()))
for s, m in [(1.0, 0.1), (1.0, 0.5), (2.0, 0.0)]:
    print(f"ab(spread {s}, min_dist {m}) =", [repr(float(v)) for v in u.find_ab_params(s, m)])
