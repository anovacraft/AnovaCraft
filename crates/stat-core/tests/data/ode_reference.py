# Reference values for the tests in crates/stat-core/src/ode.rs.
#
# Each problem is solved with SciPy's DOP853 (an eighth-order Dormand-Prince
# pair, a different method from the fifth-order solver under test) at
# rtol = 1e-13, atol = 1e-14, so the references are several orders of
# magnitude more accurate than the tolerances the tests check.
#
# Regenerate with: python3 ode_reference.py  (needs numpy and scipy; < 1 s)
import numpy as np
from scipy.integrate import solve_ivp


def solve(f, x0, y0, outputs):
    """Integrate from x0 forward through outputs > x0 and backward through outputs < x0."""
    res = {}
    for side in (1, -1):
        pts = sorted([x for x in outputs if (x - x0) * side > 0], key=lambda x: side * x)
        if not pts:
            continue
        sol = solve_ivp(f, (x0, pts[-1]), y0, method="DOP853", rtol=1e-13, atol=1e-14, t_eval=pts)
        assert sol.success, sol.message
        for i, x in enumerate(pts):
            res[x] = sol.y[:, i]
    return [res[x] for x in outputs]


def show(name, outputs, values):
    print(f"// {name}")
    for x, v in zip(outputs, values):
        print(f"({x!r}, [{', '.join(repr(float(c)) for c in v)}]),")


# Lotka-Volterra predator-prey: y1' = a y1 - b y1 y2, y2' = -c y2 + d y1 y2.
a, b, c, d = 1.5, 1.0, 3.0, 1.0
lv_out = [0.5, 1.0, 2.0, 3.5, 5.0, 7.5, 10.0, 15.0]
lv = solve(lambda x, y: [a * y[0] - b * y[0] * y[1], -c * y[1] + d * y[0] * y[1]], 0.0, [10.0, 5.0], lv_out)
show("Lotka-Volterra a=1.5 b=1 c=3 d=1, y(0) = (10, 5)", lv_out, lv)

# Nonlinear scalar (Riccati) ODE y' = 1 + sin(x)/2 - y^2, y(0) = 0.5, outputs on both sides of 0.
nl_out = [-2.0, -0.7, 0.3, 1.0, 2.5, 5.0, 10.0, 20.0]
nl = solve(lambda x, y: [1.0 + 0.5 * np.sin(x) - y[0] ** 2], 0.0, [0.5], nl_out)
show("y' = 1 + sin(x)/2 - y^2, y(0) = 0.5", nl_out, nl)
