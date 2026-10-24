"""Independent reference values for the nested and expanded gauge R&R tests.

Run from this directory: python3 -I gauge_reference.py  (writes gauge_reference.json)

* Balanced crossed and nested studies: sums of squares and ANOVA
  (expected-mean-square) variance components from the textbook formulas
  (Montgomery, Design and Analysis of Experiments, sections 13.3 and 14.1;
  Burdick, Borror & Montgomery 2005).
* Unbalanced expanded studies: REML by direct maximisation of the dense
  restricted likelihood, V = sum_t s2_t Z_t Z_t' + s2 I, over log variances
  (polished by Newton steps), and large-sample covariances of the variances
  as 2 H^-1 of -2 log L_R in the variance parametrisation (closed-form
  observed Hessian). The first is
  cross-checked against statsmodels MixedLM (variance components with a
  single all-ones group).
"""
import json
import numpy as np
from scipy import optimize

rng = np.random.default_rng(20261016)


def crossed_balanced():
    p, o, r = 6, 3, 3
    P, O, R = np.meshgrid(np.arange(p), np.arange(o), np.arange(r), indexing='ij')
    P, O = P.ravel(), O.ravel()
    y = 50 + rng.normal(0, 2.0, p)[P] + rng.normal(0, 0.8, o)[O] + rng.normal(0, 0.5, p * o)[P * o + O] \
        + rng.normal(0, 0.4, P.size)
    cell = y.reshape(p, o, r).mean(axis=2)
    gm = y.mean()
    pm, om = cell.mean(axis=1), cell.mean(axis=0)
    ss_p = o * r * ((pm - gm) ** 2).sum()
    ss_o = p * r * ((om - gm) ** 2).sum()
    ss_po = r * ((cell - pm[:, None] - om[None, :] + gm) ** 2).sum()
    ss_e = ((y.reshape(p, o, r) - cell[:, :, None]) ** 2).sum()
    ms_p, ms_o, ms_po, ms_e = ss_p / (p - 1), ss_o / (o - 1), ss_po / ((p - 1) * (o - 1)), ss_e / (p * o * (r - 1))
    comps = dict(part=(ms_p - ms_po) / (o * r), operator=(ms_o - ms_po) / (p * r),
                 interaction=(ms_po - ms_e) / r, repeatability=ms_e)
    assert min(comps.values()) > 0, comps
    return dict(y=y.tolist(), part=P.tolist(), operator=O.tolist(), ss=[ss_p, ss_o, ss_po, ss_e], components=comps)


def nested_balanced():
    o, p, r = 3, 4, 3
    O, Pw, R = np.meshgrid(np.arange(o), np.arange(p), np.arange(r), indexing='ij')
    O, Pw = O.ravel(), Pw.ravel()
    part = O * p + Pw
    y = 20 + rng.normal(0, 0.9, o)[O] + rng.normal(0, 1.5, o * p)[part] + rng.normal(0, 0.5, O.size)
    pm = y.reshape(o, p, r).mean(axis=2)
    om = pm.mean(axis=1)
    gm = y.mean()
    ss_o = p * r * ((om - gm) ** 2).sum()
    ss_p = r * ((pm - om[:, None]) ** 2).sum()
    ss_e = ((y.reshape(o, p, r) - pm[:, :, None]) ** 2).sum()
    ms_o, ms_p, ms_e = ss_o / (o - 1), ss_p / (o * (p - 1)), ss_e / (o * p * (r - 1))
    comps = dict(operator=(ms_o - ms_p) / (p * r), part=(ms_p - ms_e) / r, repeatability=ms_e)
    assert min(comps.values()) > 0, comps
    return dict(y=y.tolist(), part=part.tolist(), part_within=Pw.tolist(), operator=O.tolist(),
                ss=[ss_o, ss_p, ss_e], components=comps)


def indicator(codes):
    codes = np.asarray(codes)
    levels = np.unique(codes)
    return (codes[:, None] == levels[None, :]).astype(float)


def reml(y, X, Zs):
    n, k = len(y), X.shape[1]
    ZZ = [Z @ Z.T for Z in Zs]

    def m2ll(s2):
        V = sum(s * M for s, M in zip(s2[:-1], ZZ)) + s2[-1] * np.eye(n)
        Vi = np.linalg.inv(V)
        XtViX = X.T @ Vi @ X
        P = Vi - Vi @ X @ np.linalg.solve(XtViX, X.T @ Vi)
        _, ld = np.linalg.slogdet(V)
        _, ldx = np.linalg.slogdet(XtViX)
        return ld + ldx + y @ P @ y + (n - k) * np.log(2 * np.pi)

    start = np.full(len(Zs) + 1, np.log(np.var(y) / (len(Zs) + 1)))
    res = optimize.minimize(lambda t: m2ll(np.exp(t)), start, method='BFGS', options=dict(gtol=1e-10, maxiter=10000))
    res = optimize.minimize(lambda t: m2ll(np.exp(t)), res.x, method='Nelder-Mead',
                            options=dict(xatol=1e-12, fatol=1e-13, maxiter=40000, maxfev=40000))
    s2 = np.exp(res.x)
    # Observed Hessian of -2 log L_R in the variances, in closed form
    # (Searle, Casella & McCulloch 1992, section 6.6):
    # H_ij = -tr(P V_i P V_j) + 2 y' P V_i P V_j P y.
    Vs = ZZ + [np.eye(n)]
    m = len(s2)

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

    # Newton polish of the optimum (all variances are interior).
    for _ in range(5):
        grad, H = derivatives(s2)
        s2 = s2 - np.linalg.solve(H, grad)
    grad, H = derivatives(s2)
    assert s2.min() > 0 and np.abs(grad).max() < 1e-9, (s2, grad)
    cov = 2 * np.linalg.inv(H)
    return s2, cov, m2ll(s2)


def statsmodels_check(y, cols, vc):
    import pandas as pd
    import statsmodels.formula.api as smf
    df = pd.DataFrame(dict(y=y, **cols))
    df['one'] = 1
    md = smf.mixedlm('y ~ 1', df, groups='one', re_formula='0', vc_formula=vc)
    fit = md.fit(reml=True, method=['lbfgs'], maxiter=2000)
    return dict(zip(fit.model.exog_vc.names, fit.vcomp.tolist())), float(fit.scale)


def expanded_unbalanced():
    p, o, g, r = 6, 3, 2, 2
    P, O, G, R = np.meshgrid(np.arange(p), np.arange(o), np.arange(g), np.arange(r), indexing='ij')
    P, O, G = P.ravel(), O.ravel(), G.ravel()
    y = 100 + rng.normal(0, 3.0, p)[P] + rng.normal(0, 1.2, o)[O] + rng.normal(0, 0.9, g)[G] \
        + rng.normal(0, 0.8, p * o)[P * o + O] + rng.normal(0, 0.6, p * g)[P * g + G] \
        + rng.normal(0, 0.5, o * g)[O * g + G] + rng.normal(0, 0.5, P.size)
    keep = rng.random(P.size) > 0.18
    P, O, G, y = P[keep], O[keep], G[keep], y[keep]
    X = np.ones((len(y), 1))
    Zs = [indicator(P), indicator(O), indicator(G), indicator(P * o + O), indicator(P * g + G), indicator(O * g + G)]
    s2, cov, crit = reml(y, X, Zs)
    names = ['Part', 'Operator', 'Gauge', 'Part × Operator', 'Part × Gauge', 'Operator × Gauge', 'Repeatability']
    sm, scale = statsmodels_check(y, dict(P=P, O=O, G=G, PO=P * o + O, PG=P * g + G, OG=O * g + G),
                                  dict(P='0+C(P)', O='0+C(O)', G='0+C(G)', PO='0+C(PO)', PG='0+C(PG)', OG='0+C(OG)'))
    return dict(y=y.tolist(), part=P.tolist(), operator=O.tolist(), gauge=G.tolist(), names=names,
                variances=s2.tolist(), covariance=cov.tolist(), criterion=crit,
                statsmodels=[sm['P'], sm['O'], sm['G'], sm['PO'], sm['PG'], sm['OG'], scale])


def expanded_fixed_nested():
    """Parts within operators (destructive), a fixed two-level fixture crossed
    with everything, unequal parts per operator and missing readings."""
    parts_per_op = [4, 3, 5]
    rows = []
    pid = 0
    for op, k in enumerate(parts_per_op):
        for _ in range(k):
            for fx in range(2):
                for _rep in range(2):
                    rows.append((op, pid, fx))
            pid += 1
    O, P, F = (np.array(c) for c in zip(*rows))
    y = 7 + rng.normal(0, 0.6, 3)[O] + rng.normal(0, 1.1, pid)[P] + 0.7 * F \
        + rng.normal(0, 0.9, 6)[O * 2 + F] + rng.normal(0, 0.45, pid * 2)[P * 2 + F] + rng.normal(0, 0.35, O.size)
    keep = rng.random(O.size) > 0.12
    O, P, F, y = O[keep], P[keep], F[keep], y[keep]
    X = np.column_stack([np.ones(len(y)), np.where(F == 0, 1.0, -1.0)])
    # Terms: Part(Operator), Operator, Fixture (fixed), Operator x Fixture and
    # Part(Operator) x Fixture (random).
    Zs = [indicator(P), indicator(O), indicator(O * 2 + F), indicator(P * 2 + F)]
    s2, cov, crit = reml(y, X, Zs)
    names = ['Part(Operator)', 'Operator', 'Part(Operator) × Fixture', 'Operator × Fixture', 'Repeatability']
    # Rust term order: main terms (Part, Operator), then interactions in factor
    # order: Part × Fixture, Operator × Fixture.
    order = [0, 1, 3, 2, 4]
    return dict(y=y.tolist(), part=P.tolist(), operator=O.tolist(), fixture=F.tolist(), names=names,
                variances=s2[order].tolist(), covariance=cov[np.ix_(order, order)].tolist(), criterion=crit)


out = dict(crossed_balanced=crossed_balanced(), nested_balanced=nested_balanced(),
           expanded_unbalanced=expanded_unbalanced(), expanded_fixed_nested=expanded_fixed_nested())
with open('gauge_reference.json', 'w') as f:
    json.dump(out, f, indent=1)
print(json.dumps({k: v.get('variances', v.get('components')) for k, v in out.items()}, indent=1))
print('statsmodels', out['expanded_unbalanced']['statsmodels'])
