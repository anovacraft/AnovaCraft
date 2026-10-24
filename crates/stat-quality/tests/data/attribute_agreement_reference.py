"""Reference values for stat-quality's attribute agreement and attribute
gauge studies.

Run from the repository root:

    python3 crates/stat-quality/tests/data/attribute_agreement_reference.py

It writes attribute_agreement_reference.json beside this script.

Published examples (tables as reproduced in statsmodels 0.15's own tests,
statsmodels/stats/tests/test_inter_rater.py):

* Fleiss (1971), Psychol. Bull. 76, Table 1: 30 patients, 6 psychiatrists,
  5 diagnoses; Fleiss reports kappa = 0.430. statsmodels fleiss_kappa gives
  kappa; category kappas and the Fleiss, Nee & Landis (1979) null standard
  errors are recomputed here with NumPy from their formulas.
* The 10 subjects x 14 raters example (kappa = 0.210).
* A SAS PROC FREQ printout of the 3 x 3 table [[0,4,1],[0,8,0],[0,1,5]]:
  simple kappa 0.4842 (ASE 0.1380, ASE under H0 0.1484) and weighted kappa
  (Cicchetti-Allison weights) 0.4701 (ASE 0.1457, ASE under H0 0.1426).

A simulated study (seeded; 3 appraisers x 2 trials x 25 parts x 4 ordered
categories with a standard) is analysed here independently:

* percent agreement with Clopper-Pearson limits from scipy.stats.beta.ppf;
* Fleiss' kappa with statsmodels fleiss_kappa (plus NumPy SEs as above);
* Cohen's kappa (unweighted, linear, quadratic) with statsmodels
  cohens_kappa (kappa, std_kappa, std_kappa0);
* Kendall's W from scipy.stats.rankdata mid-ranks and the tie-corrected
  formula of Siegel & Castellan (1988) eq. 9.16, with its chi-square test;
* Kendall's tau-b with scipy.stats.kendalltau (method='asymptotic').

The attribute gauge study (AIAG MSA 4th ed. analytic method) is
re-implemented with numpy.polyfit and scipy.stats on counts simulated from a
normal gauge curve.
"""
import json, os
import numpy as np
from scipy import stats
from statsmodels.stats.inter_rater import cohens_kappa, fleiss_kappa

rng = np.random.default_rng(20261016)
out = {}

diagnoses = np.array([
    [4, 4, 4, 4, 4, 4], [2, 2, 2, 5, 5, 5], [2, 3, 3, 3, 3, 5], [5, 5, 5, 5, 5, 5],
    [2, 2, 2, 4, 4, 4], [1, 1, 3, 3, 3, 3], [3, 3, 3, 3, 5, 5], [1, 1, 3, 3, 3, 4],
    [1, 1, 4, 4, 4, 4], [5, 5, 5, 5, 5, 5], [1, 4, 4, 4, 4, 4], [1, 2, 4, 4, 4, 4],
    [2, 2, 2, 3, 3, 3], [1, 4, 4, 4, 4, 4], [2, 2, 4, 4, 4, 5], [3, 3, 3, 3, 3, 5],
    [1, 1, 1, 4, 5, 5], [1, 1, 1, 1, 1, 2], [2, 2, 4, 4, 4, 4], [1, 3, 3, 5, 5, 5],
    [5, 5, 5, 5, 5, 5], [2, 4, 4, 4, 4, 4], [2, 2, 4, 5, 5, 5], [1, 1, 4, 4, 4, 4],
    [1, 4, 4, 4, 4, 5], [2, 2, 2, 2, 2, 4], [1, 1, 1, 1, 5, 5], [2, 2, 4, 4, 4, 4],
    [1, 3, 3, 3, 3, 3], [5, 5, 5, 5, 5, 5],
])


def counts_of(ratings, c):
    """n x m ratings coded 0..c-1 -> n x c counts."""
    return np.array([[np.sum(r == j) for j in range(c)] for r in ratings], float)


def fleiss_full(counts):
    counts = np.asarray(counts, float)
    n, c = counts.shape
    m = counts[0].sum()
    p = counts.sum(0) / (n * m)
    pq = p * (1 - p)
    base = n * m * (m - 1)
    kj = [1 - (counts[:, j] * (m - counts[:, j])).sum() / (base * pq[j]) if pq[j] > 0 else None for j in range(c)]
    kappa = fleiss_kappa(counts)
    se_j = np.sqrt(2 / base)
    se = np.sqrt(2) * np.sqrt(pq.sum() ** 2 - (pq * (1 - 2 * p)).sum()) / (pq.sum() * np.sqrt(base))
    return {"kappa": kappa, "se0": se, "category_kappa": kj, "category_se0": se_j}


out["fleiss_1971"] = {"counts": counts_of(diagnoses - 1, 5).tolist(), **fleiss_full(counts_of(diagnoses - 1, 5))}
wiki = np.array([
    [0, 0, 0, 0, 14], [0, 2, 6, 4, 2], [0, 0, 3, 5, 6], [0, 3, 9, 2, 0], [2, 2, 8, 1, 1],
    [7, 7, 0, 0, 0], [3, 2, 6, 3, 0], [2, 5, 3, 2, 2], [6, 5, 2, 1, 0], [0, 2, 2, 3, 7],
], float)
out["fleiss_wiki"] = {"counts": wiki.tolist(), **fleiss_full(wiki)}


def cohen(table, wt):
    r = cohens_kappa(np.asarray(table, float), wt=wt) if wt else cohens_kappa(np.asarray(table, float))
    return {"kappa": float(r.kappa), "se": float(r.std_kappa), "se0": float(r.std_kappa0)}


sas = [[0, 4, 1], [0, 8, 0], [0, 1, 5]]
out["sas"] = {"table": sas, "none": cohen(sas, None), "linear": cohen(sas, "linear"),
              "quadratic": cohen(sas, "quadratic")}

# ---------------------------------------------------------------- simulated study
n_parts, k, m, c = 25, 3, 2, 4
truth = rng.normal(0, 1, n_parts)
cuts = np.array([-0.8, 0.0, 0.8])
standard = np.searchsorted(cuts, truth)
ratings = np.zeros((k, n_parts, m), int)
skill = [0.35, 0.5, 0.7]
shift = [0.0, 0.15, -0.1]
for a in range(k):
    for t in range(m):
        ratings[a, :, t] = np.searchsorted(cuts, truth + shift[a] + rng.normal(0, skill[a], n_parts))


def cp(x, n, level=0.95):
    a = 1 - level
    lo = 0.0 if x == 0 else stats.beta.ppf(a / 2, x, n - x + 1)
    hi = 1.0 if x == n else stats.beta.ppf(1 - a / 2, x + 1, n - x)
    return {"matched": int(x), "percent": 100 * x / n, "lower": 100 * lo, "upper": 100 * hi}


def kendall_w(sets):
    sets = np.asarray(sets, float)
    J, n = sets.shape
    R = np.array([stats.rankdata(s) for s in sets]).sum(0)
    T = 0.0
    for s in sets:
        _, t = np.unique(s, return_counts=True)
        T += (t ** 3 - t).sum()
    W = 12 * ((R - R.mean()) ** 2).sum() / (J ** 2 * (n ** 3 - n) - J * T)
    chi = J * (n - 1) * W
    return {"w": W, "chi_square": chi, "p_value": stats.chi2.sf(chi, n - 1)}


def tau(x, y):
    r = stats.kendalltau(x, y, variant="b", method="asymptotic")
    return {"tau_b": r.statistic, "p_value": r.pvalue}


def versus(apps):
    table = np.zeros((c, c))
    xs, ys = [], []
    matched = 0
    for p in range(n_parts):
        allr = np.concatenate([ratings[a, p] for a in apps])
        for r in allr:
            table[standard[p], r] += 1
            xs.append(r)
            ys.append(standard[p])
        matched += np.all(allr == standard[p])
    cat = []
    for j in range(c):
        t2 = np.array([[table[j, j], table[j].sum() - table[j, j]],
                       [table[:, j].sum() - table[j, j], 0.0]])
        t2[1, 1] = table.sum() - t2.sum()
        cat.append(cohen(t2, None))
    return {"agreement": cp(matched, n_parts), "table": table.tolist(), "none": cohen(table, None),
            "linear": cohen(table, "linear"), "quadratic": cohen(table, "quadratic"),
            "category": cat, "tau": tau(xs, ys)}


study = {"ratings": ratings.tolist(), "standard": standard.tolist(), "categories": c}
study["within"] = []
for a in range(k):
    matched = int(sum(np.all(ratings[a, p] == ratings[a, p, 0]) for p in range(n_parts)))
    study["within"].append({"agreement": cp(matched, n_parts), **fleiss_full(counts_of(ratings[a], c)),
                            "concordance": kendall_w(ratings[a].T)})
study["versus"] = [versus([a]) for a in range(k)]
study["all_versus"] = versus(list(range(k)))
allr = np.concatenate([ratings[a] for a in range(k)], axis=1)  # parts x (k m)
matched = int(sum(np.all(allr[p] == allr[p, 0]) for p in range(n_parts)))
sets = [ratings[a, :, t] for a in range(k) for t in range(m)]
pair = []
for a in range(k):
    for b in range(a + 1, k):
        tab = np.zeros((c, c))
        for p in range(n_parts):
            for t in range(m):
                tab[ratings[a, p, t], ratings[b, p, t]] += 1
        pair.append(cohen(tab, None))
study["between"] = {"agreement": cp(matched, n_parts), **fleiss_full(counts_of(allr, c)),
                    "concordance": kendall_w(sets), "pairwise": pair}
out["study"] = study

# ---------------------------------------------------------------- attribute gauge
ref = np.round(np.linspace(-0.065, -0.015, 16), 4)
limit = -0.040
gauge_mu, gauge_sd = -0.0385, 0.008
acc = rng.binomial(20, stats.norm.cdf((ref - gauge_mu) / gauge_sd)).astype(float)
pa = np.where(acc == 0, 0.0, np.where(acc == 20, 1.0, np.where(acc / 20 < 0.5, (acc + 0.5) / 20,
              np.where(acc / 20 > 0.5, (acc - 0.5) / 20, 0.5))))
inside = (acc > 0) & (acc < 20)
b1, b0 = np.polyfit(ref[inside], stats.norm.ppf(pa[inside]), 1)
x = lambda P: (stats.norm.ppf(P) - b0) / b1
bias = limit - x(0.5)
rep = abs(x(0.995) - x(0.005)) / 1.08
t = 31.3 * abs(bias) / rep
out["gauge"] = {"reference": ref.tolist(), "accepted": acc.tolist(), "trials": 20, "limit": limit,
                "adjusted": pa.tolist(), "intercept": b0, "slope": b1, "median": x(0.5), "bias": bias,
                "repeatability": rep, "t": t, "p_value": 2 * stats.t.sf(t, 19)}

path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "attribute_agreement_reference.json")
with open(path, "w") as f:
    json.dump(out, f, indent=1, default=float)
print("wrote", path)
print("Fleiss 1971 kappa", out["fleiss_1971"]["kappa"], out["fleiss_1971"]["category_kappa"],
      out["fleiss_1971"]["kappa"] / out["fleiss_1971"]["se0"])
print("gauge", acc, out["gauge"]["bias"], out["gauge"]["t"])
