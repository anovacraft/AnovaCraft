"""Exhaustive-search references for adding whole plots to a split-plot design.

Independent of the Rust code: the existing runs are kept, every setting of
the new whole plots that respects the plot restriction (the hard factor A
constant within each new whole plot) is scored by ln det(X' V^-1 X) of all
runs with NumPy, where V = I + eta Z Z' (sigma^2 = 1), and the best value is
printed. The values are recorded in `augmenting_matches_exhaustive_search`
of `crates/stat-doe/src/split_plot.rs`.

Run with:  python3 crates/stat-doe/tests/data/split_plot_augment_reference.py

Existing design: A hard, B easy, 4 runs in 2 whole plots,
(A, B) = (-1, -1), (-1, +1) | (+1, -1), (+1, +1).

1. Model 1 + A + B + AB; add 1 whole plot of 2 runs; eta = 1; levels -1, 0, +1.
2. Same model; add 2 whole plots of 2 runs; eta = 1 and eta = 4.
3. Model 1 + A + B + AB + B^2; add 2 whole plots of 3 runs; eta = 1.
"""

import itertools

import numpy as np

LEVELS = (-1.0, 0.0, 1.0)
EXISTING = [((-1.0, -1.0), 0), ((-1.0, 1.0), 0), ((1.0, -1.0), 1), ((1.0, 1.0), 1)]


def row(a, b, quadratic):
    r = [1.0, a, b, a * b]
    if quadratic:
        r.append(b * b)
    return r


def log_det(runs, plots, eta, quadratic):
    x = np.array([row(a, b, quadratic) for a, b in runs])
    z = np.array([[1.0 if p == w else 0.0 for w in range(max(plots) + 1)] for p in plots])
    v = np.eye(len(runs)) + eta * z @ z.T
    m = x.T @ np.linalg.solve(v, x)
    sign, value = np.linalg.slogdet(m)
    return value if sign > 0 else -np.inf


def best(new_plots, per_plot, eta, quadratic):
    runs0 = [r for r, _ in EXISTING]
    plots0 = [p for _, p in EXISTING]
    top = -np.inf
    for a in itertools.product(LEVELS, repeat=new_plots):
        for b in itertools.product(LEVELS, repeat=new_plots * per_plot):
            runs = runs0 + [(a[i // per_plot], b[i]) for i in range(new_plots * per_plot)]
            plots = plots0 + [2 + i // per_plot for i in range(new_plots * per_plot)]
            top = max(top, log_det(runs, plots, eta, quadratic))
    return top


if __name__ == "__main__":
    print("before, eta 1:", repr(log_det([r for r, _ in EXISTING], [p for _, p in EXISTING], 1.0, False)))
    print("case 1:", repr(best(1, 2, 1.0, False)))
    print("case 2, eta 1:", repr(best(2, 2, 1.0, False)))
    print("case 2, eta 4:", repr(best(2, 2, 4.0, False)))
    print("case 3:", repr(best(2, 3, 1.0, True)))
