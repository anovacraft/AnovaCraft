# Reference values for crates/stat-core/tests/large_parameters.rs, by direct
# numerical quadrature of each density (mpmath, 40 significant digits),
# independent of the series, continued fractions and expansions under test.
#
# Columns: kind, three parameters, reference value, and the relative condition
# number kappa of the value with respect to the beta argument the code forms
# by rounding (x = 1 - p for binomial cdfs; the better of d2/(d2 + d1 f) and
# d1 f/(d2 + d1 f) for F tails; 0 where the argument is used exactly). The test allows max(1e-12, 4 kappa eps).
#
# Regenerate with: python3 large_parameters.py  (needs mpmath; about 20 s)
import mpmath as mp
mp.mp.dps = 40
rows = []
def add(kind, a, b, c, val, kappa=0): rows.append((kind, a, b, c, val, kappa))

def integrate(logf, lo, hi, mode, sd, span=60):
    pts = [mode + j * sd for j in range(-span, span + 1)]
    pts = [p for p in pts if lo < p < hi]
    a = lo if lo != -mp.inf else mode - span * sd
    pts = [a] + pts
    total = mp.mpf(0)
    for u, v in zip(pts, pts[1:]):
        total += mp.quad(lambda t: mp.exp(logf(t)), [u, v])
    end = hi if hi != mp.inf else mode + span * sd
    if end > pts[-1]:
        total += mp.quad(lambda t: mp.exp(logf(t)), [pts[-1], end])
    if hi == mp.inf:
        total += mp.quad(lambda t: mp.exp(logf(t)), [max(end, pts[-1]), mp.inf])
    return total

def gamma_logpdf(a):
    c = mp.loggamma(a)
    return lambda t: (a - 1) * mp.log(t) - t - c

def beta_logpdf(a, b):
    c = mp.loggamma(a) + mp.loggamma(b) - mp.loggamma(a + b)
    return lambda t: (a - 1) * mp.log(t) + (b - 1) * mp.log1p(-t) - c

def t_logpdf(v):
    c = mp.loggamma((v + 1) / 2) - mp.loggamma(v / 2) - mp.log(v * mp.pi) / 2
    return lambda t: c - (v + 1) / 2 * mp.log1p(t * t / v)

def f_logpdf(d1, d2):
    c = (d1 / 2) * mp.log(d1 / d2) - (mp.loggamma(d1 / 2) + mp.loggamma(d2 / 2) - mp.loggamma((d1 + d2) / 2))
    return lambda x: c + (d1 / 2 - 1) * mp.log(x) - (d1 + d2) / 2 * mp.log1p(d1 * x / d2)

for k in [1e4, 1e5, 1e6, 1e7, 1e8]:
    for z in [-3, 0, 1, 4]:
        x = float(k + z * (2 * k) ** 0.5)
        h, xx = mp.mpf(k) / 2, mp.mpf(x) / 2
        f = gamma_logpdf(h)
        sd = mp.sqrt(h)
        add("chi2_sf", k, x, 0, integrate(f, xx, mp.inf, h - 1, sd))
        add("chi2_cdf", k, x, 0, integrate(f, mp.mpf(0), xx, h - 1, sd))
        add("chi2_pdf", k, x, 0, mp.exp(f(xx)) / 2)
for v in [1e4, 1e6, 1e8, 1e10]:
    for t in [0.5, 2.0, 5.0]:
        V, T = mp.mpf(v), mp.mpf(t)
        f = t_logpdf(V)
        add("t_p2", v, t, 0, 2 * integrate(f, T, mp.inf, mp.mpf(0), mp.mpf(1)))
        add("t_pdf", v, t, 0, mp.exp(f(T)))
for d1, d2 in [(5, 1e6), (1e5, 1e5), (1e6, 1e7), (3, 1e9), (1e7, 20)]:
    spread = (2 / d1 + 2 / d2) ** 0.5
    for z in [-2, 0, 1, 4]:
        x = float(mp.exp(z * spread))
        D1, D2, X = mp.mpf(d1), mp.mpf(d2), mp.mpf(x)
        f = f_logpdf(D1, D2)
        mode = max(mp.mpf(0), (D1 - 2) / D1 * D2 / (D2 + 2))
        sd = mp.sqrt(2 / D1 + 2 / D2) * max(mode, mp.mpf("0.05"))
        sf = integrate(f, X, mp.inf, mode, sd)
        # kappa w.r.t. w = d2/(d2 + d1 x) is x f(x) / F / (1 - w), and w.r.t.
        # 1 - w it is x f(x) / F / w; the code can use either, so take the smaller.
        w = D2 / (D2 + D1 * X)
        add("f_sf", d1, d2, x, sf, X * mp.exp(f(X)) / sf / max(w, 1 - w))
        add("f_pdf", d1, d2, x, mp.exp(f(X)))
for m in [1e5, 1e7, 1e9]:
    for z in [-3, 0, 2]:
        k = int(m + z * m ** 0.5)
        f = gamma_logpdf(mp.mpf(k + 1))
        add("pois_cdf", k, m, 0, integrate(f, mp.mpf(m), mp.inf, mp.mpf(k), mp.sqrt(k + 1)))
        add("pois_pmf", k, m, 0, mp.exp(f(mp.mpf(m))))
for n, p in [(10**6, 0.3), (10**8, 0.5), (10**9, 0.01)]:
    mu, s = n * p, (n * p * (1 - p)) ** 0.5
    for z in [-2, 0, 3]:
        k = int(mu + z * s)
        P = mp.mpf(p)
        a, b = mp.mpf(n - k), mp.mpf(k + 1)
        f = beta_logpdf(a, b)
        sd = mp.sqrt(a * b / ((a + b) ** 2 * (a + b + 1)))
        cdf = integrate(f, mp.mpf(0), 1 - P, (a - 1) / (a + b - 2), sd)
        add("binom_cdf", k, n, p, cdf, (1 - P) * mp.exp(f(1 - P)) / cdf)
        lc = mp.loggamma(n + 1) - mp.loggamma(k + 1) - mp.loggamma(n - k + 1)
        add("binom_pmf", k, n, p, mp.exp(lc + k * mp.log(P) + (n - k) * mp.log1p(-P)))
with open("ref.csv", "w") as out:
    for r in rows:
        kappa = mp.nstr(r[5], 3, min_fixed=1, max_fixed=0) if r[5] else "0"
        out.write(f"{r[0]},{r[1]!r},{r[2]!r},{r[3]!r},{mp.nstr(r[4], 20, min_fixed=1, max_fixed=0)},{kappa}\n")
print(len(rows))
