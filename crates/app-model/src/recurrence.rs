//! Repairable systems (recurrent events) bound to a dataset
//! ([`stat_reliability::repairable`]): Nelson's mean cumulative function with
//! confidence limits, per population, the difference between two
//! populations, constant-rate and power-law process fits, and the Laplace
//! and military handbook trend tests.
//!
//! **Layout:** one row per repair, with the system's identifier and its age
//! at the repair. An optional event column marks rows that are not repairs
//! (value 0): such a row gives the system's age at the end of observation.
//! A system without one is taken to be observed up to its last repair. An
//! optional population column splits the systems into groups (each system
//! in one group); with exactly two, their difference is shown.
//!
//! Excluded rows and rows missing a value are left out. Repairs are drawn on
//! the estimate as filled markers and ends of observation as hollow ones,
//! each linked to its row.

use crate::format::{format_number, format_p_value};
use crate::report::{
    LinePattern, PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, SurvivalCurveSpec, SurvivalPlotSpec,
    TextTable,
};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_linear::natural_order;
use stat_reliability::repairable::{
    HppFit, McfDifferenceStep, McfLimits, McfVariance, MeanCumulativeFunction, PowerLawFit, SystemHistory, TrendTest,
    hpp_fit, laplace_test, mcf_difference, mean_cumulative_function, military_handbook_test, power_law_fit,
};

const DIGITS: usize = 5;
const CURVE_POINTS: usize = 120;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

/// Columns and settings of a repairable-systems analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct RecurrenceRequest {
    /// System identifier.
    pub system: usize,
    /// Age at each repair (or at the end of observation).
    pub time: usize,
    /// Optional numeric column: 0 marks an end of observation, any other
    /// value a repair.
    pub event: Option<usize>,
    /// Optional population column.
    pub group: Option<usize>,
    /// Variance estimate of the mean cumulative function.
    pub variance: McfVariance,
    /// How pointwise limits are formed.
    pub limits: McfLimits,
    /// Confidence level.
    pub confidence: f64,
}

impl RecurrenceRequest {
    /// Default confidence level.
    pub const DEFAULT_CONFIDENCE: f64 = 0.95;

    /// Repairs of `system` at ages in `time`, with default settings.
    #[must_use]
    pub fn new(system: usize, time: usize) -> Self {
        Self {
            system,
            time,
            event: None,
            group: None,
            variance: McfVariance::Robust,
            limits: McfLimits::Log,
            confidence: Self::DEFAULT_CONFIDENCE,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        [self.system, self.time].into_iter().chain(self.event).chain(self.group).collect()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.system = f(self.system);
        self.time = f(self.time);
        self.event = self.event.map(f);
        self.group = self.group.map(f);
    }
}

/// One population's systems and estimates.
#[derive(Debug, Clone)]
pub struct RecurrenceGroup {
    /// Population label (empty when ungrouped).
    pub label: String,
    /// The systems.
    pub systems: Vec<SystemHistory>,
    /// `(row, age, repair)` of each analysed row.
    pub rows: Vec<(usize, f64, bool)>,
    /// Nelson's estimate.
    pub mcf: MeanCumulativeFunction,
    /// Constant-rate fit, or why it failed.
    pub hpp: std::result::Result<HppFit, String>,
    /// Power-law fit, or why it failed.
    pub power_law: std::result::Result<PowerLawFit, String>,
    /// Laplace test, or why it failed.
    pub laplace: std::result::Result<TrendTest, String>,
    /// Military handbook test, or why it failed.
    pub military: std::result::Result<TrendTest, String>,
}

/// A computed repairable-systems analysis.
#[derive(Debug, Clone)]
pub struct RecurrenceAnalysis {
    request: RecurrenceRequest,
    time_name: String,
    group_name: Option<String>,
    groups: Vec<RecurrenceGroup>,
    difference: Option<Vec<McfDifferenceStep>>,
    n_skipped: usize,
}

impl RecurrenceAnalysis {
    /// Estimate the mean cumulative function of each population.
    ///
    /// # Errors
    /// Missing or non-numeric columns, a column used twice, no usable rows,
    /// a system in two populations, a population without repairs, a repair
    /// after its system's end of observation.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &RecurrenceRequest) -> Result<Self> {
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let numeric = |i: usize| -> Result<&[f64]> {
            let c = column(i)?;
            c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))
        };
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("choose a different column for each role"));
        }
        let system_col = column(request.system)?;
        let time_col = column(request.time)?;
        let ages = numeric(request.time)?;
        let events = request.event.map(numeric).transpose()?;
        let group_col = request.group.map(column).transpose()?;
        // System id → (group label, rows).
        let mut ids: Vec<String> = Vec::new();
        let mut system_group: Vec<String> = Vec::new();
        let mut system_rows: Vec<Vec<(usize, f64, bool)>> = Vec::new();
        let mut used = 0;
        for r in 0..table.n_rows() {
            let id = system_col.level_text(r);
            let label = group_col.map(|c| c.level_text(r)).unwrap_or_default();
            let (t, e) = (ages[r], events.map_or(1.0, |e| e[r]));
            if states.is_excluded(r) || id.is_empty() || !t.is_finite() || t < 0.0 || !e.is_finite() {
                continue;
            }
            if group_col.is_some() && label.is_empty() {
                continue;
            }
            let k = match ids.iter().position(|s| *s == id) {
                Some(k) => {
                    if system_group[k] != label {
                        return Err(invalid(format!(
                            "system {id} is in population {:?} on one row and {label:?} on another: each system \
                             belongs to one population, so correct the population column (or give the systems \
                             distinct names)",
                            system_group[k]
                        )));
                    }
                    k
                }
                None => {
                    ids.push(id);
                    system_group.push(label);
                    system_rows.push(Vec::new());
                    ids.len() - 1
                }
            };
            system_rows[k].push((r, t, e != 0.0));
            used += 1;
        }
        if used == 0 {
            return Err(invalid("no usable rows (a system, and a finite, non-negative age)"));
        }
        let mut labels: Vec<String> = system_group.clone();
        labels.sort_by(|a, b| natural_order(a, b));
        labels.dedup();
        let level = request.confidence;
        let groups = labels
            .into_iter()
            .map(|label| {
                let mut systems = Vec::new();
                let mut rows = Vec::new();
                for (k, g) in system_group.iter().enumerate() {
                    if *g != label {
                        continue;
                    }
                    let r = &system_rows[k];
                    let repairs: Vec<f64> = r.iter().filter(|x| x.2).map(|x| x.1).collect();
                    let marked = r
                        .iter()
                        .filter(|x| !x.2)
                        .map(|x| x.1)
                        .fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t))));
                    let last = repairs.iter().copied().fold(0.0, f64::max);
                    let (end, ended_at_repair) = match marked {
                        Some(e) if e >= last => (e, false),
                        Some(_) => {
                            return Err(invalid(format!(
                                "system {}: a repair is after its end of observation",
                                ids[k]
                            )));
                        }
                        None => (last, true),
                    };
                    systems.push(SystemHistory { repairs, end, ended_at_repair });
                    rows.extend(r.iter().copied());
                }
                let name = if label.is_empty() { String::new() } else { format!("{label}: ") };
                let mcf = mean_cumulative_function(&systems, request.variance, request.limits, level)
                    .map_err(|e| invalid(format!("{name}{e}")))?;
                Ok(RecurrenceGroup {
                    hpp: hpp_fit(&systems, level).map_err(|e| e.to_string()),
                    power_law: power_law_fit(&systems, level).map_err(|e| e.to_string()),
                    laplace: laplace_test(&systems).map_err(|e| e.to_string()),
                    military: military_handbook_test(&systems).map_err(|e| e.to_string()),
                    label,
                    systems,
                    rows,
                    mcf,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let difference =
            (groups.len() == 2).then(|| mcf_difference(&groups[0].mcf, &groups[1].mcf, level)).transpose()?;
        Ok(Self {
            request: request.clone(),
            time_name: time_col.name.clone(),
            group_name: group_col.map(|c| c.name.clone()),
            groups,
            difference,
            n_skipped: table.n_rows() - used,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &RecurrenceRequest {
        &self.request
    }

    /// The populations (one when ungrouped), in natural order.
    #[must_use]
    pub fn groups(&self) -> &[RecurrenceGroup] {
        &self.groups
    }

    /// `M̂₁ − M̂₂` when there are exactly two populations.
    #[must_use]
    pub fn difference(&self) -> Option<&[McfDifferenceStep]> {
        self.difference.as_deref()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.group_name {
            Some(g) => format!("Repairable Systems: {} by {g}", self.time_name),
            None => format!("Repairable Systems: {}", self.time_name),
        }
    }

    fn grouped(&self) -> bool {
        self.group_name.is_some()
    }

    fn level(&self) -> String {
        format!("{}%", format_number(self.request.confidence * 100.0, 4))
    }

    fn with_group(&self, g: &RecurrenceGroup, cells: impl IntoIterator<Item = String>) -> Vec<String> {
        let mut row = Vec::new();
        if self.grouped() {
            row.push(g.label.clone());
        }
        row.extend(cells);
        row
    }

    fn headings(&self, rest: &[&str]) -> Vec<String> {
        let mut h = Vec::new();
        if let Some(g) = &self.group_name {
            h.push(g.clone());
        }
        h.extend(rest.iter().map(|s| (*s).to_owned()));
        h
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.plot(), self.table()];
        if let Some(d) = &self.difference {
            out.push(self.difference_plot(d));
            out.push(self.difference_table(d));
        }
        out.push(self.fits());
        out.push(self.trend_tests());
        out
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(self.headings(&[
            "Systems",
            "Repairs",
            "Total age observed",
            "Latest end of observation",
            "Mean repairs per system at the end",
        ]));
        for g in &self.groups {
            let repairs: usize = g.systems.iter().map(|s| s.repairs.len()).sum();
            let total: f64 = g.systems.iter().map(|s| s.end).sum();
            t.push(self.with_group(
                g,
                [
                    g.systems.len().to_string(),
                    repairs.to_string(),
                    format_number(total, DIGITS),
                    format_number(g.mcf.end, DIGITS),
                    format_number(g.mcf.at(g.mcf.end).0, DIGITS),
                ],
            ));
        }
        let skipped = if self.n_skipped > 0 {
            format!("; {} rows excluded or incomplete", self.n_skipped)
        } else {
            String::new()
        };
        section(SectionKind::RecurrenceSummary, format!("Summary{skipped}"), SectionContent::Table(t))
    }

    fn plot(&self) -> ReportSection {
        let (mut points, mut censored, mut membership, mut curves) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (k, g) in self.groups.iter().enumerate() {
            for &(row, t, repair) in &g.rows {
                points.push(RowPoint { row, x: t, y: g.mcf.at(t).0 });
                censored.push(!repair);
                membership.push(k);
            }
            let end = g.mcf.end;
            let grid = |f: &dyn Fn(f64) -> f64| -> Vec<[f64; 2]> {
                (0..=CURVE_POINTS).map(|i| end * i as f64 / CURVE_POINTS as f64).map(|t| [t, f(t)]).collect()
            };
            let mut fitted = Vec::new();
            if let Ok(p) = &g.power_law {
                fitted.push(("Power-law fit".to_owned(), LinePattern::Dashed, grid(&|t| p.mean_cumulative(t))));
            }
            if let Ok(h) = &g.hpp {
                fitted.push(("Constant-rate fit".to_owned(), LinePattern::Dotted, grid(&|t| h.rate * t)));
            }
            curves.push(SurvivalCurveSpec {
                label: g.label.clone(),
                steps: g.mcf.step_polyline(),
                band: g.mcf.band_pieces(),
                fitted,
            });
        }
        section(
            SectionKind::McfPlot,
            format!(
                "Mean Cumulative Function (steps: Nelson estimate; shaded: pointwise {} limits; filled: repair, hollow: end of observation)",
                self.level()
            ),
            SectionContent::Survival(Box::new(SurvivalPlotSpec {
                x_label: self.time_name.clone(),
                y_label: "Mean cumulative repairs per system".to_owned(),
                curves,
                points,
                censored,
                membership,
                estimate: Some("Mean cumulative function".to_owned()),
                risk_table: None,
            })),
        )
    }

    fn table(&self) -> ReportSection {
        let level = self.level();
        let lower = format!("Lower {level}");
        let upper = format!("Upper {level}");
        let mut t =
            TextTable::new(self.headings(&["Age", "Repairs", "Systems observed", "MCF", "Std error", &lower, &upper]));
        for g in &self.groups {
            for s in &g.mcf.steps {
                t.push(self.with_group(
                    g,
                    [
                        format_number(s.time, DIGITS),
                        s.repairs.to_string(),
                        s.at_risk.to_string(),
                        format_number(s.mcf, DIGITS),
                        format_number(s.std_error, 4),
                        format_number(s.lower, DIGITS),
                        format_number(s.upper, DIGITS),
                    ],
                ));
            }
        }
        section(
            SectionKind::McfTable,
            format!(
                "Mean Cumulative Function Estimates ({} variance, {} limits)",
                self.request.variance.label(),
                self.request.limits.label()
            ),
            SectionContent::Table(t),
        )
    }

    fn difference_names(&self) -> (String, String) {
        (self.groups[0].label.clone(), self.groups[1].label.clone())
    }

    fn difference_plot(&self, d: &[McfDifferenceStep]) -> ReportSection {
        let end = self.groups[0].mcf.end.min(self.groups[1].mcf.end);
        let steps = |f: &dyn Fn(&McfDifferenceStep) -> f64| -> Vec<[f64; 2]> {
            let mut out = vec![[0.0, 0.0]];
            let mut level = 0.0;
            for s in d {
                out.push([s.time, level]);
                level = f(s);
                out.push([s.time, level]);
            }
            out.push([end, level]);
            out
        };
        let (a, b) = self.difference_names();
        let spec = PlotSpec {
            x_label: self.time_name.clone(),
            y_label: format!("MCF({a}) − MCF({b})"),
            line: Some(steps(&|s| s.difference)),
            line_name: Some("MCF difference".to_owned()),
            band: Some((steps(&|s| s.lower), steps(&|s| s.upper))),
            band_name: Some(format!("Pointwise {} limits", self.level())),
            segments: vec![vec![[0.0, 0.0], [end, 0.0]]],
            ..PlotSpec::default()
        };
        section(
            SectionKind::McfDifferencePlot,
            format!("Difference of Mean Cumulative Functions ({a} − {b}; dashed: {} limits; thin: zero)", self.level()),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn difference_table(&self, d: &[McfDifferenceStep]) -> ReportSection {
        let level = self.level();
        let lower = format!("Lower {level}");
        let upper = format!("Upper {level}");
        let mut t = TextTable::new(["Age", "Difference", "Std error", lower.as_str(), upper.as_str(), "Differ"]);
        for s in d {
            t.push([
                format_number(s.time, DIGITS),
                format_number(s.difference, DIGITS),
                format_number(s.std_error, 4),
                format_number(s.lower, DIGITS),
                format_number(s.upper, DIGITS),
                if s.lower > 0.0 || s.upper < 0.0 { "yes" } else { "—" }.to_owned(),
            ]);
        }
        let (a, b) = self.difference_names();
        section(
            SectionKind::McfDifferenceTable,
            format!("Difference Estimates ({a} − {b}; “yes” where the limits exclude zero)"),
            SectionContent::Table(t),
        )
    }

    fn fits(&self) -> ReportSection {
        let level = self.level();
        let lower = format!("Lower {level}");
        let upper = format!("Upper {level}");
        let mut t = TextTable::new(self.headings(&["Model", "Parameter", "Estimate", &lower, &upper]));
        let num = |v: f64| format_number(v, DIGITS);
        for g in &self.groups {
            match &g.hpp {
                Ok(h) => t.push(self.with_group(
                    g,
                    [
                        "Constant rate".into(),
                        "Repairs per unit age ρ".into(),
                        num(h.rate),
                        num(h.interval.0),
                        num(h.interval.1),
                    ],
                )),
                Err(e) => {
                    t.push(self.with_group(
                        g,
                        ["Constant rate".into(), e.clone(), String::new(), String::new(), String::new()],
                    ))
                }
            }
            match &g.power_law {
                Ok(p) => {
                    t.push(self.with_group(
                        g,
                        [
                            "Power law".into(),
                            "Shape β".into(),
                            num(p.beta),
                            num(p.beta_interval.0),
                            num(p.beta_interval.1),
                        ],
                    ));
                    t.push(self.with_group(
                        g,
                        ["Power law".into(), "Scale λ".into(), num(p.lambda()), "—".into(), "—".into()],
                    ));
                    if let Ok(h) = &g.hpp {
                        let lr = f64::max(2.0 * (p.log_likelihood - h.log_likelihood), 0.0);
                        let p_value = stat_core::ChiSquared::new(1.0).and_then(|c| {
                            use stat_core::ContinuousDistribution;
                            c.sf(lr)
                        });
                        t.push(self.with_group(
                            g,
                            [
                                "Power law vs constant rate".into(),
                                "Likelihood-ratio χ² (1 df)".into(),
                                format_number(lr, 4),
                                "p-value".into(),
                                p_value.map_or_else(|_| "—".into(), format_p_value),
                            ],
                        ));
                    }
                }
                Err(e) => t.push(
                    self.with_group(g, ["Power law".into(), e.clone(), String::new(), String::new(), String::new()]),
                ),
            }
        }
        section(
            SectionKind::RecurrenceFits,
            "Process Fits (mean repairs by age t: ρt or λt^β; β > 1: repairs come faster with age)",
            SectionContent::Table(t),
        )
    }

    fn trend_tests(&self) -> ReportSection {
        let mut t = TextTable::new(self.headings(&["Test", "Statistic", "DF", "p-value", "Points to"]));
        for g in &self.groups {
            for (name, test) in [("Laplace", &g.laplace), ("Military handbook (MIL-HDBK-189)", &g.military)] {
                match test {
                    Ok(x) => t.push(
                        self.with_group(
                            g,
                            [
                                name.to_owned(),
                                format_number(x.statistic, 4),
                                x.df.map_or_else(|| "—".into(), |d| format_number(d, 4)),
                                format_p_value(x.p_value),
                                if x.increasing {
                                    "more frequent repairs (deterioration)"
                                } else {
                                    "less frequent repairs (improvement)"
                                }
                                .to_owned(),
                            ],
                        ),
                    ),
                    Err(e) => t.push(
                        self.with_group(g, [name.to_owned(), e.clone(), String::new(), String::new(), String::new()]),
                    ),
                }
            }
        }
        section(
            SectionKind::TrendTests,
            "Trend Tests (null hypothesis: a constant repair rate; two-sided p-values)",
            SectionContent::Table(t),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::repairable_systems;

    fn col(t: &DataTable, name: &str) -> usize {
        t.find(name).unwrap()
    }

    fn request(t: &DataTable) -> RecurrenceRequest {
        RecurrenceRequest {
            event: Some(col(t, "Repair")),
            group: Some(col(t, "Design")),
            ..RecurrenceRequest::new(col(t, "Unit"), col(t, "Age (days)"))
        }
    }

    #[test]
    fn grouped_sample_has_difference_and_fits() {
        let t = repairable_systems();
        let rows = RowStateManager::new(t.n_rows());
        let a = RecurrenceAnalysis::run(&t, &rows, &request(&t)).unwrap();
        assert_eq!(a.groups().len(), 2);
        assert!(a.difference().is_some());
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert!(kinds.contains(&SectionKind::McfDifferencePlot) && kinds.contains(&SectionKind::TrendTests));
        let SectionContent::Survival(plot) = &s[1].content else { panic!() };
        assert_eq!(plot.curves.len(), 2);
        assert_eq!(plot.points.len(), t.n_rows());
        // Every system has an end-of-observation row: one hollow marker each.
        let systems: usize = a.groups().iter().map(|g| g.systems.len()).sum();
        assert_eq!(plot.censored.iter().filter(|&&c| c).count(), systems);
        // The estimate matches the engine run on the same systems.
        let g = &a.groups()[0];
        let direct = mean_cumulative_function(&g.systems, McfVariance::Robust, McfLimits::Log, 0.95).unwrap();
        assert_eq!(g.mcf, direct);
        // The sample's story: old pumps (β = 1.6) wear out, new ones do not.
        let old = a.groups().iter().find(|g| g.label == "Old").unwrap();
        let new = a.groups().iter().find(|g| g.label == "New").unwrap();
        let (lo, hi) = (old.laplace.as_ref().unwrap(), new.laplace.as_ref().unwrap());
        assert!(lo.increasing && lo.p_value < 0.05, "{lo:?}");
        assert!(hi.p_value > 0.05, "{hi:?}");
        assert!(old.power_law.as_ref().unwrap().beta > 1.2);
    }

    #[test]
    fn ungrouped_and_errors() {
        let t = repairable_systems();
        let mut rows = RowStateManager::new(t.n_rows());
        let mut r = request(&t);
        r.group = None;
        let a = RecurrenceAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(a.groups().len(), 1);
        assert!(a.difference().is_none());
        // Excluding a system's end record ends it at its last repair.
        let end_row =
            (0..t.n_rows()).find(|&i| t.column(col(&t, "Repair")).unwrap().as_numeric().unwrap()[i] == 0.0).unwrap();
        rows.set_excluded([end_row], true);
        let b = RecurrenceAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(b.groups()[0].systems.iter().filter(|s| s.ended_at_repair).count(), 1);
        let same = RecurrenceRequest { event: Some(r.time), ..r.clone() };
        assert!(RecurrenceAnalysis::run(&t, &rows, &same).is_err());
    }
}
