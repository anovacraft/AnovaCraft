"""statsmodels (0.15) reference values for the augmented Dickey-Fuller test.

Reads the series from unit_root_reference.json (written by
unit_root_reference.R), runs statsmodels' adfuller as a black box and adds
its statistics, chosen lags, MacKinnon approximate p-values and critical
values under the key "statsmodels"; also tabulates mackinnonp and
mackinnoncrit directly.  Run from this directory after the R script:

    python3 -I unit_root_reference.py
"""

import json
import math

from statsmodels.tsa.adfvalues import mackinnoncrit, mackinnonp
from statsmodels.tsa.stattools import adfuller

PATH = "unit_root_reference.json"
with open(PATH) as f:
    data = json.load(f)

cases = []
for name, y in data["series"].items():
    for regression in ["n", "c", "ct"]:
        for autolag, maxlag in [("AIC", 8), ("BIC", 8), (None, 3), ("AIC", None)]:
            res = adfuller(y, maxlag=maxlag, regression=regression, autolag=autolag, result_object=False)
            stat, p, usedlag, nobs, crit = res[0], res[1], res[2], res[3], res[4]
            cases.append(
                {
                    "series": name,
                    "regression": regression,
                    "autolag": autolag or "fixed",
                    "maxlag": maxlag if maxlag is not None else -1,
                    "statistic": stat,
                    "p_value": p,
                    "lag": usedlag,
                    "nobs": nobs,
                    "crit": [crit["1%"], crit["5%"], crit["10%"]],
                }
            )

grid = []
for regression in ["n", "c", "ct"]:
    for tau in [-20.0, -10.0, -6.0, -4.5, -3.3, -2.9, -2.5, -2.0, -1.62, -1.5, -1.0, -0.5, 0.0, 0.5, 0.71, 1.5, 2.5, 3.0]:
        grid.append({"regression": regression, "tau": tau, "p_value": float(mackinnonp(tau, regression=regression, N=1))})

crit = []
for regression in ["n", "c", "ct"]:
    for nobs in [25, 50, 100, 500, math.inf]:
        c = mackinnoncrit(N=1, regression=regression, nobs=nobs)
        crit.append({"regression": regression, "nobs": -1 if nobs == math.inf else nobs, "crit": [float(v) for v in c]})

data["statsmodels"] = {"adf": cases, "mackinnonp": grid, "mackinnoncrit": crit}
with open(PATH, "w") as f:
    json.dump(data, f, indent=1)
