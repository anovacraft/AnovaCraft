//! Axis scales: continuous data are drawn on a transformed scale (linear,
//! base-10 log, natural log, logit, square root or reciprocal) without
//! changing the data. Fits
//! and smoothers on such an axis are computed on the transformed values, and
//! axis labels show the original units at the transformed positions.

/// A transformation of a continuous axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum AxisScale {
    /// Identity.
    #[default]
    Linear,
    /// `log₁₀ v` (positive values only).
    Log10,
    /// `ln v` (positive values only).
    Ln,
    /// `√v` (non-negative values only).
    Sqrt,
    /// `ln(v / (1 − v))` (proportions strictly between 0 and 1).
    Logit,
    /// `−1/v` (positive values only). The sign keeps larger values further
    /// along the axis, so the axis still reads in increasing order.
    Reciprocal,
}

impl AxisScale {
    /// Every scale, in menu order.
    pub const ALL: [AxisScale; 6] =
        [AxisScale::Linear, AxisScale::Log10, AxisScale::Ln, AxisScale::Logit, AxisScale::Sqrt, AxisScale::Reciprocal];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Log10 => "Log (base 10)",
            Self::Ln => "Log (natural)",
            Self::Sqrt => "Square root",
            Self::Logit => "Logit",
            Self::Reciprocal => "Reciprocal",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Log10 => "log10",
            Self::Ln => "ln",
            Self::Sqrt => "sqrt",
            Self::Logit => "logit",
            Self::Reciprocal => "reciprocal",
        }
    }

    /// The scale with this [`AxisScale::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    /// The drawn position of value `v`, or `None` if `v` is missing or outside
    /// the scale's domain (zero or negative for a log, negative for a root).
    #[must_use]
    pub fn forward(self, v: f64) -> Option<f64> {
        if !v.is_finite() {
            return None;
        }
        let t = match self {
            Self::Linear => v,
            Self::Log10 if v > 0.0 => v.log10(),
            Self::Ln if v > 0.0 => v.ln(),
            Self::Sqrt if v >= 0.0 => v.sqrt(),
            Self::Logit if v > 0.0 && v < 1.0 => (v / (1.0 - v)).ln(),
            Self::Reciprocal if v > 0.0 => -1.0 / v,
            _ => return None,
        };
        t.is_finite().then_some(t)
    }

    /// The data value at drawn position `t` (inverse of [`AxisScale::forward`]).
    #[must_use]
    pub fn inverse(self, t: f64) -> f64 {
        match self {
            Self::Linear => t,
            Self::Log10 => 10f64.powf(t),
            Self::Ln => t.exp(),
            Self::Sqrt => t * t.abs(),
            Self::Logit => 1.0 / (1.0 + (-t).exp()),
            Self::Reciprocal => -1.0 / t,
        }
    }

    /// `name` as transformed by the scale, for equations in drawn units:
    /// `log₁₀(name)`, `ln(name)`, `√(name)`, `logit(name)`, `−1/name`.
    #[must_use]
    pub fn wrap_name(self, name: &str) -> String {
        match self {
            Self::Linear => name.to_owned(),
            Self::Log10 => format!("log₁₀({name})"),
            Self::Ln => format!("ln({name})"),
            Self::Sqrt => format!("√({name})"),
            Self::Logit => format!("logit({name})"),
            Self::Reciprocal => format!("−1/{name}"),
        }
    }

    /// Name of the domain restriction, for notes about dropped values.
    #[must_use]
    pub fn domain(self) -> Option<&'static str> {
        match self {
            Self::Linear => None,
            Self::Log10 | Self::Ln | Self::Reciprocal => Some("positive values"),
            Self::Logit => Some("proportions strictly between 0 and 1"),
            Self::Sqrt => Some("non-negative values"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_inverse_and_domains() {
        for s in AxisScale::ALL {
            let values: &[f64] =
                if s == AxisScale::Logit { &[0.001, 0.25, 0.5, 0.97] } else { &[0.25, 1.0, 7.5, 1234.0] };
            for &v in values {
                let t = s.forward(v).unwrap();
                assert!((s.inverse(t) - v).abs() < 1e-9 * v.max(1.0), "{s:?} {v}");
            }
            assert_eq!(AxisScale::from_key(s.key()), Some(s));
            assert!(s.forward(f64::NAN).is_none());
        }
        assert_eq!(AxisScale::Log10.forward(1000.0), Some(3.0));
        assert!(AxisScale::Log10.forward(0.0).is_none() && AxisScale::Ln.forward(-1.0).is_none());
        assert_eq!(AxisScale::Sqrt.forward(0.0), Some(0.0));
        assert!(AxisScale::Sqrt.forward(-0.1).is_none());
        assert_eq!(AxisScale::Linear.forward(-5.0), Some(-5.0));
        assert!(AxisScale::Linear.domain().is_none());
        assert_eq!(AxisScale::Logit.forward(0.5), Some(0.0));
        assert!(AxisScale::Logit.forward(0.0).is_none() && AxisScale::Logit.forward(1.0).is_none());
        // The reciprocal scale keeps the order of the values.
        let (a, b) = (AxisScale::Reciprocal.forward(2.0).unwrap(), AxisScale::Reciprocal.forward(4.0).unwrap());
        assert!(a < b && AxisScale::Reciprocal.forward(0.0).is_none());
    }
}
