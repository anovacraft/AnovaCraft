"""Reference values for tests/distance_reference.rs, written to
distance_reference.json.

* SciPy `spatial.distance.pdist` on a 14 × 6 count table: euclidean,
  cityblock, braycurtis, jaccard (presence = nonzero), and hamming (the
  proportion of differing variables; times 6 for the count).
* R `cluster::daisy(metric = "gower", type = list(asymm = "ab"))` on 16
  rows of mixed data (two numeric columns, an ordered factor with level
  codes 1-4, a nominal factor, an asymmetric binary column) with missing
  values.
* R `cmdscale(eig = TRUE)` eigenvalues and `ape::pcoa` coordinates
  (corrections "none", "lingoes", "cailliez") of the Gower and Bray-Curtis
  matrices; for the corrections also the eigenvalues of the corrected
  distances (cmdscale) and ape's correction constant, relative eigenvalues
  and broken-stick values.
* R `hclust` (single, complete, average, ward.D2) merge heights and
  three-cluster cuts of the Gower matrix, and SciPy `cluster.hierarchy.linkage`
  heights of the same matrix (ward on the condensed matrix).

Run: python3 distance_reference.py   (numpy, scipy; Rscript with cluster, ape)
"""
import json
import os
import subprocess
import tempfile

import numpy as np
from scipy.cluster.hierarchy import linkage
from scipy.spatial.distance import pdist, squareform

here = os.path.dirname(os.path.abspath(__file__))
rng = np.random.default_rng(20261019)

counts = rng.poisson(1.4, size=(14, 6)).astype(float)
counts[counts.sum(1) == 0, 0] = 1.0

n = 16
x1 = np.round(rng.normal(10, 3, n), 2)
x2 = np.round(rng.lognormal(0, 0.8, n), 3)
ord_ = rng.integers(1, 5, n).astype(float)
nom = rng.integers(1, 4, n).astype(float)
ab = (rng.random(n) < 0.35).astype(float)
x1[[2, 9]] = np.nan
x2[5] = np.nan
ord_[11] = np.nan
nom[0] = np.nan
ab[7] = np.nan
mixed = np.column_stack([x1, x2, ord_, nom, ab])

out = {
    "counts": counts.tolist(),
    "mixed": [[None if np.isnan(v) else float(v) for v in r] for r in mixed],
    "mixed_kinds": ["numeric", "numeric", "ordinal", "nominal", "asymmetric-binary"],
}
p = counts.shape[1]
out["pdist"] = {
    "euclidean": squareform(pdist(counts, "euclidean")).tolist(),
    "manhattan": squareform(pdist(counts, "cityblock")).tolist(),
    "bray-curtis": squareform(pdist(counts, "braycurtis")).tolist(),
    "jaccard": squareform(pdist(counts != 0, "jaccard")).tolist(),
    "hamming": (squareform(pdist(counts, "hamming")) * p).tolist(),
    "simple-matching": squareform(pdist(counts, "hamming")).tolist(),
}

with tempfile.TemporaryDirectory() as tmp:
    with open(os.path.join(tmp, "mixed.csv"), "w") as f:
        f.write("x1,x2,ord,nom,ab\n")
        for r in mixed:
            f.write(",".join("NA" if np.isnan(v) else repr(float(v)) for v in r) + "\n")
    with open(os.path.join(tmp, "counts.csv"), "w") as f:
        f.write(",".join(f"v{k}" for k in range(p)) + "\n")
        for r in counts:
            f.write(",".join(repr(float(v)) for v in r) + "\n")
    np.savetxt(os.path.join(tmp, "braycurtis.csv"), np.array(out["pdist"]["bray-curtis"]),
               delimiter=",", fmt="%.17g")
    subprocess.run(["Rscript", os.path.join(here, "distance_reference.R"), tmp], check=True)

    def read(name):
        a = np.loadtxt(os.path.join(tmp, name + ".csv"), delimiter=",", dtype=str, ndmin=1)
        return np.char.strip(np.char.strip(a), '"').astype(float)

    gower = read("gower")
    out["gower"] = gower.tolist()
    for tag in ["gower", "braycurtis"]:
        e = read(f"{tag}_cmdscale_eig")
        # Drop the constant vector's zero (smallest |eigenvalue|).
        e = np.delete(e, np.argmin(np.abs(e)))
        res = {"eigenvalues": e.tolist(), "none_vectors": read(f"{tag}_none_vectors").tolist()}
        for corr in ["lingoes", "cailliez"]:
            ec = read(f"{tag}_{corr}_eig")
            ec = np.delete(ec, np.argmin(np.abs(ec)))
            res[corr] = {
                "constant": float(read(f"{tag}_{corr}_constant")[0]),
                "eigenvalues": ec.tolist(),
                "positive_eigenvalues": read(f"{tag}_{corr}_corr_eig").tolist(),
                "relative": read(f"{tag}_{corr}_relative").tolist(),
                "broken_stick": read(f"{tag}_{corr}_broken_stick").tolist(),
                "vectors": read(f"{tag}_{corr}_vectors").tolist(),
            }
        out["pcoa_" + tag] = res
    out["hclust"] = {}
    cond = squareform(gower, checks=False)
    for m, sm in [("single", "single"), ("complete", "complete"), ("average", "average"), ("ward.D2", "ward")]:
        out["hclust"][m] = {
            "height": read(f"hclust_{m}_height").tolist(),
            "cut3": read(f"hclust_{m}_cut3").astype(int).tolist(),
            "scipy_height": linkage(cond, method=sm)[:, 2].tolist(),
        }

with open(os.path.join(here, "distance_reference.json"), "w") as f:
    json.dump(out, f, indent=1)
    f.write("\n")
print("wrote distance_reference.json")
