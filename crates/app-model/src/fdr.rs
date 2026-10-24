//! False-discovery-rate adjustment of many p-values (Benjamini–Hochberg,
//! Benjamini–Yekutieli or Storey q-values) and the interval ("forest") plot,
//! shared by the screening, contingency-table and model-comparison reports.

use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind};
use stat_core::Result;
use stat_inference::pvalues::{AdjustMethod, adjust_p_values, storey_q_values};

/// The `λ` of Storey's estimate of the proportion of true null hypotheses.
pub const STOREY_LAMBDA: f64 = 0.5;

/// A false-discovery-rate adjustment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FdrMethod {
    /// Benjamini & Hochberg (1995): independent or positively dependent tests.
    BenjaminiHochberg,
    /// Benjamini & Yekutieli (2001): any dependence.
    BenjaminiYekutieli,
    /// Storey (2002) q-values with `π̂₀(λ = 0.5)`.
    Storey,
}

impl FdrMethod {
    /// Every method, in menu order.
    pub const ALL: [Self; 3] = [Self::BenjaminiHochberg, Self::BenjaminiYekutieli, Self::Storey];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::BenjaminiHochberg => "Benjamini–Hochberg",
            Self::BenjaminiYekutieli => "Benjamini–Yekutieli",
            Self::Storey => "Storey q-values",
        }
    }

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::BenjaminiHochberg => "bh",
            Self::BenjaminiYekutieli => "by",
            Self::Storey => "storey",
        }
    }

    /// The method with identifier `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }

    /// The adjusted p-values (q-values) of `p`, NaN where `p` is NaN, and for
    /// Storey's method the estimated proportion of true nulls `π̂₀`.
    ///
    /// # Errors
    /// A p-value outside `[0, 1]`.
    pub fn adjust(self, p: &[f64]) -> Result<(Vec<f64>, Option<f64>)> {
        if p.iter().all(|v| v.is_nan()) {
            return Ok((p.to_vec(), None));
        }
        Ok(match self {
            Self::BenjaminiHochberg => (adjust_p_values(p, AdjustMethod::BenjaminiHochberg)?, None),
            Self::BenjaminiYekutieli => (adjust_p_values(p, AdjustMethod::BenjaminiYekutieli)?, None),
            Self::Storey => {
                let q = storey_q_values(p, STOREY_LAMBDA)?;
                (q.q_values, Some(q.pi0))
            }
        })
    }
}

/// One row of an interval plot: label, estimate and interval.
#[derive(Debug, Clone, PartialEq)]
pub struct IntervalRow {
    /// Row label (shown as a y-axis tick).
    pub label: String,
    /// Point estimate.
    pub estimate: f64,
    /// Lower and upper limits (either may be infinite: drawn to the edge).
    pub interval: (f64, f64),
    /// Marker group (index into `groups`).
    pub group: usize,
}

/// An interval ("forest") plot: one horizontal row per item, top to bottom,
/// with a marker at the estimate, a segment over the interval and a
/// vertical reference line at `reference`.
#[must_use]
pub fn interval_plot(
    kind: SectionKind,
    title: String,
    x_label: String,
    rows: &[IntervalRow],
    groups: Vec<String>,
    reference: Option<(f64, String)>,
) -> ReportSection {
    let k = rows.len();
    let y = |i: usize| (k - i) as f64;
    let finite: Vec<f64> = rows
        .iter()
        .flat_map(|r| [r.estimate, r.interval.0, r.interval.1])
        .chain(reference.as_ref().map(|r| r.0))
        .filter(|v| v.is_finite())
        .collect();
    let (lo, hi) = finite.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let pad = if hi > lo { 0.05 * (hi - lo) } else { 1.0 };
    let clamp = |v: f64| {
        if v.is_finite() {
            v
        } else if v < 0.0 {
            lo - pad
        } else {
            hi + pad
        }
    };
    let (line, line_name) = match reference {
        Some((x, name)) => (Some(vec![[x, 0.5], [x, k as f64 + 0.5]]), Some(name)),
        None => (None, None),
    };
    let spec = PlotSpec {
        x_label,
        y_label: String::new(),
        segments: rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.estimate.is_finite())
            .map(|(i, r)| vec![[clamp(r.interval.0), y(i)], [clamp(r.interval.1), y(i)]])
            .collect(),
        annotations: Some(Annotations {
            groups,
            items: rows
                .iter()
                .enumerate()
                .filter(|(_, r)| r.estimate.is_finite())
                .map(|(i, r)| (r.estimate, y(i), String::new(), r.group))
                .collect(),
            labels: Vec::new(),
        }),
        y_ticks: Some(rows.iter().enumerate().map(|(i, r)| (y(i), r.label.clone())).collect()),
        line,
        line_name,
        ..PlotSpec::default()
    };
    ReportSection { kind, title, content: SectionContent::Plot(Box::new(spec)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods_match_r() {
        // R: p.adjust(p, "BH"), p.adjust(p, "BY").
        let p = [0.01, 0.04, 0.03, 0.2, f64::NAN];
        let (bh, _) = FdrMethod::BenjaminiHochberg.adjust(&p).unwrap();
        let want = [0.04, 0.16 / 3.0, 0.16 / 3.0, 0.2];
        for i in 0..4 {
            assert!((bh[i] - want[i]).abs() < 1e-12, "{bh:?}");
        }
        assert!(bh[4].is_nan());
        let (by, _) = FdrMethod::BenjaminiYekutieli.adjust(&p).unwrap();
        assert!((by[0] - 0.04 * (1.0 + 0.5 + 1.0 / 3.0 + 0.25)).abs() < 1e-12);
        let (q, pi0) = FdrMethod::Storey.adjust(&p).unwrap();
        // One of four p-values exceeds 0.5... none do: π̂₀ floors at 1/(m(1−λ)).
        assert!((pi0.unwrap() - 0.5).abs() < 1e-12);
        assert!((q[0] - 0.02).abs() < 1e-12);
        assert_eq!(FdrMethod::from_key("by"), Some(FdrMethod::BenjaminiYekutieli));
        let (all_nan, _) = FdrMethod::Storey.adjust(&[f64::NAN]).unwrap();
        assert!(all_nan[0].is_nan());
    }

    #[test]
    fn interval_plot_rows_top_down() {
        let rows = [
            IntervalRow { label: "a".into(), estimate: 1.0, interval: (0.0, 2.0), group: 0 },
            IntervalRow { label: "b".into(), estimate: -1.0, interval: (f64::NEG_INFINITY, 0.5), group: 1 },
        ];
        let s = interval_plot(
            SectionKind::ScreeningForest,
            "t".into(),
            "x".into(),
            &rows,
            vec!["g0".into(), "g1".into()],
            Some((0.0, "Zero".into())),
        );
        let SectionContent::Plot(p) = s.content else { panic!() };
        assert_eq!(p.y_ticks.as_ref().unwrap()[0], (2.0, "a".to_owned()));
        assert!(p.segments[1][0][0].is_finite());
        assert_eq!(p.annotations.unwrap().items[1].3, 1);
    }
}
