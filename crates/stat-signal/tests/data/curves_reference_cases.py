"""Independent reference values for the stat-signal tests.

Run with ``python3 curves_reference_cases.py`` (writes ``curves_reference_cases.json`` next
to this file). Used as black boxes:

* SciPy 1.18: ``signal.savgol_filter`` (default ``mode="interp"``) and
  ``savgol_coeffs``; ``ndimage.uniform_filter1d``, ``median_filter``,
  ``percentile_filter`` and ``gaussian_filter1d`` (default ``mode="reflect"``);
  ``integrate.cumulative_trapezoid`` and ``cumulative_simpson`` (``initial=0``);
  ``interpolate.CubicSpline`` (default not-a-knot ends, and
  ``bc_type="natural"``) and ``PchipInterpolator``; ``signal.find_peaks``,
  ``peak_prominences`` and ``peak_widths`` (``rel_height=0.5``);
  ``special.wofz`` and ``special.voigt_profile``.
* NumPy 2.5: ``gradient`` with coordinates and ``interp``.
* pandas 3.0: ``rolling(w, min_periods=1).mean()`` (trailing moving average)
  and ``ewm(alpha, adjust=False).mean()`` (exponential smoothing).
* pybaselines 1.2.1: ``Baseline.asls``, ``arpls`` (defaults ``diff_order=2``,
  ``max_iter=50``, ``tol=1e-3``) and ``snip`` (``filter_order=2``, no
  smoothing, default linear-extrapolation padding). The log-log-square-root
  SNIP variant transforms the data here before ``snip`` and back after, as
  pybaselines' documentation describes.
* lmfit 1.3.4: least-squares fits (``leastsq``, MINPACK) of two overlapping
  peaks plus a ``LinearModel`` with ``GaussianModel``, ``LorentzianModel``,
  ``PseudoVoigtModel``, ``VoigtModel`` (``gamma`` set free),
  ``SplitLorentzianModel``, and a split Gaussian written as a plain
  ``lmfit.Model`` function; shared widths and spacings by lmfit constraint
  expressions. lmfit parameterizes peaks by area (``amplitude``) and
  ``sigma``; heights, FWHMs and areas are computed here from lmfit's
  estimates, and their standard errors by the delta method from lmfit's
  covariance matrix (``result.covar``, scaled by the reduced chi-square,
  lmfit's default) with central-difference gradients. The Voigt FWHM is
  solved exactly here (lmfit's own ``fwhm`` is the Olivero approximation).
"""

import json
import os

import lmfit
import numpy as np
import pandas as pd
import pybaselines
import scipy
from scipy import integrate, interpolate, ndimage, signal, special
from scipy.optimize import brentq

rng = np.random.default_rng(20261019)
LN2 = np.log(2.0)


def lst(a):
    return [float(v) for v in np.asarray(a, dtype=float)]


out = {
    "versions": {
        "scipy": scipy.__version__,
        "numpy": np.__version__,
        "pandas": pd.__version__,
        "pybaselines": pybaselines.__version__,
        "lmfit": lmfit.__version__,
    }
}

# --- Smoothing -------------------------------------------------------------
n = 120
t = np.linspace(0.0, 6.0, n)
y = np.sin(t) + 0.3 * np.cos(3.1 * t) + rng.normal(0, 0.1, n)
delta = t[1] - t[0]
sg = []
for (w, k, d) in [(5, 2, 0), (11, 3, 0), (11, 3, 1), (15, 4, 2), (21, 2, 0), (7, 6, 0)]:
    sg.append({
        "window": w, "order": k, "derivative": d, "delta": float(delta),
        "coefficients": lst(signal.savgol_coeffs(w, k, deriv=d, delta=delta, use="dot")),
        "values": lst(signal.savgol_filter(y, w, k, deriv=d, delta=delta)),
    })
filters = {
    "centred_mean_7": lst(ndimage.uniform_filter1d(y, 7)),
    "median_9": lst(ndimage.median_filter(y, 9)),
    "percentile_9_25": lst(ndimage.percentile_filter(y, 25, 9)),
    "percentile_9_90": lst(ndimage.percentile_filter(y, 90, 9)),
    "gaussian_2_5": lst(ndimage.gaussian_filter1d(y, 2.5)),
    "gaussian_0_8": lst(ndimage.gaussian_filter1d(y, 0.8)),
    "trailing_mean_6": lst(pd.Series(y).rolling(6, min_periods=1).mean()),
    "exponential_0_3": lst(pd.Series(y).ewm(alpha=0.3, adjust=False).mean()),
}
out["smoothing"] = {"t": lst(t), "y": lst(y), "savgol": sg, "filters": filters}

# --- Calculus ------------------------------------------------------------------
xu = np.sort(rng.uniform(0, 10, 40))
xu[0], xu[-1] = 0.0, 10.0
yu = np.exp(-0.2 * xu) * np.sin(1.3 * xu) + 0.05 * xu
grid = np.linspace(0.0, 10.0, 97)
out["calculus"] = {
    "x": lst(xu), "y": lst(yu), "grid": lst(grid),
    "gradient": lst(np.gradient(yu, xu)),
    "trapezoid": lst(integrate.cumulative_trapezoid(yu, xu, initial=0)),
    "simpson": lst(integrate.cumulative_simpson(yu, x=xu, initial=0)),
    "simpson_even": lst(integrate.cumulative_simpson(y, x=t, initial=0)),
    "linear": lst(np.interp(grid, xu, yu)),
    "spline": lst(interpolate.CubicSpline(xu, yu)(grid)),
    "natural": lst(interpolate.CubicSpline(xu, yu, bc_type="natural")(grid)),
    "pchip": lst(interpolate.PchipInterpolator(xu, yu)(grid)),
}

# --- Baselines ---------------------------------------------------------------
m = 600
xs = np.arange(m, dtype=float)
background = 40 + 0.03 * xs + 8 * np.sin(xs / 210.0) + 2e-5 * (xs - 300) ** 2
peaks = (60 * np.exp(-((xs - 150) ** 2) / (2 * 6.0 ** 2))
         + 35 / (1 + ((xs - 320) / 9.0) ** 2)
         + 50 * np.exp(-((xs - 335) ** 2) / (2 * 5.0 ** 2))
         + 25 * np.exp(-((xs - 470) ** 2) / (2 * 12.0 ** 2)))
spec = background + peaks + rng.normal(0, 0.8, m)
counts = np.round(np.abs(spec * 3))
fitter = pybaselines.Baseline(xs)
asls_b, asls_p = fitter.asls(spec, lam=1e6, p=0.01)
arpls_b, arpls_p = fitter.arpls(spec, lam=1e5)
snip_b, _ = fitter.snip(spec, max_half_window=25)
snip_dec, _ = fitter.snip(spec, max_half_window=25, decreasing=True)
lls = np.log(np.log(np.sqrt(counts + 1) + 1) + 1)
snip_lls_t, _ = fitter.snip(lls, max_half_window=25)
snip_lls = -1 + (np.exp(np.exp(snip_lls_t) - 1) - 1) ** 2
out["baseline"] = {
    "y": lst(spec), "counts": lst(counts),
    "asls": {"lambda": 1e6, "p": 0.01, "values": lst(asls_b), "iterations": len(asls_p["tol_history"])},
    "arpls": {"lambda": 1e5, "values": lst(arpls_b), "iterations": len(arpls_p["tol_history"])},
    "snip": {"half_window": 25, "values": lst(snip_b)},
    "snip_decreasing": {"half_window": 25, "values": lst(snip_dec)},
    "snip_lls": {"half_window": 25, "values": lst(snip_lls)},
}

# --- Peaks ---------------------------------------------------------------------
corrected = spec - asls_b
peak_cases = []
for (height, prominence, distance) in [(None, None, None), (5.0, 8.0, None), (None, 2.0, 8), (1.0, None, 15)]:
    kw = {}
    if height is not None:
        kw["height"] = height
    if prominence is not None:
        kw["prominence"] = prominence
    if distance is not None:
        kw["distance"] = distance
    idx, _ = signal.find_peaks(corrected, **kw)
    prom, lb, rb = signal.peak_prominences(corrected, idx)
    widths = signal.peak_widths(corrected, idx, rel_height=0.5, prominence_data=(prom, lb, rb))
    peak_cases.append({
        "min_height": height, "min_prominence": prominence, "min_distance": distance,
        "index": [int(i) for i in idx], "prominence": lst(prom),
        "left_base": [int(i) for i in lb], "right_base": [int(i) for i in rb],
        "left_crossing": lst(widths[2]), "right_crossing": lst(widths[3]),
    })
out["peaks"] = {"y": lst(corrected), "cases": peak_cases}

# --- Faddeeva / Voigt ---------------------------------------------------------
zs = [(x, yy) for x in [0.0, 0.1, 0.7, 1.5, 3.0, 5.5, 12.0, 40.0] for yy in [0.0, 1e-3, 0.05, 0.4, 1.0, 3.0, 10.0]]
out["faddeeva"] = [{"x": x, "y": yy, "re": float(special.wofz(x + 1j * yy).real)} for (x, yy) in zs]
out["voigt"] = [{"x": x, "sigma": s, "gamma": g, "value": float(special.voigt_profile(x, s, g))}
                for x in [0.0, 0.5, 2.0, 7.0] for (s, g) in [(1.0, 0.0), (1.0, 0.3), (0.4, 1.2), (2.0, 2.0)]]


# --- Peak fits ------------------------------------------------------------------
def voigt_fwhm(sigma, gamma):
    half = 0.5 * special.voigt_profile(0.0, sigma, gamma)
    return 2 * brentq(lambda u: special.voigt_profile(u, sigma, gamma) - half, 0.0, 2 * (sigma * 2.3548 + 2 * gamma))


def summary(kind, v, p):
    """(height, centre, fwhm, area) of peak prefix p from lmfit values v."""
    a, c = v[p + "amplitude"], v[p + "center"]
    s = v[p + "sigma"]
    if kind == "gaussian":
        return a / (s * np.sqrt(2 * np.pi)), c, 2 * np.sqrt(2 * LN2) * s, a
    if kind == "lorentzian":
        return a / (np.pi * s), c, 2 * s, a
    if kind == "pseudo-voigt":
        f = v[p + "fraction"]
        sg = s / np.sqrt(2 * LN2)
        h = (1 - f) * a / (sg * np.sqrt(2 * np.pi)) + f * a / (np.pi * s)
        return h, c, 2 * s, a
    if kind == "voigt":
        g = v[p + "gamma"]
        return a * special.voigt_profile(0.0, s, g), c, voigt_fwhm(s, g), a
    if kind == "split-lorentzian":
        r = v[p + "sigma_r"]
        return 2 * a / (np.pi * (s + r)), c, s + r, a
    if kind == "split-gaussian":
        r = v[p + "sigma_r"]
        # Here amplitude is the height and sigma, sigma_r are half widths at half maximum.
        return a, c, s + r, a * np.sqrt(np.pi / (4 * LN2)) * (s + r)
    raise ValueError(kind)


def split_gaussian(x, amplitude, center, sigma, sigma_r):
    hw = np.where(x < center, sigma, sigma_r)
    return amplitude * np.exp(-LN2 * ((x - center) / hw) ** 2)


def peak_model(kind, prefix):
    if kind == "gaussian":
        return lmfit.models.GaussianModel(prefix=prefix)
    if kind == "lorentzian":
        return lmfit.models.LorentzianModel(prefix=prefix)
    if kind == "pseudo-voigt":
        return lmfit.models.PseudoVoigtModel(prefix=prefix)
    if kind == "voigt":
        return lmfit.models.VoigtModel(prefix=prefix)
    if kind == "split-lorentzian":
        return lmfit.models.SplitLorentzianModel(prefix=prefix)
    if kind == "split-gaussian":
        return lmfit.Model(split_gaussian, prefix=prefix)
    raise ValueError(kind)


def make_data(kind, truth, n_points=260, noise=0.08):
    x = np.linspace(1500.0, 1700.0, n_points)
    v = {}
    for i, (a, c, s, extra) in enumerate(truth):
        p = f"p{i}_"
        v[p + "amplitude"], v[p + "center"], v[p + "sigma"] = a, c, s
        if kind == "pseudo-voigt":
            v[p + "fraction"] = extra
        if kind == "voigt":
            v[p + "gamma"] = extra
        if kind in ("split-lorentzian", "split-gaussian"):
            v[p + "sigma_r"] = extra
    model = sum((peak_model(kind, f"p{i}_") for i in range(len(truth))), start=lmfit.models.LinearModel(prefix="b_"))
    yv = model.eval(x=x, b_intercept=5.0, b_slope=0.01, **v) + rng.normal(0, noise, n_points)
    return x, yv


def fit_case(name, kind, truth, starts, common=False, spacing=None):
    x, yv = make_data(kind, truth)
    k = len(starts)
    model = lmfit.models.LinearModel(prefix="b_")
    for i in range(k):
        model = model + peak_model(kind, f"p{i}_")
    params = model.make_params()
    params["b_intercept"].set(value=float(np.min(yv)))
    params["b_slope"].set(value=0.0)
    for i, (c0, h0, w0) in enumerate(starts):
        p = f"p{i}_"
        hw = w0 / 2
        params[p + "center"].set(value=c0, min=x[0], max=x[-1])
        if kind in ("gaussian", "lorentzian", "pseudo-voigt", "voigt", "split-lorentzian"):
            params[p + "sigma"].set(value=hw if kind != "gaussian" else w0 / 2.3548, min=1e-6)
            params[p + "amplitude"].set(value=h0 * w0, min=0)
        if kind == "pseudo-voigt":
            params[p + "fraction"].set(value=0.5, min=0, max=1)
        if kind == "voigt":
            params[p + "sigma"].set(value=w0 / 4 / 1.1774)
            params[p + "gamma"].set(value=w0 / 4, vary=True, expr=None, min=1e-6)
        if kind == "split-lorentzian":
            params[p + "sigma_r"].set(value=hw, min=1e-6)
        if kind == "split-gaussian":
            params[p + "amplitude"].set(value=h0, min=0)
            params[p + "sigma"].set(value=hw, min=1e-6)
            params[p + "sigma_r"].set(value=hw, min=1e-6)
        if common and i > 0:
            for w in ("sigma", "gamma", "sigma_r"):
                if p + w in params:
                    params[p + w].set(expr="p0_" + w)
        if spacing is not None and i > 0:
            if spacing == "equal":
                if "spacing" not in params:
                    params.add("spacing", value=(starts[-1][0] - starts[0][0]) / (k - 1))
                params[p + "center"].set(expr=f"p0_center + {i} * spacing")
            else:
                params[p + "center"].set(expr=f"p0_center + {i * spacing}")
    res = model.fit(yv, params, x=x, method="leastsq", fit_kws={"xtol": 1e-12, "ftol": 1e-12}, max_nfev=200000)
    names = res.var_names
    best = np.array([res.params[nm].value for nm in names])
    cov = res.covar

    def values_from(theta):
        pp = res.params.copy()
        for nm, val in zip(names, theta):
            pp[nm].set(value=val)
        pp.update_constraints()
        return {nm: pp[nm].value for nm in pp}

    def delta_se(fn):
        g = np.zeros(len(best))
        for j in range(len(best)):
            h = np.finfo(float).eps ** (1 / 3) * max(abs(best[j]), 1e-8)
            up = best.copy()
            up[j] += h
            dn = best.copy()
            dn[j] -= h
            g[j] = (fn(values_from(up)) - fn(values_from(dn))) / (2 * h)
        return float(np.sqrt(g @ cov @ g))

    peaks_out = []
    v0 = values_from(best)
    order = sorted(range(k), key=lambda i: v0[f"p{i}_center"])
    for i in order:
        p = f"p{i}_"
        vals = summary(kind, v0, p)
        ses = [delta_se(lambda v, j=j: summary(kind, v, p)[j]) for j in range(4)]
        peaks_out.append({"height": [float(vals[0]), ses[0]], "centre": [float(vals[1]), ses[1]],
                          "fwhm": [float(vals[2]), ses[2]], "area": [float(vals[3]), ses[3]]})
    return {
        "name": name, "shape": kind, "x": lst(x), "y": lst(yv),
        "starts": [{"centre": c, "height": h, "fwhm": w} for (c, h, w) in starts],
        "common_width": common, "spacing": spacing,
        "sse": float(res.chisqr), "n_free": int(res.nvarys),
        "intercept": [float(res.params["b_intercept"].value), float(res.params["b_intercept"].stderr)],
        "slope": [float(res.params["b_slope"].value), float(res.params["b_slope"].stderr)],
        "peaks": peaks_out,
    }


fits = [
    fit_case("gaussian", "gaussian", [(400.0, 1590.0, 9.0, None), (250.0, 1612.0, 8.0, None)],
             [(1585.0, 15.0, 20.0), (1615.0, 10.0, 18.0)]),
    fit_case("lorentzian", "lorentzian", [(500.0, 1590.0, 7.0, None), (300.0, 1611.0, 6.0, None)],
             [(1585.0, 15.0, 20.0), (1615.0, 10.0, 18.0)]),
    fit_case("pseudo-voigt", "pseudo-voigt", [(450.0, 1590.0, 8.0, 0.3), (280.0, 1612.0, 7.0, 0.6)],
             [(1586.0, 15.0, 18.0), (1616.0, 10.0, 16.0)]),
    fit_case("voigt", "voigt", [(450.0, 1590.0, 4.0, 3.0), (280.0, 1613.0, 3.0, 4.0)],
             [(1586.0, 15.0, 18.0), (1616.0, 10.0, 16.0)]),
    fit_case("split-lorentzian", "split-lorentzian", [(450.0, 1590.0, 5.0, 9.0), (280.0, 1615.0, 4.0, 7.0)],
             [(1586.0, 15.0, 18.0), (1617.0, 10.0, 16.0)]),
    fit_case("split-gaussian", "split-gaussian", [(20.0, 1590.0, 6.0, 10.0), (12.0, 1618.0, 5.0, 8.0)],
             [(1586.0, 15.0, 18.0), (1620.0, 10.0, 16.0)]),
    fit_case("common width, equal spacing", "gaussian",
             [(300.0, 1570.0, 7.0, None), (200.0, 1595.0, 7.0, None), (150.0, 1620.0, 7.0, None)],
             [(1568.0, 15.0, 15.0), (1597.0, 10.0, 15.0), (1623.0, 8.0, 15.0)], common=True, spacing="equal"),
    fit_case("fixed spacing", "lorentzian",
             [(300.0, 1570.0, 6.0, None), (200.0, 1600.0, 5.0, None)],
             [(1568.0, 15.0, 15.0), (1603.0, 10.0, 15.0)], spacing=30.0),
]
out["fits"] = fits

# Comparing numbers of peaks: 1, 2 and 3 Gaussians on three-peak data.
x3, y3 = make_data("gaussian", [(300.0, 1570.0, 7.0, None), (200.0, 1600.0, 8.0, None), (90.0, 1640.0, 6.0, None)])
starts3 = [(1572.0, 15.0, 15.0), (1598.0, 10.0, 15.0), (1642.0, 5.0, 15.0)]
rows = []
for kk in (1, 2, 3):
    model = lmfit.models.LinearModel(prefix="b_")
    for i in range(kk):
        model = model + lmfit.models.GaussianModel(prefix=f"p{i}_")
    params = model.make_params(b_intercept=float(np.min(y3)), b_slope=0.0)
    for i in range(kk):
        c0, h0, w0 = starts3[i]
        params[f"p{i}_center"].set(value=c0, min=x3[0], max=x3[-1])
        params[f"p{i}_sigma"].set(value=w0 / 2.3548, min=1e-6)
        params[f"p{i}_amplitude"].set(value=h0 * w0, min=0)
    r = model.fit(y3, params, x=x3, method="leastsq", fit_kws={"xtol": 1e-12, "ftol": 1e-12}, max_nfev=200000)
    nn, kf = len(x3), r.nvarys
    big_k = kf + 1
    aicc = nn * np.log(r.chisqr / nn) + 2 * big_k + 2 * big_k * (big_k + 1) / (nn - big_k - 1)
    rows.append({"peaks": kk, "sse": float(r.chisqr), "n_free": int(kf), "aicc": float(aicc)})
for i in range(1, len(rows)):
    a, b = rows[i - 1], rows[i]
    df1, df2 = b["n_free"] - a["n_free"], len(x3) - b["n_free"]
    f = ((a["sse"] - b["sse"]) / df1) / (b["sse"] / df2)
    b["f"], b["p_value"] = float(f), float(scipy.stats.f.sf(f, df1, df2))
out["peak_counts"] = {"x": lst(x3), "y": lst(y3), "starts": [{"centre": c, "height": h, "fwhm": w} for (c, h, w) in starts3],
                      "rows": rows}

with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "curves_reference_cases.json"), "w") as fh:
    json.dump(out, fh, separators=(",", ":"))
print("written")
