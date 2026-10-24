//! Gauss–Legendre and Gauss–Hermite quadrature.
//!
//! The `n`-point rule integrates polynomials of degree `2n − 1` exactly on
//! `[−1, 1]`. Nodes are the roots of the Legendre polynomial `Pₙ`, found by
//! Newton's method from the Chebyshev-like starting values
//! `cos(π(i − ¼)/(n + ½))`, with weights `2 / ((1 − x²) Pₙ'(x)²)`
//! (Abramowitz & Stegun 25.4.29; Davis & Rabinowitz, *Methods of Numerical
//! Integration*, §2.7). Composite rules split `[a, b]` into equal panels.

/// Nodes and weights of the `n`-point Gauss–Legendre rule on `[−1, 1]`,
/// nodes in increasing order.
///
/// # Panics
/// Never; `n = 0` returns empty vectors.
#[must_use]
pub fn gauss_legendre(n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut nodes = vec![0.0; n];
    let mut weights = vec![0.0; n];
    let nf = n as f64;
    for i in 0..n.div_ceil(2) {
        let mut x = (std::f64::consts::PI * (i as f64 + 0.75) / (nf + 0.5)).cos();
        let mut dp = 0.0;
        for _ in 0..100 {
            // Legendre recurrence: (j+1) P_{j+1} = (2j+1) x P_j − j P_{j−1}.
            let (mut p0, mut p1) = (1.0, x);
            for j in 1..n {
                let jf = j as f64;
                let p2 = ((2.0 * jf + 1.0) * x * p1 - jf * p0) / (jf + 1.0);
                p0 = p1;
                p1 = p2;
            }
            let pn = if n == 1 { x } else { p1 };
            let pn1 = if n == 1 { 1.0 } else { p0 };
            dp = nf * (x * pn - pn1) / (x * x - 1.0);
            let dx = pn / dp;
            x -= dx;
            if dx.abs() <= 1e-16 {
                break;
            }
        }
        let w = 2.0 / ((1.0 - x * x) * dp * dp);
        nodes[i] = -x;
        nodes[n - 1 - i] = x;
        weights[i] = w;
        weights[n - 1 - i] = w;
    }
    if n % 2 == 1 {
        nodes[n / 2] = 0.0;
    }
    (nodes, weights)
}

/// Nodes and weights of the `n`-point Gauss–Hermite rule for
/// `∫ f(x) e^{−x²} dx` over the real line, nodes in increasing order. The
/// rule is exact for polynomials of degree `2n − 1`. Nodes are the roots of
/// the Hermite polynomial `Hₙ`, found by Newton's method on the orthonormal
/// recurrence from the asymptotic starting values of *Numerical Recipes*
/// (Press et al., 3rd ed., §4.6.4), with weights `2 / H̃ₙ'(x)²` for the
/// orthonormal polynomials `H̃`.
///
/// # Panics
/// Never; `n = 0` returns empty vectors.
#[must_use]
pub fn gauss_hermite(n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut nodes = vec![0.0; n];
    let mut weights = vec![0.0; n];
    let nf = n as f64;
    let pim4 = std::f64::consts::PI.powf(-0.25);
    let mut z = 0.0_f64;
    for i in 0..n.div_ceil(2) {
        // Starting guesses for the largest roots, then from the previous ones.
        z = match i {
            0 => (2.0 * nf + 1.0).sqrt() - 1.85575 * (2.0 * nf + 1.0).powf(-1.0 / 6.0),
            1 => z - 1.14 * nf.powf(0.426) / z,
            2 => 1.86 * z - 0.86 * nodes[0],
            3 => 1.91 * z - 0.91 * nodes[1],
            _ => 2.0 * z - nodes[i - 2],
        };
        let mut pp = 0.0;
        for _ in 0..200 {
            let (mut p1, mut p2) = (pim4, 0.0);
            for j in 0..n {
                let p3 = p2;
                p2 = p1;
                let jf = j as f64;
                p1 = z * (2.0 / (jf + 1.0)).sqrt() * p2 - (jf / (jf + 1.0)).sqrt() * p3;
            }
            pp = (2.0 * nf).sqrt() * p2;
            let dz = p1 / pp;
            z -= dz;
            if dz.abs() <= 1e-15 * z.abs().max(1.0) {
                break;
            }
        }
        // Stored temporarily from the largest root down.
        nodes[i] = z;
        weights[i] = 2.0 / (pp * pp);
    }
    // Arrange symmetric pairs in increasing order.
    let half = n.div_ceil(2);
    let (pos, wpos): (Vec<f64>, Vec<f64>) = (nodes[..half].to_vec(), weights[..half].to_vec());
    for i in 0..half {
        nodes[i] = -pos[i];
        weights[i] = wpos[i];
        nodes[n - 1 - i] = pos[i];
        weights[n - 1 - i] = wpos[i];
    }
    if n % 2 == 1 {
        nodes[n / 2] = 0.0;
    }
    (nodes, weights)
}

/// A composite Gauss–Legendre rule: `panels` equal sub-intervals, each with
/// the same `n`-point rule.
#[derive(Debug, Clone)]
pub struct CompositeRule {
    nodes: Vec<f64>,
    weights: Vec<f64>,
    panels: usize,
}

impl CompositeRule {
    /// `panels` sub-intervals with an `n`-point rule in each.
    #[must_use]
    pub fn new(n: usize, panels: usize) -> Self {
        let (nodes, weights) = gauss_legendre(n);
        Self { nodes, weights, panels: panels.max(1) }
    }

    /// `∫ₐᵇ f(x) dx`.
    pub fn integrate(&self, a: f64, b: f64, mut f: impl FnMut(f64) -> f64) -> f64 {
        let h = (b - a) / self.panels as f64;
        let mut total = 0.0;
        for p in 0..self.panels {
            let mid = a + h * (p as f64 + 0.5);
            let half = 0.5 * h;
            let mut s = 0.0;
            for (x, w) in self.nodes.iter().zip(&self.weights) {
                s += w * f(mid + half * x);
            }
            total += half * s;
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hermite_rules_integrate_gaussian_moments() {
        let sqrt_pi = std::f64::consts::PI.sqrt();
        for n in [1, 2, 3, 5, 10, 20, 25, 40] {
            let (x, w) = gauss_hermite(n);
            assert_eq!((x.len(), w.len()), (n, n));
            assert!(x.windows(2).all(|p| p[0] < p[1]), "{n}: increasing");
            assert!(x.iter().zip(x.iter().rev()).all(|(a, b)| (a + b).abs() < 1e-12), "{n}: symmetric");
            // ∫ x^{2k} e^{−x²} = Γ(k + ½) = (2k − 1)!! √π / 2^k, exact for 2k ≤ 2n − 1.
            let mut double_factorial = 1.0;
            for k in 0..n {
                if 2 * k > 2 * n - 1 {
                    break;
                }
                let exact = double_factorial * sqrt_pi / 2f64.powi(k as i32);
                let got: f64 = x.iter().zip(&w).map(|(xi, wi)| wi * xi.powi(2 * k as i32)).sum();
                assert!((got - exact).abs() < 1e-10 * exact.max(1.0), "n = {n}, k = {k}: {got} vs {exact}");
                double_factorial *= (2 * k + 1) as f64;
            }
        }
        // Known 3-point rule: ±√(3/2) with weight √π/6, 0 with 2√π/3.
        let (x, w) = gauss_hermite(3);
        assert!((x[2] - 1.5f64.sqrt()).abs() < 1e-14 && x[1] == 0.0);
        assert!((w[0] - sqrt_pi / 6.0).abs() < 1e-14 && (w[1] - 2.0 * sqrt_pi / 3.0).abs() < 1e-14);
        assert!(gauss_hermite(0).0.is_empty());
    }

    #[test]
    fn rules_are_exact_for_polynomials_and_sum_to_two() {
        for n in [1, 2, 5, 16, 20] {
            let (x, w) = gauss_legendre(n);
            assert!((w.iter().sum::<f64>() - 2.0).abs() < 1e-14, "n = {n}");
            // ∫ x^{2n−2} over [−1, 1] = 2/(2n−1).
            let deg = 2 * n - 2;
            let approx: f64 = x.iter().zip(&w).map(|(x, w)| w * x.powi(deg as i32)).sum();
            assert!((approx - 2.0 / (deg as f64 + 1.0)).abs() < 1e-13, "n = {n}");
        }
        let (x, _) = gauss_legendre(2);
        assert!((x[1] - 1.0 / 3f64.sqrt()).abs() < 1e-15);
    }

    #[test]
    fn composite_rule_integrates_smooth_functions() {
        let rule = CompositeRule::new(16, 8);
        let v = rule.integrate(0.0, std::f64::consts::PI, f64::sin);
        assert!((v - 2.0).abs() < 1e-14);
        let g = rule.integrate(-10.0, 10.0, |x| (-0.5 * x * x).exp());
        assert!((g - (2.0 * std::f64::consts::PI).sqrt()).abs() < 1e-13);
    }
}
