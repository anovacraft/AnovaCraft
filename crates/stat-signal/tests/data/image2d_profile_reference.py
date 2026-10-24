"""Reference values for stat_signal::image2d::directional_profile, the radial
profiles of a centred power spectrum. Writes image2d_profile_reference.json
next to this script; tests/image2d_reference.rs reads it.

    python3 -m pip install numpy scipy
    python3 -I crates/stat-signal/tests/data/image2d_profile_reference.py

* The centred power spectrum is composed from numpy.fft (no window, mean
  removed): |fftshift(fft2(x - mean))|^2 / N^2.
* Straight cuts are read with scipy.ndimage.map_coordinates (order=1,
  bilinear) at the sample positions of the definition: steps of one column
  or one row along the direction, each read on whichever side of the centre
  lies inside the grid.
* Wedges are the mean of the cells within the half-width of the direction
  whose radial frequency rounds to each sample (halfway values to the outer
  sample), computed with NumPy masks.
"""
import json
import os

import numpy as np
import scipy
import scipy.ndimage as ndi

rng = np.random.default_rng(20261021)
rows, cols, dx, dy = 24, 40, 0.5, 2.0
r, c = np.mgrid[0:rows, 0:cols].astype(float)
image = np.cos(2 * np.pi * (5 * c / cols + 3 * r / rows)) + 0.5 * rng.normal(size=(rows, cols))
x = image - image.mean()
power = np.abs(np.fft.fftshift(np.fft.fft2(x))) ** 2 / (rows * cols) ** 2
fx = np.fft.fftshift(np.fft.fftfreq(cols, dx))
fy = np.fft.fftshift(np.fft.fftfreq(rows, dy))
sx, sy = 1 / (cols * dx), 1 / (rows * dy)
r0, c0 = rows // 2, cols // 2
FX, FY = np.meshgrid(fx, fy)
rho = np.hypot(FX, FY)
theta_cells = np.degrees(np.arctan2(-FY, FX)) % 180.0


def inside(rr, cc):
    return -1e-9 <= rr <= rows - 1 + 1e-9 and -1e-9 <= cc <= cols - 1 + 1e-9


cases = []
for direction, half in [(0.0, 0.0), (90.0, 0.0), (30.0, 0.0), (135.0, 0.0), (17.5, 0.0), (30.0, 10.0), (90.0, 4.0)]:
    t = np.radians(direction)
    step = 1.0 / max(abs(np.cos(t)) / sx, np.sin(t) / sy)
    dr, dc = -np.sin(t) / sy, np.cos(t) / sx
    freqs, coords = [], []
    k = 0
    while True:
        f = k * step
        a, b = (r0 + f * dr, c0 + f * dc), (r0 - f * dr, c0 - f * dc)
        if not (inside(*a) or inside(*b)):
            break
        freqs.append(f)
        coords.append(a if inside(*a) else b)
        k += 1
    if half == 0.0:
        rr = np.clip([p[0] for p in coords], 0, rows - 1)
        cc = np.clip([p[1] for p in coords], 0, cols - 1)
        values = ndi.map_coordinates(power, [rr, cc], order=1)
        cases.append({"direction": direction, "half_width": half, "frequency": freqs, "power": values.tolist()})
    else:
        off = (theta_cells - direction) % 180.0
        keep = (np.minimum(off, 180.0 - off) <= half + 1e-9) | (rho == 0)
        bins = np.floor(rho / step + 0.5 + 1e-9).astype(int)  # halfway values to the outer sample
        f_out, p_out, n_out = [], [], []
        for j in range(len(freqs)):
            m = keep & (bins == j)
            if m.any():
                f_out.append(j * step)
                p_out.append(float(power[m].mean()))
                n_out.append(int(m.sum()))
        cases.append({"direction": direction, "half_width": half, "frequency": f_out, "power": p_out, "count": n_out})

out = {
    "versions": {"numpy": np.__version__, "scipy": scipy.__version__},
    "image": {"rows": rows, "cols": cols, "dx": dx, "dy": dy, "data": [float("%.17g" % v) for v in image.ravel()]},
    "cases": cases,
}
with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "image2d_profile_reference.json"), "w") as fh:
    json.dump(out, fh, separators=(",", ":"))
