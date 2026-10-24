"""Reference values for stat_viz::interpolate (run: python3 interpolation_reference.py).

Independent implementations:
* Delaunay triangle count and linear interpolation: scipy.spatial.Delaunay
  (Qhull) and scipy.interpolate.LinearNDInterpolator, on coordinates scaled
  to [0, 1] over each column's range (as the Rust code does).
* Interpolating thin-plate spline: scipy.interpolate.RBFInterpolator with
  kernel="thin_plate_spline" and degree=1 (smoothing 0), on the scaled
  coordinates.
* Cross-validated thin-plate spline: the full bordered system
  [[K + n*lam*I, T], [T', 0]] solved densely by numpy, the influence matrix
  built column by column, and the GCV score n*||(I-A)z||^2 / tr(I-A)^2
  minimised over log10(lam) with scipy.optimize.minimize_scalar.
"""
import numpy as np
from scipy.spatial import Delaunay
from scipy.interpolate import LinearNDInterpolator, RBFInterpolator
from scipy.optimize import minimize_scalar

M = 2**64


def uniforms(seed, n):
    s, out = seed, []
    for _ in range(n):
        s = (s * 6364136223846793005 + 1442695040888963407) % M
        out.append((s >> 11) / float(2**53))
    return np.array(out)


x = 10.0 * uniforms(7, 40)
y = 10.0 * uniforms(11, 40) - 5.0
z = np.sin(0.5 * x) * np.cos(0.3 * y) + 0.1 * x * y / 5.0

sx = lambda v: (v - x.min()) / (x.max() - x.min())
sy = lambda v: (v - y.min()) / (y.max() - y.min())
P = np.column_stack([sx(x), sy(y)])

tri = Delaunay(P)
print("triangles", len(tri.simplices))

queries = [(1.0, -4.0), (5.0, 0.0), (9.0, 4.0), (2.5, 2.5), (7.3, -1.2), (0.05, 4.9), (4.0, -2.0)]
lin = LinearNDInterpolator(P, z)
print("LINEAR_REFERENCE")
for u, v in queries:
    val = lin([[sx(u), sy(v)]])[0]
    print(f"    ({u}, {v}, {'f64::NAN' if np.isnan(val) else repr(float(val))}),")

rbf = RBFInterpolator(P, z, kernel="thin_plate_spline", degree=1, smoothing=0.0)
print("SPLINE_REFERENCE")
for u, v in queries:
    print(f"    ({u}, {v}, {float(rbf([[sx(u), sy(v)]])[0])!r}),")

# Cross-validated spline.
noisy = z + 0.4 * (uniforms(3, 40) - 0.5)
n = len(x)


def E(r2):
    with np.errstate(divide="ignore", invalid="ignore"):
        return np.where(r2 > 0, r2 * np.log(r2) / (16 * np.pi), 0.0)


D2 = ((P[:, None, :] - P[None, :, :]) ** 2).sum(-1)
K = E(D2)
T = np.column_stack([np.ones(n), P])


def solve(lam, rhs):
    A = np.block([[K + n * lam * np.eye(n), T], [T.T, np.zeros((3, 3))]])
    sol = np.linalg.solve(A, np.concatenate([rhs, np.zeros(3)]))
    return sol[:n], sol[n:]


def fitted(lam, rhs):
    c, d = solve(lam, rhs)
    return K @ c + T @ d


def gcv(log_lam):
    lam = 10.0**log_lam
    infl = np.column_stack([fitted(lam, np.eye(n)[:, j]) for j in range(n)])
    r = noisy - infl @ noisy
    tr = n - np.trace(infl)
    return n * (r @ r) / tr**2, n - tr


grid = np.linspace(-12, 2, 281)
vals = [gcv(g)[0] for g in grid]
k = int(np.argmin(vals))
res = minimize_scalar(lambda s: gcv(s)[0], bounds=(grid[max(k - 1, 0)], grid[min(k + 1, len(grid) - 1)]),
                      method="bounded", options={"xatol": 1e-10})
lam = 10.0**res.x
score, df = gcv(res.x)
c, d = solve(lam, noisy)
q = np.array([sx(5.0), sy(0.0)])
val = E(((P - q) ** 2).sum(-1)) @ c + d[0] + d[1] * q[0] + d[2] * q[1]
print(f"GCV_REFERENCE = ({lam!r}, {score!r}, {df!r}, {float(val)!r})")
