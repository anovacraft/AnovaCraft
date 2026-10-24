"""Independent reference values for the space-filling designs in
`crates/stat-doe/src/space_filling.rs`, computed with SciPy.

Run: python3 crates/stat-doe/tests/data/space_filling_reference.py
(the values recorded in the Rust tests were produced with SciPy 1.18.1 and
NumPy 2.5.3).

1. Centered L2 discrepancy (squared, as SciPy returns it) of fixed designs:
   `scipy.stats.qmc.discrepancy(x, method="CD")`.
2. Centered L2 discrepancy (square root) of SciPy's own optimized Latin
   hypercubes, `LatinHypercube(d, scramble=False, optimization="random-cd")`,
   for seeds 0-4; the best of the five is the bar the uniform design must meet.
"""
import math

import numpy as np
from scipy.stats import qmc


def frac_design(n, k):
    """x_ij = frac((i + 1) * sqrt(p_j)), p_j the j-th prime: reproducible in Rust."""
    primes = [2, 3, 5, 7, 11, 13, 17, 19]
    return np.array([[((i + 1) * math.sqrt(primes[j])) % 1.0 for j in range(k)] for i in range(n)])


FIXED = {
    "four_2d": np.array([[0.1, 0.2], [0.4, 0.8], [0.75, 0.35], [0.95, 0.6]]),
    "u_type_6x3": (np.array([[1, 4, 6], [2, 1, 3], [3, 6, 2], [4, 3, 5], [5, 5, 1], [6, 2, 4]]) - 0.5) / 6.0,
    "one_d": np.array([[0.05], [0.5], [0.9]]),
    "frac_15x5": frac_design(15, 5),
    "frac_40x8": frac_design(40, 8),
}

for name, x in FIXED.items():
    print(f"{name}: CD^2 = {qmc.discrepancy(x, method='CD'):.17e}")

for n, k in [(10, 2), (20, 4), (30, 3), (40, 8)]:
    values = []
    for seed in range(5):
        x = qmc.LatinHypercube(d=k, scramble=False, optimization="random-cd", rng=seed).random(n)
        values.append(math.sqrt(qmc.discrepancy(x, method="CD")))
    print(f"random-cd n={n} k={k}: CD = " + ", ".join(f"{v:.6f}" for v in values) + f"; best {min(values):.6f}")
