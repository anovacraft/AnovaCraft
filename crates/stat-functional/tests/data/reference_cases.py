"""Independent reference values for the stat-functional tests.

scikit-fda 0.10.1 (with SciPy and scikit-learn), used as a black box, computes:

* B-spline basis values and first/second derivatives (``BSplineBasis``) and
  the Gram and second-derivative penalty matrices (``gram_matrix`` and
  ``L2Regularization(LinearDifferentialOperator(2)).penalty_matrix``, both
  exact piecewise-polynomial integrals in scikit-fda);
* Fourier smoothing: scikit-fda's Fourier functions take their phase from
  t = 0 (``sin(m w t)``), the crate's from the left end a
  (``sin(m w (t - a))``); both are orthonormal over one period and in the
  same order (constant, then sin/cos per harmonic). On a range starting at 0
  the bases coincide and the values are compared directly; on a range
  starting elsewhere each sin/cos pair differs by a rotation, which leaves
  the spanned space, the Gram matrix and both penalties (D^2 and the
  harmonic acceleration w^2 D + D^3) unchanged, so fitted values and the
  degrees of freedom are compared there;
* penalized least-squares smoothing with ``BasisSmoother``, whose objective
  (``_basis.py``) is  sum (y - Phi c)^2 + s * c' R c  with ``R`` from the
  regularization's ``penalty_matrix``, which itself carries the
  regularization's own ``regularization_parameter`` (left at 1). So the
  crate's lambda is ``smoothing_parameter`` with no conversion. Degrees of
  freedom are the trace of ``hat_matrix_``;
* GCV: the criterion is computed here from scikit-fda's fits, as
  sum_i n_i SSE_i / (n_i - df_i)^2 (SSE from ``BasisSmoother``'s fitted
  values, df the trace of its hat matrix). scikit-fda's own
  ``LinearSmootherGeneralizedCVScorer`` is not used: its docstring gives
  the Craven-Wahba GCV mean(r^2) / (1 - tr(S)/n)^2, but in 0.10.1 the code
  also divides each residual by 1 - S_mm (the leave-one-out shortcut)
  before applying that factor, which is a different criterion. For curves
  on two different grids the crate's criterion sums over all curves. The
  minimizer over log10(lambda) is found with SciPy's ``minimize_scalar``
  (bounded, xatol 1e-9) inside the best cell of a 0.25-decade grid;
* functional PCA (``FPCA`` on an ``FDataBasis``): centred, PCA of C L with
  W = L L' the basis Gram matrix (the computation of fda's pca.fd), through
  scikit-learn's PCA, whose ``explained_variance_`` has divisor N - 1; the
  crate's eigenvalues have divisor N, so they are written as
  explained_variance_ * (N - 1) / N. Scores are ``fit_transform``.
  Eigenfunction and score signs are arbitrary (compared up to one sign per
  component);
* landmark warps (``landmark_elastic_registration_warping``): monotone
  cubic (PCHIP) interpolation of (a, a), (mean landmark, landmark), (b, b),
  evaluated on the 401-point grid on which the crate stores warps.

All data are synthetic, generated here with fixed seeds. scikit-fda 0.10.1
needs multimethod < 2 (multimethod 2.x fails at import with a metaclass
conflict). Rerun from this directory:

    python3 reference_cases.py

which writes reference_cases.json.
"""

import json
import warnings

import numpy as np
from scipy import optimize

import skfda
from skfda import FDataBasis, FDataGrid
from skfda.misc.operators import LinearDifferentialOperator
from skfda.misc.regularization import L2Regularization
from skfda.preprocessing.dim_reduction import FPCA
from skfda.preprocessing.registration import landmark_elastic_registration_warping
from skfda.preprocessing.smoothing import BasisSmoother
from skfda.representation.basis import BSplineBasis, FourierBasis

warnings.simplefilter("ignore")
assert skfda.__version__ == "0.10.1", skfda.__version__
rng = np.random.default_rng(20261017)
out = {"skfda_version": skfda.__version__}


def mat(a):
    return np.asarray(a, dtype=float).tolist()


def d2_penalty(basis, order=2):
    return L2Regularization(LinearDifferentialOperator(order)).penalty_matrix(basis)


def smoother(basis, lam, operator):
    return BasisSmoother(
        basis,
        smoothing_parameter=lam,
        regularization=L2Regularization(operator),
        method="cholesky",
        return_basis=True,
    )


def fit(basis, t, y, lam, operator):
    """Coefficients, fitted values, df and summed GCV sum_i n SSE_i / (n - df)^2."""
    fd = FDataGrid(data_matrix=y, grid_points=t, domain_range=basis.domain_range)
    sm = smoother(basis, lam, operator)
    fdb = sm.fit_transform(fd)
    coefs = fdb.coefficients
    fitted = fdb(t)[:, :, 0]
    # The hat matrix of the same smoother returning values on the grid.
    sm_grid = BasisSmoother(
        basis, smoothing_parameter=lam, regularization=L2Regularization(operator), method="cholesky"
    )
    sm_grid.fit(fd)
    hat = sm_grid.hat_matrix_
    df = float(np.trace(hat))
    n = len(t)
    sse = ((y - fitted) ** 2).sum(axis=1)
    return coefs, fitted, df, float((n * sse / (n - df) ** 2).sum())


# ------------------------------------------------- (a, b) B-spline basis ---
bspline_cases = []
for (lo, hi), n_basis, order in [((0.0, 10.0), 9, 4), ((-1.0, 2.5), 12, 4), ((0.0, 1.0), 7, 3), ((2.0, 5.0), 10, 5)]:
    basis = BSplineBasis(domain_range=(lo, hi), n_basis=n_basis, order=order)
    t = np.concatenate([[lo, hi], lo + (hi - lo) * np.array([0.013, 0.2, 0.37, 0.5, 0.61, 0.888, 0.999])])
    values = [mat(basis(t, derivative=d)[:, :, 0].T) for d in range(3)]  # [d][point][function]
    bspline_cases.append(
        dict(
            range=[lo, hi],
            n_basis=n_basis,
            order=order,
            t=t.tolist(),
            values=values,
            gram=mat(basis.gram_matrix()),
            penalty2=mat(d2_penalty(basis)),
        )
    )
out["bspline"] = bspline_cases

# ------------------------------------------------- (d) fixed-lambda fits ---
n_curves, n_points = 12, 51
t = np.linspace(0.0, 10.0, n_points)
amp = rng.normal(1.0, 0.3, n_curves)
shift = rng.normal(0.0, 0.4, n_curves)
slope = rng.normal(0.0, 0.1, n_curves)
y = np.array(
    [a * np.sin(2 * np.pi * (t - s) / 10) + 0.5 * np.cos(4 * np.pi * t / 10) * a**2 + b * t for a, s, b in zip(amp, shift, slope)]
) + rng.normal(0.0, 0.15, (n_curves, n_points))
basis = BSplineBasis(domain_range=(0.0, 10.0), n_basis=15, order=4)
d2 = LinearDifferentialOperator(2)
fixed = []
for lam in [1e-3, 0.1, 10.0]:
    coefs, fitted, df, score = fit(basis, t, y, lam, d2)
    sse = ((y - fitted) ** 2).sum(axis=1)
    gcv = n_points * sse / (n_points - df) ** 2
    fixed.append({"lambda": lam, **dict( coefs=mat(coefs), fitted=mat(fitted), df=df, gcv=gcv.tolist())})
out["smooth"] = dict(t=t.tolist(), y=mat(y), range=[0.0, 10.0], n_basis=15, order=4, fixed=fixed)


# ------------------------------------------------- (e) GCV-chosen lambda ---
def gcv_min(groups, basis, operator, lo, hi):
    """Minimize the GCV summed over every curve of every group over log10 lambda."""

    def total(rho):
        return sum(fit(basis, tg, yg, 10.0**rho, operator)[3] for tg, yg in groups)

    # Bracket on a coarse grid, then a tight bounded search in the best cell.
    grid = np.arange(lo, hi + 1e-9, 0.25)
    vals = [total(r) for r in grid]
    k = int(np.argmin(vals))
    res = optimize.minimize_scalar(
        total, bounds=(grid[max(k - 1, 0)], grid[min(k + 1, len(grid) - 1)]), method="bounded", options=dict(xatol=1e-9)
    )
    rho = float(res.x)
    dfs = [fit(basis, tg, yg, 10.0**rho, operator)[2] for tg, yg in groups]
    return rho, dfs, float(res.fun)


rho, dfs, crit = gcv_min([(t, y)], basis, d2, -8.0, 4.0)
gcv_cases = [dict(name="common grid", groups=[0], log10_lambda=rho, df=dfs, criterion=crit)]

# Two groups of curves on different grids: the crate sums the per-curve terms.
t2 = np.sort(np.concatenate([[0.0, 10.0], rng.uniform(0.0, 10.0, 35)]))
y2 = np.array([a * np.sin(2 * np.pi * (t2 - s) / 10) + b * t2 for a, s, b in zip(amp[:5], shift[:5], slope[:5])]) + rng.normal(
    0.0, 0.25, (5, len(t2))
)
rho2, dfs2, crit2 = gcv_min([(t, y), (t2, y2)], basis, d2, -8.0, 4.0)
gcv_cases.append(dict(name="two grids", groups=[0, 1], log10_lambda=rho2, df=dfs2, criterion=crit2))
out["gcv"] = dict(second_group=dict(t=t2.tolist(), y=mat(y2)), cases=gcv_cases)

# ------------------------------------------------- (c) Fourier smoothing ---
fourier_cases = []
period = 10.0
tf = np.linspace(0.0, 10.0, 41)
yf = np.array([a * np.sin(2 * np.pi * (tf - s) / period) + 0.3 * np.cos(6 * np.pi * tf / period) for a, s in zip(amp[:4], shift[:4])])
yf = yf + rng.normal(0.0, 0.2, yf.shape)
w = 2 * np.pi / period
harmonic = LinearDifferentialOperator(weights=[0.0, w**2, 0.0, 1.0])
for a0, op_name, op, lam in [
    (0.0, "d2", d2, 0.05),
    (0.0, "harmonic", harmonic, 0.5),
    (3.0, "d2", d2, 0.05),
    (3.0, "harmonic", harmonic, 0.5),
]:
    fb = FourierBasis(domain_range=(a0, a0 + period), n_basis=11, period=period)
    ta = tf + a0
    coefs, fitted, df, score = fit(fb, ta, yf, lam, op)
    case = dict(
        start=a0,
        period=period,
        n_basis=11,
        operator=op_name,
        **{"lambda": lam},
        t=ta.tolist(),
        fitted=mat(fitted),
        df=df,
    )
    if a0 == 0.0:
        # Same basis as the crate's: compare values and coefficients too.
        case["values"] = [mat(fb(ta[:7], derivative=d)[:, :, 0].T) for d in range(3)]
        case["coefs"] = mat(coefs)
        case["penalty"] = mat(L2Regularization(op).penalty_matrix(fb))
    fourier_cases.append(case)
out["fourier"] = dict(y=mat(yf), cases=fourier_cases)

# ------------------------------------------------- (f) FPCA ---
coefs = np.array(out["smooth"]["fixed"][1]["coefs"])  # lambda = 0.1 smooths
fdb = FDataBasis(basis, coefs)
fpca = FPCA(n_components=3)
scores = fpca.fit_transform(fdb)
grid = np.linspace(0.0, 10.0, 21)
n = coefs.shape[0]
out["fpca"] = dict(
    coefs=mat(coefs),
    eigenvalues=(fpca.explained_variance_ * (n - 1) / n).tolist(),
    explained_variance_ratio=fpca.explained_variance_ratio_.tolist(),
    grid=grid.tolist(),
    eigenfunctions=mat(fpca.components_(grid)[:, :, 0]),
    harmonics=mat(fpca.components_.coefficients),
    mean=mat(fpca.mean_.coefficients[0]),
    scores=mat(scores),
)

# ------------------------------------------------- (g) landmark warps ---
lo, hi = 0.0, 10.0
wgrid = np.linspace(lo, hi, 401)
landmarks = np.array([[2.1, 5.0, 7.4], [2.8, 4.6, 8.1], [1.7, 5.9, 7.0], [2.5, 5.2, 7.9], [3.3, 6.1, 8.6]])
dummy = FDataGrid(data_matrix=np.zeros((len(landmarks), len(wgrid))), grid_points=wgrid, domain_range=(lo, hi))
warps = landmark_elastic_registration_warping(dummy, landmarks)
loc = np.array([[3.0, 5.5, 7.0]])
warps_loc = landmark_elastic_registration_warping(dummy, landmarks, location=loc[0])
out["landmark"] = dict(
    range=[lo, hi],
    grid=wgrid.tolist(),
    landmarks=mat(landmarks),
    warps=mat(warps.data_matrix[:, :, 0]),
    location=loc[0].tolist(),
    warps_location=mat(warps_loc.data_matrix[:, :, 0]),
)

with open("reference_cases.json", "w") as f:
    json.dump(out, f, indent=None, separators=(",", ":"))
    f.write("\n")
print("wrote reference_cases.json")
