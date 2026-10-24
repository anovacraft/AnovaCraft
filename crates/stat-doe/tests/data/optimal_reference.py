"""Exhaustive-search references for constrained and augmented D-optimal designs.

Independent of the Rust code: every multiset of n candidate points is scored
by ln det(X'X) with NumPy, and the best is printed. The values are recorded in
`crates/stat-doe/src/optimal.rs` and `crates/stat-doe/src/augment.rs` tests.

Run with:  python3 crates/stat-doe/tests/data/optimal_reference.py

1. Constrained region: two coded factors A, B in [-1, 1] with A + B <= 0.5,
   model 1 + A + B + AB, n = 4, 5, 6 runs.

   Candidates. For this model the determinant, as a function of one run's
   coordinate a with everything else fixed, is a convex quadratic in a
   (the model row is linear in a), so an optimal run sits at an end of its
   feasible interval in each coordinate. Those points are the vertices of
   the feasible polygon (-1,-1), (1,-1), (1,-0.5), (-0.5,1), (-1,1) and the
   points of the slanted edge A + B = 0.5 (where both coordinates are at
   their upper ends). The candidate set is therefore the vertices plus the
   slanted edge in steps of 0.05; for n = 4 and 5 a grid of interior points
   in steps of 0.5 is added as a check that no interior point helps.

2. Optimal augmentation: the 2^2 factorial plus one centre run (fixed), with
   3 runs added for the full quadratic model 1 + A + B + AB + A^2 + B^2,
   candidates the 3^2 grid {-1, 0, 1}^2 (the coordinate-exchange levels for
   quadratic factors).

3. Optimal augmentation of the 2^(3-1) half fraction I = ABC by 4 runs for
   the model with main effects and two-factor interactions, candidates the
   2^3 corners: the best is the complementary half fraction, det = 8^7.
"""

import itertools

import numpy as np


def best(candidates, n, model, fixed=()):
    F = np.array([model(c) for c in candidates], dtype=float)
    outer = np.einsum("ij,ik->ijk", F, F)
    base = sum((np.outer(model(r), model(r)) for r in fixed), np.zeros((F.shape[1], F.shape[1])))
    combos = np.array(list(itertools.combinations_with_replacement(range(len(candidates)), n)))
    best_ld, best_combo = -np.inf, None
    for s in range(0, len(combos), 200_000):
        c = combos[s : s + 200_000]
        m = outer[c].sum(axis=1) + base
        sign, ld = np.linalg.slogdet(m)
        ld[sign <= 0] = -np.inf
        i = int(np.argmax(ld))
        if ld[i] > best_ld:
            best_ld, best_combo = ld[i], c[i]
    return best_ld, [candidates[i] for i in best_combo]


def bilinear(p):
    a, b = p
    return [1.0, a, b, a * b]


def quadratic(p):
    a, b = p
    return [1.0, a, b, a * b, a * a, b * b]


def interactions3(p):
    a, b, c = p
    return [1.0, a, b, c, a * b, a * c, b * c]


def main():
    vertices = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0)]  # (1,-0.5), (-0.5,1) are on the edge
    t = np.round(np.arange(-0.5, 1.0 + 1e-9, 0.05), 10)
    edge = [(float(a), float(0.5 - a)) for a in t]
    interior = [
        (a, b)
        for a in (-0.5, 0.0, 0.5)
        for b in (-0.5, 0.0, 0.5)
        if a + b < 0.5 - 1e-12
    ]
    print("1. A + B <= 0.5, model 1 + A + B + AB")
    for n in (4, 5, 6):
        cands = vertices + edge + (interior if n < 6 else [])
        ld, pts = best(cands, n, bilinear)
        print(f"   n = {n}: ln det = {ld:.15f}  runs = {pts}")

    grid3 = [(a, b) for a in (-1.0, 0.0, 1.0) for b in (-1.0, 0.0, 1.0)]
    fixed = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (0.0, 0.0)]
    ld, pts = best(grid3, 3, quadratic, fixed)
    print("2. 2^2 + centre, add 3 runs, quadratic model")
    print(f"   ln det = {ld:.15f}  added = {pts}")

    corners = list(itertools.product((-1.0, 1.0), repeat=3))
    half = [c for c in corners if c[0] * c[1] * c[2] > 0]
    ld, pts = best(corners, 4, interactions3, half)
    print("3. 2^(3-1), I = ABC, add 4 runs, two-factor interaction model")
    print(f"   ln det = {ld:.15f} (7 ln 8 = {7 * np.log(8):.15f})  added = {pts}")


if __name__ == "__main__":
    main()
