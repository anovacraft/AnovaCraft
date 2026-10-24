"""Robust STL reference values from statsmodels (used as black-box software).

R's stl() is the reference for non-robust STL (decompose_reference.R). For
robust STL its robustness scale 6 * median|R| is, in some iterations, not the
median of the remainders (on co2 with s.window = 13 the 4th iteration uses the
246th of 468 ordered values), so after the first iterations its results drift
from the published algorithm. statsmodels' STL computes the median exactly and
serves as the reference for the robust cases; R is still compared for the
first two robustness iterations, where its median is exact.

Run from this directory:  python3 stl_robust_reference.py
which reads decompose_reference.json (for the series) and writes
stl_robust_reference.json.
"""

import json
import math

import numpy as np
from statsmodels.tsa.seasonal import STL


def next_odd(x):
    return x + 1 if x % 2 == 0 else x


def case(name, y, period, s_window, robust, t_window=None, outer=None, inner=None):
    n = len(y)
    # Periodic: statsmodels has no periodic option, so it is given the
    # definition used by the crate (and observable in R's stl output: span
    # 10 n + 1, degree 0, then position means of the seasonal).
    span = s_window if s_window is not None else 10 * n + 1
    shrink = 0.0 if s_window is None else 1.5 / s_window
    trend = t_window if t_window is not None else next_odd(math.ceil(1.5 * period / (1 - shrink)))
    low = next_odd(period)
    inner = inner if inner is not None else (1 if robust else 2)
    outer = outer if outer is not None else (15 if robust else 0)
    fit = STL(
        np.asarray(y), period=period, seasonal=span, trend=trend, low_pass=low,
        seasonal_deg=0, trend_deg=1, low_pass_deg=1, robust=robust,
        seasonal_jump=math.ceil(span / 10), trend_jump=math.ceil(trend / 10),
        low_pass_jump=math.ceil(low / 10),
    ).fit(inner_iter=inner, outer_iter=outer)
    seasonal = np.array(fit.seasonal)
    if s_window is None:
        # Periodic: every cycle made identical (position means), as R's stl.
        for j in range(period):
            seasonal[j::period] = seasonal[j::period].mean()
    remainder = np.asarray(y) - seasonal - fit.trend
    return {
        "series": name, "period": period, "s_window": s_window, "t_window": t_window,
        "robust": robust, "inner": inner, "outer": outer,
        "seasonal": seasonal.tolist(), "trend": fit.trend.tolist(),
        "remainder": remainder.tolist(), "weights": fit.weights.tolist(),
    }


def main():
    with open("decompose_reference.json") as f:
        series = json.load(f)["series"]
    co2 = series["co2"]
    ukgas = [math.log(v) for v in series["UKgas"]]
    air = [math.log(v) for v in series["AirPassengers"]]
    nottem = series["nottem"]
    cases = [
        case("co2", co2, 12, 13, True),
        case("UKgas_log", ukgas, 4, None, True),
        case("AirPassengers_log", air, 12, None, True),
        case("AirPassengers_log", air, 12, 11, True, t_window=15),
        case("nottem", nottem, 12, 7, True, inner=2, outer=5),
    ]
    with open("stl_robust_reference.json", "w") as f:
        json.dump({"stl": cases}, f)


if __name__ == "__main__":
    main()
