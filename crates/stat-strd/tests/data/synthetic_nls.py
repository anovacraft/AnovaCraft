"""Writes synthetic_nls.dat: a file in the layout of the NIST StRD nonlinear
regression files, with INVENTED data (y = 240*(1-exp(-0.0006*x)) plus noise,
fixed seed) and "certified" values computed here with SciPy's
least_squares (MINPACK LM, tolerances at machine precision) and standard
deviations from (J'J)^-1. It is not a NIST dataset; it only exercises the
parser and the certified-value harness while the NIST files are absent.
"""
import numpy as np
from scipy import optimize

rng = np.random.default_rng(42)
x = np.array([77.6, 114.9, 141.1, 190.8, 239.9, 289.0, 332.8, 378.4, 434.8, 477.3, 536.8, 593.1, 689.1, 760.0])
y = np.round(240 * (1 - np.exp(-0.0006 * x)) + rng.normal(0, 0.15, x.size), 2)
f = lambda b: b[0] * (1 - np.exp(-b[1] * x)) - y
jac = lambda b: np.column_stack([1 - np.exp(-b[1] * x), b[0] * x * np.exp(-b[1] * x)])
r = optimize.least_squares(f, [250, 0.0005], jac=jac, method='lm', xtol=2.3e-16, ftol=2.3e-16, gtol=2.3e-16)
b = r.x
sse = float(np.sum(r.fun ** 2))
df = x.size - 2
s = np.sqrt(sse / df)
J = jac(b)
se = s * np.sqrt(np.diag(np.linalg.inv(J.T @ J)))
lines = f"""Synthetic StRD-layout file (invented data; NOT a NIST dataset)
Dataset Name:  Synthetic (synthetic_nls.dat)

Procedure:     Nonlinear Least Squares Regression

Description:   Invented data written by synthetic_nls.py; the "certified"
               values were computed with SciPy least_squares.

Data:          1 Response Variable  (y)
               1 Predictor Variable (x)
               {x.size} Observations
               Lower Level of Difficulty
               Generated Data

Model:         Exponential Class
               2 Parameters (b1 to b2)

               y = b1*(1-exp[-b2*x])  +  e


          Starting values                  Certified Values

        Start 1     Start 2           Parameter     Standard Deviation
  b1 =   500         250           {b[0]:.10E}  {se[0]:.10E}
  b2 =   0.0001      0.0005        {b[1]:.10E}  {se[1]:.10E}

Residual Sum of Squares:                    {sse:.10E}
Residual Standard Deviation:                {s:.10E}
Degrees of Freedom:                                {df}
Number of Observations:                            {x.size}


Data:   y               x
"""
for yi, xi in zip(y, x):
    lines += f"      {yi:10.2f}      {xi:10.1f}\n"
open(__file__.replace('.py', '.dat'), 'w').write(lines)
print(b, se, sse)
