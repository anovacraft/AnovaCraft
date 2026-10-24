//! A seedable pseudo-random generator: SplitMix64 (Steele, Lea & Flood 2014,
//! "Fast splittable pseudorandom number generators", *OOPSLA*), shared by
//! experimental designs (run-order randomization, exchange starts) and
//! random sampling of table rows.

/// A small, fast, seedable generator (SplitMix64; Steele, Lea & Flood 2014),
/// so designs, randomizations and random samples are reproducible from their seed.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator seeded with `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform value in `[0, 1)` with 53 random bits.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// A uniform integer in `0..n` (`n > 0`).
    pub fn below(&mut self, n: usize) -> usize {
        // Multiply-shift: unbiased enough for design construction.
        ((u128::from(self.next_u64()) * n as u128) >> 64) as usize
    }

    /// A standard normal draw (Box–Muller transform; Box & Muller, *Annals
    /// of Mathematical Statistics* 29, 1958). Uses two uniforms per draw.
    pub fn normal(&mut self) -> f64 {
        // 1 − u lies in (0, 1], so the logarithm is finite.
        let u = 1.0 - self.next_f64();
        let v = self.next_f64();
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }

    /// A draw from the triangular distribution on `[low, high]` with mode
    /// `mode` (by inverting its distribution function). Requires
    /// `low ≤ mode ≤ high`; returns `low` when `low == high`.
    pub fn triangular(&mut self, low: f64, mode: f64, high: f64) -> f64 {
        triangular_quantile(self.next_f64(), low, mode, high)
    }

    /// A Gamma(`shape`, 1) draw (`shape > 0`) by the squeeze method of
    /// Marsaglia & Tsang (2000, "A simple method for generating gamma
    /// variables", *ACM TOMS* 26); a shape below one uses
    /// `G(shape + 1) · U^{1/shape}`.
    pub fn gamma(&mut self, shape: f64) -> f64 {
        if shape < 1.0 {
            // Boost in log space: `U^{1/shape}` alone underflows for tiny
            // shapes even when the product is representable.
            let u = 1.0 - self.next_f64();
            let g = self.gamma(shape + 1.0);
            return (g.ln() + u.ln() / shape).exp();
        }
        let d = shape - 1.0 / 3.0;
        let c = 1.0 / (9.0 * d).sqrt();
        loop {
            let z = self.normal();
            let t = 1.0 + c * z;
            if t <= 0.0 {
                continue;
            }
            let v = t * t * t;
            let u = 1.0 - self.next_f64();
            if u.ln() < 0.5 * z * z + d - d * v + d * v.ln() {
                return d * v;
            }
        }
    }

    /// A chi-square draw with `df > 0` degrees of freedom (`2 G(df/2)`).
    pub fn chi_square(&mut self, df: f64) -> f64 {
        2.0 * self.gamma(0.5 * df)
    }

    /// Shuffle in place (Fisher–Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

/// Quantile `u ∈ [0, 1]` of the triangular distribution on `[low, high]`
/// with mode `mode`: `low + √(u (high − low)(mode − low))` below
/// `F(mode) = (mode − low)/(high − low)`, `high − √((1 − u)(high − low)(high − mode))` above.
#[must_use]
pub fn triangular_quantile(u: f64, low: f64, mode: f64, high: f64) -> f64 {
    let width = high - low;
    if width <= 0.0 {
        return low;
    }
    let cut = (mode - low) / width;
    if u < cut { low + (u * width * (mode - low)).sqrt() } else { high - ((1.0 - u) * width * (high - mode)).sqrt() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_and_triangular_draws_have_their_moments() {
        let mut rng = Rng::new(3);
        let n = 200_000;
        let z: Vec<f64> = (0..n).map(|_| rng.normal()).collect();
        let mean = z.iter().sum::<f64>() / n as f64;
        let var = z.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
        assert!(mean.abs() < 0.01 && (var - 1.0).abs() < 0.01, "{mean} {var}");
        // Triangular(0, 1, 4): mean 5/3, variance (0 + 1 + 16 − 0 − 0 − 4)/18 = 13/18.
        let t: Vec<f64> = (0..n).map(|_| rng.triangular(0.0, 1.0, 4.0)).collect();
        let mean = t.iter().sum::<f64>() / n as f64;
        let var = t.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
        assert!((mean - 5.0 / 3.0).abs() < 0.01 && (var - 13.0 / 18.0).abs() < 0.01, "{mean} {var}");
        assert!(t.iter().all(|v| (0.0..=4.0).contains(v)));
        assert_eq!(triangular_quantile(0.25, 0.0, 1.0, 4.0), 1.0);
        assert_eq!(rng.triangular(2.0, 2.0, 2.0), 2.0);
    }

    #[test]
    fn gamma_and_chi_square_draws_have_their_moments() {
        let mut rng = Rng::new(9);
        let n = 200_000;
        for shape in [0.4, 1.0, 3.5] {
            let g: Vec<f64> = (0..n).map(|_| rng.gamma(shape)).collect();
            let mean = g.iter().sum::<f64>() / n as f64;
            let var = g.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
            // Gamma(k, 1): mean and variance k.
            assert!((mean - shape).abs() < 0.02 * shape.max(1.0) && (var - shape).abs() < 0.05 * shape.max(1.0));
            assert!(g.iter().all(|v| *v > 0.0));
        }
        let c: Vec<f64> = (0..n).map(|_| rng.chi_square(7.0)).collect();
        let mean = c.iter().sum::<f64>() / n as f64;
        assert!((mean - 7.0).abs() < 0.05, "{mean}");
    }

    #[test]
    fn reproducible_and_in_range() {
        let (mut a, mut b) = (Rng::new(42), Rng::new(42));
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
            let u = a.next_f64();
            assert!((0.0..1.0).contains(&u));
            assert!(a.below(7) < 7);
            b.next_f64();
            b.below(7);
        }
    }

    #[test]
    fn first_output_matches_reference() {
        // SplitMix64 seeded with 0 (reference implementation by Vigna).
        assert_eq!(Rng::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
    }
}
