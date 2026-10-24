"""Independent reference values for stat-doe's factorial analysis
(`crates/stat-doe/src/factorial_analysis.rs`).

Run with `python3 factorial_reference.py`; it prints the numbers the Rust
tests compare against (tolerance 1e-10 relative unless noted).

Everything is computed from the textbook definitions with NumPy least squares
and SciPy distributions, independently of the Rust code:

* effects = 2 x least-squares coefficient of the +/-1 coded term columns;
* Lenth (1989), Technometrics 31(4): s0 = 1.5 median|c|,
  PSE = 1.5 median{|c| : |c| < 2.5 s0}, d = m/3,
  ME = t_{1-a/2; d} PSE, SME = t_{g; d} PSE with g = (1 + (1-a)^{1/m})/2;
* Daniel (1959) normal scores Phi^-1((i-0.5)/m) and half-normal scores
  Phi^-1(0.5 + 0.5 (i-0.5)/m);
* Bergman & Hynen (1997), Technometrics 39(2): the location model fitted
  separately within the runs at x_j = +1 and x_j = -1,
  D_j = (RSS+/df+)/(RSS-/df-) against F(df+, df-), two-sided p;
* Box & Meyer (1986), Technometrics 28(1): ln(sum r+^2 / sum r-^2) from the
  location model's residuals; for replicated designs, effects of ln s.

Data sets (typed in from the literature):

* Box, Hunter & Hunter, Statistics for Experimenters (2nd ed., 2005), the 2^3
  pilot plant experiment: yields 60, 72, 54, 68, 52, 83, 45, 80 in standard
  order of temperature T, concentration C and catalyst K.
* Montgomery, Design and Analysis of Experiments, Example 6.2 (filtration
  rate, unreplicated 2^4): 45, 71, 48, 65, 68, 60, 80, 65, 43, 100, 45, 104,
  75, 86, 70, 96 in standard order of A, B, C, D.
* A simulated replicated 2^3 (three replicates; standard deviation larger at
  C = +1), generated once with numpy seed 20261016 and typed in here.
"""
import itertools
import numpy as np
from scipy import stats


def standard_order(k):
    """Coded runs of a 2^k in standard order (first factor fastest)."""
    return np.array([[1.0 if (i >> j) & 1 else -1.0 for j in range(k)] for i in range(2 ** k)])


def all_terms(k, max_order=None):
    max_order = max_order or k
    out = []
    for order in range(1, max_order + 1):
        out += list(itertools.combinations(range(k), order))
    return out


def model_matrix(x, terms):
    cols = [np.ones(len(x))]
    for t in terms:
        cols.append(np.prod(x[:, list(t)], axis=1))
    return np.column_stack(cols)


def label(t, names):
    return "*".join(names[j] for j in t)


def effects(x, y, terms):
    X = model_matrix(x, terms)
    b, *_ = np.linalg.lstsq(X, y, rcond=None)
    return 2.0 * b[1:], b, X


def lenth(c, alpha=0.05):
    a = np.abs(np.asarray(c))
    m = len(a)
    s0 = 1.5 * np.median(a)
    pse = 1.5 * np.median(a[a < 2.5 * s0])
    d = m / 3.0
    gamma = (1.0 + (1.0 - alpha) ** (1.0 / m)) / 2.0
    me = stats.t.ppf(1.0 - alpha / 2.0, d) * pse
    sme = stats.t.ppf(gamma, d) * pse
    return dict(s0=s0, pse=pse, df=d, me=me, sme=sme, gamma=gamma)


def rss(X, y):
    b, *_ = np.linalg.lstsq(X, y, rcond=None)
    r = y - X @ b
    return float(r @ r), r


def bergman_hynen(x, y, terms, j):
    """Location model `terms` within each half of factor j (x_j removed from every term)."""
    out = {}
    for side, sign in (("plus", 1.0), ("minus", -1.0)):
        idx = x[:, j] == sign
        reduced = []
        for t in terms:
            r = tuple(f for f in t if f != j)
            if r and r not in reduced:
                reduced.append(r)
        X = model_matrix(x[idx], reduced)
        s, _ = rss(X, y[idx])
        rank = np.linalg.matrix_rank(X)
        out[side] = (s, idx.sum() - rank)
    (sp, dp), (sm, dm) = out["plus"], out["minus"]
    d = (sp / dp) / (sm / dm)
    upper = stats.f.sf(d, dp, dm)
    lower = stats.f.cdf(d, dp, dm)
    return dict(D=d, df=(int(dp), int(dm)), p=min(1.0, 2.0 * min(upper, lower)))


def box_meyer(x, r, j):
    return np.log(np.sum(r[x[:, j] > 0] ** 2) / np.sum(r[x[:, j] < 0] ** 2))


def show(name, v):
    if np.ndim(v) == 0:
        print(f"{name} = {float(v)!r}")
    else:
        print(f"{name} = [" + ", ".join(repr(float(u)) for u in v) + "]")


def pilot_plant():
    print("# Box, Hunter & Hunter pilot plant 2^3 (T, C, K)")
    x = standard_order(3)
    y = np.array([60, 72, 54, 68, 52, 83, 45, 80], float)
    terms = all_terms(3)
    e, _, _ = effects(x, y, terms)
    print("terms", [label(t, "TCK") for t in terms])
    show("effects", e)


def filtration():
    print("# Montgomery Example 6.2 filtration rate 2^4 (A, B, C, D)")
    names = "ABCD"
    x = standard_order(4)
    y = np.array([45, 71, 48, 65, 68, 60, 80, 65, 43, 100, 45, 104, 75, 86, 70, 96], float)
    terms = all_terms(4)
    e, _, _ = effects(x, y, terms)
    print("terms", [label(t, names) for t in terms])
    show("effects", e)
    L = lenth(e)
    for k in ("s0", "pse", "df", "me", "sme", "gamma"):
        show(k, L[k])
    active = [label(t, names) for t, v in zip(terms, e) if abs(v) > L["me"]]
    print("active (beyond ME):", active)
    print("active (beyond SME):", [label(t, names) for t, v in zip(terms, e) if abs(v) > L["sme"]])
    show("lenth_t", e / L["pse"])
    show("lenth_p", 2 * stats.t.sf(np.abs(e) / L["pse"], L["df"]))
    # Daniel plotting positions for the sorted effects.
    m = len(e)
    p = (np.arange(1, m + 1) - 0.5) / m
    show("normal_scores", stats.norm.ppf(p))
    show("half_normal_scores", stats.norm.ppf(0.5 + 0.5 * p))
    # Dispersion with the location model of the Lenth-active effects.
    loc = [t for t, v in zip(terms, e) if abs(v) > L["me"]]
    print("location model:", [label(t, names) for t in loc])
    _, r = rss(model_matrix(x, loc), y)
    for j in range(4):
        bh = bergman_hynen(x, y, loc, j)
        print(f"{names[j]}: D = {float(bh['D'])!r}, df = {bh['df']}, p = {float(bh['p'])!r}, box_meyer = {float(box_meyer(x, r, j))!r}")
    # Off-target settings: two runs missed their targets.
    xo = x.copy()
    xo[2, 0] = -0.9   # run 3, A
    xo[9, 2] = 1.1    # run 10, C
    eo, _, _ = effects(xo, y, all_terms(4, 2))
    print("off-target, model up to two-factor interactions:", [label(t, names) for t in all_terms(4, 2)])
    show("off_target_effects", eo)
    X = model_matrix(xo, all_terms(4, 2))
    s, _ = rss(X, y)
    dfe = len(y) - X.shape[1]
    cov = s / dfe * np.linalg.inv(X.T @ X)
    show("off_target_effect_se", 2 * np.sqrt(np.diag(cov))[1:])


def replicated():
    print("# Simulated replicated 2^3 (3 replicates)")
    names = "ABC"
    reps = [[48.9, 50.8, 50.0], [54.4, 55.0, 55.9], [39.3, 39.1, 39.3], [52.9, 53.2, 55.8],
            [50.5, 49.0, 47.5], [52.0, 48.2, 55.2], [38.5, 46.0, 40.1], [51.3, 51.6, 59.2]]
    base = standard_order(3)
    x = np.vstack([base[i] for i in range(8) for _ in range(3)])
    y = np.array([v for r in reps for v in r])
    terms = all_terms(3)
    e, b, X = effects(x, y, terms)
    show("effects", e)
    s, _ = rss(X, y)
    dfe = len(y) - X.shape[1]
    cov = s / dfe * np.linalg.inv(X.T @ X)
    se = 2 * np.sqrt(np.diag(cov))[1:]
    show("effect_se", se)
    show("t", e / se)
    show("p", 2 * stats.t.sf(np.abs(e / se), dfe))
    print("error df", dfe)
    lns = np.log([np.std(r, ddof=1) for r in reps])
    show("ln_s", lns)
    d, _, _ = effects(base, lns, terms)
    show("dispersion_effects", d)
    L = lenth(d)
    for k in ("pse", "me", "sme"):
        show("dispersion_" + k, L[k])


def poisson_saturated():
    print("# Saturated Poisson 2^2 (log link): coefficients = OLS of ln y")
    x = standard_order(2)
    y = np.array([4.0, 9.0, 12.0, 30.0])
    terms = all_terms(2)
    X = model_matrix(x, terms)
    b = np.linalg.solve(X, np.log(y))
    show("effects", 2 * b[1:])
    cov = np.linalg.inv(X.T @ np.diag(y) @ X)
    show("effect_se", 2 * np.sqrt(np.diag(cov))[1:])


if __name__ == "__main__":
    pilot_plant()
    filtration()
    replicated()
    poisson_saturated()
