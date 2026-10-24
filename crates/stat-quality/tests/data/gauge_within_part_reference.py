"""Independent reference for an expanded gauge study with a factor nested
within parts (crates/stat-quality/tests/gauge_reference.rs).

Run from this directory: python3 -I gauge_within_part_reference.py
(writes gauge_within_part_reference.json)

Design: 6 parts x 3 operators (crossed), 3 locations measured on each part
(Location nested in Part), 2 readings per cell, about 10% of the readings
missing. Random terms: Part, Operator, Location(Part), Part x Operator,
Operator x Location(Part); residual = repeatability.

REML by direct maximisation of the dense restricted likelihood
V = sum_t s2_t Z_t Z_t' + s2 I (over log variances, polished by Newton
steps), with the large-sample covariance of the variances 2 H^-1 from the
closed-form observed Hessian (Searle, Casella & McCulloch 1992, section 6.6).

Totals, following AIAG Measurement Systems Analysis (4th ed.) and Burdick,
Borror & Montgomery (2005, ch. 9): within-part variation (Location(Part)) is
product variation, so
  part-to-part = Part + Location(Part),
  gauge R&R    = Repeatability + Operator + Part x Operator
                 + Operator x Location(Part),
  total        = part-to-part + gauge R&R,
with standard errors sqrt(1' C 1) over the summed components.
"""
import json
import numpy as np
from scipy import optimize

rng = np.random.default_rng(20261017)


def indicator(codes):
    codes = np.asarray(codes)
    levels = np.unique(codes)
    return (codes[:, None] == levels[None, :]).astype(float)


def reml(y, X, Zs):
    n, k = len(y), X.shape[1]
    ZZ = [Z @ Z.T for Z in Zs]
    Vs = ZZ + [np.eye(n)]
    m = len(Vs)

    def m2ll(s2):
        V = sum(s * M for s, M in zip(s2, Vs))
        Vi = np.linalg.inv(V)
        XtViX = X.T @ Vi @ X
        P = Vi - Vi @ X @ np.linalg.solve(XtViX, X.T @ Vi)
        _, ld = np.linalg.slogdet(V)
        _, ldx = np.linalg.slogdet(XtViX)
        return ld + ldx + y @ P @ y + (n - k) * np.log(2 * np.pi)

    start = np.full(m, np.log(np.var(y) / m))
    res = optimize.minimize(lambda t: m2ll(np.exp(t)), start, method='BFGS', options=dict(gtol=1e-10, maxiter=10000))
    res = optimize.minimize(lambda t: m2ll(np.exp(t)), res.x, method='Nelder-Mead',
                            options=dict(xatol=1e-12, fatol=1e-13, maxiter=40000, maxfev=40000))
    s2 = np.exp(res.x)

    def derivatives(s2):
        V = sum(s * M for s, M in zip(s2, Vs))
        Vi = np.linalg.inv(V)
        P = Vi - Vi @ X @ np.linalg.solve(X.T @ Vi @ X, X.T @ Vi)
        H = np.zeros((m, m))
        for i in range(m):
            for j in range(m):
                H[i, j] = -np.trace(P @ Vs[i] @ P @ Vs[j]) + 2 * y @ P @ Vs[i] @ P @ Vs[j] @ P @ y
        grad = np.array([np.trace(P @ Vs[i]) - y @ P @ Vs[i] @ P @ y for i in range(m)])
        return grad, H

    for _ in range(5):
        grad, H = derivatives(s2)
        s2 = s2 - np.linalg.solve(H, grad)
    grad, H = derivatives(s2)
    assert s2.min() > 0 and np.abs(grad).max() < 1e-9, (s2, grad)
    return s2, 2 * np.linalg.inv(H)


p, o, l, r = 6, 3, 3, 2
P, O, L, R = np.meshgrid(np.arange(p), np.arange(o), np.arange(l), np.arange(r), indexing='ij')
P, O, L = P.ravel(), O.ravel(), L.ravel()
PL = P * l + L
y = 30 + rng.normal(0, 2.0, p)[P] + rng.normal(0, 0.7, o)[O] + rng.normal(0, 1.0, p * l)[PL] \
    + rng.normal(0, 0.5, p * o)[P * o + O] + rng.normal(0, 0.4, p * l * o)[PL * o + O] + rng.normal(0, 0.4, P.size)
keep = rng.random(P.size) > 0.1
P, O, L, PL, y = P[keep], O[keep], L[keep], PL[keep], y[keep]
X = np.ones((len(y), 1))
# Rust term order: main terms Part, Operator, Location(Part); then
# interactions in factor order: Part x Operator, Operator x Location(Part).
Zs = [indicator(P), indicator(O), indicator(PL), indicator(P * o + O), indicator(PL * o + O)]
s2, cov = reml(y, X, Zs)
names = ['Part', 'Operator', 'Location(Part)', 'Part × Operator', 'Operator × Location(Part)', 'Repeatability']


def total(idx):
    return float(s2[idx].sum()), float(np.sqrt(cov[np.ix_(idx, idx)].sum()))


part, part_se = total([0, 2])
grr, grr_se = total([1, 3, 4, 5])
tot, tot_se = total([0, 1, 2, 3, 4, 5])
out = dict(y=y.tolist(), part=P.tolist(), operator=O.tolist(), location=L.tolist(), names=names,
           variances=s2.tolist(), covariance=cov.tolist(),
           part_to_part=[part, part_se], gauge_rr=[grr, grr_se], total=[tot, tot_se],
           percent_part_of_total_sd=100 * np.sqrt(part / tot))
with open('gauge_within_part_reference.json', 'w') as f:
    json.dump(out, f, indent=1)
print(dict(zip(names, s2.round(5).tolist())), 'part', part, 'grr', grr)
