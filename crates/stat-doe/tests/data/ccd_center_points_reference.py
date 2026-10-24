"""Uniform-precision centre runs of rotatable central composite designs.

Independent check of the default centre-run counts in
`crates/stat-doe/src/response_surface.rs` (Box & Hunter 1957, Ann. Math.
Statist. 28). Run with `python3 -I ccd_center_points_reference.py`.

For each factorial portion (full 2^k, or a 2^(k-p) fraction built from the
generator words of the tabulated designs, checked here to have resolution V
or more), alpha = F^(1/4) and n0 centre runs, the
design is scaled so that sum(x_i^2)/N = 1 and the scaled prediction variance
N f(x)'(X'X)^-1 f(x) of the full quadratic model is compared at the centre
and at distance 1. The uniform-precision n0 is the one that makes the two
closest. It reproduces Box & Hunter's table: 5, 6, 7 (k = 2, 3, 4); 10 and 6
(k = 5 full and half); 15 and 9 (k = 6); 21 and 14 (k = 7); 20 (k = 8 half).
The remaining resolution V portions for k = 8 to 11 extend the table (the
values asserted in `rotatable_centre_points_cover_every_portion_size`).
"""
import itertools

import numpy as np


LETTERS = "ABCDEFGHJKLMNOPQRSTU"

# Resolution V (or higher) fractions: generator words over the base factors.
FRACTIONS = {
    (5, 1): ["ABCD"],
    (6, 1): ["ABCDE"],
    (7, 1): ["ABCDEF"],
    (8, 1): ["ABCDEFG"],
    (8, 2): ["ABCD", "ABEF"],
    (9, 1): ["ABCDEFGH"],
    (9, 2): ["ACDFG", "BCEFG"],
    (10, 1): ["ABCDEFGHJ"],
    (10, 3): ["ABCG", "BCDE", "ACDF"],
    (11, 1): ["ABCDEFGHJK"],
    (11, 4): ["ABCG", "BCDE", "ACDF", "ABCDEFG"],
}


def cube_portion(k, p):
    base = np.array(list(itertools.product([-1, 1], repeat=k - p)), float)
    if p == 0:
        return base
    cols = [base] + [np.prod(base[:, [LETTERS.index(c) for c in w]], axis=1, keepdims=True) for w in FRACTIONS[(k, p)]]
    cube = np.hstack(cols)
    # Resolution V: every product of up to four distinct columns sums to zero.
    for r in range(1, 5):
        for s in itertools.combinations(range(k), r):
            assert abs(np.prod(cube[:, list(s)], axis=1).sum()) < 1e-9, (k, p, s)
    return cube


def ccd(k, p, n0):
    cube = cube_portion(k, p)
    a = len(cube) ** 0.25
    axial = []
    for j in range(k):
        for s in (-a, a):
            r = np.zeros(k)
            r[j] = s
            axial.append(r)
    return np.vstack([cube, np.array(axial), np.zeros((n0, k))])


def terms(k):
    return [()] + [(j,) for j in range(k)] + list(itertools.combinations(range(k), 2)) + [(j, j) for j in range(k)]


def model_matrix(x, t):
    return np.column_stack([np.prod(x[:, list(tt)], axis=1) if tt else np.ones(len(x)) for tt in t])


def uniform_precision(k, p):
    t = terms(k)
    gaps = []
    for n0 in range(1, 80):
        x = ccd(k, p, n0)
        n = len(x)
        xs = x / np.sqrt((x[:, 0] ** 2).sum() / n)
        inv = np.linalg.inv(model_matrix(xs, t).T @ model_matrix(xs, t))
        one = np.zeros(k)
        one[0] = 1.0
        v = [n * f @ inv @ f for f in model_matrix(np.array([np.zeros(k), one]), t)]
        gaps.append((abs(v[0] - v[1]), n0))
    best = min(gaps)[1]
    assert best < 79, "search range too short"
    return best


if __name__ == "__main__":
    portions = [(2, 0), (3, 0), (4, 0), (5, 0), (5, 1), (6, 0), (6, 1), (7, 0), (7, 1), (8, 1)]
    portions += [(8, 0), (8, 2), (9, 0), (9, 1), (9, 2), (10, 0), (10, 1), (10, 3), (11, 0), (11, 1), (11, 4)]
    for k, p in portions:
        print(f"k = {k}, 2^({k}-{p}) ({2 ** (k - p)} runs): n0 = {uniform_precision(k, p)}", flush=True)
