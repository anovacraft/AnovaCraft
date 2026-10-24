//! Survival (time-to-event) analysis bound to a dataset
//! ([`stat_reliability`]): Kaplan–Meier curves with Greenwood confidence
//! bands, median survival, a log-rank comparison of groups, and Weibull and
//! lognormal maximum-likelihood fits drawn over the nonparametric estimate.
//!
//! Each analysed row is drawn on its group's curve (filled marker for an
//! event, hollow for a censored observation), so rows can be brushed from the
//! plot. Excluded rows are left out of the estimates.

use crate::format::{format_number, format_p_value};
use crate::report::{
    LinePattern, ReportSection, RiskTable, RowPoint, SectionContent, SectionKind, SurvivalCurveSpec, SurvivalPlotSpec,
    TextTable,
};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_linear::natural_order;
use stat_reliability::{
    BandTransform, KaplanMeier, LifeDistribution, LogRankTest, ParametricFit, fit_life_distribution, kaplan_meier,
    log_rank_test,
};

const DIGITS: usize = 5;
/// Confidence level of every interval in the report.
pub const SURVIVAL_LEVEL: f64 = 0.95;
/// Points per fitted curve.
const CURVE_POINTS: usize = 160;

/// How the status column marks censoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusCoding {
    /// Non-zero status = censored (still running); zero = event.
    #[default]
    NonzeroIsCensored,
    /// Non-zero status = event (failure); zero = censored.
    NonzeroIsEvent,
}

/// Columns of a survival analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct SurvivalRequest {
    /// Time to event or censoring (numeric, non-negative).
    pub time: usize,
    /// Optional numeric status column; without one every time is an event.
    pub status: Option<usize>,
    /// Meaning of non-zero status values.
    pub coding: StatusCoding,
    /// Optional grouping column: one curve per level and a log-rank test.
    pub group: Option<usize>,
}

/// Display options of a survival report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurvivalOptions {
    /// Shade the pointwise confidence band of each curve.
    pub show_band: bool,
    /// Transformation used for confidence bands and median limits.
    pub band: BandTransform,
    /// Fit and draw the Weibull distribution.
    pub show_weibull: bool,
    /// Fit and draw the lognormal distribution.
    pub show_lognormal: bool,
    /// Show the full product-limit table.
    pub show_table: bool,
    /// Add the log-rank test for trend across the groups in order (scores
    /// are the group labels when all are numbers, else 1, 2, 3, …).
    pub trend: bool,
    /// Show the number at risk under the curves (and as a table).
    pub risk_table: bool,
    /// Also count the rows censored between consecutive times.
    pub risk_censored: bool,
}

impl Default for SurvivalOptions {
    fn default() -> Self {
        Self {
            show_band: true,
            band: BandTransform::LogLog,
            show_weibull: true,
            show_lognormal: false,
            show_table: false,
            trend: false,
            risk_table: false,
            risk_censored: false,
        }
    }
}

/// One group's data and estimates.
#[derive(Debug, Clone)]
pub struct SurvivalGroup {
    /// Group label (empty when ungrouped).
    pub label: String,
    /// `(row, time, event)` of each analysed row.
    pub rows: Vec<(usize, f64, bool)>,
    /// Kaplan–Meier estimate.
    pub km: KaplanMeier,
    /// Weibull fit, or why it failed.
    pub weibull: std::result::Result<ParametricFit, String>,
    /// Lognormal fit, or why it failed.
    pub lognormal: std::result::Result<ParametricFit, String>,
}

/// A survival analysis on the included rows.
#[derive(Debug, Clone)]
pub struct SurvivalAnalysis {
    request: SurvivalRequest,
    time_name: String,
    status_name: Option<String>,
    group_name: Option<String>,
    groups: Vec<SurvivalGroup>,
    log_rank: Option<std::result::Result<LogRankTest, String>>,
    n_skipped: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn fit(rows: &[(usize, f64, bool)], d: LifeDistribution) -> std::result::Result<ParametricFit, String> {
    let (t, e): (Vec<f64>, Vec<bool>) = rows.iter().map(|r| (r.1, r.2)).unzip();
    fit_life_distribution(&t, &e, d).map_err(|err| err.to_string())
}

impl SurvivalAnalysis {
    /// Estimate survival from the included rows with a finite, non-negative
    /// time (and a status and group value when those columns are used).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for missing or non-numeric time/status
    /// columns, a column used twice, or no usable rows; errors from
    /// [`kaplan_meier()`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SurvivalRequest) -> Result<Self> {
        let column = |i: usize| table.column(i).ok_or_else(|| invalid("no such column"));
        let time_col = column(request.time)?;
        let times = time_col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", time_col.name)))?;
        let status = match request.status {
            Some(i) => {
                let c = column(i)?;
                if i == request.time {
                    return Err(invalid("the status column must differ from the time column"));
                }
                Some((c.name.clone(), c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?))
            }
            None => None,
        };
        let group_col = match request.group {
            Some(i) if i == request.time || Some(i) == request.status => {
                return Err(invalid("the grouping column must differ from the time and status columns"));
            }
            Some(i) => Some(column(i)?),
            None => None,
        };
        let mut labels: Vec<String> = Vec::new();
        let mut members: Vec<Vec<(usize, f64, bool)>> = Vec::new();
        let mut used = 0;
        for r in 0..table.n_rows() {
            let t = times[r];
            if states.is_excluded(r) || !t.is_finite() || t < 0.0 {
                continue;
            }
            let event = match &status {
                Some((_, s)) if !s[r].is_finite() => continue,
                Some((_, s)) => (s[r] != 0.0) == (request.coding == StatusCoding::NonzeroIsEvent),
                None => true,
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
            members[g].push((r, t, event));
            used += 1;
        }
        if used == 0 {
            return Err(invalid("no rows with a usable time (finite and non-negative)"));
        }
        let mut order: Vec<usize> = (0..labels.len()).collect();
        order.sort_by(|&a, &b| natural_order(&labels[a], &labels[b]));
        let groups = order
            .into_iter()
            .map(|g| {
                let rows = std::mem::take(&mut members[g]);
                let (t, e): (Vec<f64>, Vec<bool>) = rows.iter().map(|r| (r.1, r.2)).unzip();
                Ok(SurvivalGroup {
                    label: labels[g].clone(),
                    km: kaplan_meier(&t, &e)?,
                    weibull: fit(&rows, LifeDistribution::Weibull),
                    lognormal: fit(&rows, LifeDistribution::Lognormal),
                    rows,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let log_rank = (groups.len() >= 2).then(|| {
            let mut t = Vec::new();
            let mut e = Vec::new();
            let mut g = Vec::new();
            for (k, group) in groups.iter().enumerate() {
                for &(_, time, event) in &group.rows {
                    t.push(time);
                    e.push(event);
                    g.push(k);
                }
            }
            log_rank_test(&t, &e, &g, groups.len()).map_err(|err| err.to_string())
        });
        Ok(Self {
            request: request.clone(),
            time_name: time_col.name.clone(),
            status_name: status.map(|s| s.0),
            group_name: group_col.map(|c| c.name.clone()),
            groups,
            log_rank,
            n_skipped: table.n_rows() - used,
        })
    }

    /// The groups (one when ungrouped), in natural order of their labels.
    #[must_use]
    pub fn groups(&self) -> &[SurvivalGroup] {
        &self.groups
    }

    /// The log-rank test, when there are two or more groups.
    #[must_use]
    pub fn log_rank(&self) -> Option<&std::result::Result<LogRankTest, String>> {
        self.log_rank.as_ref()
    }

    /// The request this analysis ran.
    #[must_use]
    pub fn request(&self) -> &SurvivalRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.group_name {
            Some(g) => format!("Survival: {} by {g}", self.time_name),
            None => format!("Survival: {}", self.time_name),
        }
    }

    fn grouped(&self) -> bool {
        self.group_name.is_some()
    }

    fn group_heading(&self) -> String {
        self.group_name.clone().unwrap_or_default()
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self, options: &SurvivalOptions) -> Vec<ReportSection> {
        let mut out = vec![self.summary(options)];
        if let Some(test) = &self.log_rank {
            out.push(log_rank_section(test, self, options.trend));
        }
        out.push(self.plot(options));
        if options.risk_table {
            out.push(self.risk_section(options));
        }
        let fits =
            [(options.show_weibull, LifeDistribution::Weibull), (options.show_lognormal, LifeDistribution::Lognormal)];
        for (shown, d) in fits {
            if shown {
                out.push(self.fit_section(d));
            }
        }
        if options.show_weibull && options.show_lognormal {
            out.push(self.comparison());
        }
        if options.show_table {
            out.push(self.product_limit_table(options));
        }
        out
    }

    fn summary(&self, options: &SurvivalOptions) -> ReportSection {
        let mut columns: Vec<String> = Vec::new();
        if self.grouped() {
            columns.push(self.group_heading());
        }
        columns.extend(
            ["N", "Events", "Censored", "Median survival", "Lower 95%", "Upper 95%", "Mean survival (restricted)"]
                .map(str::to_owned),
        );
        let mut table = TextTable::new(columns);
        for g in &self.groups {
            let km = &g.km;
            let (lo, hi) = km.quantile_confidence_interval(0.5, SURVIVAL_LEVEL, options.band).unwrap_or((None, None));
            let mut row: Vec<String> = Vec::new();
            if self.grouped() {
                row.push(g.label.clone());
            }
            row.extend([
                km.n().to_string(),
                km.n_events().to_string(),
                km.n_censored().to_string(),
                km.median().map_or_else(|| "not reached".to_owned(), |m| format_number(m, DIGITS)),
                lo.map_or_else(|| "—".to_owned(), |v| format_number(v, DIGITS)),
                hi.map_or_else(|| "not reached".to_owned(), |v| format_number(v, DIGITS)),
                format_number(restricted_mean(km), DIGITS),
            ]);
            table.push(row);
        }
        let skipped = if self.n_skipped > 0 {
            format!("; {} rows excluded or incomplete", self.n_skipped)
        } else {
            String::new()
        };
        let status = match (&self.status_name, self.request.coding) {
            (Some(s), StatusCoding::NonzeroIsCensored) => format!("{s} ≠ 0 marks censoring"),
            (Some(s), StatusCoding::NonzeroIsEvent) => format!("{s} ≠ 0 marks an event"),
            (None, _) => "no status column: every time is an event".to_owned(),
        };
        ReportSection {
            kind: SectionKind::SurvivalSummary,
            title: format!(
                "Summary ({status}; median limits by Brookmeyer–Crowley, {} band{skipped})",
                options.band.label()
            ),
            content: SectionContent::Table(table),
        }
    }

    fn plot(&self, options: &SurvivalOptions) -> ReportSection {
        let mut points = Vec::new();
        let mut censored = Vec::new();
        let mut membership = Vec::new();
        let mut curves = Vec::new();
        for (k, g) in self.groups.iter().enumerate() {
            for &(row, t, event) in &g.rows {
                points.push(RowPoint { row, x: t, y: g.km.survival_at(t) });
                censored.push(!event);
                membership.push(k);
            }
            let band = if options.show_band {
                g.km.band_pieces(SURVIVAL_LEVEL, options.band).unwrap_or_default()
            } else {
                Vec::new()
            };
            let end = g.km.last_time();
            let mut fitted = Vec::new();
            let shown = [
                (options.show_weibull, &g.weibull, LinePattern::Dashed),
                (options.show_lognormal, &g.lognormal, LinePattern::Dotted),
            ];
            for (on, f, pattern) in shown {
                if let (true, Ok(f)) = (on, f) {
                    let curve = (0..=CURVE_POINTS)
                        .map(|i| {
                            let t = end * i as f64 / CURVE_POINTS as f64;
                            [t, f.survival(t)]
                        })
                        .collect();
                    fitted.push((format!("{} fit", f.distribution.label()), pattern, curve));
                }
            }
            curves.push(SurvivalCurveSpec { label: g.label.clone(), steps: g.km.step_polyline(0.0), band, fitted });
        }
        let band_note = if options.show_band {
            format!("; shaded: pointwise 95% band ({})", options.band.label())
        } else {
            String::new()
        };
        ReportSection {
            kind: SectionKind::SurvivalCurves,
            title: format!("Survival Curves (steps: Kaplan–Meier; filled marker: event, hollow: censored{band_note})"),
            content: SectionContent::Survival(Box::new(SurvivalPlotSpec {
                x_label: self.time_name.clone(),
                y_label: "Survival probability".to_owned(),
                curves,
                points,
                censored,
                membership,
                estimate: None,
                risk_table: self.risk_table(options),
            })),
        }
    }

    /// The rows of each group behind the number-at-risk table under the plot.
    fn risk_table(&self, options: &SurvivalOptions) -> Option<RiskTable> {
        options.risk_table.then(|| RiskTable {
            censored: options.risk_censored,
            exits: self
                .groups
                .iter()
                .map(|g| {
                    let mut e: Vec<(f64, bool)> = g.rows.iter().map(|&(_, t, event)| (t, !event)).collect();
                    e.sort_by(|a, b| a.0.total_cmp(&b.0));
                    e
                })
                .collect(),
        })
    }

    /// Round times from 0 to the largest time, as the plot's axis ticks them.
    fn risk_times(&self) -> Vec<(f64, String)> {
        let last = self.groups.iter().map(|g| g.km.last_time()).fold(0.0, f64::max);
        if last > 0.0 { stat_viz::export::nice_tick_labels(0.0, last, 8) } else { vec![(0.0, "0".to_owned())] }
    }

    /// Number at risk (rows with a time at or after each round time) and,
    /// optionally, the number censored since the previous time.
    fn risk_section(&self, options: &SurvivalOptions) -> ReportSection {
        let times = self.risk_times();
        let mut columns: Vec<String> = Vec::new();
        if self.grouped() {
            columns.push(self.group_heading());
        }
        columns.push("Count".to_owned());
        columns.extend(times.iter().map(|t| t.1.clone()));
        let mut table = TextTable::new(columns);
        for g in &self.groups {
            let mut push = |what: &str, cells: Vec<String>| {
                let mut row: Vec<String> = Vec::new();
                if self.grouped() {
                    row.push(g.label.clone());
                }
                row.push(what.to_owned());
                row.extend(cells);
                table.push(row);
            };
            push("At risk", times.iter().map(|&(t, _)| g.km.at_risk_at(t).to_string()).collect());
            if options.risk_censored {
                let cells = times
                    .iter()
                    .enumerate()
                    .map(|(k, &(t, _))| {
                        let after = if k == 0 { f64::NEG_INFINITY } else { times[k - 1].0 };
                        g.km.censored_between(after, t).to_string()
                    })
                    .collect();
                push("Censored", cells);
            }
        }
        let censored = if options.risk_censored { "; censored: since the previous time" } else { "" };
        ReportSection {
            kind: SectionKind::NumberAtRisk,
            title: format!("Number at Risk (rows with {} at or after each time{censored})", self.time_name),
            content: SectionContent::Table(table),
        }
    }

    /// Scores of the groups for the trend test: their labels when every
    /// label is a number, otherwise 1, 2, 3, … in the groups' order.
    #[must_use]
    pub fn trend_scores(&self) -> Vec<f64> {
        let numeric: Option<Vec<f64>> = self.groups.iter().map(|g| g.label.trim().parse::<f64>().ok()).collect();
        numeric
            .filter(|v| v.iter().all(|x| x.is_finite()))
            .unwrap_or_else(|| (1..=self.groups.len()).map(|k| k as f64).collect())
    }

    fn fit_section(&self, d: LifeDistribution) -> ReportSection {
        let mut columns: Vec<String> = Vec::new();
        if self.grouped() {
            columns.push(self.group_heading());
        }
        columns.extend(["Parameter", "Estimate", "Std error", "Lower 95%", "Upper 95%"].map(str::to_owned));
        let mut table = TextTable::new(columns);
        for g in &self.groups {
            let mut push = |cells: [String; 5]| {
                let mut row: Vec<String> = Vec::new();
                if self.grouped() {
                    row.push(g.label.clone());
                }
                row.extend(cells);
                table.push(row);
            };
            let f = match d {
                LifeDistribution::Weibull => &g.weibull,
                LifeDistribution::Lognormal => &g.lognormal,
            };
            match f {
                Err(e) => push(["Not available".to_owned(), e.clone(), String::new(), String::new(), String::new()]),
                Ok(f) => {
                    for p in f.parameters(SURVIVAL_LEVEL).unwrap_or_default() {
                        push([
                            p.name.to_owned(),
                            format_number(p.estimate, DIGITS),
                            format_number(p.std_error, 4),
                            format_number(p.lower, DIGITS),
                            format_number(p.upper, DIGITS),
                        ]);
                    }
                    let (lo, hi) = f.quantile_confidence_interval(0.5, SURVIVAL_LEVEL).unwrap_or((f64::NAN, f64::NAN));
                    push([
                        "Median life".to_owned(),
                        format_number(f.median(), DIGITS),
                        String::new(),
                        format_number(lo, DIGITS),
                        format_number(hi, DIGITS),
                    ]);
                }
            }
        }
        let (kind, title) = match d {
            LifeDistribution::Weibull => {
                (SectionKind::WeibullFit, "Weibull Fit (S(t) = exp(−(t/α)^β); β > 1: failure rate rises with age)")
            }
            LifeDistribution::Lognormal => (SectionKind::LognormalFit, "Lognormal Fit (log time ~ Normal(μ, σ))"),
        };
        ReportSection { kind, title: title.to_owned(), content: SectionContent::Table(table) }
    }

    fn comparison(&self) -> ReportSection {
        let mut columns: Vec<String> = Vec::new();
        if self.grouped() {
            columns.push(self.group_heading());
        }
        columns.extend(["Distribution", "Log-likelihood", "AIC", "BIC"].map(str::to_owned));
        let mut table = TextTable::new(columns);
        for g in &self.groups {
            for f in [&g.weibull, &g.lognormal].into_iter().flatten() {
                let mut row: Vec<String> = Vec::new();
                if self.grouped() {
                    row.push(g.label.clone());
                }
                row.extend([
                    f.distribution.label().to_owned(),
                    format_number(f.log_likelihood, 7),
                    format_number(f.aic(), 7),
                    format_number(f.bic(), 7),
                ]);
                table.push(row);
            }
        }
        ReportSection {
            kind: SectionKind::FitComparison,
            title: "Fit Comparison (smaller AIC/BIC: better supported)".to_owned(),
            content: SectionContent::Table(table),
        }
    }

    fn product_limit_table(&self, options: &SurvivalOptions) -> ReportSection {
        let mut columns: Vec<String> = Vec::new();
        if self.grouped() {
            columns.push(self.group_heading());
        }
        columns.extend(
            ["Time", "At risk", "Events", "Censored", "Survival", "Std error", "Lower 95%", "Upper 95%"]
                .map(str::to_owned),
        );
        let mut table = TextTable::new(columns);
        for g in &self.groups {
            for s in &g.km.steps {
                let (lo, hi) = s.confidence_limits(SURVIVAL_LEVEL, options.band).unwrap_or((f64::NAN, f64::NAN));
                let mut row: Vec<String> = Vec::new();
                if self.grouped() {
                    row.push(g.label.clone());
                }
                row.extend([
                    format_number(s.time, DIGITS + 1),
                    s.at_risk.to_string(),
                    s.events.to_string(),
                    s.censored.to_string(),
                    format_number(s.survival, DIGITS),
                    format_number(s.std_error(), 4),
                    format_number(lo, 4),
                    format_number(hi, 4),
                ]);
                table.push(row);
            }
        }
        ReportSection {
            kind: SectionKind::ProductLimitTable,
            title: format!("Product-Limit Estimates (Greenwood standard errors; {} limits)", options.band.label()),
            content: SectionContent::Table(table),
        }
    }
}

/// Area under the Kaplan–Meier curve up to the last observed time (the
/// restricted mean survival time). It equals the mean when the largest time is
/// an event, and underestimates it otherwise.
fn restricted_mean(km: &KaplanMeier) -> f64 {
    let mut area = 0.0;
    let (mut prev_t, mut s) = (0.0, 1.0);
    for step in km.steps.iter().filter(|s| s.events > 0) {
        area += s * (step.time - prev_t);
        prev_t = step.time;
        s = step.survival;
    }
    area + s * (km.last_time() - prev_t)
}

fn log_rank_section(
    test: &std::result::Result<LogRankTest, String>,
    a: &SurvivalAnalysis,
    trend: bool,
) -> ReportSection {
    let content = match test {
        Err(e) => SectionContent::Note(format!("Not available: {e}")),
        Ok(t) => {
            let mut table = TextTable::new([a.group_heading(), "Observed events".into(), "Expected events".into()]);
            for (k, g) in a.groups.iter().enumerate() {
                table.push([
                    g.label.clone(),
                    format_number(t.observed[k], DIGITS),
                    format_number(t.expected[k], DIGITS),
                ]);
            }
            table.push([
                "Log-rank χ²".to_owned(),
                format_number(t.chi_square, DIGITS),
                format!("DF {}, p = {}", t.df, format_p_value(t.p_value)),
            ]);
            if trend {
                let scores = a.trend_scores();
                let shown: Vec<String> = scores.iter().map(|s| format_number(*s, 4)).collect();
                match t.trend(&scores) {
                    Ok(tr) => table.push([
                        format!("Trend χ² (scores {})", shown.join(", ")),
                        format_number(tr.chi_square, DIGITS),
                        format!(
                            "DF 1, p = {}; U = {} (positive: more events than expected at higher scores)",
                            format_p_value(tr.p_value),
                            format_number(tr.statistic, DIGITS)
                        ),
                    ]),
                    Err(e) => table.push(["Trend χ²".to_owned(), format!("not available: {e}"), String::new()]),
                }
            }
            SectionContent::Table(table)
        }
    };
    ReportSection {
        kind: SectionKind::LogRank,
        title: if trend {
            "Log-Rank Tests (equal survival in every group; trend across the groups in order)".to_owned()
        } else {
            "Log-Rank Test (equal survival in every group)".to_owned()
        },
        content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// Freireich et al. (1963) remission data with a censoring column.
    fn leukemia() -> DataTable {
        let mp = [6, 6, 6, 6, 7, 9, 10, 10, 11, 13, 16, 17, 19, 20, 22, 23, 25, 32, 32, 34, 35];
        let mp_cens = [0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1];
        let pl = [1, 1, 2, 2, 3, 4, 4, 5, 5, 8, 8, 8, 8, 11, 11, 12, 12, 15, 17, 22, 23];
        let weeks: Vec<f64> = mp.iter().chain(&pl).map(|&v| f64::from(v)).collect();
        let cens: Vec<f64> = mp_cens.iter().chain(&[0; 21]).map(|&v| f64::from(v)).collect();
        let arm: Vec<String> = (0..42).map(|i| if i < 21 { "6-MP" } else { "Placebo" }.to_owned()).collect();
        DataTable::new(
            "leukemia",
            vec![Column::numeric("Weeks", weeks), Column::numeric("Censored", cens), Column::text("Treatment", arm)],
        )
        .unwrap()
    }

    fn request() -> SurvivalRequest {
        SurvivalRequest { time: 0, status: Some(1), coding: StatusCoding::NonzeroIsCensored, group: Some(2) }
    }

    #[test]
    fn grouped_report_matches_published_values() {
        let t = leukemia();
        let a = SurvivalAnalysis::run(&t, &RowStateManager::new(42), &request()).unwrap();
        assert_eq!(a.title(), "Survival: Weeks by Treatment");
        let g = a.groups();
        assert_eq!((g[0].label.as_str(), g[1].label.as_str()), ("6-MP", "Placebo"));
        assert_eq!((g[0].km.median(), g[1].km.median()), (Some(23.0), Some(8.0)));
        let lr = a.log_rank().unwrap().as_ref().unwrap();
        assert!((lr.chi_square - 16.792_940_989_216_54).abs() < 1e-10);
        let w = g[0].weibull.as_ref().unwrap();
        assert!((w.weibull_shape() - 1.353_734_523_827_102_8).abs() < 1e-8);
        let all = SurvivalOptions { show_lognormal: true, show_table: true, ..SurvivalOptions::default() };
        let s = a.sections(&all);
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::SurvivalSummary,
                SectionKind::LogRank,
                SectionKind::SurvivalCurves,
                SectionKind::WeibullFit,
                SectionKind::LognormalFit,
                SectionKind::FitComparison,
                SectionKind::ProductLimitTable
            ]
        );
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        assert_eq!(summary.rows[0][..7], ["6-MP", "21", "9", "12", "23", "13", "not reached"]);
        assert_eq!(summary.rows[1][..5], ["Placebo", "21", "21", "0", "8"]);
        let SectionContent::Survival(plot) = &s[2].content else { panic!() };
        assert_eq!(plot.curves.len(), 2);
        assert_eq!(plot.points.len(), 42);
        assert_eq!(plot.censored.iter().filter(|&&c| c).count(), 12);
        assert_eq!(plot.curves[0].band.len(), 7, "one band piece per distinct event time");
        assert_eq!(plot.curves[0].fitted.len(), 2);
        assert_eq!(plot.curves[0].fitted[0].1, LinePattern::Dashed);
        // Row 3 (censored at 6 weeks) sits on the curve after the first drop.
        let p = plot.points.iter().find(|p| p.row == 3).unwrap();
        assert!((p.y - 18.0 / 21.0).abs() < 1e-12);
        let SectionContent::Table(pl) = &s[6].content else { panic!() };
        assert_eq!(pl.rows.len(), 16 + 12, "distinct times: 16 in the 6-MP arm, 12 in placebo");
        // Defaults: no lognormal, comparison or product-limit table.
        assert_eq!(a.sections(&SurvivalOptions::default()).len(), 4);
    }

    #[test]
    fn coding_exclusion_and_ungrouped() {
        let mut t = leukemia();
        // Flip the status meaning: a column of event indicators.
        let events: Vec<f64> = t.column(1).unwrap().as_numeric().unwrap().iter().map(|c| 1.0 - c).collect();
        t.replace_column(1, Column::numeric("Relapsed", events)).unwrap();
        let req = SurvivalRequest { coding: StatusCoding::NonzeroIsEvent, group: None, ..request() };
        let mut rows = RowStateManager::new(42);
        let a = SurvivalAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.groups().len(), 1);
        assert!(a.log_rank().is_none());
        assert_eq!(a.groups()[0].km.n_events(), 30);
        assert!(!a.sections(&SurvivalOptions::default()).iter().any(|s| s.kind == SectionKind::LogRank));
        // Excluding the placebo arm leaves the published 6-MP estimate.
        rows.set_excluded(21..42, true);
        let a = SurvivalAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.groups()[0].km.median(), Some(23.0));
        // Restricted mean of the 6-MP arm: area under the curve to 35 weeks.
        let rm = restricted_mean(&a.groups()[0].km);
        let expected = 6.0
            + (18.0 / 21.0)
            + 0.806_722_689_075_630_3 * 3.0
            + 0.752_941_176_470_588_2 * 3.0
            + 0.690_196_078_431_372_5 * 3.0
            + 0.627_450_980_392_156_9 * 6.0
            + 0.537_815_126_050_420_1
            + 0.448_179_271_708_683_5 * 12.0;
        assert!((rm - expected).abs() < 1e-12, "{rm} vs {expected}");
        // No status column: every time is an event.
        let a = SurvivalAnalysis::run(&t, &rows, &SurvivalRequest { status: None, ..req }).unwrap();
        assert_eq!(a.groups()[0].km.n_events(), 21);
    }

    /// The trend test and the number-at-risk table (engine values are
    /// checked against R in `stat_reliability::kaplan_meier`). With two
    /// groups the trend χ² equals the log-rank χ².
    #[test]
    fn trend_and_number_at_risk() {
        let t = leukemia();
        let a = SurvivalAnalysis::run(&t, &RowStateManager::new(42), &request()).unwrap();
        assert_eq!(a.trend_scores(), [1.0, 2.0]);
        let options =
            SurvivalOptions { trend: true, risk_table: true, risk_censored: true, ..SurvivalOptions::default() };
        let s = a.sections(&options);
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            &kinds[..4],
            [
                SectionKind::SurvivalSummary,
                SectionKind::LogRank,
                SectionKind::SurvivalCurves,
                SectionKind::NumberAtRisk
            ]
        );
        let SectionContent::Table(lr) = &s[1].content else { panic!() };
        assert_eq!(lr.rows[3][0], "Trend χ² (scores 1, 2)");
        assert_eq!(lr.rows[3][1], lr.rows[2][1], "two groups: trend = log-rank");
        // Times 0, 5, …, 35: summary(survfit(Surv(Weeks, 1 − Censored) ~ Treatment), times = seq(0, 35, 5)).
        let SectionContent::Table(risk) = &s[3].content else { panic!() };
        assert_eq!(risk.columns[2..], ["0", "5", "10", "15", "20", "25", "30", "35"].map(str::to_owned));
        assert_eq!(risk.rows[0][2..], ["21", "21", "15", "11", "8", "5", "4", "1"].map(str::to_owned));
        assert_eq!(risk.rows[1][1], "Censored");
        assert_eq!(risk.rows[1][2..], ["0", "0", "3", "1", "3", "1", "0", "4"].map(str::to_owned));
        assert_eq!(risk.rows[2][2..], ["21", "14", "8", "4", "2", "0", "0", "0"].map(str::to_owned));
        let SectionContent::Survival(plot) = &s[2].content else { panic!() };
        let table = plot.risk_table.as_ref().unwrap();
        assert!(table.censored);
        assert_eq!((table.at_risk(0, 10.0), table.at_risk(1, 10.0)), (15, 8));
        assert_eq!(table.censored_between(0, 5.0, 10.0), 3);
        // Off by default: no table under the plot.
        let s = a.sections(&SurvivalOptions::default());
        let SectionContent::Survival(plot) = &s[2].content else { panic!() };
        assert!(plot.risk_table.is_none());
    }

    #[test]
    fn invalid_requests() {
        let t = leukemia();
        let rows = RowStateManager::new(42);
        assert!(SurvivalAnalysis::run(&t, &rows, &SurvivalRequest { time: 2, ..request() }).is_err(), "text time");
        assert!(SurvivalAnalysis::run(&t, &rows, &SurvivalRequest { status: Some(0), ..request() }).is_err());
        assert!(SurvivalAnalysis::run(&t, &rows, &SurvivalRequest { group: Some(0), ..request() }).is_err());
        assert!(SurvivalAnalysis::run(&t, &rows, &SurvivalRequest { time: 9, ..request() }).is_err());
    }
}
