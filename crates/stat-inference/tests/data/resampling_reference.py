"""Reference values for the bootstrap and permutation tests (tests/resampling.rs).

Run from the repository root:

    python3 crates/stat-inference/tests/data/resampling_reference.py

It writes resampling_reference.json beside this script. Requires SciPy (>= 1.11)
and NumPy. Values come from:

* BCa: given a fixed vector of bootstrap replicates (drawn here with NumPy and
  stored in the JSON, so the Rust test uses identical replicates), the adjusted
  percentiles alpha_1, alpha_2 and acceleration a_hat from SciPy's own
  `scipy.stats._resampling._bca_interval` (the routine behind
  `scipy.stats.bootstrap(method='BCa')`), and the interval limits as
  `numpy.percentile` of the replicates at those percentiles, as `bootstrap`
  computes them. An independent NumPy transcription of Efron & Tibshirani
  eqs. 14.14-14.15 and 15.36 is checked against SciPy here as well.
* Permutation tests: `scipy.stats.permutation_test` with exact enumeration
  (`n_resamples=np.inf`), permutation types 'independent', 'samples' (paired)
  and 'pairings' (correlation).
"""

import json
import os

import numpy as np
import scipy
from scipy import stats
from scipy.special import ndtr, ndtri
from scipy.stats._resampling import _bca_interval

try:  # recent SciPy passes an array namespace
    from scipy._lib._array_api import array_namespace

    XP = array_namespace(np.asarray([1.0]))
except Exception:  # pragma: no cover
    XP = np

rng = np.random.default_rng(20261016)
LEVEL = 0.9
ALPHA = (1 - LEVEL) / 2


def bca_case(name, samples, statistic, B=2000):
    samples = [np.asarray(s, dtype=float) for s in samples]
    # Stratified resampling: each sample resampled within itself.
    reps = np.empty(B)
    for b in range(B):
        res = [s[rng.integers(0, len(s), len(s))] for s in samples]
        reps[b] = statistic(*res, axis=-1)
    theta = float(statistic(*samples, axis=-1))
    a1, a2, a_hat = _bca_interval(tuple(samples), statistic, axis=-1, alpha=ALPHA, theta_hat_b=reps, batch=None, xp=XP)
    a1, a2, a_hat = float(np.asarray(a1)), float(np.asarray(a2)), float(np.asarray(a_hat))
    # Independent transcription (Efron & Tibshirani 14.14-14.15, 15.36).
    share = (np.sum(reps < theta) + np.sum(reps <= theta)) / (2 * B)
    z0 = ndtri(share)
    num = den = 0.0
    for j, s in enumerate(samples):
        jack = []
        for i in range(len(s)):
            rest = list(samples)
            rest[j] = np.delete(s, i)
            jack.append(statistic(*rest, axis=-1))
        jack = np.asarray(jack)
        n = len(s)
        u = (n - 1) * (jack.mean() - jack)
        num += np.sum(u**3) / n**3
        den += np.sum(u**2) / n**2
    a_mine = num / (6 * den**1.5)
    za = ndtri(ALPHA)
    a1_mine = ndtr(z0 + (z0 + za) / (1 - a_mine * (z0 + za)))
    a2_mine = ndtr(z0 + (z0 - za) / (1 - a_mine * (z0 - za)))
    assert abs(a_mine - a_hat) < 1e-12 and abs(a1_mine - a1) < 1e-12 and abs(a2_mine - a2) < 1e-12
    return {
        "name": name,
        "samples": [s.tolist() for s in samples],
        "estimate": theta,
        "replicates": reps.tolist(),
        "level": LEVEL,
        "z0": float(z0),
        "acceleration": a_hat,
        "alpha_lower": a1,
        "alpha_upper": a2,
        "bca": [float(np.percentile(reps, 100 * a1)), float(np.percentile(reps, 100 * a2))],
        "percentile": [float(np.percentile(reps, 100 * ALPHA)), float(np.percentile(reps, 100 * (1 - ALPHA)))],
        "mean": float(reps.mean()),
        "std_error": float(reps.std(ddof=1)),
    }


def mean_diff(x, y, axis=-1):
    return np.mean(x, axis=axis) - np.mean(y, axis=axis)


def median_diff(x, y, axis=-1):
    return np.median(x, axis=axis) - np.median(y, axis=axis)


def paired_mean(x, y, axis=-1):
    return np.mean(x - y, axis=axis)


def pearson(x, y, axis=-1):
    xc = x - np.mean(x, axis=axis, keepdims=True)
    yc = y - np.mean(y, axis=axis, keepdims=True)
    return np.sum(xc * yc, axis=axis) / np.sqrt(np.sum(xc**2, axis=axis) * np.sum(yc**2, axis=axis))


def perm(name, kind, data, statistic, ptype, extra=None):
    out = {"name": name, "kind": kind, "data": [np.asarray(d).tolist() for d in data], **(extra or {})}
    for alt in ["two-sided", "less", "greater"]:
        r = stats.permutation_test(
            data, statistic, permutation_type=ptype, vectorized=True, n_resamples=np.inf, alternative=alt, batch=20000
        )
        out[alt] = float(r.pvalue)
        out["observed"] = float(r.statistic)
        out["arrangements"] = int(len(r.null_distribution))
    return out


skewed = np.round(rng.exponential(3.0, 20), 3)
first = np.round(rng.lognormal(1.0, 0.6, 9), 3)
second = np.round(rng.lognormal(1.4, 0.6, 11), 3)
bca = [
    bca_case("mean", [skewed], lambda x, axis=-1: np.mean(x, axis=axis)),
    bca_case("std-dev", [skewed], lambda x, axis=-1: np.std(x, ddof=1, axis=axis)),
    bca_case("difference-of-means", [first, second], mean_diff),
]

g1 = np.round(rng.gamma(2.0, 2.0, 7), 2)
g2 = np.round(rng.gamma(2.0, 3.0, 8), 2)
before = np.round(rng.normal(50, 8, 12), 1)
after = np.round(before + rng.normal(2.0, 4.0, 12), 1)
cx = np.round(rng.normal(0, 1, 7), 2)
cy = np.round(0.6 * cx + rng.normal(0, 1, 7), 2)
# Larger two-group case for the Monte Carlo check: C(20, 10) = 184,756 splits.
m1 = np.round(rng.exponential(2.0, 10), 2)
m2 = np.round(rng.exponential(3.5, 10), 2)

permutation = [
    perm("two-groups-means", "two-groups-mean", [g1, g2], mean_diff, "independent"),
    perm("two-groups-medians", "two-groups-median", [g1, g2], median_diff, "independent"),
    perm("paired", "paired", [after, before], paired_mean, "samples"),
    perm("pearson", "pearson", [cx, cy], pearson, "pairings"),
    # Spearman's rho is Pearson's r of the average ranks; permute the ranks.
    perm("spearman", "spearman", [stats.rankdata(cx), stats.rankdata(cy)], pearson, "pairings",
         {"x": cx.tolist(), "y": cy.tolist()}),
    perm("monte-carlo-means", "two-groups-mean", [m1, m2], mean_diff, "independent"),
]

here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, "resampling_reference.json"), "w") as f:
    json.dump({"scipy": scipy.__version__, "bca": bca, "permutation": permutation}, f, indent=1)
print("wrote resampling_reference.json")
