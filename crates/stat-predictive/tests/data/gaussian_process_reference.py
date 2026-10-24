"""Reference fits for `tests/gaussian_process_reference.rs`.

Gaussian-process regression in scikit-learn 1.9.1 (`GaussianProcessRegressor`,
`normalize_y=True`, `alpha=1e-10`) with the kernel
    ConstantKernel * RBF or Matern(nu=2.5) [one length scale per input] + WhiteKernel
(or no WhiteKernel and a fixed nugget passed as `alpha`). Writes, per case:
the training data, the optimum hyperparameters (log scale, in the order
signal variance, length scales, noise variance), the log marginal likelihood
there, the likelihood and its gradient at a fixed trial point, and the
predicted mean and standard deviation at test points.

Run: python3 gaussian_process_reference.py   (from this directory)
"""

import numpy as np
from sklearn.gaussian_process import GaussianProcessRegressor
from sklearn.gaussian_process.kernels import RBF, ConstantKernel, Matern, WhiteKernel

rng = np.random.default_rng(20261019)


def case(name, kernel_name, x, y, nugget=None):
    d = x.shape[1]
    if kernel_name == "se":
        base = RBF([0.5] * d, (1e-3, 1e3))
    else:
        base = Matern([0.5] * d, (1e-3, 1e3), nu=2.5)
    kernel = ConstantKernel(1.0, (1e-3, 1e3)) * base
    alpha = 1e-10
    if nugget is None:
        kernel = kernel + WhiteKernel(0.01, (1e-8, 10.0))
    else:
        alpha = nugget + 1e-10
    gp = GaussianProcessRegressor(kernel, alpha=alpha, normalize_y=True, n_restarts_optimizer=30, random_state=0)
    gp.fit(x, y)
    theta = gp.kernel_.theta
    trial = np.full_like(theta, 0.0)
    trial[0] = np.log(1.5)
    trial[1 : 1 + d] = np.log(np.linspace(0.3, 0.8, d))
    if nugget is None:
        trial[-1] = np.log(0.05)
    lml, grad = gp.log_marginal_likelihood(trial, eval_gradient=True)
    xt = rng.uniform(0, 1, size=(6, d))
    mean, sd = gp.predict(xt, return_std=True)
    with open(f"gp_{name}.txt", "w") as f:
        f.write(f"kernel {kernel_name}\n")
        f.write(f"nugget {'estimated' if nugget is None else repr(nugget)}\n")
        for xi, yi in zip(x, y):
            f.write("x " + " ".join(repr(float(v)) for v in xi) + " " + repr(float(yi)) + "\n")
        f.write("theta " + " ".join(repr(float(v)) for v in theta) + "\n")
        f.write(f"lml {float(gp.log_marginal_likelihood_value_)!r}\n")
        f.write("trial " + " ".join(repr(float(v)) for v in trial) + "\n")
        f.write(f"trial_lml {float(lml)!r}\n")
        f.write("trial_gradient " + " ".join(repr(float(v)) for v in grad) + "\n")
        for xi, m, s in zip(xt, mean, sd):
            f.write("predict " + " ".join(repr(float(v)) for v in xi) + f" {float(m)!r} {float(s)!r}\n")


# Two inputs, a smooth surface with noise.
x2 = rng.uniform(0, 1, size=(25, 2))
y2 = np.sin(5 * x2[:, 0]) + 2 * x2[:, 1] ** 2 + 0.05 * rng.normal(size=25)
case("se2", "se", x2, 10 + 3 * y2)
case("matern2", "matern", x2, 10 + 3 * y2)
# Three inputs, one of them irrelevant (its length scale grows large).
x3 = rng.uniform(0, 1, size=(30, 3))
y3 = np.cos(3 * x3[:, 0]) * np.exp(x3[:, 1]) + 0.02 * rng.normal(size=30)
case("matern3", "matern", x3, y3)
# A deterministic function with a small fixed nugget (interpolation).
x1 = np.linspace(0, 1, 9).reshape(-1, 1)
y1 = np.sin(6 * x1[:, 0]) + x1[:, 0]
case("se1_fixed", "se", x1, y1, nugget=1e-6)
