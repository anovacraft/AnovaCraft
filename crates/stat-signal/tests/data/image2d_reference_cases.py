"""Reference values for stat_signal::image2d, computed with NumPy, SciPy and
scikit-image used as black boxes. Writes image2d_reference_cases.json next to
this script; tests/image2d_reference.rs reads it.

    python3 -m pip install numpy scipy scikit-image
    python3 -I crates/stat-signal/tests/data/image2d_reference_cases.py

Versions used for the committed file are recorded in its "versions" entry.

Conventions:
* Images are row-major lists (row 0 first) built from fixed formulas plus
  noise from numpy's PCG64 with a fixed seed, and stored in the file, so the
  Rust tests use the same data.
* Transforms are in NumPy's order (zero frequency at [0, 0]).
* skimage.filters.butterworth is called with npad=0 (the image treated as
  periodic, as stat-signal does without padding) and the default
  squared_butterworth=True, so its cutoff_frequency_ratio is the cut-off D0
  in cycles per pixel.
* scipy.ndimage.fourier_gaussian with sigma s is the Gaussian low-pass with
  D0 = 1 / (2 pi s).
* skimage.filters.window computes its coordinates in float32, so the radial
  windows agree to about 1e-6 only.
* skimage.restoration.wiener and richardson_lucy are called with clip=False
  and odd-sized PSFs (whose centre both libraries place alike).
* The centred power spectrum is composed from numpy.fft (the definition in
  stat_signal::image2d::spectrum), so it checks windowing, padding and
  centring rather than an outside routine.
"""
import json
import os

import numpy as np
import scipy
import scipy.ndimage as ndi
import scipy.signal as sig
import skimage
from skimage.filters import butterworth, window
from skimage.registration import phase_cross_correlation
from skimage.restoration import richardson_lucy, wiener

rng = np.random.default_rng(20261020)
out = {"versions": {"numpy": np.__version__, "scipy": scipy.__version__, "skimage": skimage.__version__}}


def lst(a):
    return [float("%.14g" % v) for v in np.asarray(a, dtype=float).ravel()]


def cplx(a):
    a = np.asarray(a)
    return {"re": lst(a.real), "im": lst(a.imag)}


def img(a):
    a = np.asarray(a, dtype=float)
    return {"rows": a.shape[0], "cols": a.shape[1], "data": lst(a)}


def scene(rows, cols, noise=0.05):
    r, c = np.mgrid[0:rows, 0:cols].astype(float)
    a = np.exp(-((r - 0.4 * rows) ** 2 + (c - 0.55 * cols) ** 2) / (0.02 * rows * cols))
    a += 0.6 * ((r > 0.6 * rows) & (r < 0.8 * rows) & (c > 0.15 * cols) & (c < 0.45 * cols))
    a += 0.3 * np.cos(2 * np.pi * (3 * c / cols + 2 * r / rows))
    return a + noise * rng.normal(size=(rows, cols)) + 1.0


# --- Transforms ------------------------------------------------------------
fft_cases = []
for shape in [(1, 1), (1, 17), (13, 1), (7, 5), (12, 9), (32, 24), (29, 31)]:
    x = rng.normal(size=shape) + 1j * rng.normal(size=shape)
    fft_cases.append({"rows": shape[0], "cols": shape[1], "x": cplx(x), "fft2": cplx(np.fft.fft2(x)), "ifft2": cplx(np.fft.ifft2(x))})
out["fft2"] = fft_cases

rfft_cases = []
for shape in [(7, 5), (9, 10), (24, 16), (31, 17)]:
    x = rng.normal(size=shape)
    half = np.fft.rfft2(x)
    # irfft2 of a spectrum that is not exactly Hermitian: NumPy's handling is the reference.
    noisy_half = half + 0.01 * (rng.normal(size=half.shape) + 1j * rng.normal(size=half.shape))
    rfft_cases.append({"x": img(x), "rfft2": cplx(half), "noisy_half": cplx(noisy_half),
                       "irfft2": lst(np.fft.irfft2(noisy_half, s=shape))})
out["rfft2"] = rfft_cases

x = rng.normal(size=(7, 6))
out["shift"] = {"x": img(x), "fftshift": lst(np.fft.fftshift(x)), "ifftshift": lst(np.fft.ifftshift(x))}

# --- Windows ---------------------------------------------------------------
wins = []
for spec, label, shape in [("hann", "hann", (12, 9)), (("tukey", 0.3), "tukey", (10, 16))]:
    w = np.outer(sig.get_window(spec, shape[0]), sig.get_window(spec, shape[1]))
    wins.append({"kind": "separable", "window": label, "rows": shape[0], "cols": shape[1], "w": lst(w)})
for spec, label, shape in [("hann", "hann", (16, 16)), ("hann", "hann", (20, 31)), (("tukey", 0.5), "tukey", (15, 15)),
                           ("hamming", "hamming", (11, 8))]:
    wins.append({"kind": "radial", "window": label, "rows": shape[0], "cols": shape[1], "w": lst(window(spec, shape))})
out["windows"] = wins

# --- Spectrum (composed from numpy.fft) ------------------------------------
sp = scene(40, 52)
spec_cases = []
for taper, pad in [("none", None), ("hann", None), ("hann", (64, 64))]:
    x = sp - sp.mean()
    w = np.ones_like(x) if taper == "none" else np.outer(sig.get_window("hann", 40), sig.get_window("hann", 52))
    s = pad or x.shape
    f = np.fft.fft2(x * w, s=s)
    power = np.abs(f) ** 2 / (s[0] * s[1] * np.sum(w ** 2))
    spec_cases.append({"taper": taper, "pad_rows": s[0], "pad_cols": s[1], "power": lst(np.fft.fftshift(power)),
                       "magnitude": lst(np.fft.fftshift(np.abs(f) / np.sum(w))),
                       "fx": lst(np.fft.fftshift(np.fft.fftfreq(s[1], 0.5))), "fy": lst(np.fft.fftshift(np.fft.fftfreq(s[0], 2.0)))})
out["spectrum"] = {"image": img(sp), "dx": 0.5, "dy": 2.0, "cases": spec_cases}

# --- Frequency filters -----------------------------------------------------
fi = scene(40, 33)
out["filters"] = {
    "image": img(fi),
    "butterworth": [
        {"cutoff": 0.08, "high_pass": False, "order": 2.0, "out": lst(butterworth(fi, 0.08, high_pass=False, order=2.0, npad=0))},
        {"cutoff": 0.15, "high_pass": True, "order": 3.5, "out": lst(butterworth(fi, 0.15, high_pass=True, order=3.5, npad=0))},
    ],
    "gaussian": [{"sigma": s, "out": lst(np.fft.ifft2(ndi.fourier_gaussian(np.fft.fft2(fi), sigma=s)).real)} for s in [1.0, 2.5]],
}

# --- Convolution and correlation -------------------------------------------
a = rng.normal(size=(20, 17))
conv = []
for shape in [(5, 3), (4, 6), (1, 7)]:
    b = rng.normal(size=shape)
    for mode in ["full", "same", "valid"]:
        conv.append({"b": img(b), "mode": mode, "convolve": img(sig.convolve(a, b, mode=mode, method="direct")),
                     "correlate": img(sig.correlate(a, b, mode=mode, method="direct"))})
xa = scene(15, 12)
xm = xa - xa.mean()
ac = sig.correlate(xm, xm, mode="full", method="direct")
out["convolve"] = {"a": img(a), "cases": conv, "autocorr_image": img(xa), "autocorr": img(ac / ac[14, 11])}

# --- Registration ----------------------------------------------------------
ref = scene(48, 60, noise=0.0)
shift = (2.37, -4.81)
moved = np.fft.ifft2(ndi.fourier_shift(np.fft.fft2(ref), shift)).real
moved_noisy = moved + 0.02 * rng.normal(size=moved.shape)
reg = []
for u in [1, 10, 100]:
    s, err, phase = phase_cross_correlation(ref, moved_noisy, upsample_factor=u, normalization="phase")
    reg.append({"upsample": u, "shift": lst(s)})
out["registration"] = {"reference": img(ref), "moving": img(moved_noisy), "shift_applied": list(shift),
                       "fourier_shift": lst(moved), "cases": reg}

# --- Deconvolution ---------------------------------------------------------
truth = scene(36, 44, noise=0.0)
rr, cc = np.mgrid[-3:4, -3:4]
psf = np.exp(-(rr ** 2 + cc ** 2) / (2 * 1.4 ** 2))
psf /= psf.sum()
blurred = sig.convolve(truth, psf, mode="same") + 0.01 * rng.normal(size=truth.shape)
blurred = np.clip(blurred, 0, None)
out["deconvolution"] = {
    "image": img(blurred), "psf": img(psf),
    "wiener": [{"balance": b, "out": lst(wiener(blurred, psf, balance=b, clip=False))} for b in [0.01, 0.2]],
    "richardson_lucy": [{"iterations": n, "out": lst(richardson_lucy(blurred, psf, num_iter=n, clip=False))} for n in [1, 15]],
}

with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "image2d_reference_cases.json"), "w") as fh:
    json.dump(out, fh, separators=(",", ":"))
