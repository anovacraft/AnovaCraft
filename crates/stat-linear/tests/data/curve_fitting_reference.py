"""Reference values for crates/stat-linear/tests/curve_fitting_reference.rs.

Independent SciPy computations of weighted fits, global (shared-parameter)
fits, robust (Cauchy-loss) fits, the outlier rule, orthogonal-distance fits of
implicit curves (exact distances to a parametric ellipse and a circle) and Hougaard's skewness (symbolic
second derivatives with SymPy, plus a Monte Carlo check of the skewness).

Run:  python3 curve_fitting_reference.py > curve_fitting_reference.json
"""
import json
import numpy as np
import sympy as sp
from scipy import optimize, stats

out = {}

# Puromycin, treated cells (Bates & Watts 1988).
conc = np.array([0.02, 0.02, 0.06, 0.06, 0.11, 0.11, 0.22, 0.22, 0.56, 0.56, 1.10, 1.10])
rate = np.array([76, 47, 97, 107, 123, 139, 159, 152, 191, 201, 207, 200], float)
mm = lambda x, vmax, km: vmax * x / (km + x)


def lcg(seed, n):
    """Deterministic uniform(−0.5, 0.5) noise, written out in the JSON."""
    v, xs = seed, []
    for _ in range(n):
        v = (1103515245 * v + 12345) % 2**31
        xs.append(v / 2**31 - 0.5)
    return np.array(xs)


def ls(fun, start, **kw):
    r = optimize.least_squares(fun, start, xtol=1e-15, ftol=1e-15, gtol=1e-15, method="lm", **kw)
    J = r.jac
    n, p = J.shape
    s2 = 2 * r.cost / (n - p)
    cov = s2 * np.linalg.inv(J.T @ J)
    return r.x, np.sqrt(np.diag(cov)), 2 * r.cost


# 1. Weighted fit, weights 1/y².
w = 1 / rate**2
est, se, sse = ls(lambda t: np.sqrt(w) * (mm(conc, *t) - rate), [200, 0.06])
p, cov = optimize.curve_fit(mm, conc, rate, p0=[200, 0.06], sigma=rate, absolute_sigma=False, xtol=1e-15, ftol=1e-15)
assert np.allclose(est, p, rtol=1e-9)
out["weighted"] = {"x": conc.tolist(), "y": rate.tolist(), "w": w.tolist(), "estimates": est.tolist(),
                   "std_errors": se.tolist(), "sse": sse}

# 2. Global fit: two dose–response sets (4-parameter logistic in x), Lower and
#    Upper shared, Rate and Midpoint per set.
x = np.repeat(np.linspace(-3, 3, 13), 1)
def l4(x, lo, hi, rt, mid):
    return lo + (hi - lo) / (1 + np.exp(-rt * (x - mid)))
ya = l4(x, 5, 95, 1.6, -0.4) + 6 * lcg(7, x.size)
yb = l4(x, 5, 95, 1.1, 0.9) + 6 * lcg(11, x.size)
def glob(t):
    lo, hi, ra, ma, rb, mb = t
    return np.concatenate([l4(x, lo, hi, ra, ma) - ya, l4(x, lo, hi, rb, mb) - yb])
est, se, sse = ls(glob, [0, 100, 1, 0, 1, 1])
out["global"] = {"x": x.tolist(), "ya": ya.tolist(), "yb": yb.tolist(), "estimates": est.tolist(),
                 "std_errors": se.tolist(), "sse": sse}
# Simpler model: one curve for both sets.
est1, _, sse1 = ls(lambda t: np.concatenate([l4(x, *t) - ya, l4(x, *t) - yb]), [0, 100, 1, 0])
n = 2 * x.size
F = ((sse1 - sse) / 2) / (sse / (n - 6))
out["global"]["shared_sse"] = sse1
out["global"]["f"] = F
out["global"]["p"] = stats.f.sf(F, 2, n - 6)
def aicc(s, n, p):
    k = p + 1
    return n * np.log(s / n) + 2 * k + 2 * k * (k + 1) / (n - k - 1)
out["global"]["aicc_shared"] = aicc(sse1, n, 4)
out["global"]["aicc_global"] = aicc(sse, n, 6)

# 3. Robust (Cauchy-loss) fit with the scale re-estimated from the residuals:
#    alternate SciPy's own Cauchy-loss minimiser (loss="cauchy" is
#    ln(1 + (r/f_scale)²)) with RSDR = P68 · N/(N − K).
def rsdr(r, k):
    a = np.sort(np.abs(r))
    return np.percentile(a, 68.27) * len(a) / (len(a) - k)
yr = rate.copy()
yr[3] = 160.0  # a gross outlier
t = optimize.least_squares(lambda t: mm(conc, *t) - yr, [200, 0.06], method="lm", xtol=1e-15).x
for _ in range(500):
    s = rsdr(mm(conc, *t) - yr, 2)
    t_new = optimize.least_squares(lambda t: mm(conc, *t) - yr, t, loss="cauchy", f_scale=s,
                                   xtol=1e-15, ftol=1e-15, gtol=1e-15).x
    done = np.max(np.abs(t_new - t) / np.abs(t)) < 1e-12
    t = t_new
    if done:
        break
r = yr - mm(conc, *t)
s = rsdr(r, 2)
# The outlier rule on the robust residuals.
order = np.argsort(np.abs(r), kind="stable")
N, K, Q = len(r), 2, 0.01
found = []
for i in range(int(0.7 * N), N + 1):
    pv = 2 * stats.t.sf(abs(r[order[i - 1]]) / s, N - K)
    if pv < Q * (N - (i - 1)) / N:
        found = sorted(order[i - 1:].tolist())
        break
out["robust"] = {"x": conc.tolist(), "y": yr.tolist(), "estimates": t.tolist(), "scale": s, "outliers": found}

# 4. Implicit curves by orthogonal distance: an ellipse with ODRPACK.
ang = np.linspace(0, 2 * np.pi, 24, endpoint=False)
h, k_, a, b, phi = 1.0, -2.0, 4.0, 2.0, 0.5
ex = h + a * np.cos(ang) * np.cos(phi) - b * np.sin(ang) * np.sin(phi) + 0.15 * lcg(3, ang.size)
ey = k_ + a * np.cos(ang) * np.sin(phi) + b * np.sin(ang) * np.cos(phi) + 0.15 * lcg(5, ang.size)
def ellipse_distances(beta):
    """Exact orthogonal distances: the nearest point of the parametric
    ellipse found by a grid over the angle refined by Brent's method."""
    h, k, a, b, phi = beta
    out = []
    for px, py in zip(ex, ey):
        def d2(t):
            qx = h + a * np.cos(t) * np.cos(phi) - b * np.sin(t) * np.sin(phi)
            qy = k + a * np.cos(t) * np.sin(phi) + b * np.sin(t) * np.cos(phi)
            return (qx - px) ** 2 + (qy - py) ** 2
        ts = np.linspace(0, 2 * np.pi, 720)
        t0 = ts[np.argmin([d2(t) for t in ts])]
        r = optimize.minimize_scalar(d2, bracket=(t0 - 0.01, t0, t0 + 0.01), tol=1e-14)
        out.append(np.sqrt(r.fun))
    return np.array(out)
fit = optimize.least_squares(ellipse_distances, [0.8, -1.8, 3.8, 2.2, 0.4], xtol=1e-14, ftol=1e-14, gtol=1e-14)
out["ellipse"] = {"x": ex.tolist(), "y": ey.tolist(), "estimates": fit.x.tolist(), "sum_sq_distance": 2 * fit.cost}
# A circle by exact geometric distances (|p − c| − r).
cx = 2 + 3 * np.cos(ang[::2]) + 0.2 * lcg(9, 12)
cy = -1 + 3 * np.sin(ang[::2]) + 0.2 * lcg(13, 12)
est, se, sse = ls(lambda t: np.hypot(cx - t[0], cy - t[1]) - t[2], [1.5, -0.5, 2.5])
out["circle"] = {"x": cx.tolist(), "y": cy.tolist(), "estimates": est.tolist(), "std_errors": se.tolist(), "sse": sse}

# 5. Hougaard skewness for Michaelis–Menten on Puromycin, symbolic derivatives.
V, Kk, X = sp.symbols("V K X")
f = V * X / (Kk + X)
th = [V, Kk]
est, _, sse = ls(lambda t: mm(conc, *t) - rate, [200, 0.06])
subs = {V: est[0], Kk: est[1]}
grad = [sp.lambdify(X, sp.diff(f, a).subs(subs)) for a in th]
hess = [[sp.lambdify(X, sp.diff(f, a, b).subs(subs)) for b in th] for a in th]
Fm = np.array([[float(g(xx)) for g in grad] for xx in conc])
H = np.array([[[float(hess[a][b](xx)) for b in range(2)] for a in range(2)] for xx in conc])
L = np.linalg.inv(Fm.T @ Fm)
s2 = sse / (len(conc) - 2)
W = np.einsum("uj,ukl->jkl", Fm, H)
g1 = []
for i in range(2):
    tot = 0.0
    for j in range(2):
        for kk in range(2):
            for l in range(2):
                tot += L[i, j] * L[i, kk] * L[i, l] * (W[j, kk, l] + W[kk, j, l] + W[l, j, kk])
    g1.append(-s2**2 * tot / (s2 * L[i, i]) ** 1.5)
# Monte Carlo check (not used by the test): skewness of the estimates over
# simulated data sets at the fitted values with the fitted error SD.
rng = np.random.default_rng(1)
mu = mm(conc, *est)
sims = []
for _ in range(4000):
    ys = mu + rng.normal(0, np.sqrt(s2), mu.size)
    sims.append(optimize.least_squares(lambda t: mm(conc, *t) - ys, est, method="lm").x)
sims = np.array(sims)
out["hougaard"] = {"x": conc.tolist(), "y": rate.tolist(), "skewness": g1,
                   "monte_carlo_skewness": stats.skew(sims, axis=0).tolist()}

print(json.dumps(out, indent=1))
