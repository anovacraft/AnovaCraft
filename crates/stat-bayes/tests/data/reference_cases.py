"""Reference values for crates/stat-bayes/tests/sampler_reference.rs.

Run with Python 3.13 and PyMC 6.3:  python3 reference_cases.py
Prints posterior means, SDs and Monte Carlo standard errors (batch
means) from long PyMC NUTS runs, and writes the fixed draws in
diagnostics_draws.csv that diagnostics_reference.R summarizes.
"""
import numpy as np, pymc as pm

def report(tr, names):
    # Monte Carlo standard error by batch means: 25 batches per chain.
    post = tr.posterior
    for n in names:
        v = post[n].values
        chains, draws = v.shape[0], v.shape[1]
        v = v.reshape(chains, draws, -1)
        for j in range(v.shape[2]):
            x = v[:, :, j]
            batches = x.reshape(chains, 25, -1).mean(axis=2).reshape(-1)
            mcse = batches.std(ddof=1) / np.sqrt(batches.size)
            print(f"{n}[{j}] mean {float(x.mean())!r} sd {float(x.std(ddof=1))!r} mcse {float(mcse)!r}")


def lcg_draws():
    # Deterministic AR(1)-like chains (no NumPy RNG, so the Rust test can read the file).
    state = 12345
    def u():
        nonlocal state
        state = (6364136223846793005 * state + 1442695040888963407) % 2**64
        return (state >> 11) / 2**53
    chains, draws, params = 4, 500, 3
    out = np.zeros((chains, draws, params))
    for c in range(chains):
        x = np.zeros(params)
        for d in range(draws):
            e = np.array([u() - 0.5 for _ in range(params)]) * np.sqrt(12)
            x = np.array([0.5, 0.9, 0.0]) * x + e
            x2 = x.copy(); x2[2] = x[2] + (0.6 if c == 3 else 0.0)  # third parameter: one chain shifted
            out[c, d] = x2
    return out

draws = lcg_draws()
with open("diagnostics_draws.csv", "w") as f:
    f.write("chain,draw,a,b,c\n")
    for c in range(draws.shape[0]):
        for d in range(draws.shape[1]):
            f.write(f"{c},{d},{float(draws[c,d,0])!r},{float(draws[c,d,1])!r},{float(draws[c,d,2])!r}\n")
print("diagnostics: see diagnostics_reference.R (R posterior package)")

y = np.array([28., 8, -3, 7, -1, 1, 18, 12]); sig = np.array([15., 10, 16, 11, 9, 11, 10, 18])
with pm.Model():
    mu = pm.Normal("mu", 0, 5); tau = pm.HalfCauchy("tau", 5)
    z = pm.Normal("z", 0, 1, shape=8)
    theta = pm.Deterministic("theta", mu + tau * z)
    pm.Normal("y", theta, sig, observed=y)
    tr = pm.sample(25000, tune=2000, chains=4, random_seed=1, target_accept=0.95, progressbar=False)
print("eight_schools")
report(tr, ["mu", "tau", "theta"])

mpg = np.array([21.0,21.0,22.8,21.4,18.7,18.1,14.3,24.4,22.8,19.2,17.8,16.4,17.3,15.2,10.4,10.4,14.7,32.4,30.4,33.9,21.5,15.5,15.2,13.3,19.2,27.3,26.0,30.4,15.8,19.7,15.0,21.4])
wt = np.array([2.62,2.875,2.32,3.215,3.44,3.46,3.57,3.19,3.15,3.44,3.44,4.07,3.73,3.78,5.25,5.424,5.345,2.2,1.615,1.835,2.465,3.52,3.435,3.84,3.845,1.935,2.14,1.513,3.17,2.77,3.57,2.78])
vs = np.array([0,0,1,1,0,1,0,1,1,1,1,0,0,0,0,0,0,1,1,1,1,0,0,0,0,1,0,1,0,0,0,1.])
with pm.Model():
    a = pm.Normal("a", 0, 2.5); b = pm.Normal("b", 0, 2.5, shape=2)
    eta = a + b[0] * (mpg - mpg.mean()) + b[1] * (wt - wt.mean())
    pm.Bernoulli("y", logit_p=eta, observed=vs)
    pm.Deterministic("intercept", a - b[0] * mpg.mean() - b[1] * wt.mean())
    tr = pm.sample(25000, tune=2000, chains=4, random_seed=2, progressbar=False)
print("logistic")
report(tr, ["intercept", "b"])

# Poisson with a random intercept per group.
counts = np.array([2,4,3,1,0,2,5,7,6,9,8,6,1,0,2,1,3,2,4,6,5,3,4,7.])
x = np.array([0.1,0.5,0.9,1.3,1.7,2.1]*4)
grp = np.repeat(np.arange(4), 6)
with pm.Model():
    a = pm.Normal("a", 1.0, 2.5); b = pm.Normal("b", 0, 2.5)
    s = pm.Exponential("s", 1.0); z = pm.Normal("z", 0, 1, shape=4)
    pm.Poisson("y", pm.math.exp(a + b * (x - x.mean()) + s * z[grp]), observed=counts)
    pm.Deterministic("intercept", a - b * x.mean())
    pm.Deterministic("effects", s * z)
    tr = pm.sample(25000, tune=2000, chains=4, random_seed=3, target_accept=0.95, progressbar=False)
print("poisson")
report(tr, ["intercept", "b", "s", "effects"])
