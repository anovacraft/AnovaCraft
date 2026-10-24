"""Independent reference values for `stat_doe::evaluation` (design evaluation).

Run with `python3 design_evaluation_reference.py`; the printed values are
recorded in the tests of `crates/stat-doe/src/evaluation.rs`.

Everything is computed directly from the definitions with NumPy and SciPy:
* power of the two-sided t test of a model term with anticipated coefficient
  beta (units of sigma): P(F(1, n-p; lambda) > F_{1-alpha}(1, n-p)) with
  lambda = beta^2 / [(X'X)^-1]_jj, via scipy.stats.ncf and scipy.stats.f;
* variance inflation factors as diag(inverse correlation matrix);
* the average prediction variance over [-1, 1]^k by Monte Carlo integration
  (an independent check of the exact moment formula) and the
  fraction-of-design-space quantiles of d(x) = f(x)'(X'X)^-1 f(x) from a
  large NumPy uniform sample (its own generator, so the comparison is within
  Monte Carlo tolerance).
"""

import itertools

import numpy as np
from scipy import stats


def interactions(k):
    terms = [()] + [(j,) for j in range(k)]
    terms += list(itertools.combinations(range(k), 2))
    return terms


def quadratic(k):
    return interactions(k) + [(j, j) for j in range(k)]


def main_effects(k):
    return [()] + [(j,) for j in range(k)]


def model_matrix(runs, terms):
    runs = np.asarray(runs, dtype=float)
    cols = []
    for t in terms:
        c = np.ones(len(runs))
        for j in t:
            c = c * runs[:, j]
        cols.append(c)
    return np.column_stack(cols)


def power(runs, terms, beta, alpha):
    x = model_matrix(runs, terms)
    n, p = x.shape
    inv = np.linalg.inv(x.T @ x)
    df = n - p
    crit = stats.f.ppf(1 - alpha, 1, df)
    out = []
    for j in range(1, p):
        b = beta[j - 1] if hasattr(beta, "__len__") else beta
        lam = b * b / inv[j, j]
        out.append(stats.ncf.sf(crit, 1, df, lam))
    return out


def vif(runs, terms):
    x = model_matrix(runs, terms)[:, 1:]
    r = np.corrcoef(x, rowvar=False)
    return np.diag(np.linalg.inv(r))


def fds_quantiles(runs, terms, k, fractions, n=4_000_000, seed=20031):
    x = model_matrix(runs, terms)
    inv = np.linalg.inv(x.T @ x)
    rng = np.random.default_rng(seed)
    pts = rng.uniform(-1, 1, size=(n, k))
    f = model_matrix(pts, terms)
    d = np.einsum("ij,jk,ik->i", f, inv, f)
    return np.quantile(d, fractions), d.mean()


full2_3 = list(itertools.product([-1, 1], repeat=3))
full3_2 = list(itertools.product([-1, 0, 1], repeat=2))
# Face-centred central composite in 3 factors with one centre run (15 runs).
ccd = list(itertools.product([-1, 1], repeat=3))
for j in range(3):
    for s in (-1, 1):
        r = [0, 0, 0]
        r[j] = s
        ccd.append(r)
ccd.append([0, 0, 0])

np.set_printoptions(precision=17)
print("2^3, interaction model, beta=1, alpha=0.05:", repr(power(full2_3, interactions(3), 1.0, 0.05)[0]))
print("2^3, main effects, beta=1, alpha=0.05:", repr(power(full2_3, main_effects(3), 1.0, 0.05)[0]))
print("2^3, main effects, beta=0.5, alpha=0.10:", repr(power(full2_3, main_effects(3), 0.5, 0.10)[0]))
print("3^2, quadratic, beta=1.5, alpha=0.05:", [repr(v) for v in power(full3_2, quadratic(2), 1.5, 0.05)])
betas = [1.0, 2.0, 0.5, 1.0, 1.5, 1.0, 2.0, 2.5, 1.0]
print("FCD 15 runs, quadratic, betas, alpha=0.05:", [repr(v) for v in power(ccd, quadratic(3), betas, 0.05)])
print("FCD 15 runs, quadratic, VIF:", [repr(v) for v in vif(ccd, quadratic(3))])
fr = [0.0001, 0.1, 0.25, 0.5, 0.75, 0.9, 0.9999]
q, mean = fds_quantiles(full3_2, quadratic(2), 2, fr)
print("3^2 quadratic FDS quantiles at", fr, ":", [repr(v) for v in q], "MC mean", repr(mean))
q, mean = fds_quantiles(ccd, quadratic(3), 3, fr)
print("FCD quadratic FDS quantiles at", fr, ":", [repr(v) for v in q], "MC mean", repr(mean))

# Box-Behnken design in 3 factors with 3 centre runs (15 runs): exact average
# prediction variance over the cube from the exact moment matrix.
bbd = []
for a, b in [(0, 1), (0, 2), (1, 2)]:
    for s1, s2 in itertools.product([-1, 1], repeat=2):
        r = [0, 0, 0]
        r[a], r[b] = s1, s2
        bbd.append(r)
bbd += [[0, 0, 0]] * 3


def moment(a, b, k):
    powers = [0] * k
    for j in a + b:
        powers[j] += 1
    out = 1.0
    for e in powers:
        out *= 0.0 if e % 2 else 1.0 / (e + 1)
    return out


x = model_matrix(bbd, quadratic(3))
m = np.array([[moment(a, b, 3) for b in quadratic(3)] for a in quadratic(3)])
print("BBD 15 runs, quadratic, I:", repr(np.trace(m @ np.linalg.inv(x.T @ x))))

# A constrained region (`evaluate_design_in`): the square cut by the linear
# constraint x1 + x2 <= 1 and with the corner x1 < -0.5 and x2 < -0.5
# disallowed. Its average, largest and quantiles of d(x) over a fine grid
# of the region (step 0.0005, the polygon's vertices on the grid).
con = [(-1, 1), (-1, 0), (0, -1), (1, -1), (1, 0), (0, 1), (0, 0), (0.5, 0.5), (-0.5, -0.5)]
x = model_matrix(con, quadratic(2))
inv = np.linalg.inv(x.T @ x)
g = np.linspace(-1, 1, 4001)
d_all = []
for a in g:
    b = g[(a + g <= 1 + 1e-12) & ~((a < -0.5) & (g < -0.5))]
    pts = np.column_stack([np.full(len(b), a), b])
    f = model_matrix(pts, quadratic(2))
    d_all.append(np.einsum("ij,jk,ik->i", f, inv, f))
d_all = np.concatenate(d_all)
area = 4 - 0.5 - 0.25
print("constrained: fraction of grid in region", len(d_all) / 4001**2, "area/4", area / 4)
print("constrained quadratic: mean d", repr(d_all.mean()), "max d", repr(d_all.max()))
print("constrained FDS quantiles at", fr, ":", [repr(v) for v in np.quantile(d_all, fr)])
