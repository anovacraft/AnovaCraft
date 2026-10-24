"""Exhaustive-search references for D-optimal split-plot and split-split-plot designs.

Independent of the Rust code: every design that respects the plot
restrictions is scored by ln det(X' V^-1 X) with NumPy, where
V = I + eta_w Z_w Z_w' + eta_s Z_s Z_s' (sigma^2_eps = 1; Goos & Vandebroek
2003, Jones & Goos 2009), and the best value is printed. The values are
recorded in the `exhaustive_*` tests of `crates/stat-doe/tests/split_plot.rs`.

Run with:  python3 crates/stat-doe/tests/data/split_plot_reference.py

Because X' V^-1 X is a sum of one contribution per whole plot, and the whole
plots of the same size are interchangeable, it is enough to enumerate
multisets of whole-plot "types" (the settings inside one whole plot); this
is still every distinct design.

Cases (factor levels -1, +1 for linear models; -1, 0, +1 when a squared
term is in the model):

1. A hard, B easy; model 1 + A + B + AB; 8 runs in 4 whole plots of 2;
   eta = 1 and eta = 10.
2. A hard, B easy; full quadratic 1 + A + B + AB + A^2 + B^2; 12 runs in
   4 whole plots of 3; eta = 1.
3. A hard, B easy; model 1 + A + B + AB; 10 runs in 4 whole plots of sizes
   3, 3, 2, 2; eta = 1.
4. Split-split plot: A very hard, B hard, C easy; model with main effects
   and two-factor interactions (7 terms); 8 runs in 2 whole plots, each of
   2 subplots of 2 runs; eta_w = 1, eta_s = 1 and eta_w = 2, eta_s = 0.5.
5. A, B hard, C easy; model 1 + A + B + C + AB + AC + BC; 12 runs in 4
   whole plots of 3; eta = 1.
"""

import itertools

import numpy as np


def v_inverse(sizes_sub, eta_w, eta_s):
    """V^-1 of one whole plot whose subplots have the given sizes."""
    m = sum(sizes_sub)
    sp = np.repeat(np.arange(len(sizes_sub)), sizes_sub)
    v = np.eye(m) + eta_w * np.ones((m, m)) + eta_s * (sp[:, None] == sp[None, :])
    return np.linalg.inv(v)


def whole_plot_types(model, levels, stratum, sizes_sub, eta_w, eta_s):
    """Every contribution X_i' V_i^-1 X_i of one whole plot.

    stratum[j] is 0 (whole-plot factor), 1 (subplot factor) or 2 (run factor).
    """
    k = len(stratum)
    vinv = v_inverse(sizes_sub, eta_w, eta_s)
    m = sum(sizes_sub)
    w_f = [j for j in range(k) if stratum[j] == 0]
    s_f = [j for j in range(k) if stratum[j] == 1]
    r_f = [j for j in range(k) if stratum[j] == 2]
    out = {}
    for wv in itertools.product(*[levels[j] for j in w_f]):
        for sv in itertools.product(*[itertools.product(*[levels[j] for j in s_f]) for _ in sizes_sub]):
            for rv in itertools.product(*[itertools.product(*[levels[j] for j in r_f]) for _ in range(m)]):
                runs = []
                i = 0
                for s, size in enumerate(sizes_sub):
                    for _ in range(size):
                        x = [0.0] * k
                        for a, j in enumerate(w_f):
                            x[j] = wv[a]
                        for a, j in enumerate(s_f):
                            x[j] = sv[s][a]
                        for a, j in enumerate(r_f):
                            x[j] = rv[i][a]
                        runs.append(x)
                        i += 1
                X = np.array([model(x) for x in runs], dtype=float)
                c = X.T @ vinv @ X
                out[np.round(c, 12).tobytes()] = c
    return list(out.values())


def best(groups):
    """groups: list of (count, types); the best ln det over multisets."""
    pools = [list(itertools.combinations_with_replacement(range(len(t)), c)) for c, t in groups]
    sums = []
    for (c, types), pool in zip(groups, pools):
        T = np.array(types)
        sums.append(np.array([T[list(combo)].sum(axis=0) for combo in pool]))
    # Every combination of one multiset from each pool.
    total = sums[0]
    for s in sums[1:]:
        total = (total[:, None] + s[None, :]).reshape(-1, *total.shape[1:])
    sign, ld = np.linalg.slogdet(total)
    ld = np.where(sign > 0, ld, -np.inf)
    return ld.max()


def interactions2(x):
    a, b = x
    return [1, a, b, a * b]


def quadratic2(x):
    a, b = x
    return [1, a, b, a * b, a * a, b * b]


def interactions3(x):
    a, b, c = x
    return [1, a, b, c, a * b, a * c, b * c]


def main():
    two = [-1.0, 1.0]
    three = [-1.0, 0.0, 1.0]
    for eta in (1.0, 10.0):
        t = whole_plot_types(interactions2, [two, two], [0, 2], [2], eta, 0.0)
        print(f"case 1, eta = {eta}: ln det = {best([(4, t)]):.15f}")
    t = whole_plot_types(quadratic2, [three, three], [0, 2], [3], 1.0, 0.0)
    print(f"case 2: ln det = {best([(4, t)]):.15f}")
    t3 = whole_plot_types(interactions2, [two, two], [0, 2], [3], 1.0, 0.0)
    t2 = whole_plot_types(interactions2, [two, two], [0, 2], [2], 1.0, 0.0)
    print(f"case 3: ln det = {best([(2, t3), (2, t2)]):.15f}")
    for eta_w, eta_s in ((1.0, 1.0), (2.0, 0.5)):
        t = whole_plot_types(interactions3, [two, two, two], [0, 1, 2], [2, 2], eta_w, eta_s)
        print(f"case 4, eta_w = {eta_w}, eta_s = {eta_s}: ln det = {best([(2, t)]):.15f}")
    t = whole_plot_types(interactions3, [two, two, two], [0, 0, 2], [3], 1.0, 0.0)
    print(f"case 5: ln det = {best([(4, t)]):.15f}")


if __name__ == "__main__":
    main()
