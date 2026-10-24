"""Independent reference fits for stat-mixed tests.

LMM: dense marginal likelihood (V = Z G Z' + s2 I), optimised over log-Cholesky
parameters. GLMM: Laplace with a dense Newton mode search, and near-exact
adaptive Gauss-Hermite (60 nodes) for a single random intercept.
"""
import json, math
import numpy as np
from scipy import optimize, special

rng = np.random.default_rng(20261012)

# ---------------------------------------------------------------- data ---
def sleep_like():
    subj, day = np.meshgrid(np.arange(14), np.arange(10), indexing='ij')
    subj, day = subj.ravel(), day.ravel().astype(float)
    G = np.array([[600.0, 15.0], [15.0, 35.0]])
    b = rng.multivariate_normal([0, 0], G, size=14)
    y = 250 + 10 * day + b[subj, 0] + b[subj, 1] * day + rng.normal(0, 25, size=subj.size)
    keep = rng.random(subj.size) > 0.12
    return dict(y=y[keep], x=[[1.0, d] for d in day[keep]], xn=['Intercept', 'Days'],
                terms=[dict(group='Subject', codes=subj[keep].tolist(), g=14, z=[[1.0, d] for d in day[keep]],
                            effects=['Intercept', 'Days'], correlated=True)])

def crossed():
    s, it = np.meshgrid(np.arange(12), np.arange(8), indexing='ij')
    s, it = s.ravel(), it.ravel()
    keep = rng.random(s.size) > 0.15
    s, it = s[keep], it[keep]
    x = rng.normal(0, 1, size=s.size)
    f = (rng.random(s.size) > 0.5).astype(float)
    y = 5 + 1.5 * x - 2 * f + rng.normal(0, 1.2, 12)[s] + rng.normal(0, 0.8, 8)[it] + rng.normal(0, 1, s.size)
    one = [[1.0]] * s.size
    return dict(y=y, x=[[1.0, a, b] for a, b in zip(x, f)], xn=['Intercept', 'x', 'f'],
                terms=[dict(group='S', codes=s.tolist(), g=12, z=one, effects=['Intercept'], correlated=True),
                       dict(group='I', codes=it.tolist(), g=8, z=one, effects=['Intercept'], correlated=True)])

def binomial_trials():
    herd, period = np.meshgrid(np.arange(15), np.arange(4), indexing='ij')
    herd, period = herd.ravel(), period.ravel()
    size = rng.integers(2, 25, herd.size).astype(float)
    eta = -1.4 + np.array([0, -1.0, -1.1, -1.6])[period] + rng.normal(0, 0.7, 15)[herd]
    inc = rng.binomial(size.astype(int), 1 / (1 + np.exp(-eta)))
    X = [[1.0, float(p == 1), float(p == 2), float(p == 3)] for p in period]
    return dict(y=(inc / size), w=size, x=X, xn=['Intercept', 'P2', 'P3', 'P4'],
                terms=[dict(group='Herd', codes=herd.tolist(), g=15, z=[[1.0]] * herd.size, effects=['Intercept'], correlated=True)])

def count_like(family):
    g = 20
    grp = np.repeat(np.arange(g), 6)
    x = rng.normal(0, 1, grp.size)
    eta = 1.0 + 0.4 * x + rng.normal(0, 0.5, g)[grp]
    mu = np.exp(eta)
    if family == 'poisson':
        y = rng.poisson(mu).astype(float)
    elif family == 'negative_binomial':
        k = 2.5
        y = rng.negative_binomial(k, k / (k + mu)).astype(float)
    else:  # gamma, dispersion 0.3
        shape = 1 / 0.3
        y = rng.gamma(shape, mu / shape)
    return dict(y=y, x=[[1.0, a] for a in x], xn=['Intercept', 'x'],
                terms=[dict(group='G', codes=grp.tolist(), g=g, z=[[1.0]] * grp.size, effects=['Intercept'], correlated=True)])

def binary_crossed():
    s, it = np.meshgrid(np.arange(16), np.arange(10), indexing='ij')
    s, it = s.ravel(), it.ravel()
    x = rng.normal(0, 1, s.size)
    eta = -0.3 + 0.8 * x + rng.normal(0, 1.0, 16)[s] + rng.normal(0, 0.6, 10)[it]
    y = (rng.random(s.size) < 1 / (1 + np.exp(-eta))).astype(float)
    one = [[1.0]] * s.size
    return dict(y=y, x=[[1.0, a] for a in x], xn=['Intercept', 'x'],
                terms=[dict(group='S', codes=s.tolist(), g=16, z=one, effects=['Intercept'], correlated=True),
                       dict(group='I', codes=it.tolist(), g=10, z=one, effects=['Intercept'], correlated=True)])

def poisson_slope():
    g = 12
    grp = np.repeat(np.arange(g), 10)
    t = np.tile(np.linspace(0, 1, 10), g)
    b = rng.multivariate_normal([0, 0], [[0.3, 0.05], [0.05, 0.2]], size=g)
    y = rng.poisson(np.exp(1.2 + 0.7 * t + b[grp, 0] + b[grp, 1] * t)).astype(float)
    return dict(y=y, x=[[1.0, a] for a in t], xn=['Intercept', 't'],
                terms=[dict(group='G', codes=grp.tolist(), g=g, z=[[1.0, a] for a in t], effects=['Intercept', 't'], correlated=True)])

# ----------------------------------------------------------- helpers ---
def Zmat(d):
    n = len(d['y'])
    blocks = []
    for t in d['terms']:
        q = len(t['effects'])
        Z = np.zeros((n, t['g'] * q))
        z = np.array(t['z'])
        for i, c in enumerate(t['codes']):
            Z[i, c * q:(c + 1) * q] = z[i]
        blocks.append((Z, q, t['g']))
    return blocks

def chol_from(params, q):
    L = np.zeros((q, q)); k = 0
    for j in range(q):
        for i in range(j, q):
            L[i, j] = math.exp(params[k]) if i == j else params[k]; k += 1
    return L

def n_chol(q):
    return q * (q + 1) // 2

def G_of(blocks, params):
    mats, k = [], 0
    for Z, q, g in blocks:
        L = chol_from(params[k:k + n_chol(q)], q); k += n_chol(q)
        mats.append(np.kron(np.eye(g), L @ L.T))
    from scipy.linalg import block_diag
    return block_diag(*mats), k

# --------------------------------------------------------------- LMM ---
def lmm_fit(d, reml):
    y = np.array(d['y']); X = np.array(d['x']); n, p = X.shape
    blocks = Zmat(d); Z = np.hstack([b[0] for b in blocks])
    def m2l(par):
        G, k = G_of(blocks, par[:-1]); s2 = math.exp(2 * par[-1])
        V = Z @ G @ Z.T + s2 * np.eye(n)
        Vi = np.linalg.inv(V)
        XVX = X.T @ Vi @ X
        beta = np.linalg.solve(XVX, X.T @ Vi @ y)
        r = y - X @ beta
        val = np.linalg.slogdet(V)[1] + r @ Vi @ r
        if reml:
            return val + np.linalg.slogdet(XVX)[1] + (n - p) * math.log(2 * math.pi)
        return val + n * math.log(2 * math.pi)
    k0 = sum(n_chol(q) for _, q, _ in blocks)
    x0 = np.zeros(k0 + 1); x0[-1] = math.log(np.std(y))
    best = None
    for start in [x0, x0 + 0.5, x0 - 0.5]:
        r = optimize.minimize(m2l, start, method='BFGS', options=dict(gtol=1e-9, maxiter=10000))
        r = optimize.minimize(m2l, r.x, method='Nelder-Mead', options=dict(xatol=1e-10, fatol=1e-12, maxiter=40000, maxfev=40000))
        if best is None or r.fun < best.fun: best = r
    par = best.x
    G, _ = G_of(blocks, par[:-1]); s2 = math.exp(2 * par[-1])
    # Variance components (term by term, diagonal of each term's matrix) and correlations.
    comps, cors, k = [], [], 0
    for (Zb, q, g), t in zip(blocks, d['terms']):
        L = chol_from(par[k:k + n_chol(q)], q); k += n_chol(q)
        S = L @ L.T
        comps += [float(S[a, a]) for a in range(q)]
        cors += [float(S[a, b] / math.sqrt(S[a, a] * S[b, b])) for a in range(q) for b in range(a + 1, q)]
    V = Z @ G @ Z.T + s2 * np.eye(n); Vi = np.linalg.inv(V)
    C = np.linalg.inv(X.T @ Vi @ X); beta = C @ X.T @ Vi @ y
    # Satterthwaite in the variance parametrisation (ψ = unique entries of each G_k, then s2).
    def unpack(psi):
        mats, k = [], 0
        for Zb, q, g in blocks:
            S = np.zeros((q, q))
            for j in range(q):
                for i in range(j, q):
                    S[i, j] = S[j, i] = psi[k]; k += 1
            mats.append(np.kron(np.eye(g), S))
        from scipy.linalg import block_diag
        return block_diag(*mats), psi[-1]
    psi0 = []
    k = 0
    for Zb, q, g in blocks:
        L = chol_from(par[k:k + n_chol(q)], q); k += n_chol(q); S = L @ L.T
        for j in range(q):
            for i in range(j, q):
                psi0.append(S[i, j])
    psi0.append(s2); psi0 = np.array(psi0)
    def dev_psi(psi):
        G, s2 = unpack(psi)
        V = Z @ G @ Z.T + s2 * np.eye(n)
        Vi = np.linalg.inv(V); XVX = X.T @ Vi @ X
        b = np.linalg.solve(XVX, X.T @ Vi @ y); r = y - X @ b
        v = np.linalg.slogdet(V)[1] + r @ Vi @ r
        return v + (np.linalg.slogdet(XVX)[1] if reml else 0.0)
    def cov_psi(psi):
        G, s2 = unpack(psi)
        V = Z @ G @ Z.T + s2 * np.eye(n)
        return np.linalg.inv(X.T @ np.linalg.inv(V) @ X)
    m = len(psi0); H = np.zeros((m, m)); h = 1e-4 * np.maximum(np.abs(psi0), 1e-2)
    for a in range(m):
        for b in range(m):
            ea = np.zeros(m); ea[a] = h[a]; eb = np.zeros(m); eb[b] = h[b]
            H[a, b] = (dev_psi(psi0 + ea + eb) - dev_psi(psi0 + ea - eb) - dev_psi(psi0 - ea + eb) + dev_psi(psi0 - ea - eb)) / (4 * h[a] * h[b])
    A = 2 * np.linalg.inv(H)
    grads = []
    for a in range(m):
        ea = np.zeros(m); ea[a] = h[a]
        grads.append((cov_psi(psi0 + ea) - cov_psi(psi0 - ea)) / (2 * h[a]))
    dfs = []
    for j in range(p):
        l = np.zeros(p); l[j] = 1
        g = np.array([l @ G_ @ l for G_ in grads]); v = l @ C @ l
        dfs.append(float(2 * v * v / (g @ A @ g)))
    return dict(crit=float(best.fun), beta=beta.tolist(), se=np.sqrt(np.diag(C)).tolist(), comps=comps, cors=cors,
                s2=float(s2), df=dfs)

# -------------------------------------------------------------- GLMM ---
def fam_fns(family):
    if family == 'binomial':
        inv = lambda e: 1 / (1 + np.exp(-e)); mu_eta = lambda m: m * (1 - m); var = lambda m, s: m * (1 - m)
        def logf(y, m, w, s):
            sc = np.round(w * y)
            return special.gammaln(w + 1) - special.gammaln(sc + 1) - special.gammaln(w - sc + 1) + special.xlogy(sc, m) + special.xlogy(w - sc, 1 - m)
        wfac = lambda m, w, s: w
    else:
        inv = np.exp; mu_eta = lambda m: m
        if family == 'poisson':
            var = lambda m, s: m
            logf = lambda y, m, w, s: w * (special.xlogy(y, m) - m - special.gammaln(y + 1))
            wfac = lambda m, w, s: w
        elif family == 'gamma':
            var = lambda m, s: m * m
            def logf(y, m, w, s):
                a = w / s
                return a * np.log(a / m) + (a - 1) * np.log(y) - a * y / m - special.gammaln(a)
            wfac = lambda m, w, s: w / s
        else:
            var = lambda m, s: m + m * m / s
            def logf(y, m, w, s):
                k = s
                return w * (special.gammaln(y + k) - special.gammaln(k) - special.gammaln(y + 1) + k * np.log(k / (k + m)) + special.xlogy(y, m / (k + m)))
            wfac = lambda m, w, s: w
    return inv, mu_eta, var, logf, wfac

def glmm_setup(d, family):
    y = np.array(d['y']); X = np.array(d['x']); n, p = X.shape
    w = np.array(d.get('w', [1.0] * n))
    blocks = Zmat(d); Z = np.hstack([b[0] for b in blocks])
    return y, X, w, blocks, Z

def lam_of(blocks, th):
    """Λ from lme4-style θ (lower triangle by columns; diagonals may be any sign)."""
    from scipy.linalg import block_diag
    mats, k = [], 0
    for Zb, q, g in blocks:
        L = np.zeros((q, q))
        for j in range(q):
            for i in range(j, q):
                L[i, j] = th[k]; k += 1
        mats.append(np.kron(np.eye(g), L))
    return block_diag(*mats), k

def laplace_dev(d, family, th, beta, scale):
    y, X, w, blocks, Z = glmm_setup(d, family)
    inv, mu_eta, var, logf, wfac = fam_fns(family)
    Lam, _ = lam_of(blocks, th); ZL = Z @ Lam; q = ZL.shape[1]
    off = X @ beta; u = np.zeros(q)
    obj = lambda u: -2 * logf(y, inv(off + ZL @ u), w, scale).sum() + u @ u
    cur = obj(u)
    for it in range(200):
        eta = off + ZL @ u; m = inv(eta)
        W = wfac(m, w, scale) * mu_eta(m) ** 2 / var(m, scale)
        z = (eta - off) + (y - m) / mu_eta(m)
        A = ZL.T @ (W[:, None] * ZL) + np.eye(q)
        un = np.linalg.solve(A, ZL.T @ (W * z))
        t = 1.0
        while obj(u + t * (un - u)) > cur + 1e-12 and t > 1e-8: t /= 2
        u = u + t * (un - u); new = obj(u)
        if abs(cur - new) < 1e-13 * (abs(new) + 1): cur = new; break
        cur = new
    m = inv(off + ZL @ u); W = wfac(m, w, scale) * mu_eta(m) ** 2 / var(m, scale)
    A = ZL.T @ (W[:, None] * ZL) + np.eye(q)
    return float(-2 * logf(y, m, w, scale).sum() + u @ u + np.linalg.slogdet(A)[1])

def agq_dev(d, family, th, beta, scale, nq=60):
    """Near-exact: per-group adaptive GH (observed-curvature scaling), single intercept."""
    y, X, w, blocks, Z = glmm_setup(d, family)
    inv, mu_eta, var, logf, wfac = fam_fns(family)
    codes = np.array(d['terms'][0]['codes']); g = d['terms'][0]['g']
    xs, ws = np.polynomial.hermite.hermgauss(nq)
    sig = abs(th[0]); off = X @ beta; tot = 0.0
    for j in range(g):
        r = codes == j
        h = lambda u: logf(y[r], inv(off[r] + sig * u), w[r], scale).sum() - 0.5 * u * u
        res = optimize.minimize_scalar(lambda u: -h(u), bounds=(-30, 30), method='bounded', options=dict(xatol=1e-12))
        uh = res.x; e = 1e-4
        curv = -(h(uh + e) - 2 * h(uh) + h(uh - e)) / e ** 2
        s = 1 / math.sqrt(curv)
        vals = np.array([math.log(wk) + xk * xk + h(uh + math.sqrt(2) * s * xk) for xk, wk in zip(xs, ws)])
        mx = vals.max()
        tot += math.log(math.sqrt(2) * s) + mx + math.log(np.exp(vals - mx).sum()) - 0.5 * math.log(2 * math.pi)
    return -2 * tot

def glmm_fit(d, family, method):
    y, X, w, blocks, Z = glmm_setup(d, family); p = X.shape[1]
    nth = sum(n_chol(q) for _, q, _ in blocks)
    has_scale = family in ('gamma', 'negative_binomial')
    def f(par):
        th = par[:nth]; beta = par[nth:nth + p]; sc = math.exp(par[-1]) if has_scale else 1.0
        try:
            return laplace_dev(d, family, th, beta, sc) if method == 'laplace' else agq_dev(d, family, th, beta, sc)
        except Exception:
            return 1e100
    x0 = np.concatenate([[1.0 if i == j else 0.0 for (_, q, _) in blocks for j in range(q) for i in range(j, q)], np.zeros(p), [0.0] if has_scale else []])
    best = None
    for start in [x0]:
        r = optimize.minimize(f, start, method='Nelder-Mead', options=dict(xatol=1e-9, fatol=1e-11, maxiter=60000, maxfev=60000))
        r = optimize.minimize(f, r.x, method='BFGS', options=dict(gtol=1e-8))
        r = optimize.minimize(f, r.x, method='Nelder-Mead', options=dict(xatol=1e-10, fatol=1e-12, maxiter=60000, maxfev=60000))
        if best is None or r.fun < best.fun: best = r
    par = best.x; th = par[:nth]
    Lam, _ = lam_of(blocks, th)
    comps, cors, k = [], [], 0
    for Zb, q, g in blocks:
        L = np.zeros((q, q))
        for j in range(q):
            for i in range(j, q):
                L[i, j] = th[k]; k += 1
        S = L @ L.T
        comps += [float(S[a, a]) for a in range(q)]
        cors += [float(S[a, b] / math.sqrt(S[a, a] * S[b, b])) for a in range(q) for b in range(a + 1, q)]
    # SEs: inverse Hessian (×2) β block, over θ, β, (log scale).
    m = len(par); H = np.zeros((m, m)); h = 1e-4 * np.maximum(np.abs(par), 0.1)
    for a in range(m):
        for b in range(a + 1):
            ea = np.zeros(m); ea[a] = h[a]; eb = np.zeros(m); eb[b] = h[b]
            H[a, b] = H[b, a] = (f(par + ea + eb) - f(par + ea - eb) - f(par - ea + eb) + f(par - ea - eb)) / (4 * h[a] * h[b])
    cov = 2 * np.linalg.inv(H)
    se = np.sqrt(np.diag(cov)[nth:nth + p])
    out = dict(crit=float(best.fun), beta=par[nth:nth + p].tolist(), se=se.tolist(), comps=comps, cors=cors)
    if has_scale: out['scale'] = math.exp(par[-1])
    return out

cases = {}
d = sleep_like(); cases['lmm_slopes'] = dict(data=d, family='normal', reml=lmm_fit(d, True), ml=lmm_fit(d, False))
d = crossed(); cases['lmm_crossed'] = dict(data=d, family='normal', reml=lmm_fit(d, True), ml=lmm_fit(d, False))
d = binomial_trials(); cases['binomial_trials'] = dict(data=d, family='binomial', laplace=glmm_fit(d, 'binomial', 'laplace'), agq=glmm_fit(d, 'binomial', 'agq'))
for fam in ['poisson', 'gamma', 'negative_binomial']:
    d = count_like(fam); cases[fam] = dict(data=d, family=fam, laplace=glmm_fit(d, fam, 'laplace'), agq=glmm_fit(d, fam, 'agq'))
d = binary_crossed(); cases['binary_crossed'] = dict(data=d, family='binomial', laplace=glmm_fit(d, 'binomial', 'laplace'))
d = poisson_slope(); cases['poisson_slope'] = dict(data=d, family='poisson', laplace=glmm_fit(d, 'poisson', 'laplace'))

def clean(o):
    if isinstance(o, dict): return {k: clean(v) for k, v in o.items()}
    if isinstance(o, (list, tuple)): return [clean(v) for v in o]
    if isinstance(o, np.ndarray): return clean(o.tolist())
    if isinstance(o, (np.floating,)): return float(o)
    if isinstance(o, (np.integer,)): return int(o)
    return o
json.dump(clean(cases), open('cases.json', 'w'))
for k, v in cases.items():
    print(k, {m: {kk: vv for kk, vv in r.items() if kk in ('crit', 'beta', 'comps', 'scale', 'df')} for m, r in v.items() if m in ('reml', 'ml', 'laplace', 'agq')})
