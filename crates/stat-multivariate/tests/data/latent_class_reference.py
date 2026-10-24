"""Reference values for the latent class tests (stat-multivariate/src/latent_class/tests.rs).

Two data sets:

1. Stouffer & Toby's role-conflict items (four binary items, 216
   respondents), the classic example of Goodman (1974, Biometrika 61) and
   Linzer & Lewis (2011, J. Stat. Software 42(10)), fitted with two classes.
2. latent_class_data.csv, written here: 500 simulated respondents, six items
   with 2, 2, 2, 3, 3 and 4 levels from three classes, with about 5% of the
   responses missing at random (empty cells).

For each, stepmix 3.0 (measurement 'categorical' or 'categorical_nan', full
information maximum likelihood) is run from 30 random starts to absolute
tolerance 1e-13; classes are then ordered from largest to smallest. The
log-likelihood is recomputed here from stepmix's parameters.

Standard errors: an independent numerical observed information. The
log-likelihood is written in multinomial logits with the LAST level (and
last class) as baseline, its gradient is computed analytically, and the
Hessian by central differences of that gradient (step 1e-5); the
covariance of the logits is carried to the probabilities by the delta
method. (Standard errors on the probability scale do not depend on the
choice of baseline at the maximum.)

Run: python3 latent_class_reference.py   (numpy, stepmix)
"""
import os
import numpy as np
from stepmix import StepMix

here = os.path.dirname(os.path.abspath(__file__))


def stouffer_toby():
    # Patterns over items A, B, C, D (0 = universalistic, 1 = particularistic)
    # with their frequencies (Goodman 1974, Table 1).
    counts = {
        (0, 0, 0, 0): 42, (0, 0, 0, 1): 23, (0, 0, 1, 0): 6, (0, 0, 1, 1): 25,
        (0, 1, 0, 0): 6, (0, 1, 0, 1): 24, (0, 1, 1, 0): 7, (0, 1, 1, 1): 38,
        (1, 0, 0, 0): 1, (1, 0, 0, 1): 4, (1, 0, 1, 0): 1, (1, 0, 1, 1): 6,
        (1, 1, 0, 0): 2, (1, 1, 0, 1): 9, (1, 1, 1, 0): 2, (1, 1, 1, 1): 20,
    }
    rows = [p for p, c in counts.items() for _ in range(c)]
    return np.array(rows, dtype=float), [2, 2, 2, 2]


def simulate():
    rng = np.random.default_rng(20261016)
    levels = [2, 2, 2, 3, 3, 4]
    sizes = [0.5, 0.3, 0.2]
    rho = [
        [[0.85, 0.15], [0.30, 0.70], [0.10, 0.90]],
        [[0.80, 0.20], [0.75, 0.25], [0.15, 0.85]],
        [[0.90, 0.10], [0.20, 0.80], [0.30, 0.70]],
        [[0.70, 0.20, 0.10], [0.20, 0.60, 0.20], [0.10, 0.20, 0.70]],
        [[0.60, 0.30, 0.10], [0.15, 0.25, 0.60], [0.20, 0.60, 0.20]],
        [[0.50, 0.30, 0.15, 0.05], [0.10, 0.20, 0.40, 0.30], [0.05, 0.15, 0.30, 0.50]],
    ]
    n = 500
    cls = rng.choice(3, size=n, p=sizes)
    X = np.empty((n, len(levels)))
    for i in range(n):
        for j, r in enumerate(levels):
            X[i, j] = rng.choice(r, p=rho[j][cls[i]])
    miss = rng.random(X.shape) < 0.05
    X[miss] = np.nan
    # No respondent with every item missing.
    assert not np.isnan(X).all(axis=1).any()
    with open(os.path.join(here, "latent_class_data.csv"), "w") as f:
        f.write(",".join(f"item{j + 1}" for j in range(len(levels))) + "\n")
        for row in X:
            f.write(",".join("" if np.isnan(v) else str(int(v)) for v in row) + "\n")
    return X, levels


def loglik(X, levels, pi, rho):
    n = X.shape[0]
    K = len(pi)
    logs = np.tile(np.log(pi), (n, 1))
    for j in range(len(levels)):
        obs = ~np.isnan(X[:, j])
        codes = X[obs, j].astype(int)
        for k in range(K):
            logs[obs, k] += np.log(rho[j][k][codes])
    m = logs.max(1, keepdims=True)
    lse = m[:, 0] + np.log(np.exp(logs - m).sum(1))
    return lse.sum(), np.exp(logs - lse[:, None])


def fit_stepmix(X, levels, K, nan):
    model = StepMix(n_components=K, measurement="categorical_nan" if nan else "categorical",
                    n_init=30, max_iter=20000, abs_tol=1e-13, rel_tol=0.0,
                    random_state=7, verbose=0, progress_bar=0)
    model.fit(X)
    p = model.get_parameters()
    pi = np.asarray(p["weights"])
    L = max(levels)
    pis = np.asarray(p["measurement"]["pis"]).reshape(K, len(levels), L)
    order = np.argsort(-pi, kind="stable")
    pi = pi[order]
    rho = [np.array([pis[k, j, :r] / pis[k, j, :r].sum() for k in order]) for j, r in enumerate(levels)]
    return pi, rho


# --- independent standard errors -------------------------------------------

def unpack(theta, levels, K):
    g = np.append(theta[:K - 1], 0.0)
    pi = np.exp(g - g.max()); pi /= pi.sum()
    rho, at = [], K - 1
    for r in levels:
        item = []
        for k in range(K):
            b = np.append(theta[at:at + r - 1], 0.0); at += r - 1
            p = np.exp(b - b.max()); item.append(p / p.sum())
        rho.append(np.array(item))
    return pi, rho


def pack(pi, rho, levels, K):
    out = list(np.log(pi[:-1] / pi[-1]))
    for j, r in enumerate(levels):
        for k in range(K):
            out += list(np.log(rho[j][k][:-1] / rho[j][k][-1]))
    return np.array(out)


def gradient(theta, X, levels, K):
    pi, rho = unpack(theta, levels, K)
    _, h = loglik(X, levels, pi, rho)
    g = list((h - pi).sum(0)[:K - 1])
    for j, r in enumerate(levels):
        obs = ~np.isnan(X[:, j])
        Y = np.eye(r)[X[obs, j].astype(int)]
        for k in range(K):
            w = h[obs, k][:, None]
            g += list((w * (Y - rho[j][k])).sum(0)[:r - 1])
    return np.array(g)


def standard_errors(X, levels, pi, rho):
    K = len(pi)
    theta = pack(pi, rho, levels, K)
    m = len(theta)
    H = np.empty((m, m))
    step = 1e-5
    for a in range(m):
        e = np.zeros(m); e[a] = step
        H[:, a] = (gradient(theta + e, X, levels, K) - gradient(theta - e, X, levels, K)) / (2 * step)
    H = 0.5 * (H + H.T)
    V = np.linalg.inv(-H)

    def delta(p, idx):
        J = np.diag(p) - np.outer(p, p)
        J = J[:, :-1]
        return np.sqrt(np.einsum("ra,ab,rb->r", J, V[np.ix_(idx, idx)], J))

    se_pi = delta(pi, list(range(K - 1)))
    at = K - 1
    se_rho = []
    for r in levels:
        item = []
        for k in range(K):
            item.append(delta(rho[len(se_rho)][k], list(range(at, at + r - 1))))
            at += r - 1
        se_rho.append(item)
    return se_pi, se_rho


def report(name, X, levels, K, nan):
    pi, rho = fit_stepmix(X, levels, K, nan)
    ll, _ = loglik(X, levels, pi, rho)
    print(f"== {name}, K = {K}")
    print(f"ll = {ll!r}")
    print("pi =", [float(v) for v in pi])
    for j in range(len(levels)):
        print(f"rho[{j}] =", [[float(v) for v in rho[j][k]] for k in range(K)])
    if K > 1:
        se_pi, se_rho = standard_errors(X, levels, pi, rho)
        print("se_pi =", [float(v) for v in se_pi])
        for j in range(len(levels)):
            print(f"se_rho[{j}] =", [[float(v) for v in se_rho[j][k]] for k in range(K)])
    return ll


X, levels = stouffer_toby()
report("Stouffer-Toby", X, levels, 2, nan=False)
# Pattern goodness of fit (Goodman 1974 reports 2.72 on 6 df for this model).
pi, rho = fit_stepmix(X, levels, 2, nan=False)
patterns, counts = np.unique(X.astype(int), axis=0, return_counts=True)
g2 = x2 = 0.0
for p, o in zip(patterns, counts):
    e = len(X) * sum(pi[k] * np.prod([rho[j][k][p[j]] for j in range(4)]) for k in range(2))
    g2 += 2 * o * np.log(o / e)
    x2 += (o - e) ** 2 / e
print(f"G2 = {g2!r}, X2 = {x2!r}")
X, levels = simulate()
for K in (1, 2, 3, 4):
    if K == 3:
        report("simulated", X, levels, K, nan=True)
    else:
        pi, rho = fit_stepmix(X, levels, K, nan=True)
        print(f"== simulated, K = {K}: ll = {loglik(X, levels, pi, rho)[0]!r}")
