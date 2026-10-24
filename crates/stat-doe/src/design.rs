//! The [`Design`] type shared by every generator, and its export to a
//! [`DataTable`].
//!
//! Runs are stored in **coded units**: for two-level factors the low and high
//! settings are −1 and +1, the centre is 0, and axial points of a central
//! composite design lie at ±α. Each factor carries its low and high settings in
//! actual units; the linear map `actual = centre + coded · half-range` (with
//! `centre = (low + high)/2`, `half-range = (high − low)/2`) converts between the
//! two.
//!
//! Runs are generated in **standard order** (for factorial designs, Yates order:
//! the first factor alternates fastest). [`Design::to_table`] can randomize the
//! run order with a seeded shuffle and records the standard order in a column, so
//! the experiment can be run in random order and analysed in either.

pub use stat_core::rng::Rng;
use stat_core::{Result, StatError};
use stat_data::{Column, DataTable, MeasurementLevel};

/// One experimental factor.
#[derive(Debug, Clone, PartialEq)]
pub struct Factor {
    /// Column name.
    pub name: String,
    /// Setting coded −1.
    pub low: f64,
    /// Setting coded +1.
    pub high: f64,
}

impl Factor {
    /// A factor with the given name and low/high settings.
    #[must_use]
    pub fn new(name: impl Into<String>, low: f64, high: f64) -> Self {
        Self { name: name.into(), low, high }
    }

    /// A factor named `name` with coded settings −1 and +1.
    #[must_use]
    pub fn coded(name: impl Into<String>) -> Self {
        Self::new(name, -1.0, 1.0)
    }

    /// Convert a coded value to actual units.
    #[must_use]
    pub fn actual(&self, coded: f64) -> f64 {
        (self.low + self.high) / 2.0 + coded * (self.high - self.low) / 2.0
    }
}

/// Default factor names: `A, B, …, H, J, …, Z` (the letter I is skipped, a
/// convention that avoids confusion with the identity in defining relations),
/// then `X27, X28, …`.
#[must_use]
pub fn default_factor_names(k: usize) -> Vec<String> {
    let letters: Vec<char> = ('A'..='Z').filter(|&c| c != 'I').collect();
    (0..k).map(|i| letters.get(i).map_or_else(|| format!("X{}", i + 1), |c| c.to_string())).collect()
}

/// The role of a run within its design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointType {
    /// A corner (factorial) point.
    Factorial,
    /// An axial ("star") point of a central composite design.
    Axial,
    /// A centre point.
    Center,
    /// An edge mid-point (Box–Behnken).
    Edge,
    /// A fold-over point of a definitive screening design.
    Foldover,
    /// A point chosen by an optimal-design algorithm or a general grid.
    Other,
}

impl PointType {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Factorial => "Factorial",
            Self::Axial => "Axial",
            Self::Center => "Center",
            Self::Edge => "Edge",
            Self::Foldover => "Fold-over",
            Self::Other => "Design",
        }
    }
}

/// A generated design: factors, runs in coded units, and a description.
#[derive(Debug, Clone, PartialEq)]
pub struct Design {
    /// Short description, e.g. `"2^(5−1) fractional factorial, resolution V"`.
    pub description: String,
    /// The factors, one per column of `runs`.
    pub factors: Vec<Factor>,
    /// Runs in standard order, coded units.
    pub runs: Vec<Vec<f64>>,
    /// The role of each run.
    pub point_types: Vec<PointType>,
    /// Further information (defining relation, aliases, efficiencies…).
    pub notes: Vec<String>,
}

/// Units for exported factor columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Units {
    /// Coded units (−1, 0, +1, ±α).
    Coded,
    /// Actual units from each factor's low and high settings.
    #[default]
    Actual,
}

/// Options for [`Design::to_table`].
#[derive(Debug, Clone, PartialEq)]
pub struct TableOptions {
    /// Coded or actual units.
    pub units: Units,
    /// Randomize the run order with this seed (`None` keeps standard order).
    pub randomize: Option<u64>,
    /// Names of empty numeric response columns to add (values to be filled in).
    pub responses: Vec<String>,
    /// Add a nominal "Point type" column when the design has more than one type.
    pub point_type_column: bool,
}

impl Default for TableOptions {
    fn default() -> Self {
        Self { units: Units::Actual, randomize: None, responses: vec!["Y".to_owned()], point_type_column: true }
    }
}

impl Design {
    /// A design, checking that every run has one value per factor.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if a run has the wrong length or the
    /// point types do not match the runs.
    pub fn new(
        description: impl Into<String>,
        factors: Vec<Factor>,
        runs: Vec<Vec<f64>>,
        point_types: Vec<PointType>,
    ) -> Result<Self> {
        if let Some(r) = runs.iter().find(|r| r.len() != factors.len()) {
            return Err(StatError::DimensionMismatch {
                context: "design run",
                expected: factors.len(),
                found: r.len(),
            });
        }
        if point_types.len() != runs.len() {
            return Err(StatError::DimensionMismatch {
                context: "point types",
                expected: runs.len(),
                found: point_types.len(),
            });
        }
        Ok(Self { description: description.into(), factors, runs, point_types, notes: Vec::new() })
    }

    /// Number of runs.
    #[must_use]
    pub fn n_runs(&self) -> usize {
        self.runs.len()
    }

    /// Number of factors.
    #[must_use]
    pub fn n_factors(&self) -> usize {
        self.factors.len()
    }

    /// Coded values of factor `j` in standard order.
    #[must_use]
    pub fn column(&self, j: usize) -> Vec<f64> {
        self.runs.iter().map(|r| r[j]).collect()
    }

    /// Replace the factors (names and ranges), keeping the runs.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if the number of factors changes;
    /// [`StatError::InvalidArgument`] for duplicate or blank names, or a factor
    /// whose low and high settings are equal or not finite.
    pub fn with_factors(mut self, factors: Vec<Factor>) -> Result<Self> {
        if factors.len() != self.factors.len() {
            return Err(StatError::DimensionMismatch {
                context: "factors",
                expected: self.factors.len(),
                found: factors.len(),
            });
        }
        for (i, f) in factors.iter().enumerate() {
            if f.name.trim().is_empty() || factors[..i].iter().any(|g| g.name == f.name) {
                return Err(StatError::InvalidArgument {
                    message: format!("factor names must be unique and non-blank ({:?})", f.name),
                });
            }
            if !(f.low.is_finite() && f.high.is_finite()) || f.low == f.high {
                return Err(StatError::InvalidArgument {
                    message: format!("factor {:?} needs different, finite low and high settings", f.name),
                });
            }
        }
        self.factors = factors;
        Ok(self)
    }

    /// Append `count` centre points.
    pub fn add_center_points(&mut self, count: usize) {
        let k = self.factors.len();
        for _ in 0..count {
            self.runs.push(vec![0.0; k]);
            self.point_types.push(PointType::Center);
        }
    }

    /// Export as a data table: a `Run` column (1, 2, … in the order to run),
    /// a `Std order` column when randomized, one numeric column per factor, an
    /// optional `Point type` column, and empty response columns.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if a response name clashes with a factor
    /// or another column.
    pub fn to_table(&self, name: &str, options: &TableOptions) -> Result<DataTable> {
        let n = self.n_runs();
        let mut order: Vec<usize> = (0..n).collect();
        if let Some(seed) = options.randomize {
            Rng::new(seed).shuffle(&mut order);
        }
        let mut columns = vec![Column::numeric("Run", (1..=n).map(|i| i as f64).collect())];
        if options.randomize.is_some() {
            let mut c = Column::numeric("Std order", order.iter().map(|&i| (i + 1) as f64).collect());
            c.level = MeasurementLevel::Ordinal;
            columns.push(c);
        }
        for (j, f) in self.factors.iter().enumerate() {
            let values = order
                .iter()
                .map(|&i| {
                    let c = self.runs[i][j];
                    match options.units {
                        Units::Coded => c,
                        Units::Actual => f.actual(c),
                    }
                })
                .collect();
            columns.push(Column::numeric(f.name.clone(), values));
        }
        let distinct_types = {
            let mut t = self.point_types.clone();
            t.sort_by_key(|p| *p as u8);
            t.dedup();
            t.len()
        };
        if options.point_type_column && distinct_types > 1 {
            columns.push(Column::text(
                "Point type",
                order.iter().map(|&i| self.point_types[i].label().to_owned()).collect(),
            ));
        }
        for r in &options.responses {
            columns.push(Column::numeric(r.clone(), vec![f64::NAN; n]));
        }
        DataTable::new(name, columns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn design() -> Design {
        let runs = vec![vec![-1.0, -1.0], vec![1.0, -1.0], vec![-1.0, 1.0], vec![1.0, 1.0], vec![0.0, 0.0]];
        let types = vec![PointType::Factorial; 4].into_iter().chain([PointType::Center]).collect();
        Design::new("test", vec![Factor::new("Temp", 100.0, 200.0), Factor::new("Time", 1.0, 3.0)], runs, types)
            .unwrap()
    }

    #[test]
    fn names_skip_i() {
        let n = default_factor_names(10);
        assert_eq!(n.join(""), "ABCDEFGHJK");
        assert_eq!(default_factor_names(26)[24], "Z");
        assert_eq!(default_factor_names(26)[25], "X26");
    }

    #[test]
    fn export_actual_and_coded_units() {
        let d = design();
        let t = d.to_table("doe", &TableOptions::default()).unwrap();
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Run", "Temp", "Time", "Point type", "Y"]);
        assert_eq!(t.column(1).unwrap().as_numeric().unwrap(), &[100.0, 200.0, 100.0, 200.0, 150.0]);
        assert_eq!(t.column(2).unwrap().as_numeric().unwrap()[4], 2.0);
        assert_eq!(t.column(3).unwrap().cell_text(4), "Center");
        assert!(t.column(4).unwrap().as_numeric().unwrap().iter().all(|v| v.is_nan()));
        let coded = d
            .to_table("doe", &TableOptions { units: Units::Coded, responses: vec![], ..TableOptions::default() })
            .unwrap();
        assert_eq!(coded.column(1).unwrap().as_numeric().unwrap(), &[-1.0, 1.0, -1.0, 1.0, 0.0]);
        assert_eq!(coded.n_cols(), 4);
    }

    #[test]
    fn randomized_order_is_a_reproducible_permutation() {
        let d = design();
        let opts = TableOptions { randomize: Some(42), ..TableOptions::default() };
        let a = d.to_table("doe", &opts).unwrap();
        let b = d.to_table("doe", &opts).unwrap();
        assert_eq!(a, b, "same seed, same order");
        let std: Vec<f64> = a.column(1).unwrap().as_numeric().unwrap().to_vec();
        let mut sorted = std.clone();
        sorted.sort_by(f64::total_cmp);
        assert_eq!(sorted, [1.0, 2.0, 3.0, 4.0, 5.0]);
        // Each run keeps its settings: row i is standard run std[i].
        let temp = a.column(2).unwrap().as_numeric().unwrap();
        for (i, &s) in std.iter().enumerate() {
            assert_eq!(temp[i], d.factors[0].actual(d.runs[s as usize - 1][0]));
        }
    }

    #[test]
    fn factor_validation() {
        let d = design();
        assert!(d.clone().with_factors(vec![Factor::coded("A")]).is_err());
        assert!(d.clone().with_factors(vec![Factor::coded("A"), Factor::coded("A")]).is_err());
        assert!(d.clone().with_factors(vec![Factor::coded("A"), Factor::new("B", 1.0, 1.0)]).is_err());
        assert!(
            d.with_factors(vec![Factor::coded("A"), Factor::new("B", 5.0, 1.0)]).is_ok(),
            "reversed range is allowed"
        );
        assert!(Design::new("x", vec![Factor::coded("A")], vec![vec![1.0, 2.0]], vec![PointType::Other]).is_err());
    }

    #[test]
    fn rng_shuffle_and_range() {
        let mut r = Rng::new(7);
        let mut v: Vec<usize> = (0..50).collect();
        r.shuffle(&mut v);
        let mut s = v.clone();
        s.sort_unstable();
        assert_eq!(s, (0..50).collect::<Vec<_>>());
        assert_ne!(v, s, "50 items are not left in order");
        assert!((0..1000).all(|_| r.below(3) < 3));
    }
}
