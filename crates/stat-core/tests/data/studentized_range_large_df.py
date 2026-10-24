"""Reference values for the studentized range at k = 3, nu = 1e6
(crates/stat-core/tests/extreme_arguments.rs).

P(Q > q) = ∫ f_S(s) P(R > q s) ds with S = sqrt(chi2_nu / nu) and
P(R > w) = 1 - k ∫ φ(z) [Φ(z) - Φ(z - w)]^(k-1) dz, both by SciPy quad.
SciPy's own stats.studentized_range treats nu this large as infinite, so it
is not used. Run: python3 studentized_range_large_df.py (about a minute).
"""
import numpy as np
from scipy import integrate, optimize, special

K = 3
NU = 1e6


def range_cdf(w):
    f = lambda z: K * np.exp(-z * z / 2) / np.sqrt(2 * np.pi) * (special.ndtr(z) - special.ndtr(z - w)) ** (K - 1)
    return integrate.quad(f, -12, 12, epsabs=1e-17, epsrel=1e-14, limit=400, points=[0, w / 2, w])[0]


LN_C = np.log(2) + NU / 2 * np.log(NU / 2) - special.gammaln(NU / 2)
SD = 1 / np.sqrt(2 * NU)


def density_s(s):
    return np.exp(LN_C + (NU - 1) * np.log(s) - NU * s * s / 2)


def sf(q):
    g = lambda u: density_s(1 + u * SD) * SD * (1.0 - range_cdf(q * (1 + u * SD)))
    return integrate.quad(g, -14, 14, epsabs=1e-24, epsrel=1e-12, limit=200)[0]


print("sf(7)", repr(sf(7.0)))
print("q(0.95)", repr(optimize.brentq(lambda q: sf(q) - 0.05, 3.3, 3.33, xtol=1e-14)))
print("q(1 - 1e-10)", repr(optimize.brentq(lambda q: np.log(sf(q)) - np.log(1e-10), 8.0, 11.0, xtol=1e-13)))
