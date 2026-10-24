"""Writes hard/*.dat: files in the layout of the NIST StRD nonlinear
regression files with INVENTED data (fixed seed) for four model shapes that
are known to be hard to fit (a two-point-like saturation curve, a
generalized logistic, a narrow Gaussian peak and a rational function),
each with a far start (Start 1) and a near start (Start 2). They are NOT
NIST datasets. The "certified" values are SciPy least_squares fits (MINPACK
LM, tolerances at machine precision) from Start 2, with standard deviations
from (J'J)^-1 with the analytic Jacobians written below. SciPy's own fit
from Start 1 is printed for comparison.
"""
import numpy as np
from scipy import optimize

rng = np.random.default_rng(7)


def write(name, model, xs, ys, starts, b, se, sse, df, level):
    p = len(b)
    lines = f"""Synthetic StRD-layout file (invented data; NOT a NIST dataset)
Dataset Name:  {name} ({name}.dat)

Procedure:     Nonlinear Least Squares Regression

Data:          1 Response Variable  (y)
               1 Predictor Variable (x)
               {len(xs)} Observations
               {level} Level of Difficulty
               Generated Data

Model:         Synthetic Class
               {p} Parameters (b1 to b{p})

               y = {model}  +  e


          Starting values                  Certified Values

        Start 1     Start 2           Parameter     Standard Deviation
"""
    for k in range(p):
        lines += f"  b{k+1} =   {starts[0][k]:<12g}{starts[1][k]:<16g}{b[k]:.10E}  {se[k]:.10E}\n"
    lines += f"""
Residual Sum of Squares:                    {sse:.10E}
Residual Standard Deviation:                {np.sqrt(sse/df):.10E}
Degrees of Freedom:                                {df}
Number of Observations:                            {len(xs)}


Data:   y               x
"""
    for yi, xi in zip(ys, xs):
        lines += f"      {yi:.6f}      {xi:.4f}\n"
    open(__file__.replace('hard_nls.py', f'hard/{name}.dat'), 'w').write(lines)


def case(name, model, f, jac, x, truth, sd, starts, level):
    x = np.asarray(x, float)
    y = np.round(f(x, np.asarray(truth)) + rng.normal(0, sd, x.size), 6)
    r = optimize.least_squares(lambda b: f(x, b) - y, starts[1], jac=lambda b: jac(x, b), method='lm',
                               xtol=2.3e-16, ftol=2.3e-16, gtol=2.3e-16, max_nfev=100000)
    b = r.x
    sse = float(np.sum(r.fun ** 2))
    df = x.size - len(b)
    J = jac(x, b)
    se = np.sqrt(sse / df) * np.sqrt(np.diag(np.linalg.inv(J.T @ J)))
    far = optimize.least_squares(lambda b: f(x, b) - y, starts[0], jac=lambda b: jac(x, b), method='lm',
                                 xtol=2.3e-16, ftol=2.3e-16, gtol=2.3e-16, max_nfev=100000)
    print(name, b, se, sse, '| scipy from start 1:', far.x, np.sum(far.fun ** 2))
    write(name, model, x, y, starts, b, se, sse, df, level)


e = np.exp
case('SatCurve', 'b1*(1-exp[-b2*x])',
     lambda x, b: b[0] * (1 - e(-b[1] * x)),
     lambda x, b: np.column_stack([1 - e(-b[1] * x), b[0] * x * e(-b[1] * x)]),
     [1, 2, 3, 5, 7, 10], [213, 0.55], 12.0, [[1, 1], [100, 0.75]], 'Higher')


def rat(x, b):
    return b[0] / (1 + e(b[1] - b[2] * x)) ** (1 / b[3])


def ratj(x, b):
    u = 1 + e(b[1] - b[2] * x)
    v = u ** (-1 / b[3])
    du1 = e(b[1] - b[2] * x)
    return np.column_stack([v, b[0] * v * (-1 / b[3]) * du1 / u, b[0] * v * (-1 / b[3]) * (-x * du1) / u,
                            b[0] * v * np.log(u) / b[3] ** 2])


case('GenLogistic', 'b1 / ((1+exp[b2-b3*x])**(1/b4))', rat, ratj, np.arange(1, 16.0), [700, 5.3, 0.76, 1.3], 8.0,
     [[100, 10, 1, 1], [700, 5, 0.75, 1.3]], 'Higher')


def peak(x, b):
    return (b[0] / b[1]) * e(-0.5 * ((x - b[2]) / b[1]) ** 2)


def peakj(x, b):
    g = e(-0.5 * ((x - b[2]) / b[1]) ** 2)
    z = (x - b[2]) / b[1]
    return np.column_stack([g / b[1], -b[0] / b[1] ** 2 * g + (b[0] / b[1]) * g * z ** 2 / b[1],
                            (b[0] / b[1]) * g * z / b[1]])


case('NarrowPeak', '(b1/b2) * exp[-0.5*((x-b3)/b2)**2]', peak, peakj, np.arange(400, 500.1, 2.5),
     [1.55, 4.09, 451.5], 0.003, [[1, 10, 500], [1.5, 5, 450]], 'Higher')


def mgh(x, b):
    return b[0] * (x ** 2 + x * b[1]) / (x ** 2 + x * b[2] + b[3])


def mghj(x, b):
    d = x ** 2 + x * b[2] + b[3]
    n = x ** 2 + x * b[1]
    return np.column_stack([n / d, b[0] * x / d, -b[0] * n * x / d ** 2, -b[0] * n / d ** 2])


xm = np.array([4, 2, 1, 0.5, 0.25, 0.167, 0.125, 0.1, 0.0833, 0.0714, 0.0625])
case('RationalKinetics', 'b1*(x**2+x*b2) / (x**2+x*b3+b4)', mgh, mghj, xm, [0.19, 0.19, 0.12, 0.13], 0.002,
     [[25, 39, 41.5, 39], [0.25, 0.39, 0.415, 0.39]], 'Higher')
