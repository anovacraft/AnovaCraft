"""Fieller ratio-of-means reference values (SciPy) for
stat_inference::means::ratio_of_means; written from Fieller (1954) and
Tamhane & Logan (2004) independently of the Rust code."""
import numpy as np
from scipy import stats, optimize

a = np.array([12.1, 14.3, 11.8, 13.9, 15.2, 12.7, 14.8, 13.3])
b = np.array([8.2, 9.1, 7.7, 9.8, 8.9, 8.4, 9.5])

def fieller(a, b, rho0, pooled, level=0.95):
    n1, n2 = len(a), len(b)
    m1, m2 = a.mean(), b.mean()
    s1, s2 = a.var(ddof=1), b.var(ddof=1)
    if pooled:
        sp2 = ((n1-1)*s1 + (n2-1)*s2)/(n1+n2-2)
        v1, v2 = sp2/n1, sp2/n2
        df = lambda r: n1+n2-2
    else:
        v1, v2 = s1/n1, s2/n2
        df = lambda r: (v1 + r*r*v2)**2/(v1**2/(n1-1) + (r*r*v2)**2/(n2-1))
    t = (m1 - rho0*m2)/np.sqrt(v1 + rho0**2*v2)
    p = 2*stats.t.sf(abs(t), df(rho0))
    est = m1/m2
    q = stats.t.ppf(1-(1-level)/2, df(est))
    # Limits as the roots of the test statistic equal to ±q, found numerically.
    f = lambda r: (m1 - r*m2)**2 - q*q*(v1 + r*r*v2)
    lo = optimize.brentq(f, est - 10, est)
    hi = optimize.brentq(f, est, est + 10)
    return est, t, df(rho0), p, lo, hi

for pooled in (True, False):
    print("pooled" if pooled else "welch", ["%.17g" % v for v in fieller(a, b, 1.5, pooled)])
