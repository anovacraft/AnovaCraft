"""Reference values for stat-signal, computed with NumPy, SciPy, PyWavelets
and scikit-image used as black boxes. Writes reference_cases.json next to
this script; tests/reference.rs reads it.

    python3 -m pip install numpy scipy PyWavelets scikit-image
    python3 -I crates/stat-signal/tests/data/reference_cases.py

Versions used for the committed file are recorded in its "versions" entry.

Conventions:
* Signals are built from fixed formulas plus noise from numpy's PCG64 with
  fixed seeds, and stored in the file, so the Rust tests use the same data.
* Two-sided spectra are in NumPy's transform order (fftfreq).
* Richardson-Lucy: scikit-image is called with clip=False and a kernel
  normalised to sum 1 (stat-signal normalises the kernel itself).
* Wavelet denoising is composed from pywt.wavedec, pywt.threshold and
  pywt.waverec; the SureShrink threshold (not in PyWavelets) is computed here
  directly from Donoho & Johnstone (1995), an independent implementation of
  the published rule.
"""
import json
import os

import numpy as np
import pywt
import scipy
import scipy.signal as sig
import skimage
from skimage.restoration import richardson_lucy

rng = np.random.default_rng(20261019)
out = {"versions": {"numpy": np.__version__, "scipy": scipy.__version__, "pywt": pywt.__version__, "skimage": skimage.__version__}}


def lst(a):
    # 14 significant digits keep the file small; tests use relative tolerances ≥ 1e-10.
    return [float("%.14g" % v) for v in np.asarray(a).ravel()]


def cplx(a):
    a = np.asarray(a)
    return {"re": lst(a.real), "im": lst(a.imag)}


# --- FFT -------------------------------------------------------------------
fft_cases = []
for n in [1, 2, 3, 5, 8, 12, 17, 30, 64, 97, 100, 127, 210, 1000, 1009]:
    x = rng.normal(size=n) + 1j * rng.normal(size=n)
    fft_cases.append({"n": n, "x": cplx(x), "fft": cplx(np.fft.fft(x))})
out["fft"] = fft_cases

real = rng.normal(size=45)
out["rfft"] = {"x": lst(real), "n": 64, "rfft": cplx(np.fft.rfft(real, 64)), "irfft_n": 45, "irfft": lst(np.fft.irfft(np.fft.rfft(real), 45))}

# --- Windows -------------------------------------------------------------
windows = []
for spec, label in [("boxcar", "rectangular"), ("hann", "hann"), ("hamming", "hamming"), ("blackman", "blackman"),
                    ("flattop", "flattop"), (("kaiser", 8.6), "kaiser"), (("tukey", 0.3), "tukey")]:
    for m in [1, 2, 7, 16, 33]:
        windows.append({"window": label, "m": m, "periodic": lst(sig.get_window(spec, m, fftbins=True)),
                        "symmetric": lst(sig.get_window(spec, m, fftbins=False))})
out["windows"] = windows

# --- Spectra -------------------------------------------------------------
fs = 50.0
t = np.arange(301) / fs
signal = 2.0 * np.sin(2 * np.pi * 6.0 * t) + 0.7 * np.cos(2 * np.pi * 13.3 * t + 0.4) + 0.3 * t + rng.normal(scale=0.5, size=t.size)
out["signal"] = {"fs": fs, "x": lst(signal)}

periodograms = []
for window, wl in [("boxcar", "rectangular"), ("hann", "hann"), (("kaiser", 8.6), "kaiser")]:
    for detrend in (["constant", "linear", False] if wl == "rectangular" else ["linear"]):
        for scaling, onesided, nfft in [("density", True, None), ("spectrum", False, 512), ("density", False, None), ("spectrum", True, 512)]:
            if True:
                if True:
                    f, p = sig.periodogram(signal, fs=fs, window=window, nfft=nfft, detrend=detrend,
                                           return_onesided=onesided, scaling=scaling)
                    periodograms.append({"window": wl, "detrend": str(detrend), "scaling": scaling, "onesided": onesided,
                                         "nfft": nfft or 0, "f": lst(f), "p": lst(p)})
out["periodogram"] = periodograms

# Amplitude and phase from a direct NumPy transform of the detrended, windowed signal.
w = sig.get_window("hann", signal.size)
xd = sig.detrend(signal, type="linear") * w
X = np.fft.rfft(xd, 400)
amp = np.abs(X) / w.sum()
amp[1:-1] *= 2  # nfft = 400 is even: Nyquist not doubled
out["amplitude"] = {"window": "hann", "detrend": "linear", "nfft": 400, "amplitude": lst(amp), "phase": lst(np.angle(X))}

welchs = []
for window, wl in [("hann", "hann"), ("hamming", "hamming"), (("tukey", 0.25), "tukey")]:
    for nperseg, noverlap in [(64, None), (100, 30), (128, 0)]:
        for nfft, detrend in [(None, "constant"), (256, "linear")]:
            if True:
                for scaling, onesided in [("density", True), ("spectrum", False)]:
                    f, p = sig.welch(signal, fs=fs, window=window, nperseg=nperseg, noverlap=noverlap, nfft=nfft,
                                     detrend=detrend, scaling=scaling, return_onesided=onesided)
                    welchs.append({"window": wl, "nperseg": nperseg, "noverlap": -1 if noverlap is None else noverlap,
                                   "nfft": nfft or 0, "detrend": detrend, "scaling": scaling, "onesided": onesided,
                                   "f": lst(f), "p": lst(p)})
out["welch"] = welchs

f, tt, S = sig.spectrogram(signal, fs=fs, nperseg=40, noverlap=10)
out["spectrogram"] = {"nperseg": 40, "noverlap": 10, "f": lst(f), "t": lst(tt), "S": [lst(S[:, j]) for j in range(S.shape[1])]}
f, tt, S = sig.spectrogram(signal, fs=fs, window="hann", nperseg=64, noverlap=None, nfft=128, scaling="spectrum")
out["spectrogram2"] = {"nperseg": 64, "nfft": 128, "f": lst(f), "t": lst(tt), "S": [lst(S[:, j]) for j in range(S.shape[1])]}

# --- IIR / FIR filters ---------------------------------------------------
filt_x = signal
iir = []
designs = [
    ("butter", {}, 4, "lowpass", 8.0), ("butter", {}, 3, "highpass", 4.0), ("butter", {}, 5, "bandpass", (4.0, 9.0)),
    ("butter", {}, 2, "bandstop", (5.0, 7.0)), ("butter", {}, 12, "lowpass", 10.0),
    ("cheby1", {"rp": 0.5}, 4, "lowpass", 8.0), ("cheby1", {"rp": 1.0}, 5, "highpass", 4.0),
    ("cheby1", {"rp": 2.0}, 3, "bandpass", (4.0, 9.0)), ("cheby1", {"rp": 1.0}, 4, "bandstop", (5.0, 7.0)),
    ("cheby2", {"rs": 40.0}, 4, "lowpass", 8.0), ("cheby2", {"rs": 30.0}, 5, "highpass", 4.0),
    ("cheby2", {"rs": 50.0}, 3, "bandpass", (4.0, 9.0)), ("cheby2", {"rs": 40.0}, 4, "bandstop", (5.0, 7.0)),
    ("ellip", {"rp": 1.0, "rs": 40.0}, 4, "lowpass", 8.0), ("ellip", {"rp": 0.5, "rs": 60.0}, 5, "highpass", 4.0),
    ("ellip", {"rp": 1.0, "rs": 50.0}, 3, "bandpass", (4.0, 9.0)), ("ellip", {"rp": 0.1, "rs": 40.0}, 4, "bandstop", (5.0, 7.0)),
    ("ellip", {"rp": 1.0, "rs": 30.0}, 1, "lowpass", 8.0), ("ellip", {"rp": 0.2, "rs": 80.0}, 8, "lowpass", 3.0),
    ("cheby1", {"rp": 0.5}, 1, "lowpass", 8.0), ("cheby2", {"rs": 40.0}, 1, "lowpass", 8.0),
]
for family, params, order, btype, wn in designs:
    sos = getattr(sig, family)(order, **({"rp": params["rp"]} if family == "cheby1" else {}),
                               **({"rs": params["rs"]} if family == "cheby2" else {}),
                               **({"rp": params["rp"], "rs": params["rs"]} if family == "ellip" else {}),
                               Wn=wn, btype=btype, output="sos", fs=fs)
    w_, h = sig.sosfreqz(sos, worN=64, fs=fs)
    iir.append({"family": family, "rp": params.get("rp", 0.0), "rs": params.get("rs", 0.0), "order": order, "btype": btype,
                "wn": lst(np.atleast_1d(wn)), "forward": lst(sig.sosfilt(sos, filt_x)), "filtfilt": lst(sig.sosfiltfilt(sos, filt_x)),
                "freq": lst(w_), "h": cplx(h)})
out["iir"] = iir

fir = []
for taps, btype, cutoff, window, wl in [(31, "lowpass", 8.0, "hamming", "hamming"), (41, "highpass", 5.0, "hann", "hann"),
                                         (51, "bandpass", (4.0, 9.0), ("kaiser", 6.0), "kaiser"), (33, "bandstop", (5.0, 7.0), "blackman", "blackman"),
                                         (20, "lowpass", 10.0, "boxcar", "rectangular")]:
    pass_zero = btype in ("lowpass", "bandstop")
    b = sig.firwin(taps, cutoff, window=window, pass_zero=pass_zero, fs=fs)
    w_, h = sig.freqz(b, 1, worN=64, fs=fs)
    fir.append({"taps": taps, "btype": btype, "cutoff": lst(np.atleast_1d(cutoff)), "window": wl, "b": lst(b),
                "forward": lst(sig.lfilter(b, 1, filt_x)), "filtfilt": lst(sig.filtfilt(b, 1, filt_x)), "h": cplx(h)})
out["fir"] = fir

b, a = sig.iirnotch(13.3, 20.0, fs=fs)
out["notch"] = {"f0": 13.3, "q": 20.0, "b": lst(b), "a": lst(a), "filtfilt": lst(sig.filtfilt(b, a, filt_x, padtype="odd", padlen=9))}

# --- Convolution, correlation, deconvolution, Hilbert ----------------------
a1 = rng.normal(size=60)
k1 = np.exp(-0.5 * ((np.arange(9) - 4) / 1.5) ** 2)
k2 = rng.normal(size=77)
conv = {"a": lst(a1), "k1": lst(k1), "k2": lst(k2)}
for mode in ["full", "same", "valid"]:
    conv[f"k1_{mode}"] = lst(sig.convolve(a1, k1, mode=mode, method="direct"))
    conv[f"k2_{mode}"] = lst(sig.fftconvolve(a1, k2, mode=mode))
conv["circular"] = lst(np.real(np.fft.ifft(np.fft.fft(a1) * np.fft.fft(np.r_[k1, np.zeros(51)]))))
out["convolution"] = conv

y1 = rng.normal(size=80)
x1 = np.r_[np.zeros(7), y1[:-7]] + 0.2 * rng.normal(size=80)
full = sig.correlate(x1, y1, mode="full")
lags = sig.correlation_lags(x1.size, y1.size, mode="full")
xm, ym = x1 - x1.mean(), y1 - y1.mean()
coef = sig.correlate(xm, ym, mode="full") / np.sqrt((xm ** 2).sum() * (ym ** 2).sum())
out["correlation"] = {"x": lst(x1), "y": lst(y1), "lags": [int(v) for v in lags], "raw": lst(full), "coef": lst(coef),
                      "lag_of_max": int(lags[np.argmax(np.abs(coef))])}

hx = signal[:200]
z = sig.hilbert(hx)
out["hilbert"] = {"x": lst(hx), "analytic": cplx(z), "envelope": lst(np.abs(z)), "phase": lst(np.unwrap(np.angle(z))),
                  "frequency": lst(np.diff(np.unwrap(np.angle(z))) / (2 * np.pi) * fs)}
z = sig.hilbert(signal[:201])
out["hilbert_odd"] = {"x": lst(signal[:201]), "analytic": cplx(z)}

truth = np.zeros(120)
truth[[20, 45, 50, 90]] = [5.0, 3.0, 4.0, 2.0]
truth += 0.2
psf = np.exp(-0.5 * ((np.arange(15) - 7) / 2.0) ** 2)
psf /= psf.sum()
blurred = sig.convolve(truth, psf, mode="same")
rl = richardson_lucy(blurred, psf, num_iter=30, clip=False)
lam = 0.01
H = np.fft.fft(np.roll(np.r_[psf, np.zeros(120 - psf.size)], -7))
wien = np.real(np.fft.ifft(np.fft.fft(blurred) * np.conj(H) / (np.abs(H) ** 2 + lam)))
out["deconvolution"] = {"blurred": lst(blurred), "psf": lst(psf), "iterations": 30, "richardson_lucy": lst(rl), "lambda": lam,
                        "wiener": lst(wien)}

# --- Wavelets ----------------------------------------------------------------
wx = signal[:157]
wav = []
names = ["haar"] + [f"db{i}" for i in range(1, 11)] + [f"sym{i}" for i in range(2, 11)] + [f"coif{i}" for i in range(1, 6)]
for name in names:
    for mode in ["symmetric", "periodization", "zero"]:
        cA, cD = pywt.dwt(wx, name, mode=mode)
        level = min(3, pywt.dwt_max_level(wx.size, name)) or 1
        dec = pywt.wavedec(wx, name, mode=mode, level=level)
        wav.append({"name": name, "mode": mode, "cA": lst(cA), "cD": lst(cD),
                    "level": level, "wavedec": [lst(c) for c in dec] if mode != "zero" else [], "max_level": pywt.dwt_max_level(wx.size, name),
                    "dec_lo": lst(pywt.Wavelet(name).dec_lo), "dec_hi": lst(pywt.Wavelet(name).dec_hi),
                    "rec_hi": lst(pywt.Wavelet(name).rec_hi)})
out["wavelet_x"] = lst(wx)
out["wavelets"] = wav


def sure(x):
    m = x.size
    universal = np.sqrt(2 * np.log(m))
    if ((x ** 2).sum() - m) / m <= np.log2(m) ** 1.5 / np.sqrt(m):
        return universal
    sq = np.sort(x ** 2)
    k = np.arange(1, m + 1)
    risk = m - 2 * k + np.cumsum(sq) + (m - k) * sq
    return min(np.sqrt(sq[np.argmin(risk)]), universal)


den = []
blocks = np.repeat([0.0, 4.0, -2.0, 1.0, 3.0, 0.0, -1.0, 2.0], 64)
noisy = blocks + rng.normal(scale=0.6, size=blocks.size)
for name, rule, kind, noise in [("haar", "universal", "soft", "finest"), ("db4", "universal", "hard", "finest"),
                                ("sym8", "sure", "soft", "finest"), ("coif2", "sure", "hard", "each"), ("db2", "universal", "soft", "each")]:
    dec = pywt.wavedec(noisy, name, mode="symmetric")
    finest = np.median(np.abs(dec[-1])) / 0.6745
    thr = []
    new = [dec[0]]
    for d in dec[1:]:
        s = finest if noise == "finest" else np.median(np.abs(d)) / 0.6745
        lam = s * np.sqrt(2 * np.log(noisy.size)) if rule == "universal" else s * sure(d / s)
        thr.append(float(lam))
        new.append(pywt.threshold(d, lam, mode=kind))
    den.append({"name": name, "rule": rule, "kind": kind, "noise": noise, "thresholds": thr, "signal": lst(pywt.waverec(new, name, mode="symmetric"))})
out["denoise_x"] = lst(noisy)
out["denoise"] = den

cwts = []
cx = signal[:180]
scales = [1.0, 1.5, 2.0, 3.7, 5.0, 8.0, 12.5, 20.0]
for name, label in [("morl", "morlet"), ("mexh", "mexican_hat")]:
    coef, freqs = pywt.cwt(cx, scales, name, sampling_period=1.0 / fs)
    cwts.append({"wavelet": label, "coef": [lst(r) for r in coef]})
out["cwt"] = {"x": lst(cx), "scales": scales, "cases": cwts}

with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "reference_cases.json"), "w") as fh:
    json.dump(out, fh, separators=(",", ":"))
