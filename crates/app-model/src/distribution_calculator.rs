//! Distribution calculator and random data: probabilities, quantiles,
//! density and moments for any distribution in [`stat_core::catalog`], a
//! plot of the density (or probability mass) with the region of interest
//! marked, and seeded random columns.
//!
//! The region is one of `X ≤ a`, `X ≥ a`, `a ≤ X ≤ b` or `X < a or X > b`.
//! For discrete families the bounds are inclusive at whole numbers, so
//! `P(a ≤ X ≤ b) = F(b) − F(⌈a⌉ − 1)`. The report also inverts the
//! distribution at a chosen probability `p`, giving the lower and upper
//! quantiles `F⁻¹(p)` and `F⁻¹(1 − p)`.

use crate::compare::section;
use crate::format::format_number;
use crate::project::{invalid, member, obj, s};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_core::catalog::{Distribution, Family};
use stat_data::{Column, JsonValue};

const DIGITS: usize = 6;

/// The region whose probability is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbabilityRegion {
    /// `X ≤ a`.
    Below,
    /// `X ≥ a`.
    Above,
    /// `a ≤ X ≤ b`.
    Between,
    /// `X < a` or `X > b`.
    Outside,
}

impl ProbabilityRegion {
    /// All regions, in menu order.
    pub const ALL: [Self; 4] = [Self::Below, Self::Above, Self::Between, Self::Outside];

    /// Display label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Below => "X ≤ a",
            Self::Above => "X ≥ a",
            Self::Between => "a ≤ X ≤ b",
            Self::Outside => "X < a or X > b",
        }
    }

    /// Key in saved files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Below => "below",
            Self::Above => "above",
            Self::Between => "between",
            Self::Outside => "outside",
        }
    }

    /// `true` when the region uses the second value `b`.
    #[must_use]
    pub fn two_sided(self) -> bool {
        matches!(self, Self::Between | Self::Outside)
    }
}

/// Settings of a distribution-calculator report.
#[derive(Debug, Clone, PartialEq)]
pub struct CalculatorRequest {
    /// The distribution family.
    pub family: Family,
    /// Its parameters, in the order of [`Family::parameters`].
    pub params: Vec<f64>,
    /// The region whose probability is shown.
    pub region: ProbabilityRegion,
    /// The values `a` and `b` bounding the region (`b` is used only by
    /// two-sided regions).
    pub values: (f64, f64),
    /// Probability at which the distribution is inverted.
    pub probability: f64,
}

impl CalculatorRequest {
    /// `family` with its default parameters, the region below its median.
    #[must_use]
    pub fn new(family: Family) -> Self {
        let params = family.default_parameters();
        let (a, b) = Distribution::new(family, &params)
            .ok()
            .map_or((0.0, 1.0), |d| (d.quantile(0.5).unwrap_or(0.0), d.quantile(0.9).unwrap_or(1.0)));
        Self { family, params, region: ProbabilityRegion::Below, values: (a, b), probability: 0.05 }
    }

    /// Project-file form (the calculator uses no columns).
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let params = JsonValue::Object(
            self.family
                .parameters()
                .iter()
                .zip(&self.params)
                .map(|(p, v)| (p.key.to_owned(), JsonValue::number(*v)))
                .collect(),
        );
        obj([
            ("analysis", s("distribution-calculator")),
            ("family", s(self.family.key())),
            ("parameters", params),
            ("region", s(self.region.key())),
            ("a", JsonValue::number(self.values.0)),
            ("b", JsonValue::number(self.values.1)),
            ("probability", JsonValue::number(self.probability)),
        ])
    }

    /// Read the project-file form; parameters left out take their defaults.
    ///
    /// # Errors
    /// An unknown family or region.
    pub fn from_json(v: &JsonValue) -> Result<Self> {
        let key = member(v, "family")?.as_str().unwrap_or_default();
        let family = Family::from_key(key).ok_or_else(|| invalid(format!("unknown distribution {key:?}")))?;
        let given = v.get("parameters");
        let params = family
            .parameters()
            .iter()
            .map(|p| given.and_then(|g| g.get(p.key)).and_then(JsonValue::as_f64).unwrap_or(p.default))
            .collect();
        let region = match v.get("region").and_then(JsonValue::as_str) {
            None => ProbabilityRegion::Below,
            Some(k) => ProbabilityRegion::ALL
                .into_iter()
                .find(|r| r.key() == k)
                .ok_or_else(|| invalid(format!("unknown probability region {k:?}")))?,
        };
        let d = Self::new(family);
        Ok(Self {
            family,
            params,
            region,
            values: (
                v.get("a").and_then(JsonValue::as_f64).unwrap_or(d.values.0),
                v.get("b").and_then(JsonValue::as_f64).unwrap_or(d.values.1),
            ),
            probability: v.get("probability").and_then(JsonValue::as_f64).unwrap_or(0.05),
        })
    }
}

/// A computed distribution-calculator report.
#[derive(Debug, Clone)]
pub struct CalculatorReport {
    request: CalculatorRequest,
    /// The distribution.
    pub distribution: Distribution,
    /// Probability of the region.
    pub probability: f64,
}

/// `P(X < x)` (strict) for a discrete distribution, `P(X ≤ x)` otherwise.
fn below_strict(d: &Distribution, x: f64) -> f64 {
    if d.family().is_discrete() { if x.ceil() - 1.0 < 0.0 { 0.0 } else { d.cdf(x.ceil() - 1.0) } } else { d.cdf(x) }
}

/// Probability of `region` with bounds `(a, b)`.
fn region_probability(d: &Distribution, region: ProbabilityRegion, (a, b): (f64, f64)) -> f64 {
    match region {
        ProbabilityRegion::Below => d.cdf(a),
        ProbabilityRegion::Above => {
            if d.family().is_discrete() {
                1.0 - below_strict(d, a)
            } else {
                d.sf(a)
            }
        }
        ProbabilityRegion::Between => (d.cdf(b) - below_strict(d, a)).max(0.0),
        ProbabilityRegion::Outside => (below_strict(d, a) + d.sf(b)).min(1.0),
    }
}

/// `true` when `x` lies in the region.
fn in_region(region: ProbabilityRegion, (a, b): (f64, f64), x: f64) -> bool {
    match region {
        ProbabilityRegion::Below => x <= a,
        ProbabilityRegion::Above => x >= a,
        ProbabilityRegion::Between => a <= x && x <= b,
        ProbabilityRegion::Outside => x < a || x > b,
    }
}

impl CalculatorReport {
    /// Build the distribution and compute the region's probability.
    ///
    /// # Errors
    /// Invalid parameters, a NaN bound, `b < a` for a two-sided region, or a
    /// probability outside (0, 1).
    pub fn run(request: &CalculatorRequest) -> Result<Self> {
        let distribution = Distribution::new(request.family, &request.params)?;
        let (a, b) = request.values;
        if a.is_nan() || (request.region.two_sided() && (b.is_nan() || b < a)) {
            return Err(invalid("the region needs a ≤ b (and numbers for both)"));
        }
        if !(request.probability > 0.0 && request.probability < 1.0) {
            return Err(invalid("the probability to invert must lie strictly between 0 and 1"));
        }
        let probability = region_probability(&distribution, request.region, request.values);
        Ok(Self { request: request.clone(), distribution, probability })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &CalculatorRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let params: Vec<String> = self.request.params.iter().map(|v| format_number(*v, 6)).collect();
        format!("Distribution Calculator: {}({})", self.request.family.label(), params.join(", "))
    }

    /// The region with `a` and `b` substituted.
    fn region_text(&self) -> String {
        let (a, b) = self.request.values;
        let (a, b) = (format_number(a, DIGITS), format_number(b, DIGITS));
        match self.request.region {
            ProbabilityRegion::Below => format!("X ≤ {a}"),
            ProbabilityRegion::Above => format!("X ≥ {a}"),
            ProbabilityRegion::Between => format!("{a} ≤ X ≤ {b}"),
            ProbabilityRegion::Outside => format!("X < {a} or X > {b}"),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let d = &self.distribution;
        let family = self.request.family;
        let discrete = family.is_discrete();
        let mut params = TextTable::new(["Quantity", "Value"]);
        for (p, v) in family.parameters().iter().zip(d.params()) {
            params.push([p.name.to_owned(), format_number(*v, DIGITS)]);
        }
        let moment = |v: Option<f64>| v.map_or_else(|| "undefined".to_owned(), |v| format_number(v, DIGITS));
        params.push(["Mean".to_owned(), moment(d.mean())]);
        params.push(["Standard deviation".to_owned(), moment(d.std_dev())]);
        let (lo, hi) = d.support();
        let bound = |v: f64| format_number(v, DIGITS);
        params.push([
            "Support".to_owned(),
            format!("{} to {}{}", bound(lo), bound(hi), if discrete { " (whole numbers)" } else { "" }),
        ]);
        let mut out = vec![section(
            SectionKind::CalculatorDistribution,
            format!("{} Distribution", family.label()),
            SectionContent::Table(params),
        )];
        let (a, b) = self.request.values;
        let p = self.request.probability;
        let mut probs = TextTable::new(["Quantity", "Value"]);
        probs.push([format!("P({})", self.region_text()), format_number(self.probability, DIGITS)]);
        probs.push(["Complement".to_owned(), format_number(1.0 - self.probability, DIGITS)]);
        let f = if discrete { "P(X = x)" } else { "Density f(x)" };
        probs.push([format!("{f} at a = {}", format_number(a, DIGITS)), format_number(d.density(a), DIGITS)]);
        if self.request.region.two_sided() {
            probs.push([format!("{f} at b = {}", format_number(b, DIGITS)), format_number(d.density(b), DIGITS)]);
        }
        let q = |p: f64| d.quantile(p).map_or_else(|e| format!("not available: {e}"), |v| format_number(v, DIGITS));
        probs.push([format!("Quantile at p = {}", format_number(p, DIGITS)), q(p)]);
        probs.push([format!("Quantile at 1 − p = {}", format_number(1.0 - p, DIGITS)), q(1.0 - p)]);
        out.push(section(
            SectionKind::CalculatorProbabilities,
            if discrete {
                "Probabilities and Quantiles (bounds inclusive at whole numbers; quantile: smallest x with \
                 P(X ≤ x) ≥ p)"
                    .to_owned()
            } else {
                "Probabilities and Quantiles".to_owned()
            },
            SectionContent::Table(probs),
        ));
        out.push(self.plot());
        out
    }

    /// Density curve (or mass sticks) with the region marked by hatching
    /// (continuous) or markers on the sticks (discrete).
    fn plot(&self) -> ReportSection {
        let d = &self.distribution;
        let region = self.request.region;
        let values = self.request.values;
        let (lo_s, hi_s) = d.support();
        let mut lo = d.quantile(0.001).unwrap_or(lo_s);
        let mut hi = d.quantile(0.999).unwrap_or(hi_s);
        // Show the bounds when they are near the bulk of the distribution.
        let width = (hi - lo).max(1e-12);
        for v in [Some(values.0), region.two_sided().then_some(values.1)].into_iter().flatten() {
            if v.is_finite() && v > lo - width && v < hi + width {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        let mut segments = Vec::new();
        let mut items = Vec::new();
        let mut line = None;
        let (title, groups) = if self.request.family.is_discrete() {
            let (start, end) = (lo.floor().max(lo_s), hi.ceil().min(hi_s));
            let step = ((end - start) / 400.0).ceil().max(1.0);
            let mut x = start;
            while x <= end {
                let m = d.density(x);
                segments.push(vec![[x, 0.0], [x, m]]);
                if in_region(region, values, x) {
                    items.push((x, m, String::new(), 0));
                }
                x += step;
            }
            (format!("Probability Mass (marker: values in {})", self.region_text()), vec!["In region".to_owned()])
        } else {
            let lo_c = lo.max(lo_s);
            let hi_c = hi.min(hi_s);
            let n = 400;
            let curve: Vec<[f64; 2]> = (0..=n)
                .map(|i| {
                    let x = lo_c + (hi_c - lo_c) * i as f64 / n as f64;
                    [x, d.density(x)]
                })
                .filter(|p| p[1].is_finite())
                .collect();
            for (i, p) in curve.iter().enumerate() {
                if i % 4 == 0 && in_region(region, values, p[0]) {
                    segments.push(vec![[p[0], 0.0], *p]);
                }
            }
            line = Some(curve);
            (format!("Density (hatched: {})", self.region_text()), Vec::new())
        };
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: "x".to_owned(),
            y_label: if self.request.family.is_discrete() { "P(X = x)".to_owned() } else { "Density".to_owned() },
            points: Vec::new(),
            line,
            line_name: Some("Density".to_owned()),
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: (!items.is_empty()).then(|| Annotations { groups, items, labels: Vec::new() }),
            segments,
            ..PlotSpec::default()
        };
        section(SectionKind::CalculatorPlot, title, SectionContent::Plot(Box::new(spec)))
    }
}

/// A column of `n` seeded random draws from `family` with `params`: the same
/// seed always gives the same values.
///
/// # Errors
/// Invalid parameters.
pub fn random_column(name: &str, family: Family, params: &[f64], n: usize, seed: u64) -> Result<Column> {
    let d = Distribution::new(family, params)?;
    Ok(Column::numeric(name.to_owned(), d.draws(n, seed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_of(s: &[ReportSection], kind: SectionKind) -> &TextTable {
        match &s.iter().find(|x| x.kind == kind).unwrap().content {
            SectionContent::Table(t) => t,
            _ => panic!(),
        }
    }

    /// Region probabilities against SciPy 1.18 (`norm.cdf`, `binom.cdf`):
    /// Φ(1.96) − Φ(−1.96); Binomial(10, 0.3): P(2 ≤ X ≤ 4) = F(4) − F(1)
    /// and P(X ≥ 2) = 1 − F(1).
    #[test]
    fn region_probabilities() {
        let normal = CalculatorRequest {
            region: ProbabilityRegion::Between,
            values: (-1.96, 1.96),
            ..CalculatorRequest::new(Family::Normal)
        };
        let r = CalculatorReport::run(&normal).unwrap();
        assert!((r.probability - 0.950_004_209_703_559).abs() < 1e-12);
        let out =
            CalculatorReport::run(&CalculatorRequest { region: ProbabilityRegion::Outside, ..normal.clone() }).unwrap();
        assert!((out.probability - 0.049_995_790_296_440_87).abs() < 1e-12);
        let binom = CalculatorRequest {
            family: Family::Binomial,
            params: vec![10.0, 0.3],
            region: ProbabilityRegion::Between,
            values: (2.0, 4.0),
            probability: 0.05,
        };
        let r = CalculatorReport::run(&binom).unwrap();
        let reference = BINOM_F4 - BINOM_F1;
        assert!((r.probability - reference).abs() < 1e-12, "{}", r.probability);
        let above =
            CalculatorReport::run(&CalculatorRequest { region: ProbabilityRegion::Above, ..binom.clone() }).unwrap();
        assert!((above.probability - (1.0 - BINOM_F1)).abs() < 1e-12);
        let s = r.sections();
        assert_eq!(table_of(&s, SectionKind::CalculatorDistribution).rows[0][0], "Trials n");
        assert!(s.iter().any(|x| x.kind == SectionKind::CalculatorPlot));
        assert!(CalculatorReport::run(&CalculatorRequest { values: (3.0, 1.0), ..binom.clone() }).is_err());
        assert_eq!(CalculatorRequest::from_json(&binom.to_json()).unwrap(), binom);
    }

    /// SciPy 1.18: `binom.cdf(4, 10, 0.3)` and `binom.cdf(1, 10, 0.3)`.
    const BINOM_F4: f64 = 0.849_731_667_400_000_1;
    const BINOM_F1: f64 = 0.149_308_345_899_999_92;

    /// Every family runs with its defaults, and random columns are seeded.
    #[test]
    fn every_family_and_random_columns() {
        for f in Family::ALL {
            let r = CalculatorReport::run(&CalculatorRequest::new(f)).unwrap_or_else(|e| panic!("{f:?}: {e}"));
            assert!((0.0..=1.0).contains(&r.probability), "{f:?}");
            assert_eq!(r.sections().len(), 3);
        }
        let a = random_column("x", Family::Gamma, &[2.0, 3.0], 50, 7).unwrap();
        let b = random_column("x", Family::Gamma, &[2.0, 3.0], 50, 7).unwrap();
        assert_eq!(a.as_numeric(), b.as_numeric());
        assert!(random_column("x", Family::Gamma, &[-1.0, 3.0], 5, 7).is_err());
    }
}
