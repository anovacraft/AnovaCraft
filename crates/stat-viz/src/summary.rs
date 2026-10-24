//! Summary statistics of groups of rows, for summary points, bars and lines,
//! with error intervals.
//!
//! Frequencies count observations: a row with frequency 3 is three equal
//! observations in every statistic. Weights enter the mean (a weighted mean
//! `Σ fᵢwᵢyᵢ / Σ fᵢwᵢ`) and its error intervals; other statistics use
//! frequencies only. Quantiles use definition 7 of Hyndman & Fan (1996) on
//! the frequency-expanded sample.

use stat_core::{ContinuousDistribution, StudentsT};

use crate::stats::weighted_quantile;

/// A statistic summarizing a group.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SummaryStat {
    /// Weighted mean.
    #[default]
    Mean,
    /// Median.
    Median,
    /// Sum (of frequency × value).
    Sum,
    /// Smallest value.
    Min,
    /// Largest value.
    Max,
    /// Standard deviation (divisor total frequency − 1).
    StdDev,
    /// Number of observations (total frequency).
    Count,
    /// The `p`-th quantile, `p` in [0, 1].
    Percentile(f64),
}

impl SummaryStat {
    /// The statistics offered in menus (percentile shown at its current p).
    pub const CHOICES: [SummaryStat; 8] = [
        SummaryStat::Mean,
        SummaryStat::Median,
        SummaryStat::Sum,
        SummaryStat::Min,
        SummaryStat::Max,
        SummaryStat::StdDev,
        SummaryStat::Count,
        SummaryStat::Percentile(0.9),
    ];

    /// Display name, e.g. `Mean` or `90th percentile`.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Mean => "Mean".to_owned(),
            Self::Median => "Median".to_owned(),
            Self::Sum => "Sum".to_owned(),
            Self::Min => "Minimum".to_owned(),
            Self::Max => "Maximum".to_owned(),
            Self::StdDev => "Standard deviation".to_owned(),
            Self::Count => "Count".to_owned(),
            Self::Percentile(p) => {
                let pct = (p * 100.0).clamp(0.0, 100.0);
                let r = pct.round();
                let suffix = match (r as i64 % 100, r as i64 % 10) {
                    (11..=13, _) => "th",
                    (_, 1) => "st",
                    (_, 2) => "nd",
                    (_, 3) => "rd",
                    _ => "th",
                };
                if (pct - r).abs() < 1e-9 {
                    format!("{r}{suffix} percentile")
                } else {
                    format!("{} percentile", crate::view::format_value(pct))
                }
            }
        }
    }

    /// Short name for saving (`p90` style for percentiles).
    #[must_use]
    pub fn key(self) -> String {
        match self {
            Self::Mean => "mean".to_owned(),
            Self::Median => "median".to_owned(),
            Self::Sum => "sum".to_owned(),
            Self::Min => "min".to_owned(),
            Self::Max => "max".to_owned(),
            Self::StdDev => "sd".to_owned(),
            Self::Count => "count".to_owned(),
            Self::Percentile(p) => format!("p{}", crate::view::format_value(p * 100.0)),
        }
    }

    /// The statistic with this [`SummaryStat::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "mean" => Some(Self::Mean),
            "median" => Some(Self::Median),
            "sum" => Some(Self::Sum),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            "sd" => Some(Self::StdDev),
            "count" => Some(Self::Count),
            _ => key
                .strip_prefix('p')
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| (0.0..=100.0).contains(v))
                .map(|v| Self::Percentile(v / 100.0)),
        }
    }

    /// `true` for the same statistic ignoring a percentile's p.
    #[must_use]
    pub fn same_kind(self, other: Self) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }
}

/// Error interval drawn around a mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ErrorBars {
    /// None.
    #[default]
    None,
    /// Mean ± one standard error.
    StdErr,
    /// Confidence interval for the mean (t-based, at the element's level).
    Confidence,
    /// Mean ± one standard deviation.
    StdDev,
}

impl ErrorBars {
    /// Every kind, in menu order.
    pub const ALL: [ErrorBars; 4] = [ErrorBars::None, ErrorBars::StdErr, ErrorBars::Confidence, ErrorBars::StdDev];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::StdErr => "± standard error",
            Self::Confidence => "Confidence interval",
            Self::StdDev => "± standard deviation",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::StdErr => "se",
            Self::Confidence => "ci",
            Self::StdDev => "sd",
        }
    }

    /// The kind with this [`ErrorBars::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.key() == key)
    }
}

/// Which side of the mean error bars reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ErrorBarDirection {
    /// Above and below.
    #[default]
    Both,
    /// Above only.
    Upper,
    /// Below only.
    Lower,
}

impl ErrorBarDirection {
    /// Every direction, in menu order.
    pub const ALL: [ErrorBarDirection; 3] =
        [ErrorBarDirection::Both, ErrorBarDirection::Upper, ErrorBarDirection::Lower];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Both => "Both ways",
            Self::Upper => "Upper only",
            Self::Lower => "Lower only",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::Upper => "upper",
            Self::Lower => "lower",
        }
    }

    /// The direction with this [`ErrorBarDirection::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.key() == key)
    }
}

/// How error bars are drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ErrorBarLook {
    /// Color (`None`: the layer's own, e.g. each subgroup's).
    pub color: Option<crate::layer::Rgba>,
    /// Line width in screen points.
    pub width: f32,
    /// Cap width as a multiple of the layer's usual cap (0: no caps).
    pub cap: f64,
    /// Which side of the mean the bars reach.
    pub direction: ErrorBarDirection,
}

impl Default for ErrorBarLook {
    fn default() -> Self {
        Self { color: None, width: 1.5, cap: 1.0, direction: ErrorBarDirection::Both }
    }
}

impl ErrorBarLook {
    /// `true` when nothing differs from the defaults.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// One observation of a group: value, frequency, weight.
pub type Obs = (f64, f64, f64);

/// The usable observations: finite value, positive finite frequency and weight.
fn usable(obs: &[Obs]) -> Vec<Obs> {
    obs.iter()
        .copied()
        .filter(|&(v, f, w)| v.is_finite() && f.is_finite() && f > 0.0 && w.is_finite() && w > 0.0)
        .collect()
}

/// The statistic of a group (`None` when it has no usable observation, or
/// fewer than two for a standard deviation).
#[must_use]
pub fn summarize(obs: &[Obs], stat: SummaryStat) -> Option<f64> {
    let obs = usable(obs);
    if obs.is_empty() {
        return None;
    }
    let total_f: f64 = obs.iter().map(|o| o.1).sum();
    let quantile = |p: f64| weighted_quantile(&obs.iter().map(|o| (o.0, o.1)).collect::<Vec<_>>(), p);
    let value = match stat {
        SummaryStat::Mean => {
            let fw: f64 = obs.iter().map(|o| o.1 * o.2).sum();
            obs.iter().map(|o| o.0 * o.1 * o.2).sum::<f64>() / fw
        }
        SummaryStat::Median => quantile(0.5)?,
        SummaryStat::Percentile(p) => quantile(p.clamp(0.0, 1.0))?,
        SummaryStat::Sum => obs.iter().map(|o| o.0 * o.1).sum(),
        SummaryStat::Min => obs.iter().map(|o| o.0).fold(f64::INFINITY, f64::min),
        SummaryStat::Max => obs.iter().map(|o| o.0).fold(f64::NEG_INFINITY, f64::max),
        SummaryStat::Count => total_f,
        SummaryStat::StdDev => standard_deviation(&obs)?,
    };
    value.is_finite().then_some(value)
}

/// Frequency-weighted standard deviation about the weighted mean, divisor
/// `Σf − 1`.
fn standard_deviation(obs: &[Obs]) -> Option<f64> {
    let total_f: f64 = obs.iter().map(|o| o.1).sum();
    if total_f <= 1.0 {
        return None;
    }
    let fw: f64 = obs.iter().map(|o| o.1 * o.2).sum();
    let mean = obs.iter().map(|o| o.0 * o.1 * o.2).sum::<f64>() / fw;
    let ss: f64 = obs.iter().map(|o| o.1 * o.2 * (o.0 - mean).powi(2)).sum::<f64>() * total_f / fw;
    Some((ss / (total_f - 1.0)).sqrt())
}

/// The error interval `(low, high)` around the group's mean, or `None` for
/// [`ErrorBars::None`] or with fewer than two observations. The standard error
/// is `s / √(Σf)`; the confidence interval uses `t` with `Σf − 1` degrees of
/// freedom at `level`.
#[must_use]
pub fn error_interval(obs: &[Obs], kind: ErrorBars, level: f64) -> Option<(f64, f64)> {
    if kind == ErrorBars::None {
        return None;
    }
    let obs = usable(obs);
    let mean = summarize(&obs, SummaryStat::Mean)?;
    let sd = standard_deviation(&obs)?;
    let n: f64 = obs.iter().map(|o| o.1).sum();
    let half = match kind {
        ErrorBars::StdDev => sd,
        ErrorBars::StdErr => sd / n.sqrt(),
        ErrorBars::Confidence => {
            let t = StudentsT::new(n - 1.0).ok()?.quantile(0.5 + 0.5 * level.clamp(0.5, 0.999)).ok()?;
            t * sd / n.sqrt()
        }
        ErrorBars::None => return None,
    };
    half.is_finite().then_some((mean - half, mean + half))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(v: &[f64]) -> Vec<Obs> {
        v.iter().map(|&x| (x, 1.0, 1.0)).collect()
    }

    #[test]
    fn statistics() {
        let o = obs(&[1.0, 2.0, 3.0, 4.0, 10.0]);
        assert_eq!(summarize(&o, SummaryStat::Mean), Some(4.0));
        assert_eq!(summarize(&o, SummaryStat::Median), Some(3.0));
        assert_eq!(summarize(&o, SummaryStat::Sum), Some(20.0));
        assert_eq!(summarize(&o, SummaryStat::Min), Some(1.0));
        assert_eq!(summarize(&o, SummaryStat::Max), Some(10.0));
        assert_eq!(summarize(&o, SummaryStat::Count), Some(5.0));
        assert_eq!(summarize(&o, SummaryStat::Percentile(0.25)), Some(2.0));
        let sd = summarize(&o, SummaryStat::StdDev).unwrap();
        let direct = ((9.0 + 4.0 + 1.0 + 0.0 + 36.0) / 4.0f64).sqrt();
        assert!((sd - direct).abs() < 1e-12);
        // Frequencies repeat; weights shift only the mean.
        let f = vec![(1.0, 3.0, 1.0), (5.0, 1.0, 1.0)];
        assert_eq!(summarize(&f, SummaryStat::Mean), Some(2.0));
        assert_eq!(summarize(&f, SummaryStat::Median), Some(1.0));
        assert_eq!(summarize(&f, SummaryStat::Count), Some(4.0));
        let w = vec![(1.0, 1.0, 3.0), (5.0, 1.0, 1.0)];
        assert_eq!(summarize(&w, SummaryStat::Mean), Some(2.0));
        assert_eq!(summarize(&w, SummaryStat::Median), Some(3.0));
        // Degenerate groups.
        assert!(summarize(&[], SummaryStat::Mean).is_none());
        assert!(summarize(&obs(&[f64::NAN]), SummaryStat::Max).is_none());
        assert!(summarize(&obs(&[2.0]), SummaryStat::StdDev).is_none());
        assert_eq!(summarize(&obs(&[2.0]), SummaryStat::Median), Some(2.0));
    }

    #[test]
    fn error_intervals() {
        let o = obs(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let sd = 2.5f64.sqrt();
        let (a, b) = error_interval(&o, ErrorBars::StdDev, 0.95).unwrap();
        assert!((a - (3.0 - sd)).abs() < 1e-12 && (b - (3.0 + sd)).abs() < 1e-12);
        let (a, _) = error_interval(&o, ErrorBars::StdErr, 0.95).unwrap();
        assert!((a - (3.0 - sd / 5f64.sqrt())).abs() < 1e-12);
        // t(0.975, 4) = 2.776445.
        let (a, _) = error_interval(&o, ErrorBars::Confidence, 0.95).unwrap();
        assert!((a - (3.0 - 2.776_445_105 * sd / 5f64.sqrt())).abs() < 1e-6);
        assert!(error_interval(&o, ErrorBars::None, 0.95).is_none());
        assert!(error_interval(&obs(&[3.0]), ErrorBars::StdErr, 0.95).is_none());
        assert!(error_interval(&obs(&[3.0, 3.0]), ErrorBars::Confidence, 0.95).is_some_and(|(a, b)| a == b));
    }

    #[test]
    fn names_and_keys() {
        for s in SummaryStat::CHOICES {
            assert!(SummaryStat::from_key(&s.key()).is_some_and(|t| t.same_kind(s)));
        }
        assert_eq!(SummaryStat::from_key("p37.5"), Some(SummaryStat::Percentile(0.375)));
        assert_eq!(SummaryStat::Percentile(0.9).label(), "90th percentile");
        assert_eq!(SummaryStat::Percentile(0.21).label(), "21st percentile");
        assert_eq!(SummaryStat::Percentile(0.12).label(), "12th percentile");
        assert!(SummaryStat::from_key("p120").is_none() && SummaryStat::from_key("x").is_none());
        for e in ErrorBars::ALL {
            assert_eq!(ErrorBars::from_key(e.key()), Some(e));
        }
    }
}
