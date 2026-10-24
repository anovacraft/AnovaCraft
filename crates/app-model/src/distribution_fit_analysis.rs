//! Distribution identification: maximum-likelihood fits of many continuous
//! distributions to one column ([`stat_core::distribution_fit`]), bound to the
//! dataset's row state.
//!
//! The report ranks the fitted families by AIC with each one's
//! Anderson–Darling statistic and p-value (and, for a family with a
//! threshold, the likelihood-ratio test of that threshold), lists the
//! parameter estimates, draws the best fit over a histogram, and gives a
//! quantile–quantile plot (Wilk & Gnanadesikan 1968) for each of the
//! best-fitting families: the ordered values against the fitted
//! distribution's quantiles at the median-rank plotting positions
//! `(i − 0.3)/(n + 0.4)` (Benard & Bos-Levenbach 1953), so a good fit lies
//! along the line `y = x`. Optionally the Johnson and Box–Cox
//! transformations to normality are listed alongside, with the normality
//! test of the transformed values.

use crate::distribution::histogram_bars;
use crate::format::{format_number, format_p_value};
use crate::linear_analysis::normal_quantile_points;
use crate::report::{
    BarChartSpec, BarItem, PlotSpec, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use stat_core::box_cox::{BoxCoxFit, box_cox_fit, box_cox_transform};
use stat_core::distribution_fit::{
    FamilyFit, FitFamily, FittedDistribution, IdentifyOptions, JohnsonTransformation, PValueMethod,
    identify_distribution, johnson_transformation,
};
use stat_core::normality::{NormalityTest, anderson_darling};
use stat_core::{Result, StatError};
use stat_data::{DataTable, MeasurementLevel, RowStateManager};

const DIGITS: usize = 6;

/// What to fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistributionFitRequest {
    /// Continuous column.
    pub column: usize,
    /// Families to fit (default: all fourteen).
    pub families: Vec<FitFamily>,
    /// Also list the Johnson and Box–Cox transformations to normality.
    pub transformations: bool,
    /// Quantile plots for this many of the best families by AIC
    /// (`0`: none).
    pub plots: usize,
}

impl DistributionFitRequest {
    /// Every family, transformations listed, plots of the best four.
    #[must_use]
    pub fn new(column: usize) -> Self {
        Self { column, families: FitFamily::ALL.to_vec(), transformations: true, plots: 4 }
    }

    /// Columns the report reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        vec![self.column]
    }

    /// The request with its column renumbered by `map` (`None` if the column
    /// is gone).
    #[must_use]
    pub fn map_columns(&self, map: impl Fn(usize) -> Option<usize>) -> Option<Self> {
        Some(Self { column: map(self.column)?, ..self.clone() })
    }
}

/// The Box–Cox transformation and the normality test of its transformed values.
#[derive(Debug, Clone)]
struct BoxCox {
    fit: BoxCoxFit,
    normality: NormalityTest,
}

/// A completed identification.
#[derive(Debug, Clone)]
pub struct DistributionFitAnalysis {
    request: DistributionFitRequest,
    name: String,
    /// (row, value) of every value used, in row order.
    data: Vec<(usize, f64)>,
    /// Each family's fit, or why it could not be fitted.
    fits: Vec<(FitFamily, std::result::Result<FamilyFit, String>)>,
    johnson: Option<std::result::Result<JohnsonTransformation, String>>,
    box_cox: Option<std::result::Result<BoxCox, String>>,
    exclusion_revision: u64,
}

impl DistributionFitAnalysis {
    /// Fit the request's families to the column's included, non-missing values.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a missing or non-continuous column
    /// or no families; errors of [`identify_distribution`] for the sample as
    /// a whole (fewer than four values, constant data).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &DistributionFitRequest) -> Result<Self> {
        let col = table
            .column(request.column)
            .ok_or_else(|| StatError::InvalidArgument { message: format!("no column {}", request.column) })?;
        let values = match (col.as_numeric(), col.level) {
            (Some(v), MeasurementLevel::Continuous) => v,
            _ => {
                return Err(StatError::InvalidArgument {
                    message: format!("“{}” is not a continuous column", col.name),
                });
            }
        };
        if request.families.is_empty() {
            return Err(StatError::InvalidArgument { message: "choose at least one distribution".to_owned() });
        }
        let data: Vec<(usize, f64)> = (0..table.n_rows())
            .filter(|&r| values[r].is_finite() && !states.is_excluded(r))
            .map(|r| (r, values[r]))
            .collect();
        let xs: Vec<f64> = data.iter().map(|d| d.1).collect();
        let fits = identify_distribution(&xs, &request.families, &IdentifyOptions::default())?
            .into_iter()
            .map(|(f, r)| (f, r.map_err(|e| crate::clipboard::error_text(&e))))
            .collect();
        let (johnson, box_cox) = if request.transformations {
            let johnson = Some(johnson_transformation(&xs).map_err(|e| crate::clipboard::error_text(&e)));
            let box_cox = Some(if xs.iter().all(|v| *v > 0.0) {
                box_cox_fit(&xs, 0.95)
                    .and_then(|fit| {
                        let t = box_cox_transform(&xs, fit.lambda)?;
                        Ok(BoxCox { fit, normality: anderson_darling(&t)? })
                    })
                    .map_err(|e| crate::clipboard::error_text(&e))
            } else {
                Err("needs every value to be positive".to_owned())
            });
            (johnson, box_cox)
        } else {
            (None, None)
        };
        Ok(Self {
            request: request.clone(),
            name: col.name.clone(),
            data,
            fits,
            johnson,
            box_cox,
            exclusion_revision: states.exclusion_revision(),
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &DistributionFitRequest {
        &self.request
    }

    /// `true` if row exclusion changed since the fits.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Distribution Identification: {}", self.name)
    }

    /// The successful fits, best (smallest AIC) first.
    #[must_use]
    pub fn ranked(&self) -> Vec<&FamilyFit> {
        let mut ok: Vec<&FamilyFit> = self.fits.iter().filter_map(|(_, r)| r.as_ref().ok()).collect();
        ok.sort_by(|a, b| a.fit.aic().total_cmp(&b.fit.aic()));
        ok
    }

    /// The fit of `family`, if it was requested and succeeded.
    #[must_use]
    pub fn fit(&self, family: FitFamily) -> Option<&FamilyFit> {
        self.fits.iter().find(|(f, _)| *f == family)?.1.as_ref().ok()
    }

    /// The Johnson transformation, when listed and found.
    #[must_use]
    pub fn johnson(&self) -> Option<&JohnsonTransformation> {
        self.johnson.as_ref()?.as_ref().ok()
    }

    /// Report sections: goodness of fit, parameter estimates, the best fit
    /// over a histogram, and quantile plots of the best families.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let ranked = self.ranked();
        let mut out = vec![self.summary(&ranked)];
        out.extend(self.not_fitted());
        out.push(self.parameters(&ranked));
        if let Some(best) = ranked.first() {
            out.push(self.histogram(&best.fit));
        }
        for f in ranked.iter().take(self.request.plots) {
            out.push(self.quantile_plot(&f.fit));
        }
        if self.request.plots > 0 {
            if let Some(Ok(j)) = &self.johnson {
                out.push(self.transformed_plot("Johnson", |x| j.transform(x)));
            }
            if let Some(Ok(BoxCox { fit, .. })) = &self.box_cox {
                let lambda = fit.lambda;
                out.push(self.transformed_plot("Box–Cox", move |x| {
                    box_cox_transform(&[x], lambda).map_or(f64::NAN, |v| v[0])
                }));
            }
        }
        out
    }

    fn summary(&self, ranked: &[&FamilyFit]) -> ReportSection {
        let mut t = TextTable::new([
            "Distribution",
            "Parameters",
            "Anderson–Darling A²",
            "p-value",
            "Threshold test p",
            "AIC",
            "Δ AIC",
        ]);
        let best_aic = ranked.first().map_or(f64::NAN, |f| f.fit.aic());
        for (i, f) in ranked.iter().enumerate() {
            let name = if i == 0 {
                format!("{}  ◀ best AIC", f.fit.family.label())
            } else {
                f.fit.family.label().to_owned()
            };
            t.push([
                name,
                f.fit.family.n_parameters().to_string(),
                format_number(f.anderson_darling.statistic, 4),
                ad_p_text(f),
                f.threshold_test.map_or_else(|| "—".to_owned(), |th| format_p_value(th.p_value)),
                format_number(f.fit.aic(), DIGITS),
                format_number(f.fit.aic() - best_aic, 4),
            ]);
        }
        for (family, r) in &self.fits {
            if r.is_err() {
                t.push([
                    family.label().to_owned(),
                    "not fitted".to_owned(),
                    "—".into(),
                    "—".into(),
                    "—".into(),
                    "—".into(),
                    "—".into(),
                ]);
            }
        }
        let mut transformation_row = |name: &str, test: Option<&NormalityTest>, err: Option<&String>| match (test, err)
        {
            (Some(test), _) => t.push([
                format!("{name} transformation (normality of transformed values)"),
                "—".to_owned(),
                format_number(test.statistic, 4),
                format_p_value(test.p_value),
                "—".to_owned(),
                "—".to_owned(),
                "—".to_owned(),
            ]),
            (None, Some(_)) => t.push([
                format!("{name} transformation"),
                "not found".to_owned(),
                "—".into(),
                "—".into(),
                "—".into(),
                "—".into(),
                "—".into(),
            ]),
            _ => {}
        };
        if let Some(j) = &self.johnson {
            transformation_row("Johnson", j.as_ref().ok().map(|j| &j.normality), j.as_ref().err());
        }
        if let Some(b) = &self.box_cox {
            transformation_row("Box–Cox", b.as_ref().ok().map(|b| &b.normality), b.as_ref().err());
        }
        ReportSection {
            kind: SectionKind::DistributionFitSummary,
            title: format!("Goodness of Fit (n = {})", self.data.len()),
            content: SectionContent::Table(t),
        }
    }

    /// Why each family or transformation could not be fitted, below the
    /// table rather than in its narrow Parameters column (where long reasons
    /// pushed the statistics off screen).
    fn not_fitted(&self) -> Option<ReportSection> {
        let mut lines: Vec<String> =
            self.fits.iter().filter_map(|(f, r)| r.as_ref().err().map(|e| format!("{}: {e}", f.label()))).collect();
        for (name, err) in [
            ("Johnson transformation", self.johnson.as_ref().and_then(|j| j.as_ref().err())),
            ("Box–Cox transformation", self.box_cox.as_ref().and_then(|b| b.as_ref().err())),
        ] {
            if let Some(e) = err {
                lines.push(format!("{name}: {e}"));
            }
        }
        (!lines.is_empty()).then(|| ReportSection {
            kind: SectionKind::DistributionFitSummary,
            title: "Not Fitted".to_owned(),
            content: SectionContent::Note(lines.join("\n")),
        })
    }

    fn parameters(&self, ranked: &[&FamilyFit]) -> ReportSection {
        let mut t = TextTable::new(["Distribution", "Parameter", "Estimate"]);
        for f in ranked {
            for (i, p) in f.fit.parameters().into_iter().enumerate() {
                let name = if i == 0 { f.fit.family.label().to_owned() } else { String::new() };
                t.push([name, p.name.to_owned(), format_number(p.value, DIGITS)]);
            }
        }
        if let Some(Ok(j)) = &self.johnson {
            t.push(["Johnson transformation".to_owned(), "Type".to_owned(), j.family.label().to_owned()]);
            for (name, v) in [("Shape γ", j.gamma), ("Shape η", j.eta), ("Location ε", j.epsilon)] {
                t.push([String::new(), name.to_owned(), format_number(v, DIGITS)]);
            }
            if j.family != stat_core::distribution_fit::JohnsonFamily::Sl {
                t.push([String::new(), "Scale λ".to_owned(), format_number(j.lambda, DIGITS)]);
            }
        }
        if let Some(Ok(BoxCox { fit, .. })) = &self.box_cox {
            t.push(["Box–Cox transformation".to_owned(), "Power λ".to_owned(), format_number(fit.lambda, 4)]);
        }
        ReportSection {
            kind: SectionKind::DistributionFitParameters,
            title: "Parameter Estimates".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn histogram(&self, best: &FittedDistribution) -> ReportSection {
        let bars = histogram_bars(&self.data);
        let curve = density_curve(best, &bars, self.data.len()).map(|c| (format!("{} fit", best.family.label()), c));
        ReportSection {
            kind: SectionKind::DistributionFitHistogram,
            title: format!("Histogram with Best Fit ({})", best.family.label()),
            content: SectionContent::Bars(BarChartSpec {
                x_label: self.name.clone(),
                y_label: "Count".to_owned(),
                bars,
                x_ticks: None,
                curve,
                references: Vec::new(),
                x_time: None,
            }),
        }
    }

    fn sorted(&self) -> Vec<(usize, f64)> {
        let mut sorted = self.data.clone();
        sorted.sort_by(|a, b| a.1.total_cmp(&b.1));
        sorted
    }

    fn quantile_plot(&self, fit: &FittedDistribution) -> ReportSection {
        let sorted = self.sorted();
        let n = sorted.len() as f64;
        let points: Vec<RowPoint> = sorted
            .iter()
            .enumerate()
            .map(|(i, &(row, y))| RowPoint { row, x: fit.quantile((i as f64 + 0.7) / (n + 0.4)), y })
            .filter(|p| p.x.is_finite())
            .collect();
        let line = identity_line(&points);
        ReportSection {
            kind: SectionKind::ProbabilityPlot,
            title: format!("Quantile Plot: {}", fit.family.label()),
            content: SectionContent::Plot(Box::new(PlotSpec {
                x_label: format!("{} quantile (fitted)", fit.family.label()),
                y_label: self.name.clone(),
                points,
                line,
                line_name: Some("Reference line y = x".to_owned()),
                band: None,
                groups: None,
                x_ticks: None,
                panes: Vec::new(),
                annotations: None,
                x_time: None,
                y_time: None,
                segments: Vec::new(),
                ..PlotSpec::default()
            })),
        }
    }

    fn transformed_plot(&self, name: &str, transform: impl Fn(f64) -> f64) -> ReportSection {
        let mut sorted: Vec<(usize, f64)> =
            self.data.iter().map(|&(r, x)| (r, transform(x))).filter(|d| d.1.is_finite()).collect();
        sorted.sort_by(|a, b| a.1.total_cmp(&b.1));
        let points = normal_quantile_points(&sorted);
        let ys: Vec<f64> = sorted.iter().map(|d| d.1).collect();
        let line = stat_core::descriptive::moments(&ys).ok().and_then(|m| {
            let s = m.std_dev?;
            let (lo, hi) = (points.first()?.x, points.last()?.x);
            Some(vec![[lo, m.mean + s * lo], [hi, m.mean + s * hi]])
        });
        ReportSection {
            kind: SectionKind::ProbabilityPlot,
            title: format!("Normal Quantile Plot: {name}-Transformed {}", self.name),
            content: SectionContent::Plot(Box::new(PlotSpec {
                x_label: "Normal quantile".to_owned(),
                y_label: format!("{name}-transformed {}", self.name),
                points,
                line,
                line_name: Some("Normal reference line".to_owned()),
                band: None,
                groups: None,
                x_ticks: None,
                panes: Vec::new(),
                annotations: None,
                x_time: None,
                y_time: None,
                segments: Vec::new(),
                ..PlotSpec::default()
            })),
        }
    }
}

/// The p-value text, marking a simulated p-value at its smallest attainable
/// value as an upper bound.
fn ad_p_text(f: &FamilyFit) -> String {
    let ad = &f.anderson_darling;
    match ad.method {
        PValueMethod::Simulation { replicates } if ad.p_value <= 1.0 / (replicates as f64 + 1.0) + 1e-15 => {
            format!("< {}", format_p_value(ad.p_value))
        }
        _ => format_p_value(ad.p_value),
    }
}

/// The line `y = x` across the points.
pub(crate) fn identity_line(points: &[RowPoint]) -> Option<Polyline> {
    let lo = points.iter().map(|p| p.x.min(p.y)).fold(f64::INFINITY, f64::min);
    let hi = points.iter().map(|p| p.x.max(p.y)).fold(f64::NEG_INFINITY, f64::max);
    (lo.is_finite() && hi > lo).then(|| vec![[lo, lo], [hi, hi]])
}

/// A fitted density scaled to histogram counts across the bars.
fn density_curve(fit: &FittedDistribution, bars: &[BarItem], n: usize) -> Option<Polyline> {
    let width = bars.first()?.width;
    let lo = bars.first()?.center - width / 2.0;
    let hi = bars.last()?.center + width / 2.0;
    let scale = n as f64 * width;
    const POINTS: usize = 160;
    let curve: Polyline = (0..=POINTS)
        .map(|i| {
            let x = lo + (hi - lo) * i as f64 / POINTS as f64;
            [x, scale * fit.pdf(x)]
        })
        .filter(|p| p[1].is_finite())
        .collect();
    (curve.len() > 1).then_some(curve)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table(values: Vec<f64>) -> DataTable {
        DataTable::new("t", vec![Column::numeric("Strength", values)]).unwrap()
    }

    fn weibull_sample(n: usize) -> Vec<f64> {
        let mut rng = stat_core::rng::Rng::new(77);
        (0..n).map(|_| 20.0 + 8.0 * (-(1.0 - rng.next_f64()).ln()).powf(1.0 / 1.8)).collect()
    }

    #[test]
    fn report_ranks_fits_and_plots_the_best() {
        let t = table(weibull_sample(120));
        let rows = RowStateManager::new(t.n_rows());
        let a = DistributionFitAnalysis::run(&t, &rows, &DistributionFitRequest::new(0)).unwrap();
        assert_eq!(a.title(), "Distribution Identification: Strength");
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            &kinds[..3],
            &[
                SectionKind::DistributionFitSummary,
                SectionKind::DistributionFitParameters,
                SectionKind::DistributionFitHistogram
            ]
        );
        // Four quantile plots of the best families plus the two transformations.
        assert_eq!(kinds.iter().filter(|k| **k == SectionKind::ProbabilityPlot).count(), 6);
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        assert!(summary.rows[0][0].ends_with("◀ best AIC"));
        assert_eq!(summary.rows.len(), FitFamily::ALL.len() + 2);
        // Ranked by AIC.
        let ranked = a.ranked();
        assert!(ranked.windows(2).all(|w| w[0].fit.aic() <= w[1].fit.aic()));
        // The generating family (with or without threshold) is among the best three.
        assert!(
            ranked.iter().take(3).any(|f| matches!(f.fit.family, FitFamily::Weibull | FitFamily::Weibull3)),
            "{:?}",
            ranked.iter().map(|f| f.fit.family).collect::<Vec<_>>()
        );
        // Quantile plot points lie near y = x for the best fit, and carry rows.
        let SectionContent::Plot(p) = &s[3].content else { panic!() };
        assert_eq!(p.points.len(), 120);
        let worst = p.points.iter().map(|q| (q.x - q.y).abs()).fold(0.0, f64::max);
        assert!(worst < 4.0, "{worst}");
        // The histogram curve has about the bars' area (n × bin width).
        let SectionContent::Bars(h) = &s[2].content else { panic!() };
        let (_, curve) = h.curve.as_ref().unwrap();
        let area: f64 = curve.windows(2).map(|w| (w[1][0] - w[0][0]) * (w[0][1] + w[1][1]) / 2.0).sum();
        assert!((area / (120.0 * h.bars[0].width) - 1.0).abs() < 0.05, "{area}");
    }

    #[test]
    fn exclusions_negative_data_and_options() {
        let mut values = weibull_sample(60);
        for v in values.iter_mut().take(5) {
            *v -= 40.0;
        }
        let t = table(values);
        let mut rows = RowStateManager::new(t.n_rows());
        let request = DistributionFitRequest {
            families: vec![FitFamily::Normal, FitFamily::Weibull, FitFamily::Weibull3],
            plots: 0,
            ..DistributionFitRequest::new(0)
        };
        let a = DistributionFitAnalysis::run(&t, &rows, &request).unwrap();
        assert!(a.fit(FitFamily::Weibull).is_none(), "negative values rule out the two-parameter Weibull");
        assert!(a.fit(FitFamily::Normal).is_some());
        let s = a.sections();
        assert!(s.iter().all(|x| x.kind != SectionKind::ProbabilityPlot));
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        assert!(summary.rows.iter().any(|r| r[0] == "Weibull" && r[1].starts_with("not fitted")));
        assert!(summary.rows.iter().any(|r| r[0] == "Box–Cox transformation" && r[1].starts_with("not found")));
        // The reasons sit in a note after the table, in plain words: long
        // reasons in the table pushed its statistics off screen.
        assert!(summary.rows.iter().all(|r| r[1].len() <= 12), "short cells: {:?}", summary.rows);
        let SectionContent::Note(why) = &s[1].content else { panic!("a note of what was not fitted") };
        assert!(why.starts_with("Weibull: ") && !why.contains("invalid argument"), "{why}");
        assert!(why.contains("Box–Cox transformation: "), "{why}");
        // Excluding the shifted rows makes the two-parameter Weibull fit, and marks the report stale.
        rows.set_excluded([0, 1, 2, 3, 4], true);
        assert!(a.is_stale(&rows));
        let b = DistributionFitAnalysis::run(&t, &rows, &request).unwrap();
        assert!(b.fit(FitFamily::Weibull).is_some());
        // Without transformations their rows are left out.
        let c = DistributionFitAnalysis::run(&t, &rows, &DistributionFitRequest { transformations: false, ..request })
            .unwrap();
        let SectionContent::Table(summary) = &c.sections()[0].content else { panic!() };
        assert_eq!(summary.rows.len(), 3);
        assert!(c.johnson().is_none());
    }

    #[test]
    fn rejects_unsuitable_columns() {
        let t = DataTable::new(
            "t",
            vec![
                Column::numeric("Strength", vec![1.0, 2.0, 3.0, 4.0, 5.0]),
                Column::text("Name", ["a", "b", "c", "d", "e"].map(String::from).to_vec()),
            ],
        )
        .unwrap();
        let rows = RowStateManager::new(5);
        assert!(DistributionFitAnalysis::run(&t, &rows, &DistributionFitRequest::new(1)).is_err());
        assert!(DistributionFitAnalysis::run(&t, &rows, &DistributionFitRequest::new(7)).is_err());
        let none = DistributionFitRequest { families: Vec::new(), ..DistributionFitRequest::new(0) };
        assert!(DistributionFitAnalysis::run(&t, &rows, &none).is_err());
        let constant = table(vec![2.0; 6]);
        assert!(
            DistributionFitAnalysis::run(&constant, &RowStateManager::new(6), &DistributionFitRequest::new(0)).is_err()
        );
    }
}
