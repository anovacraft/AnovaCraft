"""Reference fits for stat-discrete's ordinal and nominal logistic regression.

Run from the repository root:

    python3 crates/stat-discrete/tests/data/polytomous_reference.py

It writes polytomous_reference.json beside this script. The datasets are
simulated here (seeded); nothing is taken from a publication.

* Proportional odds: statsmodels OrderedModel(distr='logit'). Its thresholds
  are reported in its own parametrization (first cut, then log increments);
  they are converted to cut-points, with standard errors by the delta method
  on statsmodels' covariance. Standard errors are also recomputed here from
  an analytic observed information (NumPy), independently of statsmodels'
  numerical Hessian.
* Parallel-lines score test: an independent NumPy implementation of the
  general (non-proportional) cumulative logit model, P(Y <= j) =
  F(a_j - x'b_j). Its score and expected information are evaluated at the
  proportional-odds fit by central finite differences of the log-likelihood
  and of the category probabilities; S = U' I^-1 U on p(J-2) df.
* Baseline-category logits: statsmodels MNLogit (reference = code 0); a
  second reference is obtained by recoding the response.
* Likelihood-ratio tests per term: refits without that term's columns.
"""
import json, os
import numpy as np
from scipy import stats, special
from statsmodels.miscmodels.ordinal_model import OrderedModel
from statsmodels.discrete.discrete_model import MNLogit
import warnings
warnings.simplefilter('ignore')

rng = np.random.default_rng(20261016)


def ordinal_data(n, beta, cuts, violate=0.0):
    x1 = rng.normal(0, 1, n)
    x2 = (rng.random(n) < 0.45).astype(float)
    g = rng.integers(0, 3, n)  # three-level factor, reference coded
    X = np.column_stack([x1, x2, (g == 1).astype(float), (g == 2).astype(float)])
    eta = X @ beta
    J = len(cuts) + 1
    u = rng.random(n)
    y = np.zeros(n, dtype=int)
    for j, a in enumerate(cuts):
        # Non-proportional odds when violate != 0: the x1 slope varies by cut.
        cum = special.expit(a - eta - violate * j * x1)
        y += (u > cum).astype(int)
    return X, y


def ordinal_probs(theta, X, J):
    a, b = theta[:J - 1], theta[J - 1:]
    eta = X @ b if X.shape[1] else np.zeros(X.shape[0])
    cum = np.column_stack([np.zeros(len(eta))] + [special.expit(aj - eta) for aj in a] + [np.ones(len(eta))])
    return np.diff(cum, axis=1)


def ordinal_info(theta, X, y, J):
    """Analytic observed information (negative Hessian) of the PO model."""
    a, b = theta[:J - 1], theta[J - 1:]
    q = len(theta)
    H = np.zeros((q, q))
    eta = X @ b if X.shape[1] else np.zeros(X.shape[0])
    for i in range(len(y)):
        k = y[i]
        u = a[k] - eta[i] if k < J - 1 else np.inf
        l = a[k - 1] - eta[i] if k > 0 else -np.inf
        Fu = 1.0 if np.isinf(u) else special.expit(u)
        Fl = 0.0 if np.isinf(l) else special.expit(l)
        p = Fu - Fl
        fu = Fu * (1 - Fu); fl = Fl * (1 - Fl)
        f1u = fu * (1 - 2 * Fu); f1l = fl * (1 - 2 * Fl)
        du = np.zeros(q); dl = np.zeros(q)
        if k < J - 1: du[k] = 1
        if k > 0: dl[k - 1] = 1
        du[J - 1:] = -X[i]; dl[J - 1:] = -X[i]
        huu = f1u / p - (fu / p) ** 2
        hll = -f1l / p - (fl / p) ** 2
        hul = fu * fl / p ** 2
        H += huu * np.outer(du, du) + hll * np.outer(dl, dl) + hul * (np.outer(du, dl) + np.outer(dl, du))
    return -H


def ordinal_loglik(theta, X, y, J):
    P = ordinal_probs(theta, X, J)
    return float(np.sum(np.log(P[np.arange(len(y)), y])))


def fit_ordered(X, y, J, names):
    if X.shape[1] == 0:
        counts = np.bincount(y, minlength=J)
        cum = np.cumsum(counts)[:-1] / len(y)
        thr = np.log(cum / (1 - cum))
        return dict(thresholds=thr.tolist(), slopes=[], llf=ordinal_loglik(thr, X, y, J))
    m = OrderedModel(y, X, distr='logit')
    r = m.fit(method='bfgs', maxiter=20000, gtol=1e-12, disp=False)
    r = m.fit(method='newton', start_params=r.params, maxiter=200, tol=1e-14, disp=False)
    p = np.asarray(r.params)
    k = X.shape[1]
    raw = p[k:]
    thr = m.transform_threshold_params(raw)[1:-1]
    Jac = np.zeros((J - 1, J - 1))
    Jac[:, 0] = 1
    for mcol in range(1, J - 1):
        Jac[mcol:, mcol] = np.exp(raw[mcol])
    cov = np.asarray(r.cov_params())
    cov_thr = Jac @ cov[k:, k:] @ Jac.T
    theta = np.concatenate([thr, p[:k]])
    info = ordinal_info(theta, X, y, J)
    cov_an = np.linalg.inv(info)
    return dict(
        thresholds=thr.tolist(), slopes=p[:k].tolist(),
        threshold_se_statsmodels=np.sqrt(np.diag(cov_thr)).tolist(),
        slope_se_statsmodels=np.asarray(r.bse)[:k].tolist(),
        se_analytic=np.sqrt(np.diag(cov_an)).tolist(),
        llf=float(r.llf), llf_numpy=ordinal_loglik(theta, X, y, J),
        probabilities_first_rows=ordinal_probs(theta, X[:5], J).tolist(),
    )


def general_probs(params, X, J):
    """General cumulative logit: params = a (J-1), then b_1..b_{J-1} (p each)."""
    p = X.shape[1]
    a = params[:J - 1]
    B = params[J - 1:].reshape(J - 1, p)
    cum = np.column_stack([np.zeros(X.shape[0])] + [special.expit(a[j] - X @ B[j]) for j in range(J - 1)]
                          + [np.ones(X.shape[0])])
    return np.diff(cum, axis=1)


def score_test(theta, X, y, J):
    p = X.shape[1]
    a, b = theta[:J - 1], theta[J - 1:]
    params = np.concatenate([a, np.tile(b, J - 1)])
    q = len(params)
    h = 1e-6
    ll = lambda t: np.sum(np.log(general_probs(t, X, J)[np.arange(len(y)), y]))
    U = np.zeros(q)
    dP = np.zeros((q, X.shape[0], J))
    for r in range(q):
        e = np.zeros(q); e[r] = h
        U[r] = (ll(params + e) - ll(params - e)) / (2 * h)
        dP[r] = (general_probs(params + e, X, J) - general_probs(params - e, X, J)) / (2 * h)
    P = general_probs(params, X, J)
    I = np.einsum('rik,sik,ik->rs', dP, dP, 1.0 / P)
    S = float(U @ np.linalg.solve(I, U))
    df = p * (J - 2)
    return dict(chi_square=S, df=df, p_value=float(stats.chi2.sf(S, df)))


def ordinal_case(name, n, beta, cuts, violate, drops):
    X, y = ordinal_data(n, np.array(beta), cuts, violate)
    J = len(cuts) + 1
    names = ['x1', 'x2', 'g: 1', 'g: 2']
    full = fit_ordered(X, y, J, names)
    theta = np.array(full['thresholds'] + full['slopes'])
    counts = np.bincount(y, minlength=J)
    ll0 = float(np.sum(counts * np.log(counts / n)))
    reduced = []
    for cols in drops:
        keep = [j for j in range(X.shape[1]) if j not in cols]
        reduced.append(dict(drop=cols, llf=fit_ordered(X[:, keep], y, J, [names[j] for j in keep])['llf']))
    return dict(name=name, x=X.tolist(), y=y.tolist(), levels=J, names=names, fit=full, llnull=ll0,
                score_test=score_test(theta, X, y, J), reduced=reduced)


def nominal_data(n):
    x1 = rng.normal(0, 1, n)
    x2 = rng.uniform(-1, 2, n)
    f = (rng.random(n) < 0.5).astype(float)
    X = np.column_stack([x1, x2, f])
    B = np.array([[0.0, 0.0, 0.0, 0.0], [0.3, 0.9, -0.5, 0.6], [-0.4, -0.6, 0.8, 1.1]])  # rows: classes
    eta = np.column_stack([B[c, 0] + X @ B[c, 1:] for c in range(3)])
    P = special.softmax(eta, axis=1)
    y = np.array([rng.choice(3, p=pi) for pi in P])
    return X, y


def fit_mnlogit(X, y):
    Xc = np.column_stack([np.ones(len(y)), X])
    r = MNLogit(y, Xc).fit(method='newton', maxiter=200, tol=1e-14, disp=False)
    return dict(params=np.asarray(r.params).T.tolist(), bse=np.asarray(r.bse).T.tolist(), llf=float(r.llf),
                llnull=float(r.llnull), prsquared=float(r.prsquared),
                probabilities_first_rows=np.asarray(r.predict(Xc[:5])).tolist())


def nominal_case():
    X, y = nominal_data(240)
    full = fit_mnlogit(X, y)
    # Reference category 2: recode so that 2 becomes code 0 (order 2, 0, 1).
    recode = np.array([1, 2, 0])[y]
    ref2 = fit_mnlogit(X, recode)
    reduced = []
    for cols in [[0], [1], [2]]:
        keep = [j for j in range(X.shape[1]) if j not in cols]
        reduced.append(dict(drop=cols, llf=fit_mnlogit(X[:, keep], y)['llf']))
    return dict(name='nominal', x=X.tolist(), y=y.tolist(), levels=3, names=['x1', 'x2', 'f'], fit=full,
                reference_2=ref2, reduced=reduced)


out = dict(
    ordinal=[
        ordinal_case('proportional', 180, [0.8, -0.6, 0.4, 1.0], [-1.0, 0.2, 1.5], 0.0, [[0], [1], [2, 3]]),
        ordinal_case('non_proportional', 220, [0.5, 0.7, -0.3, 0.6], [-1.2, -0.2, 0.9, 2.0], 0.9, [[0], [2, 3]]),
    ],
    nominal=nominal_case(),
)
with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'polytomous_reference.json'), 'w') as fh:
    json.dump(out, fh, indent=1)
for c in out['ordinal']:
    f = c['fit']
    print(c['name'], f['llf'], f['llf_numpy'], c['score_test'])
    print('  se sm ', f['threshold_se_statsmodels'] + f['slope_se_statsmodels'])
    print('  se an ', f['se_analytic'])
print('nominal', out['nominal']['fit']['llf'], out['nominal']['reference_2']['llf'])
