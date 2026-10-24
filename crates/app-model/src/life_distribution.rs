//! Life distributions bound to a dataset ([`stat_reliability::censored_fit`]
//! and [`stat_reliability::life_data`]): maximum-likelihood fits of the
//! chosen families to exact, right-, left- and interval-censored failure
//! times, compared by log-likelihood, AIC, BIC and a censoring-adjusted
//! Anderson–Darling distance, with likelihood-ratio tests of nested forms;
//! Turnbull's nonparametric estimate; for one family (chosen, or the best by
//! AIC) a probability plot with a confidence band, parameter estimates with
//! Wald and likelihood-ratio intervals, percentiles and failure
//! probabilities, survival and hazard plots; and optionally the Weibull with
//! a known shape for data with few or no failures.
//!
//! **Layouts:** one time column with an optional censor column (a censored
//! row is still running at its time), or start and end columns of the
//! interval in which each unit failed: an empty start means the unit had
//! failed by the end (left-censored), an empty end that it was still running
//! at the start (right-censored), equal values an exact failure. An optional
//! frequency column counts units per row; an optional group column fits each
//! group separately. Excluded rows and rows without usable values are left
//! out.

use crate::format::{format_number, format_p_value};
use crate::report::{
    PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, SurvivalCurveSpec,
    SurvivalPlotSpec, TextTable,
};
use crate::survival::StatusCoding;
use stat_core::distribution_fit::FitFamily;
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_linear::natural_order;
use stat_reliability::{
    CensorKind, CensoredFit, KnownShapeWeibull, LifeData, LifeObservation, NESTED_FAMILIES, NestedComparison,
    NonparametricEstimate, censored_anderson_darling, fit_censored, nested_comparison, turnbull, weibull_known_shape,
};

const DIGITS: usize = 5;
const CURVE_POINTS: usize = 160;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

/// Where the failure times come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifeDataLayout {
    /// One time column; an optional numeric censor column marks units still
    /// running at their time (right-censored).
    Times {
        /// Time of failure or censoring.
        time: usize,
        /// Optional censor column.
        censor: Option<usize>,
        /// Which censor values mean "censored".
        coding: StatusCoding,
    },
    /// Start and end of the interval in which each unit failed.
    Intervals {
        /// Start (empty: failed by the end).
        start: usize,
        /// End (empty: still running at the start).
        end: usize,
    },
}

/// Columns and settings of a life-distribution analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct LifeDistributionRequest {
    /// Data layout.
    pub layout: LifeDataLayout,
    /// Optional frequency column (units per row).
    pub frequency: Option<usize>,
    /// Optional group column (a fit per group).
    pub group: Option<usize>,
    /// Families to fit and compare.
    pub families: Vec<FitFamily>,
    /// Family shown in detail (`None`: the best by AIC).
    pub detail: Option<FitFamily>,
    /// Confidence level of every interval.
    pub confidence: f64,
    /// Probabilities `p` whose quantiles (`t_p`) are estimated.
    pub probabilities: Vec<f64>,
    /// Times `t` whose failure probabilities `F(t)` are estimated.
    pub times: Vec<f64>,
    /// Also fit a Weibull with this known shape.
    pub known_shape: Option<f64>,
}

impl LifeDistributionRequest {
    /// Default confidence level.
    pub const DEFAULT_CONFIDENCE: f64 = 0.95;

    /// The families fitted by default.
    pub const DEFAULT_FAMILIES: [FitFamily; 6] = [
        FitFamily::Exponential,
        FitFamily::Weibull,
        FitFamily::Lognormal,
        FitFamily::LogLogistic,
        FitFamily::Gamma,
        FitFamily::Weibull3,
    ];

    /// The default percentiles: 1%, 10% and 50%.
    pub const DEFAULT_PROBABILITIES: [f64; 3] = [0.01, 0.1, 0.5];

    /// Failure times in `time` with default settings.
    #[must_use]
    pub fn new(layout: LifeDataLayout) -> Self {
        Self {
            layout,
            frequency: None,
            group: None,
            families: Self::DEFAULT_FAMILIES.to_vec(),
            detail: None,
            confidence: Self::DEFAULT_CONFIDENCE,
            probabilities: Self::DEFAULT_PROBABILITIES.to_vec(),
            times: Vec::new(),
            known_shape: None,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = match self.layout {
            LifeDataLayout::Times { time, censor, .. } => std::iter::once(time).chain(censor).collect(),
            LifeDataLayout::Intervals { start, end } => vec![start, end],
        };
        out.extend(self.frequency);
        out.extend(self.group);
        out
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        match &mut self.layout {
            LifeDataLayout::Times { time, censor, .. } => {
                *time = f(*time);
                *censor = censor.map(f);
            }
            LifeDataLayout::Intervals { start, end } => {
                *start = f(*start);
                *end = f(*end);
            }
        }
        self.frequency = self.frequency.map(f);
        self.group = self.group.map(f);
    }
}

/// One family's fit to one group.
#[derive(Debug, Clone)]
pub struct FamilyResult {
    /// The family.
    pub family: FitFamily,
    /// The fit, or why it failed.
    pub fit: std::result::Result<CensoredFit, String>,
    /// Censoring-adjusted Anderson–Darling distance (NaN when not fitted).
    pub anderson_darling: f64,
}

/// One group's data and fits.
#[derive(Debug, Clone)]
pub struct LifeGroup {
    /// Group label (empty when ungrouped).
    pub label: String,
    /// `(row, observation)` of each analysed row.
    pub rows: Vec<(usize, LifeObservation)>,
    /// The observations.
    pub data: LifeData,
    /// Turnbull's estimate.
    pub nonparametric: NonparametricEstimate,
    /// Fits in the order requested.
    pub fits: Vec<FamilyResult>,
    /// Likelihood-ratio tests of nested forms that were both fitted.
    pub nested: Vec<NestedComparison>,
    /// Weibull with a known shape, or why it could not be fitted.
    pub known_shape: Option<std::result::Result<KnownShapeWeibull, String>>,
}

impl LifeGroup {
    /// The fit of `family`, when it succeeded.
    #[must_use]
    pub fn fit(&self, family: FitFamily) -> Option<&CensoredFit> {
        self.fits.iter().find(|f| f.family == family).and_then(|f| f.fit.as_ref().ok())
    }
}

/// A computed life-distribution analysis.
#[derive(Debug, Clone)]
pub struct LifeDistributionAnalysis {
    request: LifeDistributionRequest,
    time_name: String,
    group_name: Option<String>,
    groups: Vec<LifeGroup>,
    detail: Option<FitFamily>,
    n_skipped: usize,
}

/// A cell's number, NaN when missing or not numeric text.
fn value(table: &DataTable, col: usize, row: usize) -> f64 {
    table.column(col).and_then(|c| c.as_numeric()).map_or(f64::NAN, |v| v[row])
}

impl LifeDistributionAnalysis {
    /// Fit the requested families to the included rows.
    ///
    /// # Errors
    /// Missing or non-numeric columns, a column used twice, no families, a
    /// confidence level outside (0.5, 1), or no usable rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &LifeDistributionRequest) -> Result<Self> {
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("choose a different column for each role"));
        }
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        for &c in &cols {
            let col = column(c)?;
            if Some(c) != request.group && col.as_numeric().is_none() {
                return Err(invalid(format!("{} must be numeric", col.name)));
            }
        }
        if request.families.is_empty() {
            return Err(invalid("choose at least one distribution"));
        }
        if !(request.confidence > 0.5 && request.confidence < 1.0) {
            return Err(invalid("the confidence level must be between 0.5 and 1"));
        }
        let time_name = match request.layout {
            LifeDataLayout::Times { time, .. } => column(time)?.name.clone(),
            LifeDataLayout::Intervals { start, end } => format!("{}–{}", column(start)?.name, column(end)?.name),
        };
        let group_col = request.group.map(column).transpose()?;
        let mut labels: Vec<String> = Vec::new();
        let mut members: Vec<Vec<(usize, LifeObservation)>> = Vec::new();
        let mut used = 0;
        for r in 0..table.n_rows() {
            if states.is_excluded(r) {
                continue;
            }
            let weight = request.frequency.map_or(1.0, |c| value(table, c, r));
            if !(weight.is_finite() && weight > 0.0) {
                continue;
            }
            let obs = match request.layout {
                LifeDataLayout::Times { time, censor, coding } => {
                    let t = value(table, time, r);
                    if !t.is_finite() {
                        continue;
                    }
                    let censored = match censor {
                        Some(c) => {
                            let s = value(table, c, r);
                            if !s.is_finite() {
                                continue;
                            }
                            (s != 0.0) == (coding == StatusCoding::NonzeroIsCensored)
                        }
                        None => false,
                    };
                    if censored { LifeObservation::right(t, weight) } else { LifeObservation::exact(t, weight) }
                }
                LifeDataLayout::Intervals { start, end } => {
                    let (a, b) = (value(table, start, r), value(table, end, r));
                    let (lo, hi) = (
                        if a.is_finite() { a } else { f64::NEG_INFINITY },
                        if b.is_finite() { b } else { f64::INFINITY },
                    );
                    if !lo.is_finite() && !hi.is_finite() || lo > hi {
                        continue;
                    }
                    LifeObservation::interval(lo, hi, weight)
                }
            };
            let label = group_col.map(|c| c.level_text(r)).unwrap_or_default();
            if group_col.is_some() && label.is_empty() {
                continue;
            }
            let g = labels.iter().position(|l| *l == label).unwrap_or_else(|| {
                labels.push(label);
                members.push(Vec::new());
                labels.len() - 1
            });
            members[g].push((r, obs));
            used += 1;
        }
        if used == 0 {
            return Err(invalid("no rows with usable failure or censoring times"));
        }
        let mut order: Vec<usize> = (0..labels.len()).collect();
        order.sort_by(|&a, &b| natural_order(&labels[a], &labels[b]));
        let level = request.confidence;
        let groups = order
            .into_iter()
            .map(|g| {
                let rows = std::mem::take(&mut members[g]);
                let name = if labels[g].is_empty() { String::new() } else { format!("{}: ", labels[g]) };
                let data =
                    LifeData::new(rows.iter().map(|r| r.1).collect()).map_err(|e| invalid(format!("{name}{e}")))?;
                let nonparametric = turnbull(&data).map_err(|e| invalid(format!("{name}{e}")))?;
                let fits: Vec<FamilyResult> = request
                    .families
                    .iter()
                    .map(|&family| {
                        let fit = fit_censored(&data, family).map_err(|e| e.to_string());
                        let anderson_darling = fit
                            .as_ref()
                            .map_or(f64::NAN, |f| censored_anderson_darling(&nonparametric, &f.distribution));
                        FamilyResult { family, fit, anderson_darling }
                    })
                    .collect();
                let fitted = |f: FitFamily| fits.iter().find(|r| r.family == f).and_then(|r| r.fit.as_ref().ok());
                let nested = NESTED_FAMILIES
                    .iter()
                    .filter_map(|&(a, b)| nested_comparison(fitted(a)?, fitted(b)?).ok())
                    .collect();
                let known_shape =
                    request.known_shape.map(|beta| weibull_known_shape(&data, beta, level).map_err(|e| e.to_string()));
                Ok(LifeGroup { label: labels[g].clone(), rows, data, nonparametric, fits, nested, known_shape })
            })
            .collect::<Result<Vec<_>>>()?;
        let detail = request.detail.or_else(|| {
            // The family with the smallest total AIC over the groups where every group was fitted.
            request
                .families
                .iter()
                .filter_map(|&f| {
                    let total: Option<f64> = groups.iter().map(|g| g.fit(f).map(CensoredFit::aic)).sum();
                    total.filter(|t| t.is_finite()).map(|t| (f, t))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|x| x.0)
        });
        Ok(Self {
            request: request.clone(),
            time_name,
            group_name: group_col.map(|c| c.name.clone()),
            groups,
            detail,
            n_skipped: table.n_rows() - used,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &LifeDistributionRequest {
        &self.request
    }

    /// The groups (one when ungrouped), in natural order.
    #[must_use]
    pub fn groups(&self) -> &[LifeGroup] {
        &self.groups
    }

    /// The family shown in detail (`None` when no family could be fitted).
    #[must_use]
    pub fn detail(&self) -> Option<FitFamily> {
        self.detail
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.group_name {
            Some(g) => format!("Life Distributions: {} by {g}", self.time_name),
            None => format!("Life Distributions: {}", self.time_name),
        }
    }

    fn grouped(&self) -> bool {
        self.group_name.is_some()
    }

    fn level(&self) -> String {
        format!("{}%", format_number(self.request.confidence * 100.0, 4))
    }

    fn headings(&self, rest: &[&str]) -> Vec<String> {
        let mut h = Vec::new();
        if let Some(g) = &self.group_name {
            h.push(g.clone());
        }
        h.extend(rest.iter().map(|s| (*s).to_owned()));
        h
    }

    fn with_group(&self, g: &LifeGroup, cells: impl IntoIterator<Item = String>) -> Vec<String> {
        let mut row = Vec::new();
        if self.grouped() {
            row.push(g.label.clone());
        }
        row.extend(cells);
        row
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.comparison()];
        if self.groups.iter().any(|g| !g.nested.is_empty()) {
            out.push(self.nested());
        }
        if let Some(family) = self.detail {
            let fitted: Vec<(&LifeGroup, &CensoredFit)> =
                self.groups.iter().filter_map(|g| g.fit(family).map(|f| (g, f))).collect();
            if !fitted.is_empty() {
                // The gamma's probability scale depends on its fitted shape:
                // one plot per group.
                if matches!(family, FitFamily::Gamma | FitFamily::Gamma3) && fitted.len() > 1 {
                    for pair in &fitted {
                        out.push(self.probability_plot(family, std::slice::from_ref(pair)));
                    }
                } else {
                    out.push(self.probability_plot(family, &fitted));
                }
                out.push(self.parameters(family, &fitted));
                if !self.request.probabilities.is_empty() {
                    out.push(self.percentiles(family, &fitted));
                }
                if !self.request.times.is_empty() {
                    out.push(self.probabilities(family, &fitted));
                }
                out.push(self.survival_plot(family, &fitted));
                out.push(self.hazard_plot(family, &fitted));
            }
        }
        out.push(self.nonparametric_table());
        if self.request.known_shape.is_some() {
            out.push(self.known_shape());
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(self.headings(&[
            "Units",
            "Exact failures",
            "Right-censored",
            "Left-censored",
            "Interval-censored",
        ]));
        for g in &self.groups {
            t.push(self.with_group(
                g,
                [
                    num(g.data.total_weight()),
                    num(g.data.weight_of(CensorKind::Exact)),
                    num(g.data.weight_of(CensorKind::Right)),
                    num(g.data.weight_of(CensorKind::Left)),
                    num(g.data.weight_of(CensorKind::Interval)),
                ],
            ));
        }
        let skipped = if self.n_skipped > 0 {
            format!("; {} rows excluded or incomplete", self.n_skipped)
        } else {
            String::new()
        };
        section(SectionKind::LifeDataSummary, format!("Life Data Summary{skipped}"), SectionContent::Table(t))
    }

    fn comparison(&self) -> ReportSection {
        let mut t = TextTable::new(self.headings(&[
            "Distribution",
            "Parameters",
            "Log-likelihood",
            "AIC",
            "BIC",
            "Anderson–Darling (adjusted)",
        ]));
        for g in &self.groups {
            let mut rows: Vec<&FamilyResult> = g.fits.iter().collect();
            rows.sort_by(|a, b| match (&a.fit, &b.fit) {
                (Ok(x), Ok(y)) => x.aic().total_cmp(&y.aic()),
                (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            });
            for r in rows {
                let name = if Some(r.family) == self.detail {
                    format!("{} (shown in detail)", r.family.label())
                } else {
                    r.family.label().to_owned()
                };
                match &r.fit {
                    Ok(f) => t.push(self.with_group(
                        g,
                        [
                            name,
                            r.family.n_parameters().to_string(),
                            num(f.log_likelihood()),
                            num(f.aic()),
                            num(f.bic()),
                            format_number(r.anderson_darling, 4),
                        ],
                    )),
                    Err(e) => t.push(self.with_group(
                        g,
                        [
                            name,
                            r.family.n_parameters().to_string(),
                            e.clone(),
                            String::new(),
                            String::new(),
                            String::new(),
                        ],
                    )),
                }
            }
        }
        section(
            SectionKind::LifeModelComparison,
            "Model Comparison (smaller AIC, BIC and Anderson–Darling are better; ordered by AIC)",
            SectionContent::Table(t),
        )
    }

    fn nested(&self) -> ReportSection {
        let mut t = TextTable::new(self.headings(&["Simpler model", "Within", "Likelihood-ratio χ²", "DF", "p-value"]));
        for g in &self.groups {
            for c in &g.nested {
                t.push(self.with_group(
                    g,
                    [
                        c.reduced.label().to_owned(),
                        c.full.label().to_owned(),
                        format_number(c.statistic, 4),
                        c.df.to_string(),
                        format_p_value(c.p_value),
                    ],
                ));
            }
        }
        section(
            SectionKind::LifeNestedTests,
            "Nested Models (a small p-value favours the larger model)",
            SectionContent::Table(t),
        )
    }

    fn probability_plot(&self, family: FitFamily, fitted: &[(&LifeGroup, &CensoredFit)]) -> ReportSection {
        let log_x = matches!(
            family,
            FitFamily::Weibull
                | FitFamily::Weibull3
                | FitFamily::Lognormal
                | FitFamily::Lognormal3
                | FitFamily::LogLogistic
                | FitFamily::LogLogistic3
        );
        let level = self.request.confidence;
        let x_of = |fit: &CensoredFit, t: f64| -> f64 {
            if log_x {
                let u = t - fit.distribution.threshold;
                if u > 0.0 { u.ln() } else { f64::NAN }
            } else {
                t
            }
        };
        let (mut points, mut membership, mut lines) = (Vec::new(), Vec::new(), Vec::new());
        let mut band: Option<(Polyline, Polyline)> = None;
        let mut fitted_line = None;
        let (mut y_lo, mut y_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut t_lo, mut t_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (k, (g, fit)) in fitted.iter().enumerate() {
            let d = &fit.distribution;
            let cum = g.nonparametric.cumulative();
            // Plotting position at a time: the midpoint of the estimate's
            // jump there, or the estimate itself.
            let position = |t: f64| -> Option<f64> {
                for (j, iv) in g.nonparametric.intervals.iter().enumerate() {
                    if iv.upper == t && iv.probability > 0.0 {
                        let before = if j == 0 { 0.0 } else { cum[j - 1] };
                        return Some(0.5 * (before + cum[j]));
                    }
                }
                g.nonparametric.cdf_at(t)
            };
            for &(row, o) in &g.rows {
                if !o.upper.is_finite() {
                    continue;
                }
                let Some(p) = position(o.upper).filter(|p| *p > 0.0 && *p < 1.0) else { continue };
                let (x, y) = (x_of(fit, o.upper), d.standard_quantile(p));
                if x.is_finite() && y.is_finite() {
                    points.push(RowPoint { row, x, y });
                    membership.push(k);
                    (y_lo, y_hi) = (y_lo.min(y), y_hi.max(y));
                    (t_lo, t_hi) = (t_lo.min(o.upper), t_hi.max(o.upper));
                }
            }
        }
        if !t_lo.is_finite() {
            (t_lo, t_hi) = (0.0, 1.0);
        }
        for (g, fit) in fitted {
            let d = &fit.distribution;
            let lb = d.lower_bound();
            let span = (t_hi - t_lo).max(1e-9 * t_hi.abs()).max(1e-12);
            let (a, b) = if log_x {
                let a = (t_lo - lb).max(1e-12);
                (lb + a / 1.5, lb + (t_hi - lb) * 1.5)
            } else {
                (t_lo - 0.1 * span, t_hi + 0.1 * span)
            };
            let grid: Vec<f64> = (0..=CURVE_POINTS)
                .map(|i| {
                    let f = i as f64 / CURVE_POINTS as f64;
                    if log_x {
                        lb + ((a - lb).ln() + f * ((b - lb).ln() - (a - lb).ln())).exp()
                    } else {
                        a + f * (b - a)
                    }
                })
                .filter(|&t| t > lb)
                .collect();
            let line: Polyline = grid
                .iter()
                .map(|&t| [x_of(fit, t), d.standardize(t)])
                .filter(|p| p[0].is_finite() && p[1].is_finite())
                .collect();
            if fitted.len() == 1 {
                let mut lower = Vec::new();
                let mut upper = Vec::new();
                for &t in &grid {
                    let (lo, hi) = fit.probability_band(t, level);
                    let x = x_of(fit, t);
                    let (ylo, yhi) = (d.standard_quantile(lo), d.standard_quantile(hi));
                    if x.is_finite() && ylo.is_finite() && yhi.is_finite() {
                        lower.push([x, ylo]);
                        upper.push([x, yhi]);
                    }
                }
                band = Some((lower, upper));
                fitted_line = Some(line);
            } else {
                lines.push(line);
            }
            let _ = g;
        }
        // Probability ticks over the plotted range.
        let d0 = &fitted[0].1.distribution;
        let (y_lo, y_hi) = if y_lo.is_finite() { (y_lo, y_hi) } else { (-2.0, 2.0) };
        let pad = 0.15 * (y_hi - y_lo).max(1.0);
        let y_ticks: Vec<(f64, String)> = [
            0.0001, 0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.3, 0.5, 0.7, 0.8, 0.9, 0.95, 0.98, 0.99,
            0.995, 0.999, 0.9999,
        ]
        .iter()
        .map(|&p| (d0.standard_quantile(p), p))
        .filter(|(y, _)| *y >= y_lo - pad && *y <= y_hi + pad)
        .map(|(y, p)| (y, format!("{}%", format_number(p * 100.0, 3))))
        .collect();
        let x_ticks = log_x.then(|| {
            let lb = if family.has_threshold() && fitted.len() == 1 { d0.threshold } else { 0.0 };
            log_ticks(t_lo - lb, t_hi - lb).into_iter().map(|v| (v.ln(), format_number(v, 4))).collect()
        });
        let shifted = family.has_threshold() && log_x;
        let x_label = match (log_x, shifted) {
            (true, true) => format!("{} − threshold (log scale)", self.time_name),
            (true, false) => format!("{} (log scale)", self.time_name),
            _ => self.time_name.clone(),
        };
        let groups = (fitted.len() > 1).then(|| PointGroups {
            title: self.group_name.clone().unwrap_or_default(),
            labels: fitted.iter().map(|(g, _)| g.label.clone()).collect(),
            membership: membership.clone(),
            lines,
        });
        let heading = if fitted.len() == 1 && self.grouped() {
            format!("{} Probability Plot ({})", family.label(), fitted[0].0.label)
        } else {
            format!("{} Probability Plot", family.label())
        };
        let spec = PlotSpec {
            x_label,
            y_label: "Fraction failing".to_owned(),
            points,
            line: fitted_line,
            line_name: Some(format!("{} fit", family.label())),
            band_name: Some(format!("Pointwise {} limits", self.level())),
            band,
            point_name: Some("Nonparametric estimate".to_owned()),
            groups,
            x_ticks,
            y_ticks: Some(y_ticks),
            ..PlotSpec::default()
        };
        section(
            SectionKind::LifeProbabilityPlot,
            format!(
                "{heading} (points: Turnbull estimate at each failure or inspection time; line: fit; dashed: pointwise {} limits)",
                self.level()
            ),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn interval_headings(&self, first: &[&str]) -> Vec<String> {
        let level = self.level();
        let mut cols: Vec<String> = first.iter().map(|s| (*s).to_owned()).collect();
        cols.extend([
            "Std error".to_owned(),
            format!("Wald lower {level}"),
            format!("Wald upper {level}"),
            format!("Likelihood-ratio lower {level}"),
            format!("Likelihood-ratio upper {level}"),
        ]);
        let refs: Vec<&str> = cols.iter().map(String::as_str).collect();
        self.headings(&refs)
    }

    fn parameters(&self, family: FitFamily, fitted: &[(&LifeGroup, &CensoredFit)]) -> ReportSection {
        let mut t = TextTable::new(self.interval_headings(&["Parameter", "Estimate"]));
        let level = self.request.confidence;
        for (g, fit) in fitted {
            for p in fit.parameters(level, true) {
                let lr = p.likelihood_ratio.unwrap_or((f64::NAN, f64::NAN));
                t.push(self.with_group(
                    g,
                    [
                        p.name.to_owned(),
                        num(p.estimate),
                        format_number(p.std_error, 4),
                        num(p.wald.0),
                        num(p.wald.1),
                        num(lr.0),
                        num(lr.1),
                    ],
                ));
            }
        }
        section(
            SectionKind::LifeParameters,
            format!("{} Parameter Estimates (maximum likelihood)", family.label()),
            SectionContent::Table(t),
        )
    }

    fn percentiles(&self, family: FitFamily, fitted: &[(&LifeGroup, &CensoredFit)]) -> ReportSection {
        let mut t = TextTable::new(self.interval_headings(&["Fraction failing", "Time"]));
        let level = self.request.confidence;
        for (g, fit) in fitted {
            for &p in &self.request.probabilities {
                let q = fit.quantile(p, level, true);
                let lr = q.likelihood_ratio.unwrap_or((f64::NAN, f64::NAN));
                t.push(self.with_group(
                    g,
                    [
                        format!("{}%", format_number(p * 100.0, 4)),
                        num(q.estimate),
                        format_number(q.std_error, 4),
                        num(q.wald.0),
                        num(q.wald.1),
                        num(lr.0),
                        num(lr.1),
                    ],
                ));
            }
        }
        section(
            SectionKind::LifePercentiles,
            format!("{} Percentiles (time by which a fraction of units fail)", family.label()),
            SectionContent::Table(t),
        )
    }

    fn probabilities(&self, family: FitFamily, fitted: &[(&LifeGroup, &CensoredFit)]) -> ReportSection {
        let mut t = TextTable::new(self.interval_headings(&["Time", "Failure probability"]));
        let level = self.request.confidence;
        for (g, fit) in fitted {
            for &time in &self.request.times {
                let f = fit.failure_probability(time, level, true);
                let lr = f.likelihood_ratio.unwrap_or((f64::NAN, f64::NAN));
                t.push(self.with_group(
                    g,
                    [
                        num(time),
                        num(f.estimate),
                        format_number(f.std_error, 4),
                        num(f.wald.0),
                        num(f.wald.1),
                        num(lr.0),
                        num(lr.1),
                    ],
                ));
            }
        }
        section(
            SectionKind::LifeProbabilities,
            format!("{} Failure Probabilities (reliability = 1 − failure probability)", family.label()),
            SectionContent::Table(t),
        )
    }

    /// Plot range: from the smaller of zero and the earliest finite time to the latest.
    fn time_range(&self) -> (f64, f64) {
        let (lo, hi) = self
            .groups
            .iter()
            .map(|g| g.data.finite_range())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (c, d)| (a.min(c), b.max(d)));
        (lo.min(0.0), hi)
    }

    fn survival_plot(&self, family: FitFamily, fitted: &[(&LifeGroup, &CensoredFit)]) -> ReportSection {
        let (start, end) = self.time_range();
        let (mut points, mut censored, mut membership, mut curves) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (k, g) in self.groups.iter().enumerate() {
            let surv = |t: f64| g.nonparametric.cdf_at(t).map(|f| 1.0 - f);
            for &(row, o) in &g.rows {
                let t = match o.kind() {
                    CensorKind::Exact | CensorKind::Right => o.lower,
                    CensorKind::Left | CensorKind::Interval => o.upper,
                };
                if let Some(s) = surv(t) {
                    points.push(RowPoint { row, x: t, y: s });
                    censored.push(o.kind() != CensorKind::Exact);
                    membership.push(k);
                }
            }
            let mut fit_curves = Vec::new();
            if let Some((_, fit)) = fitted.iter().find(|(h, _)| std::ptr::eq(*h, g)) {
                let d = &fit.distribution;
                let line = (0..=CURVE_POINTS)
                    .map(|i| start + (end - start) * i as f64 / CURVE_POINTS as f64)
                    .map(|t| [t, d.ln_sf(t).exp()])
                    .filter(|p| p[1].is_finite())
                    .collect();
                fit_curves.push((format!("{} fit", family.label()), crate::report::LinePattern::Dashed, line));
            }
            curves.push(SurvivalCurveSpec {
                label: g.label.clone(),
                steps: g.nonparametric.survival_polyline(start),
                band: Vec::new(),
                fitted: fit_curves,
            });
        }
        section(
            SectionKind::LifeSurvivalPlot,
            format!(
                "Survival (steps: Turnbull estimate, straight across inspection intervals; dashed: {} fit; filled: failure, hollow: censored)",
                family.label()
            ),
            SectionContent::Survival(Box::new(SurvivalPlotSpec {
                x_label: self.time_name.clone(),
                y_label: "Surviving fraction".to_owned(),
                curves,
                points,
                censored,
                membership,
                estimate: None,
                risk_table: None,
            })),
        )
    }

    fn hazard_plot(&self, family: FitFamily, fitted: &[(&LifeGroup, &CensoredFit)]) -> ReportSection {
        let (start, end) = self.time_range();
        let level = self.request.confidence;
        let curves = fitted
            .iter()
            .map(|(g, fit)| {
                let lb = fit.distribution.lower_bound();
                let a = if lb.is_finite() { lb.max(start) + 0.005 * (end - lb.max(start)) } else { start };
                let grid: Vec<f64> =
                    (0..=CURVE_POINTS).map(|i| a + (end - a) * i as f64 / CURVE_POINTS as f64).collect();
                let steps: Polyline = grid.iter().map(|&t| [t, fit.hazard(t)]).filter(|p| p[1].is_finite()).collect();
                let band = grid
                    .windows(2)
                    .filter_map(|w| {
                        let m = 0.5 * (w[0] + w[1]);
                        let (lo, hi) = fit.hazard_band(m, level);
                        (lo.is_finite() && hi.is_finite()).then_some([w[0], w[1], lo, hi])
                    })
                    .collect();
                SurvivalCurveSpec { label: g.label.clone(), steps, band, fitted: Vec::new() }
            })
            .collect();
        section(
            SectionKind::LifeHazardPlot,
            format!(
                "{} Hazard (failures per unit time among survivors; shaded: pointwise {} limits)",
                family.label(),
                self.level()
            ),
            SectionContent::Survival(Box::new(SurvivalPlotSpec {
                x_label: self.time_name.clone(),
                y_label: "Hazard rate".to_owned(),
                curves,
                points: Vec::new(),
                censored: Vec::new(),
                membership: Vec::new(),
                estimate: Some(format!("{} hazard", family.label())),
                risk_table: None,
            })),
        )
    }

    fn nonparametric_table(&self) -> ReportSection {
        let mut t =
            TextTable::new(self.headings(&["From", "To", "Probability", "Fraction failed after", "Surviving after"]));
        for g in &self.groups {
            let cum = g.nonparametric.cumulative();
            for (iv, c) in g.nonparametric.intervals.iter().zip(cum) {
                if iv.probability <= 0.0 {
                    continue;
                }
                t.push(self.with_group(
                    g,
                    [
                        num(iv.lower),
                        num(iv.upper),
                        format_number(iv.probability, 4),
                        format_number(c, 4),
                        format_number(1.0 - c, 4),
                    ],
                ));
            }
        }
        section(
            SectionKind::LifeNonparametric,
            "Turnbull Nonparametric Estimate (probability of failing in each interval; equals Kaplan–Meier for exact and right-censored data)",
            SectionContent::Table(t),
        )
    }

    fn known_shape(&self) -> ReportSection {
        let level = self.level();
        let mut t = TextTable::new(self.headings(&[
            "Quantity",
            "Estimate",
            &format!("Lower {level}"),
            &format!("Upper {level}"),
            &format!("One-sided lower bound {level}"),
        ]));
        let beta = self.request.known_shape.unwrap_or(f64::NAN);
        for g in &self.groups {
            match &g.known_shape {
                Some(Ok(k)) => {
                    t.push(self.with_group(
                        g,
                        ["Failures".into(), num(k.failures), String::new(), String::new(), String::new()],
                    ));
                    t.push(self.with_group(
                        g,
                        [
                            "Scale α".into(),
                            num(k.scale),
                            num(k.scale_interval.0),
                            num(k.scale_interval.1),
                            num(k.scale_lower_bound),
                        ],
                    ));
                    for &p in &self.request.probabilities {
                        t.push(self.with_group(
                            g,
                            [
                                format!("Time to {}% failing", format_number(p * 100.0, 4)),
                                num(k.quantile(p)),
                                String::new(),
                                String::new(),
                                num(k.quantile_lower_bound(p)),
                            ],
                        ));
                    }
                    for &time in &self.request.times {
                        t.push(self.with_group(
                            g,
                            [
                                format!("Reliability at {}", num(time)),
                                num(k.reliability(time)),
                                String::new(),
                                String::new(),
                                num(k.reliability_lower_bound(time)),
                            ],
                        ));
                    }
                }
                Some(Err(e)) => {
                    t.push(self.with_group(g, [e.clone(), String::new(), String::new(), String::new(), String::new()]))
                }
                None => {}
            }
        }
        section(
            SectionKind::LifeKnownShape,
            format!(
                "Weibull with Known Shape β = {} (Nelson 1985 chi-square limits; usable with few or no failures)",
                format_number(beta, 4)
            ),
            SectionContent::Table(t),
        )
    }
}

/// Values `{1, 2, 5} × 10ᵏ` covering `[lo, hi]` (positive), at least two.
fn log_ticks(lo: f64, hi: f64) -> Vec<f64> {
    let lo = lo.max(1e-300);
    let hi = hi.max(lo * 1.0001);
    let mut out = Vec::new();
    let start = lo.log10().floor() as i32 - 1;
    let stop = hi.log10().ceil() as i32 + 1;
    let decades = stop - start;
    let mantissas: &[f64] = if decades > 4 {
        &[1.0]
    } else if decades > 2 {
        &[1.0, 2.0, 5.0]
    } else {
        &[1.0, 1.5, 2.0, 3.0, 5.0, 7.0]
    };
    for k in start..=stop {
        for &m in mantissas {
            let v = m * 10f64.powi(k);
            if v >= lo / 1.6 && v <= hi * 1.6 {
                out.push(v);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::inspection_life_data;

    fn col(t: &DataTable, name: &str) -> usize {
        t.find(name).unwrap()
    }

    fn interval_request(t: &DataTable) -> LifeDistributionRequest {
        LifeDistributionRequest {
            frequency: Some(col(t, "Units")),
            times: vec![1000.0],
            ..LifeDistributionRequest::new(LifeDataLayout::Intervals {
                start: col(t, "Last pass (h)"),
                end: col(t, "Found failed (h)"),
            })
        }
    }

    #[test]
    fn sample_report_has_every_section() {
        let t = inspection_life_data();
        let rows = RowStateManager::new(t.n_rows());
        let a = LifeDistributionAnalysis::run(&t, &rows, &interval_request(&t)).unwrap();
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        for k in [
            SectionKind::LifeDataSummary,
            SectionKind::LifeModelComparison,
            SectionKind::LifeNestedTests,
            SectionKind::LifeProbabilityPlot,
            SectionKind::LifeParameters,
            SectionKind::LifePercentiles,
            SectionKind::LifeProbabilities,
            SectionKind::LifeSurvivalPlot,
            SectionKind::LifeHazardPlot,
            SectionKind::LifeNonparametric,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing");
        }
        let g = &a.groups()[0];
        assert!(g.data.weight_of(CensorKind::Interval) > 0.0 && g.data.weight_of(CensorKind::Right) > 0.0);
        // The detail family is the best by AIC.
        let best =
            g.fits.iter().filter_map(|f| f.fit.as_ref().ok()).min_by(|x, y| x.aic().total_cmp(&y.aic())).unwrap();
        assert_eq!(a.detail(), Some(best.family()));
        // Points of the probability plot are rows with an upper end.
        let s = a.sections();
        let plot = s.iter().find(|s| s.kind == SectionKind::LifeProbabilityPlot).unwrap();
        let SectionContent::Plot(p) = &plot.content else { panic!() };
        assert!(!p.points.is_empty() && p.band.is_some() && p.y_ticks.is_some());
    }

    #[test]
    fn grouped_known_shape_and_errors() {
        let t = inspection_life_data();
        let rows = RowStateManager::new(t.n_rows());
        let mut r = interval_request(&t);
        r.group = Some(col(&t, "Supplier"));
        r.detail = Some(FitFamily::Weibull);
        r.known_shape = Some(2.0);
        let a = LifeDistributionAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(a.groups().len(), 2);
        let s = a.sections();
        let known = s.iter().find(|s| s.kind == SectionKind::LifeKnownShape).unwrap();
        // Interval data cannot use the closed form: each group says why.
        let SectionContent::Table(k) = &known.content else { panic!() };
        assert!(k.rows.iter().all(|row| row[1].contains("right-censored")), "{:?}", k.rows);
        let same = LifeDistributionRequest { frequency: Some(col(&t, "Last pass (h)")), ..r.clone() };
        assert!(LifeDistributionAnalysis::run(&t, &rows, &same).is_err());
        let none = LifeDistributionRequest { families: Vec::new(), ..r };
        assert!(LifeDistributionAnalysis::run(&t, &rows, &none).is_err());
    }

    #[test]
    fn time_layout_with_censor_column() {
        let t = DataTable::new(
            "t",
            vec![
                stat_data::Column::numeric("Hours", vec![120.0, 190.0, 260.0, 300.0, 410.0, 500.0, 500.0, 500.0]),
                stat_data::Column::numeric("Censored", vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
            ],
        )
        .unwrap();
        let rows = RowStateManager::new(t.n_rows());
        let r = LifeDistributionRequest {
            known_shape: Some(1.5),
            families: vec![FitFamily::Weibull, FitFamily::Exponential],
            ..LifeDistributionRequest::new(LifeDataLayout::Times {
                time: 0,
                censor: Some(1),
                coding: StatusCoding::NonzeroIsCensored,
            })
        };
        let a = LifeDistributionAnalysis::run(&t, &rows, &r).unwrap();
        let g = &a.groups()[0];
        assert_eq!(g.data.weight_of(CensorKind::Right), 3.0);
        let k = g.known_shape.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(k.failures, 5.0);
        // Exact and right-censored data: Turnbull is Kaplan–Meier.
        let times: Vec<f64> = t.column(0).unwrap().as_numeric().unwrap().to_vec();
        let events: Vec<bool> = t.column(1).unwrap().as_numeric().unwrap().iter().map(|&c| c == 0.0).collect();
        let km = stat_reliability::kaplan_meier(&times, &events).unwrap();
        for s in &km.steps {
            let f = g.nonparametric.cdf_at(s.time).unwrap();
            assert!((1.0 - f - s.survival).abs() < 1e-9, "{} {}", 1.0 - f, s.survival);
        }
    }
}
