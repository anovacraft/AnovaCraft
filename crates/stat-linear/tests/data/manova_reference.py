"""Reference values for stat_linear::manova (statsmodels MANOVA / mv_test).

Run: python3 -I crates/stat-linear/tests/data/manova_reference.py
Prints the statistics pasted into crates/stat-linear/tests/manova_reference.rs.

1. Plastic film data (Johnson & Wichern, Applied Multivariate Statistical
   Analysis, 6th ed., Example 6.12/6.13; also R's `?summary.manova`): tear,
   gloss and opacity by extrusion rate and additive amount, 2x2 with 5
   replicates.
2. A deterministic unbalanced layout: three responses on a three-level
   factor g, a two-level factor f, their interaction, and a covariate x.
   Generated here and printed with two decimals so the Rust test uses the
   exact same numbers. Sum-to-zero coding (Type III tests).
"""
import numpy as np
import pandas as pd
from statsmodels.multivariate.manova import MANOVA

np.set_printoptions(precision=12, suppress=False)

tear = [6.5, 6.2, 5.8, 6.5, 6.5, 6.9, 7.2, 6.9, 6.1, 6.3, 6.7, 6.6, 7.2, 7.1, 6.8, 7.1, 7.0, 7.2, 7.5, 7.6]
gloss = [9.5, 9.9, 9.6, 9.6, 9.2, 9.1, 10.0, 9.9, 9.5, 9.4, 9.1, 9.3, 8.3, 8.4, 8.5, 9.2, 8.8, 9.7, 10.1, 9.2]
opacity = [4.4, 6.4, 3.0, 4.1, 0.8, 5.7, 2.0, 3.9, 1.9, 5.7, 2.8, 4.1, 3.8, 1.6, 3.4, 8.4, 5.2, 6.9, 2.7, 1.9]
rate = ["Low"] * 10 + ["High"] * 10
additive = (["Low"] * 5 + ["High"] * 5) * 2
film = pd.DataFrame(dict(tear=tear, gloss=gloss, opacity=opacity, rate=rate, additive=additive))


def show(title, res):
    print("==", title)
    for term, r in res.results.items():
        if term == "Intercept":
            continue
        st = r["stat"]
        for name in st.index:
            row = st.loc[name]
            print(f"{term:>20} {name:<24} value={float(row['Value']):.12g} F={float(row['F Value']):.12g} "
                  f"df=({float(row['Num DF']):.12g},{float(row['Den DF']):.12g}) p={float(row['Pr > F']):.12g}")


m = MANOVA.from_formula("tear + gloss + opacity ~ C(rate, Sum) * C(additive, Sum)", data=film)
show("film", m.mv_test())

rng = np.random.default_rng(20261016)
n = 31
g = np.array(["a", "b", "c"])[rng.integers(0, 3, n)]
f = np.array(["u", "v"])[rng.integers(0, 2, n)]
x = np.round(rng.normal(10, 2, n), 2)
eff_g = {"a": [0, 0, 0], "b": [1.0, -0.5, 0.3], "c": [0.4, 0.8, -0.2]}
eff_f = {"u": [0, 0, 0], "v": [0.3, 0.1, 0.5]}
rows = []
for i in range(n):
    mu = np.array(eff_g[g[i]]) + np.array(eff_f[f[i]]) + 0.2 * (x[i] - 10) * np.array([1, 0.5, -0.4])
    e = rng.multivariate_normal([0, 0, 0], [[1, 0.5, 0.2], [0.5, 1, 0.3], [0.2, 0.3, 1]])
    rows.append(np.round(mu + e, 2))
Y = np.array(rows)
df = pd.DataFrame(dict(y1=Y[:, 0], y2=Y[:, 1], y3=Y[:, 2], g=g, f=f, x=x))
print("== data")
print("g:", list(g))
print("f:", list(f))
print("x:", list(x))
for j in range(3):
    print(f"y{j+1}:", list(Y[:, j]))
# Patsy centres nothing; our design centres x only inside higher-order
# effects, and x enters as a main effect only here, so the tests agree.
m2 = MANOVA.from_formula("y1 + y2 + y3 ~ C(g, Sum) * C(f, Sum) + x", data=df)
res2 = m2.mv_test()
show("unbalanced", res2)

# Contrast mean(a) - mean(b) (least-squares means) under Sum coding:
# columns Intercept, g[S.a], g[S.b], f[S.u], g:f (2), x.
names = list(m2.exog_names)
print("exog:", names)
L = np.zeros((1, len(names)))
L[0, names.index("C(g, Sum)[S.a]")] = 1.0
L[0, names.index("C(g, Sum)[S.b]")] = -1.0
res3 = m2.mv_test([("a - b", L)])
show("contrast", res3)

# Box's M for the film's four cells (independent NumPy implementation).
cells = [film[(film.rate == r) & (film.additive == a)][["tear", "gloss", "opacity"]].to_numpy()
         for r in ["Low", "High"] for a in ["Low", "High"]]
p = 3
k = len(cells)
ns = np.array([len(c) for c in cells])
S = [np.cov(c, rowvar=False) for c in cells]
Sp = sum((ni - 1) * Si for ni, Si in zip(ns, S)) / (ns.sum() - k)
M = (ns.sum() - k) * np.log(np.linalg.det(Sp)) - sum((ni - 1) * np.log(np.linalg.det(Si)) for ni, Si in zip(ns, S))
c = (np.sum(1 / (ns - 1)) - 1 / (ns.sum() - k)) * (2 * p * p + 3 * p - 1) / (6 * (p + 1) * (k - 1))
from scipy import stats
chi = (1 - c) * M
dfm = p * (p + 1) * (k - 1) / 2
print(f"== boxm M={M:.12g} chi2={chi:.12g} df={dfm} p={stats.chi2.sf(chi, dfm):.12g}")

# Error SSCP of the film model.
from numpy.linalg import lstsq
X = np.column_stack([np.ones(20), np.where(film.rate == "Low", -1, 1), np.where(film.additive == "Low", -1, 1)])
X = np.column_stack([X, X[:, 1] * X[:, 2]])
Yf = film[["tear", "gloss", "opacity"]].to_numpy()
B = lstsq(X, Yf, rcond=None)[0]
R = Yf - X @ B
print("== film E", repr(R.T @ R))
