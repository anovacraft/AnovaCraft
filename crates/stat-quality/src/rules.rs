//! Run rules for detecting non-random patterns on control charts.
//!
//! The eight tests of Nelson (1984), "The Shewhart Control Chart — Tests for
//! Special Causes", *Journal of Quality Technology* 16(4), 237–239. Zones are
//! measured from the center line in units of the plotted statistic's standard
//! deviation σ_z = (UCL − CL)/3 ([`ControlLimits::zone_sigma`]):
//!
//! | rule | pattern |
//! |---|---|
//! | 1 | one point beyond a control limit |
//! | 2 | nine points in a row on the same side of the center line |
//! | 3 | six points in a row steadily increasing or decreasing |
//! | 4 | fourteen points in a row alternating up and down |
//! | 5 | two of three points in a row more than 2σ_z from center, same side |
//! | 6 | four of five points in a row more than 1σ_z from center, same side |
//! | 7 | fifteen points in a row within 1σ_z of center (either side) |
//! | 8 | eight points in a row more than 1σ_z from center (either side) |
//!
//! # Conventions
//! * A violation is reported at the point that **completes** the pattern; while a
//!   pattern continues, each further point is reported too.
//! * Rules 5 and 6 report a point only if that point is itself one of the
//!   points beyond the zone boundary (on the same side).
//! * Comparisons are strict: a point exactly on the center line belongs to neither
//!   side; a tie between consecutive points breaks a trend or an alternation.
//! * Rule 1 uses the actual limits (so a lower limit clipped at zero is honoured).
//! * A missing value (`None`, e.g. the first moving range) breaks every pattern.

use crate::charts::ControlLimits;

/// One of Nelson's eight run rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RunRule {
    /// 1: a point beyond a control limit.
    BeyondLimits,
    /// 2: nine points in a row on the same side of the center line.
    NineSameSide,
    /// 3: six points in a row steadily increasing or decreasing.
    SixTrending,
    /// 4: fourteen points in a row alternating up and down.
    FourteenAlternating,
    /// 5: two of three points beyond 2σ, same side.
    TwoOfThreeBeyondTwoSigma,
    /// 6: four of five points beyond 1σ, same side.
    FourOfFiveBeyondOneSigma,
    /// 7: fifteen points in a row within 1σ.
    FifteenWithinOneSigma,
    /// 8: eight points in a row beyond 1σ, either side.
    EightBeyondOneSigma,
}

impl RunRule {
    /// All eight rules in order.
    pub const ALL: [Self; 8] = [
        Self::BeyondLimits,
        Self::NineSameSide,
        Self::SixTrending,
        Self::FourteenAlternating,
        Self::TwoOfThreeBeyondTwoSigma,
        Self::FourOfFiveBeyondOneSigma,
        Self::FifteenWithinOneSigma,
        Self::EightBeyondOneSigma,
    ];

    /// Rule number, 1–8.
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            Self::BeyondLimits => 1,
            Self::NineSameSide => 2,
            Self::SixTrending => 3,
            Self::FourteenAlternating => 4,
            Self::TwoOfThreeBeyondTwoSigma => 5,
            Self::FourOfFiveBeyondOneSigma => 6,
            Self::FifteenWithinOneSigma => 7,
            Self::EightBeyondOneSigma => 8,
        }
    }

    /// Short description of the pattern.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::BeyondLimits => "Point beyond a control limit",
            Self::NineSameSide => "9 points in a row on one side of the center line",
            Self::SixTrending => "6 points in a row steadily increasing or decreasing",
            Self::FourteenAlternating => "14 points in a row alternating up and down",
            Self::TwoOfThreeBeyondTwoSigma => "2 of 3 points beyond 2σ on the same side",
            Self::FourOfFiveBeyondOneSigma => "4 of 5 points beyond 1σ on the same side",
            Self::FifteenWithinOneSigma => "15 points in a row within 1σ of the center line",
            Self::EightBeyondOneSigma => "8 points in a row beyond 1σ on either side",
        }
    }

    /// Number of consecutive points the pattern spans.
    #[must_use]
    pub fn window(self) -> usize {
        match self {
            Self::BeyondLimits => 1,
            Self::NineSameSide => 9,
            Self::SixTrending => 6,
            Self::FourteenAlternating => 14,
            Self::TwoOfThreeBeyondTwoSigma => 3,
            Self::FourOfFiveBeyondOneSigma => 5,
            Self::FifteenWithinOneSigma => 15,
            Self::EightBeyondOneSigma => 8,
        }
    }
}

/// A rule broken at a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuleViolation {
    /// The rule.
    pub rule: RunRule,
    /// Index of the point that completes (or continues) the pattern.
    pub index: usize,
    /// Index of the first point of the pattern's window.
    pub window_start: usize,
}

/// Apply `rules` to a sequence of chart values. Violations are returned ordered
/// by point index, then rule number. When the limits have no spread (zone σ of
/// zero) the zone rules 5–8 are not applied.
#[must_use]
pub fn check_rules(values: &[Option<f64>], limits: &ControlLimits, rules: &[RunRule]) -> Vec<RuleViolation> {
    let sigma = limits.zone_sigma();
    let mut out = Vec::new();
    // Process each maximal run of defined values separately.
    let mut start = 0;
    while start < values.len() {
        if values[start].is_none() {
            start += 1;
            continue;
        }
        let end = values[start..].iter().position(Option::is_none).map_or(values.len(), |p| start + p);
        let segment: Vec<f64> = values[start..end].iter().map(|v| v.unwrap_or(f64::NAN)).collect();
        for &rule in rules {
            check_segment(&segment, start, limits, sigma, rule, &mut out);
        }
        start = end;
    }
    out.sort_by_key(|v| (v.index, v.rule));
    out
}

/// Position of a value relative to the center, in zone units.
fn zone(x: f64, center: f64, sigma: f64) -> f64 {
    if sigma > 0.0 { (x - center) / sigma } else { 0.0 }
}

fn side(x: f64, center: f64) -> i8 {
    if x > center {
        1
    } else if x < center {
        -1
    } else {
        0
    }
}

fn check_segment(
    seg: &[f64],
    offset: usize,
    limits: &ControlLimits,
    sigma: f64,
    rule: RunRule,
    out: &mut Vec<RuleViolation>,
) {
    let w = rule.window();
    let center = limits.center;
    // The zone rules (5–8) measure distance in σ; with no spread there are
    // no zones, so they cannot fire (rule 7 would otherwise flag every run of
    // fifteen equal values).
    let zone_rule = matches!(
        rule,
        RunRule::TwoOfThreeBeyondTwoSigma
            | RunRule::FourOfFiveBeyondOneSigma
            | RunRule::FifteenWithinOneSigma
            | RunRule::EightBeyondOneSigma
    );
    if zone_rule && (sigma.is_nan() || sigma <= 0.0) {
        return;
    }
    let z: Vec<f64> = seg.iter().map(|&x| zone(x, center, sigma)).collect();
    let mut flag = |i: usize| {
        out.push(RuleViolation { rule, index: offset + i, window_start: offset + i + 1 - w });
    };
    match rule {
        RunRule::BeyondLimits => {
            for (i, &x) in seg.iter().enumerate() {
                if x > limits.upper || x < limits.lower {
                    flag(i);
                }
            }
        }
        RunRule::NineSameSide => {
            for i in w - 1..seg.len() {
                let s = side(seg[i], center);
                if s != 0 && seg[i + 1 - w..=i].iter().all(|&x| side(x, center) == s) {
                    flag(i);
                }
            }
        }
        RunRule::SixTrending => {
            for i in w - 1..seg.len() {
                let d = &seg[i + 1 - w..=i];
                let up = d.windows(2).all(|p| p[1] > p[0]);
                let down = d.windows(2).all(|p| p[1] < p[0]);
                if up || down {
                    flag(i);
                }
            }
        }
        RunRule::FourteenAlternating => {
            for i in w - 1..seg.len() {
                let d: Vec<f64> = seg[i + 1 - w..=i].windows(2).map(|p| p[1] - p[0]).collect();
                let alternating = d.iter().all(|&x| x != 0.0) && d.windows(2).all(|p| (p[0] > 0.0) != (p[1] > 0.0));
                if alternating {
                    flag(i);
                }
            }
        }
        RunRule::TwoOfThreeBeyondTwoSigma | RunRule::FourOfFiveBeyondOneSigma => {
            let (needed, bound) = if rule == RunRule::TwoOfThreeBeyondTwoSigma { (2, 2.0) } else { (4, 1.0) };
            for i in 0..seg.len() {
                let lo = (i + 1).saturating_sub(w);
                for s in [1.0, -1.0] {
                    let beyond = |zz: f64| zz * s > bound;
                    if beyond(z[i]) && z[lo..=i].iter().filter(|&&zz| beyond(zz)).count() >= needed {
                        // Window starts where the counted points begin (may be shorter at the start).
                        out.push(RuleViolation { rule, index: offset + i, window_start: offset + lo });
                    }
                }
            }
        }
        RunRule::FifteenWithinOneSigma => {
            for i in w - 1..seg.len() {
                if z[i + 1 - w..=i].iter().all(|zz| zz.abs() < 1.0) {
                    flag(i);
                }
            }
        }
        RunRule::EightBeyondOneSigma => {
            for i in w - 1..seg.len() {
                if z[i + 1 - w..=i].iter().all(|zz| zz.abs() > 1.0) {
                    flag(i);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Center 0, σ_z = 1, limits ±3.
    fn limits() -> ControlLimits {
        ControlLimits { center: 0.0, lower: -3.0, upper: 3.0 }
    }

    fn check(values: &[f64], rule: RunRule) -> Vec<usize> {
        let v: Vec<Option<f64>> = values.iter().copied().map(Some).collect();
        check_rules(&v, &limits(), &[rule]).iter().map(|x| x.index).collect()
    }

    /// A constant series charted against limits with no spread has no zones:
    /// rule 7 (and the other zone rules) flag nothing.
    #[test]
    fn constant_series_without_spread_flags_no_zone_rule() {
        let flat = ControlLimits { center: 5.0, lower: 5.0, upper: 5.0 };
        let v = vec![Some(5.0); 20];
        assert!(check_rules(&v, &flat, &RunRule::ALL).is_empty());
    }

    #[test]
    fn rule_1_beyond_limits() {
        assert_eq!(check(&[0.0, 3.1, 2.9, -3.5, 3.0], RunRule::BeyondLimits), vec![1, 3]);
        // A clipped lower limit at zero (dispersion charts) is honoured.
        let clipped = ControlLimits { center: 1.0, lower: 0.0, upper: 2.5 };
        let r = check_rules(&[Some(0.1), Some(2.6)], &clipped, &[RunRule::BeyondLimits]);
        assert_eq!(r.iter().map(|v| v.index).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn rule_2_nine_on_one_side() {
        let mut v = vec![0.5; 9];
        assert_eq!(check(&v, RunRule::NineSameSide), vec![8]);
        v.push(0.2);
        assert_eq!(check(&v, RunRule::NineSameSide), vec![8, 9], "continuing run keeps flagging");
        v[4] = 0.0;
        assert!(check(&v, RunRule::NineSameSide).is_empty(), "a point on the center line breaks the run");
        assert!(check(&[0.5; 8], RunRule::NineSameSide).is_empty());
        assert_eq!(check(&[-0.1; 9], RunRule::NineSameSide), vec![8]);
    }

    #[test]
    fn rule_3_six_trending() {
        assert_eq!(check(&[0.0, 0.1, 0.2, 0.3, 0.4, 0.5], RunRule::SixTrending), vec![5]);
        assert_eq!(check(&[1.0, 0.8, 0.6, 0.4, 0.2, 0.0, -0.2], RunRule::SixTrending), vec![5, 6]);
        assert!(check(&[0.0, 0.1, 0.2, 0.2, 0.3, 0.4], RunRule::SixTrending).is_empty(), "a tie breaks the trend");
        assert!(check(&[0.0, 0.1, 0.2, 0.3, 0.4], RunRule::SixTrending).is_empty(), "five points are not enough");
    }

    #[test]
    fn rule_4_fourteen_alternating() {
        let v: Vec<f64> = (0..14).map(|i| if i % 2 == 0 { 0.5 } else { -0.5 }).collect();
        assert_eq!(check(&v, RunRule::FourteenAlternating), vec![13]);
        assert!(check(&v[..13], RunRule::FourteenAlternating).is_empty());
        let mut broken = v.clone();
        broken[7] = broken[6];
        assert!(check(&broken, RunRule::FourteenAlternating).is_empty());
    }

    #[test]
    fn rule_5_two_of_three_beyond_two_sigma() {
        assert_eq!(check(&[2.5, 0.0, 2.1], RunRule::TwoOfThreeBeyondTwoSigma), vec![2]);
        assert_eq!(check(&[2.5, 2.2], RunRule::TwoOfThreeBeyondTwoSigma), vec![1]);
        // Opposite sides do not count together.
        assert!(check(&[2.5, 0.0, -2.1], RunRule::TwoOfThreeBeyondTwoSigma).is_empty());
        // Too far apart (only one in any window of three).
        assert!(check(&[2.5, 0.0, 0.0, 2.1], RunRule::TwoOfThreeBeyondTwoSigma).is_empty());
        // The completing point itself must be beyond 2σ.
        assert_eq!(check(&[2.5, 2.2, 0.0], RunRule::TwoOfThreeBeyondTwoSigma), vec![1]);
        // Exactly 2σ is not beyond.
        assert!(check(&[2.0, 2.0, 2.0], RunRule::TwoOfThreeBeyondTwoSigma).is_empty());
    }

    #[test]
    fn rule_6_four_of_five_beyond_one_sigma() {
        assert_eq!(check(&[-1.5, -1.2, 0.0, -2.0, -1.1], RunRule::FourOfFiveBeyondOneSigma), vec![4]);
        assert!(check(&[-1.5, -1.2, 0.0, 0.0, -1.1], RunRule::FourOfFiveBeyondOneSigma).is_empty());
        assert!(check(&[1.5, 1.2, 0.0, -2.0, 1.1], RunRule::FourOfFiveBeyondOneSigma).is_empty(), "mixed sides");
    }

    #[test]
    fn rule_7_fifteen_within_one_sigma() {
        let v: Vec<f64> = (0..15).map(|i| if i % 2 == 0 { 0.3 } else { -0.4 }).collect();
        assert_eq!(check(&v, RunRule::FifteenWithinOneSigma), vec![14]);
        let mut w = v.clone();
        w[7] = 1.0;
        assert!(check(&w, RunRule::FifteenWithinOneSigma).is_empty(), "exactly 1σ is not within");
    }

    #[test]
    fn rule_8_eight_beyond_one_sigma_either_side() {
        let v = [1.5, -1.5, 2.0, -1.2, 1.1, -2.5, 1.3, -1.4];
        assert_eq!(check(&v, RunRule::EightBeyondOneSigma), vec![7]);
        let mut w = v;
        w[3] = 0.5;
        assert!(check(&w, RunRule::EightBeyondOneSigma).is_empty());
    }

    #[test]
    fn missing_values_break_patterns_and_results_are_ordered() {
        let mut v: Vec<Option<f64>> = vec![Some(0.5); 10];
        v[4] = None;
        assert!(check_rules(&v, &limits(), &[RunRule::NineSameSide]).is_empty());
        let w = vec![Some(3.5), Some(2.5), Some(2.6), None, Some(-4.0)];
        let r = check_rules(&w, &limits(), &RunRule::ALL);
        let summary: Vec<(usize, u8)> = r.iter().map(|x| (x.index, x.rule.number())).collect();
        assert_eq!(summary, vec![(0, 1), (1, 5), (2, 5), (4, 1)]);
        assert_eq!(r[1].window_start, 0);
    }

    #[test]
    fn rule_metadata() {
        assert_eq!(RunRule::ALL.iter().map(|r| r.number()).collect::<Vec<_>>(), (1..=8).collect::<Vec<u8>>());
        assert_eq!(RunRule::FourteenAlternating.window(), 14);
        assert!(RunRule::SixTrending.description().contains("6 points"));
    }
}
