# Reference fits for the neural network tests (crates/stat-predictive/src/neural.rs),
# computed with scikit-learn's MLPRegressor / MLPClassifier (1.9.1), which
# minimize the same objective: mean loss (half squared error, or
# cross-entropy with a logistic unit for two classes and a softmax for more)
# plus (alpha / 2n) times the sum of squared weights (biases unpenalized).
#
# Run from this directory:  python3 neural_reference.py
# It writes neural_<case>.txt, one per case: the data (rounded to six
# decimals, then refitted on the rounded values), the fitted layers, the
# objective at the fit (sklearn's loss_) and the predictions for every row.
# It also writes the gradient there from scikit-learn's back-propagation.
# The tests evaluate the same weights and check the objective, the
# gradient and the predictions.
import numpy as np
from sklearn.neural_network import MLPClassifier, MLPRegressor

rng = np.random.default_rng(20261017)


def gradient(model, x, y):
    """The objective and its gradient at the fitted weights, by scikit-learn's
    own back-propagation."""
    if isinstance(model, MLPClassifier):
        target = model._label_binarizer.transform(y)
    else:
        target = y.reshape(-1, 1)
    units = [x.shape[1]] + list(model.hidden_layer_sizes) + [model.n_outputs_]
    activations = [x] + [None] * (len(units) - 1)
    deltas = [None] * (len(units) - 1)
    coef_grads = [np.empty((a, b)) for a, b in zip(units[:-1], units[1:])]
    intercept_grads = [np.empty(b) for b in units[1:]]
    return model._backprop(x, target, None, activations, deltas, coef_grads, intercept_grads)


def write(name, x, y, model, alpha, activation, kind):
    with open(f"neural_{name}.txt", "w") as f:
        f.write(f"kind {kind}\nactivation {activation}\nalpha {alpha!r}\n")
        f.write(f"rows {x.shape[0]}\ninputs {x.shape[1]}\n")
        for row, t in zip(x, y):
            f.write("x " + " ".join(repr(float(v)) for v in row) + f" {float(t)!r}\n")
        loss, cg, ig = gradient(model, x, y)
        for w, b, gw, gb in zip(model.coefs_, model.intercepts_, cg, ig):
            # Stored as (outputs x inputs), row-major.
            f.write(f"layer {w.shape[0]} {w.shape[1]}\n")
            f.write("w " + " ".join(repr(float(v)) for v in w.T.ravel()) + "\n")
            f.write("b " + " ".join(repr(float(v)) for v in b) + "\n")
            f.write("gw " + " ".join(repr(float(v)) for v in gw.T.ravel()) + "\n")
            f.write("gb " + " ".join(repr(float(v)) for v in gb) + "\n")
        f.write(f"loss {float(model.loss_)!r}\n")
        if kind == "regression":
            pred = model.predict(x).reshape(-1, 1)
        else:
            pred = model.predict_proba(x)
        for p in pred:
            f.write("p " + " ".join(repr(float(v)) for v in p) + "\n")
        g = np.abs(np.concatenate([c.ravel() for c in model.coefs_])).max()
        print(name, "loss", model.loss_, "iterations", model.n_iter_, "max |w|", g)


# Regression: curved surface in two inputs, one hidden layer of 4 tanh nodes.
n = 120
x = np.round(rng.uniform(-2, 2, (n, 2)), 6)
y = np.round(np.sin(1.5 * x[:, 0]) + 0.5 * x[:, 1] ** 2 + 0.1 * rng.normal(size=n), 6)
m = MLPRegressor(hidden_layer_sizes=(4,), activation="tanh", solver="lbfgs", alpha=0.01, tol=1e-12, max_iter=20000,
                 max_fun=200000, random_state=1).fit(x, y)
write("regression", x, y, m, 0.01, "tanh", "regression")

# Two classes: two hidden layers (3, 2) of logistic nodes.
n = 150
x = np.round(rng.uniform(-2, 2, (n, 3)), 6)
y = (x[:, 0] ** 2 + x[:, 1] + 0.3 * rng.normal(size=n) > 1.0).astype(int)
m = MLPClassifier(hidden_layer_sizes=(3, 2), activation="logistic", solver="lbfgs", alpha=0.1, tol=1e-12, max_iter=20000,
                  max_fun=200000, random_state=2).fit(x, y)
write("binary", x, y, m, 0.1, "logistic", "classification")

# Three classes: one hidden layer of 5 ReLU nodes.
n = 150
x = np.round(rng.uniform(-2, 2, (n, 2)), 6)
r = x[:, 0] ** 2 + x[:, 1] ** 2
y = np.where(r < 1.2, 0, np.where(x[:, 0] > 0, 1, 2))
m = MLPClassifier(hidden_layer_sizes=(5,), activation="relu", solver="lbfgs", alpha=1.0, tol=1e-12, max_iter=20000,
                  max_fun=200000, random_state=3).fit(x, y)
write("multiclass", x, y, m, 1.0, "relu", "classification")
