"""Reference values for stat-discrete's stratified tables and r x c exact test.

Run from the repository root:

    python3 crates/stat-discrete/tests/data/stratified_reference.py

It writes stratified_reference.json beside this script.

* 2 x 2 x K: statsmodels 0.15 StratifiedTable (Cochran-Mantel-Haenszel test
  with and without continuity correction, Mantel-Haenszel pooled odds ratio,
  its Robins-Breslow-Greenland standard error and interval, Breslow-Day test
  with and without Tarone's adjustment). The Mantel-Haenszel pieces are also
  recomputed here from the published formulas with NumPy as a cross-check.
* r x c x K generalized CMH statistics (Landis, Heyman & Koch 1978): an
  independent NumPy implementation that forms the full vectorized count
  vector, its null mean and its Kronecker-product covariance per stratum,
  then the quadratic forms for the general-association, row-mean-score and
  correlation contrasts (np.linalg.solve).
* r x c Fisher exact test: small tables by brute-force enumeration of every
  table with the observed margins (itertools); larger ones by an exhaustive
  column-by-column convolution of the exact distribution of the table
  probability (no bounds or pruning).

Datasets:
* "rabbits": the penicillin and rabbits 2 x 2 x 5 data printed in R's
  mantelhaen.test documentation (delay of injection by cure, per penicillin
  level).
* "job_satisfaction": the 4 x 4 x 2 job satisfaction by income table, by
  gender, printed in the same documentation (Agresti 2002, Table 7.8); its
  general association statistic is 10.2 on 9 df there. Its pooled 4 x 4 table
  is R's fisher.test "Job" example.
* "trial": an invented 2 x 2 x 4 multi-centre table, and "ordinal": an
  invented 3 x 4 x 3 table (seeded random counts).
"""
import itertools
import json
import math
import os

import numpy as np
from scipy import stats
from statsmodels.stats.contingency_tables import StratifiedTable

rng = np.random.default_rng(20261016)

rabbits = [
    [[0, 6], [0, 5]],
    [[3, 3], [0, 6]],
    [[6, 0], [2, 4]],
    [[5, 1], [6, 0]],
    [[2, 0], [5, 0]],
]
trial = [
    [[11, 9], [6, 14]],
    [[16, 4], [10, 12]],
    [[7, 8], [3, 12]],
    [[20, 10], [14, 18]],
]
# Income rows (<5000, 5000-15000, 15000-25000, >25000) by satisfaction
# columns (very dissatisfied, a little, moderately, very satisfied).
female = np.array([[1, 3, 11, 2], [2, 3, 17, 3], [0, 1, 8, 5], [0, 2, 4, 2]])
male = np.array([[1, 1, 2, 1], [0, 3, 5, 1], [0, 0, 7, 3], [0, 1, 9, 6]])
job = [female.tolist(), male.tolist()]
ordinal = [rng.integers(0, 9, size=(3, 4)).tolist() for _ in range(3)]


def mh_numpy(strata, level=0.95):
    t = np.array(strata, dtype=float)
    a, b, c, d = t[:, 0, 0], t[:, 0, 1], t[:, 1, 0], t[:, 1, 1]
    n = a + b + c + d
    keep = n >= 2
    a, b, c, d, n = a[keep], b[keep], c[keep], d[keep], n[keep]
    e = (a + b) * (a + c) / n
    v = (a + b) * (c + d) * (a + c) * (b + d) / (n * n * (n - 1))
    dev = (a - e).sum()
    r, s = a * d / n, b * c / n
    p, q = (a + d) / n, (b + c) / n
    R, S = r.sum(), s.sum()
    var = (p * r).sum() / (2 * R * R) + (p * s + q * r).sum() / (2 * R * S) + (q * s).sum() / (2 * S * S)
    return {"cmh": dev * dev / v.sum(), "or": R / S, "se": math.sqrt(var)}


def two_by_two_case(name, strata):
    st = StratifiedTable([np.array(x, dtype=float) for x in strata])
    plain = st.test_null_odds(correction=False)
    corr = st.test_null_odds(correction=True)
    lo, hi = st.oddsratio_pooled_confint(alpha=0.05)
    # Breslow-Day uses the strata whose margins are all positive (a stratum
    # with a zero margin adds nothing to the Mantel-Haenszel sums either, so
    # the pooled odds ratio is unchanged).
    informative = [x for x in strata if min(np.sum(x, axis=0).min(), np.sum(x, axis=1).min()) > 0]
    sub = StratifiedTable([np.array(x, dtype=float) for x in informative])
    assert abs(sub.oddsratio_pooled - st.oddsratio_pooled) < 1e-12
    bd = sub.test_equal_odds(adjust=False)
    tarone = sub.test_equal_odds(adjust=True)
    mine = mh_numpy(strata)
    assert abs(mine["cmh"] - plain.statistic) < 1e-10
    assert abs(mine["or"] - st.oddsratio_pooled) < 1e-10
    return {
        "name": name,
        "strata": strata,
        "cmh": plain.statistic,
        "cmh_p": plain.pvalue,
        "cmh_corrected": corr.statistic,
        "cmh_corrected_p": corr.pvalue,
        "odds_ratio": st.oddsratio_pooled,
        "log_or_se": st.logodds_pooled_se,
        "log_or_se_numpy": mine["se"],
        "lower": lo,
        "upper": hi,
        "breslow_day_df": len(informative) - 1,
        "breslow_day": bd.statistic,
        "breslow_day_p": bd.pvalue,
        "tarone": tarone.statistic,
        "tarone_p": tarone.pvalue,
    }


def cmh_general(strata, u, a):
    strata = [np.array(t, dtype=float) for t in strata]
    r, c = strata[0].shape

    def contrast(ar, ac):
        b = np.kron(ar, ac)
        g = np.zeros(b.shape[0])
        v = np.zeros((b.shape[0], b.shape[0]))
        for t in strata:
            n = t.sum()
            if n < 2:
                continue
            p = t.sum(axis=1) / n
            q = t.sum(axis=0) / n
            m = n * np.kron(p, q)
            cov = n * n / (n - 1) * np.kron(np.diag(p) - np.outer(p, p), np.diag(q) - np.outer(q, q))
            g += b @ (t.reshape(-1) - m)
            v += b @ cov @ b.T
        stat = float(g @ np.linalg.solve(v, g))
        df = b.shape[0]
        return {"statistic": stat, "df": df, "p": float(stats.chi2.sf(stat, df))}

    first_r = np.eye(r)[: r - 1]
    first_c = np.eye(c)[: c - 1]
    return {
        "correlation": contrast(np.array([u], dtype=float), np.array([a], dtype=float)),
        "row_mean_scores": contrast(first_r, np.array([a], dtype=float)),
        "general_association": contrast(first_r, first_c),
    }


def ln_fact(n):
    return math.lgamma(n + 1)


def tables_with_margins(rows, cols):
    """Every table (list of rows) with the given margins."""
    r, c = len(rows), len(cols)
    cells = []

    def go(i, remaining_cols, acc):
        if i == r - 1:
            if all(x >= 0 for x in remaining_cols) and sum(remaining_cols) == rows[-1]:
                yield acc + [list(remaining_cols)]
            return
        for row in itertools.product(*[range(min(rows[i], rc) + 1) for rc in remaining_cols]):
            if sum(row) == rows[i]:
                yield from go(i + 1, [x - y for x, y in zip(remaining_cols, row)], acc + [list(row)])

    yield from go(0, list(cols), cells)


def fisher_brute(t):
    rows = [sum(x) for x in t]
    cols = [sum(x[j] for x in t) for j in range(len(t[0]))]
    n = sum(rows)
    k = sum(map(ln_fact, rows)) + sum(map(ln_fact, cols)) - ln_fact(n)
    obs = sum(ln_fact(x) for row in t for x in row)
    p = 0.0
    count = 0
    for tab in tables_with_margins(rows, cols):
        count += 1
        v = sum(ln_fact(x) for row in tab for x in row)
        if v >= obs - 1e-7:
            p += math.exp(k - v)
    return p, count


def fisher_convolution(t):
    """Exact distribution of sum ln n_ij!, column by column, no pruning."""
    rows = tuple(sum(x) for x in t)
    cols = [sum(x[j] for x in t) for j in range(len(t[0]))]
    n = sum(rows)
    k = sum(map(ln_fact, rows)) + sum(map(ln_fact, cols)) - ln_fact(n)
    obs = sum(ln_fact(x) for row in t for x in row)
    def splits(rem, total):
        if len(rem) == 1:
            if total <= rem[0]:
                yield (total,)
            return
        rest = sum(rem[1:])
        for x in range(max(0, total - rest), min(rem[0], total) + 1):
            for tail in splits(rem[1:], total - x):
                yield (x,) + tail

    layer = {(rows, 0): (0.0, 1)}  # (remaining rows, quantized value) -> (value, multiplicity)
    for col in cols:
        nxt = {}
        for (rem, _), (val, mult) in layer.items():
            for split in splits(rem, col):
                v = val + sum(map(ln_fact, split))
                key = (tuple(x - y for x, y in zip(rem, split)), round(v * 1e9))
                old = nxt.get(key, (v, 0))
                nxt[key] = (v, old[1] + mult)
        layer = nxt
    p = 0.0
    for (_, _), (v, mult) in layer.items():
        if v >= obs - 1e-7:
            p += mult * math.exp(k - v)
    return p


small = [
    [[3, 1, 4], [1, 5, 2]],
    [[2, 0, 3], [1, 4, 0], [0, 2, 5]],
    [[1, 2, 3, 4], [4, 3, 2, 1]],
    [[0, 3, 1], [2, 2, 2], [3, 0, 1], [1, 1, 4]],
    [[4, 0, 0], [0, 4, 0], [0, 0, 4]],
    [[2, 7], [8, 2]],
]
fisher = []
for t in small:
    p, count = fisher_brute(t)
    assert abs(p - fisher_convolution(t)) < 1e-12
    fisher.append({"table": t, "p": p, "tables": count, "method": "enumeration"})
medium = rng.integers(0, 6, size=(3, 5)).tolist()
fisher.append({"table": medium, "p": fisher_convolution(medium), "method": "convolution"})

out = {
    "two_by_two": [two_by_two_case("rabbits", rabbits), two_by_two_case("trial", trial)],
    "general": [
        {"name": "job_satisfaction", "strata": job, "row_scores": [1, 2, 3, 4], "column_scores": [1, 2, 3, 4],
         **cmh_general(job, [1, 2, 3, 4], [1, 2, 3, 4])},
        {"name": "ordinal", "strata": ordinal, "row_scores": [1, 2, 4], "column_scores": [0, 1, 2, 5],
         **cmh_general(ordinal, [1, 2, 4], [0, 1, 2, 5])},
        {"name": "trial_as_general", "strata": trial, "row_scores": [1, 2], "column_scores": [1, 2],
         **cmh_general(trial, [1, 2], [1, 2])},
    ],
    "fisher": fisher,
}
path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "stratified_reference.json")
with open(path, "w") as f:
    json.dump(out, f, indent=1)
print(json.dumps({g["name"]: g["general_association"]["statistic"] for g in out["general"]}))
print([round(x["p"], 6) for x in fisher])
