//! NIST StRD univariate summary statistics datasets.
//!
//! Certified values (mean, standard deviation, lag-1 autocorrelation) are given
//! to 15 significant digits. NumAcc1–4 are generated datasets and are built here
//! from their published construction; the others were transcribed from the
//! published listings.

/// One univariate reference dataset.
#[derive(Debug, Clone, PartialEq)]
pub struct UnivariateDataset {
    /// NIST dataset name.
    pub name: &'static str,
    /// NIST difficulty rating.
    pub difficulty: crate::Difficulty,
    /// Observations.
    pub values: Vec<f64>,
    /// Certified sample mean.
    pub mean: f64,
    /// Certified sample standard deviation (divisor n − 1).
    pub std_dev: f64,
    /// Certified lag-1 autocorrelation.
    pub autocorrelation: f64,
}

/// The bundled univariate datasets, ordered by difficulty.
#[must_use]
pub fn univariate_datasets() -> Vec<UnivariateDataset> {
    use crate::Difficulty::{Average, Higher, Lower};
    vec![
        UnivariateDataset {
            name: "NumAcc1",
            difficulty: Lower,
            values: vec![10_000_001.0, 10_000_003.0, 10_000_002.0],
            mean: 10_000_002.0,
            std_dev: 1.0,
            autocorrelation: -0.5,
        },
        UnivariateDataset {
            name: "Michelso",
            difficulty: Lower,
            values: MICHELSO.to_vec(),
            mean: 299.852_400_000_000,
            std_dev: 0.079_010_547_819_051_8,
            autocorrelation: 0.535_199_668_621_283,
        },
        UnivariateDataset {
            name: "Lew",
            difficulty: Lower,
            values: LEW.to_vec(),
            mean: -177.435_000_000_000,
            std_dev: 277.332_168_044_316,
            autocorrelation: -0.307_304_800_605_679,
        },
        UnivariateDataset {
            name: "Mavro",
            difficulty: Lower,
            values: MAVRO.to_vec(),
            mean: 2.001_856_000_000_00,
            std_dev: 0.000_429_123_454_003_053,
            autocorrelation: 0.937_989_183_438_248,
        },
        numacc("NumAcc2", Average, 1.2, 1.1, 1.3),
        numacc("NumAcc3", Average, 1_000_000.2, 1_000_000.1, 1_000_000.3),
        numacc("NumAcc4", Higher, 10_000_000.2, 10_000_000.1, 10_000_000.3),
    ]
}

/// NumAcc2–4: 1001 values, `first` followed by 500 pairs `(low, high)`.
/// Certified mean = `first`, sd = 0.1, lag-1 autocorrelation = −0.999.
fn numacc(name: &'static str, difficulty: crate::Difficulty, first: f64, low: f64, high: f64) -> UnivariateDataset {
    let mut values = Vec::with_capacity(1001);
    values.push(first);
    for _ in 0..500 {
        values.push(low);
        values.push(high);
    }
    UnivariateDataset { name, difficulty, values, mean: first, std_dev: 0.1, autocorrelation: -0.999 }
}

#[rustfmt::skip]
const MICHELSO: [f64; 100] = [
    299.85, 299.74, 299.90, 300.07, 299.93, 299.85, 299.95, 299.98, 299.98, 299.88,
    300.00, 299.98, 299.93, 299.65, 299.76, 299.81, 300.00, 300.00, 299.96, 299.96,
    299.96, 299.94, 299.96, 299.94, 299.88, 299.80, 299.85, 299.88, 299.90, 299.84,
    299.83, 299.79, 299.81, 299.88, 299.88, 299.83, 299.80, 299.79, 299.76, 299.80,
    299.88, 299.88, 299.88, 299.86, 299.72, 299.72, 299.62, 299.86, 299.97, 299.95,
    299.88, 299.91, 299.85, 299.87, 299.84, 299.84, 299.85, 299.84, 299.84, 299.84,
    299.89, 299.81, 299.81, 299.82, 299.80, 299.77, 299.76, 299.74, 299.75, 299.76,
    299.91, 299.92, 299.89, 299.86, 299.88, 299.72, 299.84, 299.85, 299.85, 299.78,
    299.89, 299.84, 299.78, 299.81, 299.76, 299.81, 299.79, 299.81, 299.82, 299.85,
    299.87, 299.87, 299.81, 299.74, 299.81, 299.94, 299.95, 299.80, 299.81, 299.87,
];

#[rustfmt::skip]
const MAVRO: [f64; 50] = [
    2.00180, 2.00170, 2.00180, 2.00190, 2.00180, 2.00170, 2.00150, 2.00140, 2.00150, 2.00150,
    2.00170, 2.00180, 2.00180, 2.00190, 2.00190, 2.00210, 2.00200, 2.00160, 2.00140, 2.00130,
    2.00130, 2.00150, 2.00150, 2.00160, 2.00150, 2.00140, 2.00130, 2.00140, 2.00150, 2.00140,
    2.00150, 2.00160, 2.00150, 2.00160, 2.00190, 2.00200, 2.00200, 2.00210, 2.00220, 2.00230,
    2.00240, 2.00250, 2.00270, 2.00260, 2.00260, 2.00260, 2.00270, 2.00260, 2.00250, 2.00240,
];

#[rustfmt::skip]
const LEW: [f64; 200] = [
    -213.0, -564.0, -35.0, -15.0, 141.0, 115.0, -420.0, -360.0, 203.0, -338.0,
    -431.0, 194.0, -220.0, -513.0, 154.0, -125.0, -559.0, 92.0, -21.0, -579.0,
    -52.0, 99.0, -543.0, -175.0, 162.0, -457.0, -346.0, 204.0, -300.0, -474.0,
    164.0, -107.0, -572.0, -8.0, 83.0, -541.0, -224.0, 180.0, -420.0, -374.0,
    201.0, -236.0, -531.0, 83.0, 27.0, -564.0, -112.0, 131.0, -507.0, -254.0,
    199.0, -311.0, -495.0, 143.0, -46.0, -579.0, -90.0, 136.0, -472.0, -338.0,
    202.0, -287.0, -477.0, 169.0, -124.0, -568.0, 17.0, 48.0, -568.0, -135.0,
    162.0, -430.0, -422.0, 172.0, -74.0, -577.0, -13.0, 92.0, -534.0, -243.0,
    194.0, -355.0, -465.0, 156.0, -81.0, -578.0, -64.0, 139.0, -449.0, -384.0,
    193.0, -198.0, -538.0, 110.0, -44.0, -577.0, -6.0, 66.0, -552.0, -164.0,
    161.0, -460.0, -344.0, 205.0, -281.0, -504.0, 134.0, -28.0, -576.0, -118.0,
    156.0, -437.0, -381.0, 200.0, -220.0, -540.0, 83.0, 11.0, -568.0, -160.0,
    172.0, -414.0, -408.0, 188.0, -125.0, -572.0, -32.0, 139.0, -492.0, -321.0,
    205.0, -262.0, -504.0, 142.0, -83.0, -574.0, 0.0, 48.0, -571.0, -106.0,
    137.0, -501.0, -266.0, 190.0, -391.0, -406.0, 194.0, -186.0, -553.0, 83.0,
    -13.0, -577.0, -49.0, 103.0, -515.0, -280.0, 201.0, 300.0, -506.0, 131.0,
    -45.0, -578.0, -80.0, 138.0, -462.0, -361.0, 201.0, -211.0, -554.0, 32.0,
    74.0, -533.0, -235.0, 187.0, -372.0, -442.0, 182.0, -147.0, -566.0, 25.0,
    68.0, -535.0, -244.0, 194.0, -351.0, -463.0, 174.0, -125.0, -570.0, 15.0,
    72.0, -550.0, -190.0, 172.0, -424.0, -385.0, 198.0, -218.0, -536.0, 96.0,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datasets_have_documented_sizes() {
        let sizes: Vec<(&str, usize)> = univariate_datasets().iter().map(|d| (d.name, d.values.len())).collect();
        assert_eq!(
            sizes,
            vec![
                ("NumAcc1", 3),
                ("Michelso", 100),
                ("Lew", 200),
                ("Mavro", 50),
                ("NumAcc2", 1001),
                ("NumAcc3", 1001),
                ("NumAcc4", 1001)
            ]
        );
    }
}
