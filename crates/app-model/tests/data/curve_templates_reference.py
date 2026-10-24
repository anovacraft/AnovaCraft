"""Reference fits for the assay curve templates (app_model::curve_templates).

For every template this script simulates one data set (NumPy's
default_rng(20261018), drawn in template order), fits the template's model
independently with SciPy (scipy.optimize.least_squares, Levenberg-Marquardt,
tolerances 1e-15, started at the true values), and writes the estimates,
standard errors s*sqrt(diag((J'WJ)^-1)) with J by complex-step
differentiation, the weighted residual sum of squares and the template's
derived quantities to curve_templates_reference.json. The Rust test
tests/curve_templates_reference.rs fits the same data from the templates'
own automatic starting values and compares.

Run: python3 curve_templates_reference.py  (NumPy 2.x, SciPy 1.18).
"""

import json
import math

import numpy as np
from scipy.optimize import least_squares
from scipy.special import wrightomega

rng = np.random.default_rng(20261018)
R = 8.314462618
LN2 = math.log(2.0)
cases = {}


def rep(values, k):
    return np.repeat(np.asarray(values, dtype=float), k)


def fit(name, model, names, true, x, y, z=None, group=None, weighting="equal", constants=None,
        derived=None, extra=None):
    """Fit `model(theta, x, z, g)`; `names` per global parameter."""
    x = np.asarray(x, dtype=float)
    y = np.asarray(y, dtype=float)
    w = np.ones_like(y)
    if weighting == "inverse_y2":
        w = 1.0 / y ** 2
    elif weighting == "inverse_y":
        w = 1.0 / np.abs(y)
    sw = np.sqrt(w)
    numeric_z = z is not None and not isinstance(z[0], str)
    zr = np.asarray(z, dtype=float) if numeric_z else None

    def resid(t):
        return sw * (model(t, x, zr, group) - y)

    sol = least_squares(resid, np.asarray(true, dtype=float), method="lm", xtol=1e-15, ftol=1e-15,
                        gtol=1e-15, max_nfev=200000, x_scale="jac")
    theta = sol.x
    # Complex-step Jacobian of the weighted model.
    p = len(theta)
    J = np.zeros((len(y), p))
    for j in range(p):
        h = 1e-30 * max(abs(theta[j]), 1e-8)
        tc = theta.astype(complex)
        tc[j] += 1j * h
        J[:, j] = sw * np.imag(model(tc, x.astype(complex), None if zr is None else zr.astype(complex), group)) / h
    sse = float(np.sum(resid(theta) ** 2))
    df = len(y) - p
    s2 = sse / df
    cov = s2 * np.linalg.inv(J.T @ J)
    se = np.sqrt(np.diag(cov))
    case = {
        "x": x.tolist(),
        "y": y.tolist(),
        "weighting": weighting,
        "names": names,
        "estimates": theta.tolist(),
        "std_errors": se.tolist(),
        "sse": sse,
        "df": df,
    }
    if z is not None:
        case["z"] = [v if isinstance(v, str) else float(v) for v in z]
    if group is not None:
        case["group"] = list(group)
    if constants:
        case["constants"] = constants
    if derived:
        case["derived"] = {k: float(f(theta)) for k, f in derived.items()}
    if extra:
        case.update(extra(theta, cov))
    cases[name] = case


def noisy(mu, sd):
    return np.round(mu + rng.normal(0.0, sd, len(mu)), 4)


def relative(mu, cv):
    return np.round(mu * (1.0 + rng.normal(0.0, cv, len(mu))), 5)


p10 = lambda v: 10.0 ** v
exp = np.exp

# Standard curves -----------------------------------------------------------
xs = np.concatenate([[0.0, 0.0], rep(15.625 * 2.0 ** np.arange(8), 2)])
four = lambda t, x, z, g: t[3] + (t[0] - t[3]) / (1.0 + (x / t[2]) ** t[1])
y = relative(four([0.08, 1.2, 150.0, 2.6], xs, None, None), 0.03)
fit("standard_4pl", four, ["A", "B", "C", "D"], [0.08, 1.2, 150.0, 2.6], xs, y, weighting="inverse_y2")
five = lambda t, x, z, g: t[3] + (t[0] - t[3]) / (1.0 + (x / t[2]) ** t[1]) ** t[4]
y = relative(five([0.08, 1.2, 150.0, 2.6, 0.6], xs, None, None), 0.02)
fit("standard_5pl", five, ["A", "B", "C", "D", "G"], [0.08, 1.2, 150.0, 2.6, 0.6], xs, y, weighting="inverse_y2")

# Receptor binding ----------------------------------------------------------
conc = rep([0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0], 3)
one = lambda t, x, z, g: t[0] * x / (t[1] + x)
fit("saturation_one_site", one, ["Bmax", "Kd"], [1200.0, 1.5], conc, noisy(one([1200.0, 1.5], conc, 0, 0), 30.0))

xx = np.concatenate([conc, conc])
kind = ["Total"] * len(conc) + ["Nonspecific"] * len(conc)
ind = np.array([1.0] * len(conc) + [0.0] * len(conc))
tns = lambda t, x, z, g: t[2] * x + t[3] + ind * t[0] * x / (t[1] + x)
fit("saturation_total_nonspecific", tns, ["Bmax", "Kd", "NS", "Background"], [1200.0, 1.5, 40.0, 50.0], xx,
    noisy(tns([1200.0, 1.5, 40.0, 50.0], xx, 0, 0), 30.0), z=kind)

c2 = rep(np.geomspace(0.01, 100.0, 12), 2)
two = lambda t, x, z, g: t[0] * x / (t[1] + x) + t[2] * x / (t[3] + x)
fit("saturation_two_sites", two, ["Bmax1", "Kd1", "Bmax2", "Kd2"], [400.0, 0.1, 1000.0, 10.0], c2,
    noisy(two([400.0, 0.1, 1000.0, 10.0], c2, 0, 0), 15.0))

lx = rep(np.arange(-11.0, -4.99, 0.5), 2)
comp = lambda t, x, z, g: t[1] + (t[0] - t[1]) / (1.0 + 10.0 ** (x - t[2]))
L, KD = 1e-9, 5e-10
fit("competition_one_site", comp, ["Top", "Bottom", "logIC50"], [2000.0, 150.0, -8.0], lx,
    noisy(comp([2000.0, 150.0, -8.0], lx, 0, 0), 50.0), constants={"ligand": L, "ligand_kd": KD},
    derived={"Ki (M)": lambda t: p10(t[2]) / (1.0 + L / KD), "pKi": lambda t: -math.log10(p10(t[2]) / (1.0 + L / KD))})

lx2 = rep(np.arange(-12.0, -3.99, 0.5), 2)
comp2 = lambda t, x, z, g: t[1] + (t[0] - t[1]) * (t[2] / (1.0 + 10.0 ** (x - t[3])) + (1.0 - t[2]) / (1.0 + 10.0 ** (x - t[4])))
fit("competition_two_sites", comp2, ["Top", "Bottom", "F", "logIC50_1", "logIC50_2"],
    [2000.0, 100.0, 0.4, -9.5, -6.5], lx2, noisy(comp2([2000.0, 100.0, 0.4, -9.5, -6.5], lx2, 0, 0), 30.0),
    constants={"ligand": L, "ligand_kd": KD},
    derived={"Ki, high affinity (M)": lambda t: p10(t[3]) / (1.0 + L / KD)})

lh = rep(np.arange(-11.0, -5.99, 0.5), 2)
LH = 1e-9
homo = lambda t, x, z, g: t[0] * LH / (LH + 10.0 ** x + 10.0 ** t[1]) + t[2]
fit("homologous_competition", homo, ["Bmax", "logKd", "NS"], [3000.0, -9.0, 100.0], lh,
    noisy(homo([3000.0, -9.0, 100.0], lh, 0, 0), 30.0), constants={"ligand": LH},
    derived={"Kd (M)": lambda t: p10(t[1])})

ta = rep(np.arange(0.0, 60.1, 5.0), 2)
assoc = lambda t, x, z, g: t[0] * (1.0 - exp(-t[1] * x))
fit("association", assoc, ["Ymax", "kob"], [1500.0, 0.08], ta, noisy(assoc([1500.0, 0.08], ta, 0, 0), 30.0),
    constants={"ligand": 2e-9, "koff": 0.02},
    derived={"kon = (kob − koff)/[L]": lambda t: (t[1] - 0.02) / 2e-9, "Half-time ln 2/kob": lambda t: LN2 / t[1]})

ts = np.tile(np.arange(0.0, 60.1, 5.0), 3)
ls = rep([1e-9, 3e-9, 1e-8], 13)
sev = lambda t, x, z, g: t[0] * z / (z + t[2] / t[1]) * (1.0 - exp(-(t[1] * z + t[2]) * x))
fit("association_several", sev, ["Bmax", "kon", "koff"], [2000.0, 1e7, 0.02], ts,
    noisy(sev([2000.0, 1e7, 0.02], ts, ls, 0), 25.0), z=ls, derived={"Kd = koff/kon (M)": lambda t: t[2] / t[1]})

td = np.arange(0.0, 120.1, 10.0)
diss = lambda t, x, z, g: (t[0] - t[1]) * exp(-t[2] * x) + t[1]
fit("dissociation", diss, ["Y0", "NS", "koff"], [2000.0, 150.0, 0.03], td, noisy(diss([2000.0, 150.0, 0.03], td, 0, 0), 30.0),
    derived={"Half-life ln 2/koff": lambda t: LN2 / t[2]})

LK, K1, K2 = 2e-9, 1e7, 0.02
tk = np.tile(np.arange(0.0, 120.1, 10.0), 3)
ik = rep([0.0, 1e-8, 3e-8], 13)


def mahan(t, x, z, g):
    ka = K1 * LK + K2
    kb = t[1] * z + t[2]
    s = np.sqrt((ka - kb) ** 2 + 4.0 * K1 * t[1] * LK * z)
    kf = 0.5 * (ka + kb + s)
    kslow = 0.5 * (ka + kb - s)
    q = t[0] * K1 * LK / (kf - kslow)
    return q * (t[2] * (kf - kslow) / (kf * kslow) + (t[2] - kf) / kf * exp(-kf * x) - (t[2] - kslow) / kslow * exp(-kslow * x))


fit("competitor_kinetics", mahan, ["Bmax", "k3", "k4"], [2500.0, 5e6, 0.01], tk, noisy(mahan([2500.0, 5e6, 0.01], tk, ik, 0), 20.0),
    z=ik, constants={"ligand": LK, "k1": K1, "k2": K2}, derived={"Competitor Kd = k4/k3 (M)": lambda t: t[2] / t[1]})

# Enzyme kinetics -----------------------------------------------------------
s8 = rep(0.5 * 2.0 ** np.arange(8), 2)
mm = lambda t, x, z, g: t[0] * x / (t[1] + x)
fit("michaelis_menten", mm, ["Vmax", "Km"], [100.0, 4.0], s8, noisy(mm([100.0, 4.0], s8, 0, 0), 2.0),
    constants={"enzyme": 0.01}, derived={"kcat = Vmax/[E]": lambda t: t[0] / 0.01, "kcat/Km": lambda t: t[0] / 0.01 / t[1]})
s10 = rep(0.5 * 2.0 ** np.arange(10), 2)
si = lambda t, x, z, g: t[0] * x / (t[1] + x * (1.0 + x / t[2]))
fit("substrate_inhibition", si, ["Vmax", "Km", "Ksi"], [150.0, 5.0, 60.0], s10, noisy(si([150.0, 5.0, 60.0], s10, 0, 0), 2.0))
hill = lambda t, x, z, g: t[0] * x ** t[2] / (t[1] ** t[2] + x ** t[2])
fit("allosteric_hill", hill, ["Vmax", "Khalf", "h"], [100.0, 6.0, 2.2], s8, noisy(hill([100.0, 6.0, 2.2], s8, 0, 0), 2.0))

S = np.tile([0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0], 4)
I = rep([0.0, 1.0, 3.0, 10.0], 7)
models = {
    "competitive_inhibition": (lambda t, x, z, g: t[0] * x / (t[1] * (1.0 + z / t[2]) + x), [100.0, 4.0, 2.0]),
    "uncompetitive_inhibition": (lambda t, x, z, g: t[0] * x / (t[1] + x * (1.0 + z / t[2])), [100.0, 4.0, 3.0]),
    "noncompetitive_inhibition": (lambda t, x, z, g: t[0] / (1.0 + z / t[2]) * x / (t[1] + x), [100.0, 4.0, 2.5]),
    "mixed_inhibition": (lambda t, x, z, g: t[0] * x / (t[1] * (1.0 + z / t[2]) + x * (1.0 + z / (t[3] * t[2]))),
                         [100.0, 4.0, 2.0, 4.0]),
}
for key, (f, true) in models.items():
    names = ["Vmax", "Km", "Ki"] + (["alpha"] if len(true) == 4 else [])
    fit(key, f, names, true, S, noisy(f(true, S, I, 0), 2.0), z=I)

ET = 10.0
ti = np.array([0.0, 1.0, 2.0, 4.0, 6.0, 8.0, 10.0, 15.0, 20.0, 30.0, 40.0, 50.0])
tight = lambda t, x, z, g: t[0] * (1.0 - ((ET + x + t[1]) - np.sqrt((ET + x + t[1]) ** 2 - 4.0 * ET * x)) / (2.0 * ET))
fit("tight_binding", tight, ["V0", "Kiapp"], [100.0, 2.0], rep(ti, 2), noisy(tight([100.0, 2.0], rep(ti, 2), 0, 0), 1.5),
    constants={"enzyme": ET, "substrate": 20.0, "km": 5.0},
    derived={"Ki = Kiapp/(1 + [S]/Km), competitive": lambda t: t[1] / (1.0 + 20.0 / 5.0)})

S0 = 50.0
tp = np.arange(0.0, 40.1, 2.5)
prog = lambda t, x, z, g: S0 - t[1] * wrightomega(np.log(S0 / t[1]) + (S0 - t[0] * x) / t[1])
fit("progress_curve", prog, ["Vmax", "Km"], [2.5, 8.0], tp, noisy(np.real(prog([2.5, 8.0], tp, 0, 0)), 0.5),
    constants={"s0": S0})

# Dose-response ---------------------------------------------------------------
ld = rep(np.arange(-10.0, -3.99, 0.5), 2)
ago = lambda t, x, z, g: t[0] + (t[1] - t[0]) / (1.0 + 10.0 ** ((t[2] - x) * t[3]))
fit("agonist", ago, ["Bottom", "Top", "logEC50", "Hill"], [5.0, 95.0, -7.2, 0.9], ld, noisy(ago([5.0, 95.0, -7.2, 0.9], ld, 0, 0), 3.0),
    constants={"f": 80.0},
    derived={"EC50 (M)": lambda t: p10(t[2]), "ECF (M), F from the settings": lambda t: p10(t[2]) * 4.0 ** (1.0 / t[3])})
inh = lambda t, x, z, g: t[1] + (t[0] - t[1]) / (1.0 + 10.0 ** ((x - t[2]) * t[3]))
fit("inhibitor", inh, ["Top", "Bottom", "logIC50", "Hill"], [100.0, 10.0, -6.5, 1.3], ld, noisy(inh([100.0, 10.0, -6.5, 1.3], ld, 0, 0), 3.0),
    constants={"f": 80.0}, derived={"IC50 (M)": lambda t: p10(t[2])})
lb = rep(np.arange(-10.0, -1.99, 0.5), 2)
bell = lambda t, x, z, g: (t[1] + (t[0] - t[1]) / (1.0 + 10.0 ** ((x - t[3]) * t[5]))
                           + (t[2] - t[1]) / (1.0 + 10.0 ** ((t[4] - x) * t[6])))
fit("bell_shaped", bell, ["Bottom1", "Plateau", "Bottom2", "logEC50_1", "logEC50_2", "nH1", "nH2"],
    [0.0, 100.0, 10.0, -8.0, -4.5, 1.0, 1.5], lb, noisy(bell([0.0, 100.0, 10.0, -8.0, -4.5, 1.0, 1.5], lb, 0, 0), 3.0))
lbi = rep(np.arange(-11.0, -2.99, 0.5), 2)
bi = lambda t, x, z, g: t[0] + (t[1] - t[0]) * (t[2] / (1.0 + 10.0 ** ((t[3] - x) * t[5])) + (1.0 - t[2]) / (1.0 + 10.0 ** ((t[4] - x) * t[6])))
fit("biphasic", bi, ["Bottom", "Top", "F", "logEC50_1", "logEC50_2", "nH1", "nH2"], [0.0, 100.0, 0.4, -9.0, -6.0, 1.0, 1.2], lbi,
    noisy(bi([0.0, 100.0, 0.4, -9.0, -6.0, 1.0, 1.2], lbi, 0, 0), 2.0))

lg = np.tile(np.arange(-10.0, -2.99, 0.5), 4)
B = rep([0.0, 1e-8, 1e-7, 1e-6], 15)
gs = lambda t, x, z, g: t[0] + (t[1] - t[0]) / (1.0 + 10.0 ** ((t[2] + np.log10(1.0 + (z * 10.0 ** t[4]) ** t[5]) - x) * t[3]))
fit("gaddum_schild", gs, ["Bottom", "Top", "logEC50", "Hill", "pA2", "SchildSlope"], [0.0, 100.0, -8.0, 1.0, 8.5, 1.0], lg,
    noisy(gs([0.0, 100.0, -8.0, 1.0, 8.5, 1.0], lg, B, 0), 3.0), z=B,
    derived={"KB = 10^−pA2 (M), valid when the Schild slope is 1": lambda t: p10(-t[4])})

lo = np.tile(rep(np.arange(-10.0, -3.99, 0.5), 2), 2)
grp = ["A"] * 26 + ["B"] * 26
gi = np.array([0] * 26 + [1] * 26)


def oper(t, x, z, g):
    # Basal, Em, n, logKA (A), logTau (A), logKA (B), logTau (B)
    ka = np.where(gi == 0, t[3], t[5])
    tau = np.where(gi == 0, t[4], t[6])
    a = 10.0 ** x
    ta = (10.0 ** tau * a) ** t[2]
    return t[0] + (t[1] - t[0]) * ta / (ta + (a + 10.0 ** ka) ** t[2])


true = [0.0, 100.0, 1.2, -6.5, 1.0, -7.0, 0.0]
fit("operational_model", oper, ["Basal", "Em", "n", "logKA (A)", "logTau (A)", "logKA (B)", "logTau (B)"], true, lo,
    noisy(oper(true, lo, 0, 0), 2.0), group=grp)

lr = np.tile(rep(np.arange(-9.0, -3.99, 0.5), 2), 2)
grp2 = ["Standard"] * 22 + ["Test"] * 22
g2 = np.array([0] * 22 + [1] * 22)
pot = lambda t, x, z, g: t[0] + (t[1] - t[0]) / (1.0 + 10.0 ** ((np.where(g2 == 0, t[2], t[3]) - x) * t[4]))


def potency_extra(t, cov):
    d = t[2] - t[3]
    se = math.sqrt(cov[2, 2] + cov[3, 3] - 2.0 * cov[2, 3])
    return {"relative_potency": 10.0 ** d, "relative_potency_se_log": se}


fit("relative_potency", pot, ["Bottom", "Top", "logEC50 (Standard)", "logEC50 (Test)", "Hill"], [2.0, 100.0, -7.0, -6.7, 1.0], lr,
    noisy(pot([2.0, 100.0, -7.0, -6.7, 1.0], lr, 0, 0), 3.0), group=grp2, extra=potency_extra)

# Growth, decay and survival ------------------------------------------------
tg = np.arange(0.0, 24.1, 1.0)
gomp = lambda t, x, z, g: t[0] * exp(-exp(t[1] * math.e / t[0] * (t[2] - x) + 1.0))
fit("modified_gompertz", gomp, ["A", "mu", "lambda"], [8.0, 0.9, 2.0], tg, noisy(gomp([8.0, 0.9, 2.0], tg, 0, 0), 0.1))


def baranyi(t, x, z, g):
    h0 = t[2] * t[3]
    a = x + np.log(exp(-t[2] * x) + exp(-h0) - exp(-t[2] * x - h0)) / t[2]
    return t[0] + t[2] * a - np.log(1.0 + (exp(t[2] * a) - 1.0) / exp(t[1] - t[0]))


fit("baranyi_roberts", baranyi, ["y0", "ymax", "mu", "lambda"], [2.3, 20.0, 1.0, 3.0], tg, noisy(baranyi([2.3, 20.0, 1.0, 3.0], tg, 0, 0), 0.15))
te = rep(np.arange(0.0, 20.1, 2.0), 2)
dec = lambda t, x, z, g: (t[0] - t[1]) * exp(-t[2] * x) + t[1]
fit("one_phase_decay", dec, ["Y0", "Plateau", "K"], [100.0, 10.0, 0.3], te, noisy(dec([100.0, 10.0, 0.3], te, 0, 0), 2.0),
    derived={"Half-life ln 2/K": lambda t: LN2 / t[2]})
t2 = rep([0.0, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 10.0, 12.0, 15.0, 20.0, 25.0, 30.0], 2)
dec2 = lambda t, x, z, g: t[0] + t[1] * exp(-t[2] * x) + t[3] * exp(-t[4] * x)
fit("two_phase_decay", dec2, ["Plateau", "SpanFast", "Kfast", "SpanSlow", "Kslow"], [5.0, 60.0, 1.2, 35.0, 0.1], t2,
    noisy(dec2([5.0, 60.0, 1.2, 35.0, 0.1], t2, 0, 0), 1.0), derived={"Percent fast (%)": lambda t: 100.0 * t[1] / (t[1] + t[3])})
asc = lambda t, x, z, g: t[0] + (t[1] - t[0]) * (1.0 - exp(-t[2] * x))
fit("one_phase_association", asc, ["Y0", "Plateau", "K"], [5.0, 100.0, 0.25], te, noisy(asc([5.0, 100.0, 0.25], te, 0, 0), 2.0))
tpd = np.arange(0.0, 20.1, 1.0)


def pdecay(t, x, z, g):
    xr = np.real(x)
    return np.where(xr < np.real(t[2]), t[0] + 0.0 * x, t[1] + (t[0] - t[1]) * exp(-t[3] * (x - t[2])))


fit("plateau_then_decay", pdecay, ["Y0", "Plateau", "X0", "K"], [100.0, 20.0, 5.3, 0.4], tpd, noisy(np.real(pdecay([100.0, 20.0, 5.3, 0.4], tpd, 0, 0)), 2.0))

dose = rep([0.0, 1.0, 2.0, 4.0, 6.0, 8.0], 3)
plated = rep([100, 200, 400, 1000, 4000, 10000], 3)
lq = lambda t, x, z, g: t[0] * exp(-(t[1] * x + t[2] * x ** 2))
colonies = rng.poisson(plated * lq([0.6, 0.25, 0.03], dose, 0, 0))
fit("linear_quadratic_survival", lq, ["PE", "alpha", "beta"], [0.6, 0.25, 0.03], dose, colonies / plated, weighting="inverse_y",
    derived={"alpha/beta (Gy)": lambda t: t[1] / t[2], "SF2, surviving fraction at 2 Gy": lambda t: math.exp(-2.0 * t[1] - 4.0 * t[2])})
vb = rep(np.arange(-80.0, 40.1, 10.0), 2)
boltz = lambda t, x, z, g: t[0] + (t[1] - t[0]) / (1.0 + exp((t[2] - x) / t[3]))
fit("boltzmann", boltz, ["Bottom", "Top", "V50", "Slope"], [0.0, 1.0, -20.0, 8.0], vb, noisy(boltz([0.0, 1.0, -20.0, 8.0], vb, 0, 0), 0.02))
tk2 = rep(np.arange(280.0, 340.1, 5.0), 2)
arr = lambda t, x, z, g: exp(t[0] - 1000.0 * t[1] / (R * x))
fit("arrhenius", arr, ["lnA", "Ea"], [25.0, 70.0], tk2, relative(arr([25.0, 70.0], tk2, 0, 0), 0.05), weighting="inverse_y2",
    constants={"t_ref": 298.15}, derived={"k at the reference temperature": lambda t: math.exp(t[0] - 1000.0 * t[1] / (R * 298.15))})

with open(__file__.replace(".py", ".json"), "w") as f:
    json.dump(cases, f, indent=1)
print(len(cases), "cases")
