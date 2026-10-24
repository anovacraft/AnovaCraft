//! Competing risks bound to a dataset
//! ([`stat_reliability::competing_risks`]): Aalen–Johansen cumulative
//! incidence of each cause (failure mode) with limits, stacked or overlaid,
//! per group; Gray's test of each cause across groups; Fine–Gray
//! subdistribution hazards regression on covariates with predicted cumulative
//! incidence; and, for engineering data, a life distribution per failure mode
//! combined into the series system's failure probability.
//!
//! **Layout:** one row per unit (or per set of identical units, with a
//! frequency column): the time of failure or censoring, and the cause or
//! failure mode, one value of which means "censored" (still running). An
//! optional group column compares groups; optional covariate columns
//! (continuous, or nominal coded by indicators of all levels but the first)
//! enter the regression. Excluded rows and rows without a time or cause are
//! left out; rows missing a covariate are left out of the regression only.
//!
//! Failure-mode fits use every unit together (the group column is not used
//! there).

use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, s};
use crate::report::{
    LinePattern, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, SurvivalCurveSpec, SurvivalPlotSpec,
    TextTable,
};
use stat_core::Result;
use stat_core::distribution_fit::FitFamily;
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_linear::{ModelDesign, Predictor, PredictorValues, TermKind, TermValue, natural_order};
use stat_reliability::competing_risks::{
    CompetingObservation, CumulativeIncidence, FineGrayFit, GrayTest, SeriesSystem, aalen_johansen, fine_gray,
    fit_failure_modes, gray_test, mode_life_data,
};
use stat_reliability::{CensoredFit, NonparametricEstimate, turnbull};

const DIGITS: usize = 5;
const CURVE_POINTS: usize = 160;

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

/// Columns and settings of a competing-risks analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct CompetingRisksRequest {
    /// Time of failure or censoring.
    pub time: usize,
    /// Cause (failure mode) of each failure.
    pub cause: usize,
    /// The cause-column value meaning "censored" (`None`: nothing is censored).
    pub censor_value: Option<String>,
    /// Optional group column.
    pub group: Option<usize>,
    /// Optional frequency column.
    pub frequency: Option<usize>,
    /// Covariates of the Fine–Gray regression (none: no regression).
    pub covariates: Vec<usize>,
    /// The cause the regression models (`None`: the first cause).
    pub regression_cause: Option<String>,
    /// Covariate settings for predicted cumulative incidence, one value per
    /// covariate (text: a number or a level); empty for automatic profiles.
    pub profiles: Vec<Vec<String>>,
    /// Draw the incidence curves stacked (each the sum of the causes up to it).
    pub stacked: bool,
    /// Fit this life distribution to each failure mode and combine them into
    /// the series system (`None`: no mode fits).
    pub mode_family: Option<FitFamily>,
    /// Times at which the system's failure probability is tabulated (empty:
    /// automatic).
    pub times: Vec<f64>,
    /// Confidence level.
    pub confidence: f64,
}

impl CompetingRisksRequest {
    /// Default confidence level.
    pub const DEFAULT_CONFIDENCE: f64 = 0.95;

    /// Times in `time`, causes in `cause`, with default settings.
    #[must_use]
    pub fn new(time: usize, cause: usize) -> Self {
        Self {
            time,
            cause,
            censor_value: None,
            group: None,
            frequency: None,
            covariates: Vec::new(),
            regression_cause: None,
            profiles: Vec::new(),
            stacked: false,
            mode_family: None,
            times: Vec::new(),
            confidence: Self::DEFAULT_CONFIDENCE,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = vec![self.time, self.cause];
        out.extend(self.group);
        out.extend(self.frequency);
        out.extend(self.covariates.iter().filter(|c| !out.contains(c)).copied().collect::<Vec<_>>());
        out
    }

    /// `true` when a column plays two roles. The group column may also be a
    /// covariate; time, cause and frequency columns may not be anything else.
    #[must_use]
    pub fn has_role_conflict(&self) -> bool {
        let roles: Vec<usize> =
            [Some(self.time), Some(self.cause), self.group, self.frequency].into_iter().flatten().collect();
        let repeated = |v: &[usize]| (1..v.len()).any(|i| v[..i].contains(&v[i]));
        repeated(&roles)
            || repeated(&self.covariates)
            || self.covariates.iter().any(|c| [Some(self.time), Some(self.cause), self.frequency].contains(&Some(*c)))
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.time = f(self.time);
        self.cause = f(self.cause);
        self.group = self.group.map(f);
        self.frequency = self.frequency.map(f);
        for c in &mut self.covariates {
            *c = f(*c);
        }
    }

    /// The project-file (JSON) form, columns by name.
    pub(crate) fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let n = JsonValue::number;
        let mut m: Vec<(String, JsonValue)> = vec![
            ("analysis".into(), s("competing-risks")),
            ("time".into(), col(self.time)),
            ("cause".into(), col(self.cause)),
        ];
        if let Some(v) = &self.censor_value {
            m.push(("censor_value".into(), s(v)));
        }
        if let Some(c) = self.group {
            m.push(("group".into(), col(c)));
        }
        if let Some(c) = self.frequency {
            m.push(("frequency".into(), col(c)));
        }
        if !self.covariates.is_empty() {
            m.push(("covariates".into(), JsonValue::Array(self.covariates.iter().map(|&c| col(c)).collect())));
        }
        if let Some(c) = &self.regression_cause {
            m.push(("regression_cause".into(), s(c)));
        }
        if !self.profiles.is_empty() {
            m.push((
                "profiles".into(),
                JsonValue::Array(
                    self.profiles.iter().map(|p| JsonValue::Array(p.iter().map(|v| s(v)).collect())).collect(),
                ),
            ));
        }
        m.push(("stacked".into(), JsonValue::Bool(self.stacked)));
        if let Some(f) = self.mode_family {
            m.push(("mode_family".into(), s(f.key())));
        }
        if !self.times.is_empty() {
            m.push(("times".into(), JsonValue::Array(self.times.iter().map(|&t| n(t)).collect())));
        }
        m.push(("confidence".into(), n(self.confidence)));
        JsonValue::Object(m)
    }

    /// Read the JSON form (see [`to_json`](Self::to_json)).
    pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let name_of = |x: &JsonValue, key: &str| -> Result<usize> {
            let name = x.as_str().ok_or_else(|| invalid(format!("competing risks {key}: expected a column name")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let col = |key: &str| name_of(member(v, key)?, key);
        let optional = |key: &str| -> Result<Option<usize>> {
            match v.get(key) {
                None | Some(JsonValue::Null) => Ok(None),
                Some(x) => name_of(x, key).map(Some),
            }
        };
        let text = |key: &str| v.get(key).and_then(JsonValue::as_str).map(str::to_owned);
        let mut r = Self::new(col("time")?, col("cause")?);
        r.censor_value = text("censor_value");
        r.group = optional("group")?;
        r.frequency = optional("frequency")?;
        if let Some(items) = v.get("covariates").and_then(JsonValue::as_array) {
            r.covariates = items.iter().map(|x| name_of(x, "covariates")).collect::<Result<_>>()?;
        }
        r.regression_cause = text("regression_cause");
        if let Some(items) = v.get("profiles").and_then(JsonValue::as_array) {
            r.profiles = items
                .iter()
                .map(|p| {
                    p.as_array()
                        .map(|a| a.iter().map(|x| x.as_str().unwrap_or_default().to_owned()).collect())
                        .ok_or_else(|| invalid("competing risks: each profile is a list of values"))
                })
                .collect::<Result<_>>()?;
        }
        r.stacked = v.get("stacked").and_then(JsonValue::as_bool).unwrap_or(false);
        r.mode_family = match text("mode_family") {
            None => None,
            Some(k) => Some(
                FitFamily::from_key(&k)
                    .ok_or_else(|| invalid(format!("competing risks: unknown distribution {k:?}")))?,
            ),
        };
        if let Some(items) = v.get("times").and_then(JsonValue::as_array) {
            r.times = items.iter().filter_map(JsonValue::as_f64).collect();
        }
        r.confidence = v.get("confidence").and_then(JsonValue::as_f64).unwrap_or(Self::DEFAULT_CONFIDENCE);
        Ok(r)
    }
}

/// The Fine–Gray part of the analysis.
#[derive(Debug, Clone)]
pub struct RegressionResult {
    /// Index of the modelled cause.
    pub cause: usize,
    /// Design-column names.
    pub names: Vec<String>,
    /// The fit.
    pub fit: FineGrayFit,
    /// Units used (weighted) and rows left out for a missing covariate.
    pub n_used: f64,
    /// Rows left out of the regression for a missing covariate.
    pub n_missing: usize,
    /// Profiles: label and coded covariates.
    pub profiles: Vec<(String, Vec<f64>)>,
}

/// One group's estimates.
#[derive(Debug, Clone)]
pub struct CompetingGroup {
    /// Group label (empty when ungrouped).
    pub label: String,
    /// `(row, observation)` of each analysed row.
    pub rows: Vec<(usize, CompetingObservation)>,
    /// The Aalen–Johansen estimates.
    pub incidence: CumulativeIncidence,
}

/// A computed competing-risks analysis.
#[derive(Debug, Clone)]
pub struct CompetingRisksAnalysis {
    request: CompetingRisksRequest,
    time_name: String,
    cause_name: String,
    group_name: Option<String>,
    causes: Vec<String>,
    groups: Vec<CompetingGroup>,
    gray: Vec<std::result::Result<GrayTest, String>>,
    regression: Option<std::result::Result<RegressionResult, String>>,
    modes: Option<Vec<std::result::Result<CensoredFit, String>>>,
    n_skipped: usize,
}

fn value(table: &DataTable, col: usize, row: usize) -> f64 {
    table.column(col).and_then(|c| c.as_numeric()).map_or(f64::NAN, |v| v[row])
}

impl CompetingRisksAnalysis {
    /// Estimate the cumulative incidences and run the requested tests and fits.
    ///
    /// # Errors
    /// Missing or non-numeric time or frequency columns, a column used twice,
    /// a confidence level outside (0.5, 1), no usable rows or no failures.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &CompetingRisksRequest) -> Result<Self> {
        if request.has_role_conflict() {
            return Err(invalid("choose a different column for each role"));
        }
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        for c in [Some(request.time), request.frequency].into_iter().flatten() {
            let col = column(c)?;
            if col.as_numeric().is_none() {
                return Err(invalid(format!("{} must be numeric", col.name)));
            }
        }
        if !(request.confidence > 0.5 && request.confidence < 1.0) {
            return Err(invalid("the confidence level must be between 0.5 and 1"));
        }
        let time_col = column(request.time)?;
        let cause_col = column(request.cause)?;
        let group_col = request.group.map(column).transpose()?;
        let censor = request.censor_value.as_deref().map(str::trim);
        // Usable rows: (row, time, cause label or None, weight, group label).
        let mut usable: Vec<UsableRow> = Vec::new();
        for r in 0..table.n_rows() {
            if states.is_excluded(r) {
                continue;
            }
            let t = value(table, request.time, r);
            let w = request.frequency.map_or(1.0, |c| value(table, c, r));
            let label = cause_col.level_text(r).trim().to_owned();
            if !(t.is_finite() && t >= 0.0) || !(w.is_finite() && w > 0.0) || label.is_empty() {
                continue;
            }
            let g = group_col.map(|c| c.level_text(r)).unwrap_or_default();
            if group_col.is_some() && g.trim().is_empty() {
                continue;
            }
            let cause = if censor == Some(label.as_str()) { None } else { Some(label) };
            usable.push((r, t, cause, w, g));
        }
        if usable.is_empty() {
            return Err(invalid("no rows with a usable time and cause"));
        }
        let mut causes: Vec<String> = usable.iter().filter_map(|u| u.2.clone()).collect();
        causes.sort_by(|a, b| natural_order(a, b));
        causes.dedup();
        if causes.is_empty() {
            return Err(invalid("every unit is censored: there are no failures"));
        }
        let mut labels: Vec<String> = usable.iter().map(|u| u.4.clone()).collect();
        labels.sort_by(|a, b| natural_order(a, b));
        labels.dedup();
        let obs_of = |u: &UsableRow| CompetingObservation {
            time: u.1,
            cause: u.2.as_ref().and_then(|c| causes.iter().position(|x| x == c)),
            weight: u.3,
        };
        let groups = labels
            .iter()
            .map(|label| {
                let rows: Vec<(usize, CompetingObservation)> =
                    usable.iter().filter(|u| u.4 == *label).map(|u| (u.0, obs_of(u))).collect();
                let obs: Vec<CompetingObservation> = rows.iter().map(|r| r.1).collect();
                let name = if label.is_empty() { String::new() } else { format!("{label}: ") };
                let incidence = aalen_johansen(&obs, causes.len()).map_err(|e| invalid(format!("{name}{e}")))?;
                Ok(CompetingGroup { label: label.clone(), rows, incidence })
            })
            .collect::<Result<Vec<_>>>()?;
        let all_obs: Vec<CompetingObservation> = usable.iter().map(obs_of).collect();
        let gray = if groups.len() >= 2 {
            let group_index: Vec<usize> =
                usable.iter().map(|u| labels.iter().position(|l| *l == u.4).unwrap_or(0)).collect();
            (0..causes.len())
                .map(|j| gray_test(&all_obs, &group_index, groups.len(), j).map_err(|e| e.to_string()))
                .collect()
        } else {
            Vec::new()
        };
        let regression = (!request.covariates.is_empty())
            .then(|| regression(table, request, &usable, &causes, &obs_of).map_err(|e| e.to_string()));
        let modes = request.mode_family.map(|family| {
            fit_failure_modes(&all_obs, causes.len(), family)
                .into_iter()
                .map(|r| r.map_err(|e| e.to_string()))
                .collect()
        });
        Ok(Self {
            request: request.clone(),
            time_name: time_col.name.clone(),
            cause_name: cause_col.name.clone(),
            group_name: group_col.map(|c| c.name.clone()),
            causes,
            groups,
            gray,
            regression,
            modes,
            n_skipped: table.n_rows() - usable.len(),
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &CompetingRisksRequest {
        &self.request
    }

    /// Cause labels in natural order.
    #[must_use]
    pub fn causes(&self) -> &[String] {
        &self.causes
    }

    /// The groups (one when ungrouped).
    #[must_use]
    pub fn groups(&self) -> &[CompetingGroup] {
        &self.groups
    }

    /// Gray's test of each cause (empty without two or more groups).
    pub fn gray_tests(&self) -> &[std::result::Result<GrayTest, String>] {
        &self.gray
    }

    /// The Fine–Gray regression, when covariates were given.
    #[must_use]
    pub fn regression(&self) -> Option<&std::result::Result<RegressionResult, String>> {
        self.regression.as_ref()
    }

    /// The failure-mode fits, when requested.
    #[must_use]
    pub fn mode_fits(&self) -> Option<&[std::result::Result<CensoredFit, String>]> {
        self.modes.as_deref()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.group_name {
            Some(g) => format!("Competing Risks: {} by {} and {g}", self.time_name, self.cause_name),
            None => format!("Competing Risks: {} by {}", self.time_name, self.cause_name),
        }
    }

    fn level(&self) -> String {
        format!("{}%", format_number(self.request.confidence * 100.0, 4))
    }

    fn grouped(&self) -> bool {
        self.group_name.is_some()
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
        let mut out = vec![self.summary(), self.incidence_plot(), self.incidence_table()];
        if !self.gray.is_empty() {
            out.push(self.gray_section());
        }
        match &self.regression {
            Some(Ok(r)) => {
                out.push(self.hazard_ratios(r));
                out.push(self.regression_fit(r));
                out.push(self.prediction_plot(r));
            }
            Some(Err(e)) => out.push(section(
                SectionKind::SubdistributionHazards,
                "Subdistribution Hazards Regression",
                SectionContent::Note(format!("The regression could not be fitted: {e}")),
            )),
            None => {}
        }
        if let Some(modes) = &self.modes {
            out.push(self.mode_table(modes));
            let fitted: Vec<CensoredFit> = modes.iter().filter_map(|m| m.as_ref().ok().cloned()).collect();
            if fitted.len() == modes.len() {
                let system = SeriesSystem { modes: fitted };
                out.push(self.system_plot(&system));
                out.push(self.system_table(&system));
            } else {
                out.push(section(
                    SectionKind::SeriesSystemProbabilities,
                    "Series System",
                    SectionContent::Note(
                        "Every failure mode needs a fitted distribution before the modes can be combined into the \
                         system's failure probability."
                            .into(),
                    ),
                ));
            }
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let mut cols: Vec<String> = vec!["Units".into()];
        cols.extend(self.causes.iter().map(|c| format!("Failed: {c}")));
        cols.push("Censored".into());
        let refs: Vec<&str> = cols.iter().map(String::as_str).collect();
        let mut t = TextTable::new(self.headings(&refs));
        for g in &self.groups {
            let mut row = Vec::new();
            if self.grouped() {
                row.push(g.label.clone());
            }
            let total: f64 = g.rows.iter().map(|r| r.1.weight).sum();
            row.push(num(total));
            for j in 0..self.causes.len() {
                row.push(num(g.rows.iter().filter(|r| r.1.cause == Some(j)).map(|r| r.1.weight).sum()));
            }
            row.push(num(g.rows.iter().filter(|r| r.1.cause.is_none()).map(|r| r.1.weight).sum()));
            t.push(row);
        }
        let skipped = if self.n_skipped > 0 {
            format!("; {} rows excluded or incomplete", self.n_skipped)
        } else {
            String::new()
        };
        section(SectionKind::CompetingRisksSummary, format!("Summary{skipped}"), SectionContent::Table(t))
    }

    fn curve_label(&self, g: &CompetingGroup, name: &str) -> String {
        if self.grouped() { format!("{}: {name}", g.label) } else { name.to_owned() }
    }

    fn incidence_plot(&self) -> ReportSection {
        let k = self.causes.len();
        let stacked = self.request.stacked;
        let (mut curves, mut points, mut censored, mut membership) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for g in &self.groups {
            let base = curves.len();
            let aj = &g.incidence;
            for (j, cause) in self.causes.iter().enumerate() {
                let band =
                    if stacked { Vec::new() } else { aj.band_pieces(j, self.request.confidence).unwrap_or_default() };
                let label = if stacked && j > 0 {
                    self.curve_label(g, &format!("{cause} (stacked)"))
                } else {
                    self.curve_label(g, cause)
                };
                curves.push(SurvivalCurveSpec {
                    label,
                    steps: aj.step_polyline(j, 0.0, stacked),
                    band,
                    fitted: Vec::new(),
                });
            }
            // The all-causes curve (1 − Ŝ), which is the top stacked curve.
            let all_index = if stacked {
                base + k - 1
            } else {
                let mut steps = vec![[0.0, 0.0]];
                let mut level = 0.0;
                for s in &aj.steps {
                    steps.push([s.time, level]);
                    level = 1.0 - s.survival;
                    steps.push([s.time, level]);
                }
                steps.push([aj.end, level]);
                curves.push(SurvivalCurveSpec {
                    label: self.curve_label(g, "All causes"),
                    steps,
                    band: Vec::new(),
                    fitted: Vec::new(),
                });
                base + k
            };
            for &(row, o) in &g.rows {
                let (curve, y) = match o.cause {
                    Some(j) => {
                        let y = if stacked {
                            (0..=j).map(|i| aj.incidence_at(i, o.time)).sum()
                        } else {
                            aj.incidence_at(j, o.time)
                        };
                        (base + j, y)
                    }
                    None => (all_index, 1.0 - aj.survival_at(o.time)),
                };
                points.push(RowPoint { row, x: o.time, y });
                censored.push(o.cause.is_none());
                membership.push(curve);
            }
        }
        let how = if stacked {
            "stacked: each band between curves is one cause; the top curve is all causes"
        } else {
            "overlaid, with pointwise limits; also all causes together"
        };
        section(
            SectionKind::CumulativeIncidencePlot,
            format!(
                "Cumulative Incidence by {} ({how}; filled: failure, hollow: censored; {} log(−log) limits)",
                self.cause_name,
                self.level()
            ),
            SectionContent::Survival(Box::new(SurvivalPlotSpec {
                x_label: self.time_name.clone(),
                y_label: "Cumulative incidence".to_owned(),
                curves,
                points,
                censored,
                membership,
                estimate: None,
                risk_table: None,
            })),
        )
    }

    fn incidence_table(&self) -> ReportSection {
        let level = self.level();
        let lower = format!("Lower {level}");
        let upper = format!("Upper {level}");
        let mut t = TextTable::new(self.headings(&[
            &self.cause_name,
            "Time",
            "At risk",
            "Failures",
            "Cumulative incidence",
            "Std error",
            &lower,
            &upper,
        ]));
        for g in &self.groups {
            for (j, cause) in self.causes.iter().enumerate() {
                for (k, s) in g.incidence.steps.iter().enumerate() {
                    if s.failures[j] <= 0.0 {
                        continue;
                    }
                    let (lo, hi) = g.incidence.limits(j, k, self.request.confidence).unwrap_or((f64::NAN, f64::NAN));
                    let mut row = Vec::new();
                    if self.grouped() {
                        row.push(g.label.clone());
                    }
                    row.extend([
                        cause.clone(),
                        num(s.time),
                        num(s.at_risk),
                        num(s.failures[j]),
                        num(s.incidence[j]),
                        format_number(s.std_error(j), 4),
                        num(lo),
                        num(hi),
                    ]);
                    t.push(row);
                }
            }
        }
        section(
            SectionKind::CumulativeIncidenceTable,
            "Cumulative Incidence Estimates (Aalen–Johansen; delta-method standard errors)",
            SectionContent::Table(t),
        )
    }

    fn gray_section(&self) -> ReportSection {
        let mut t = TextTable::new([self.cause_name.as_str(), "χ²", "DF", "p-value"]);
        for (cause, test) in self.causes.iter().zip(&self.gray) {
            match test {
                Ok(g) => t.push([cause.clone(), format_number(g.statistic, 4), num(g.df), format_p_value(g.p_value)]),
                Err(e) => t.push([cause.clone(), e.clone(), String::new(), String::new()]),
            }
        }
        section(
            SectionKind::GrayTests,
            format!(
                "Gray's Test of Equal Cumulative Incidence across {} (one test per cause)",
                self.group_name.clone().unwrap_or_default()
            ),
            SectionContent::Table(t),
        )
    }

    fn hazard_ratios(&self, r: &RegressionResult) -> ReportSection {
        let level = self.level();
        let lower = format!("Lower {level}");
        let upper = format!("Upper {level}");
        let mut t = TextTable::new([
            "Term",
            "Coefficient",
            "Robust std error",
            "z",
            "p-value",
            "Subdistribution hazard ratio",
            lower.as_str(),
            upper.as_str(),
        ]);
        let ratios = r.fit.hazard_ratios(self.request.confidence).unwrap_or_default();
        for (name, h) in r.names.iter().zip(ratios) {
            t.push([
                name.clone(),
                num(h.coefficient),
                format_number(h.std_error, 4),
                format_number(h.z, 4),
                format_p_value(h.p_value),
                num(h.hazard_ratio),
                num(h.lower),
                num(h.upper),
            ]);
        }
        section(
            SectionKind::SubdistributionHazards,
            format!(
                "Subdistribution Hazards Regression (Fine–Gray) for {} = {}: hazard ratios above 1 raise its cumulative incidence",
                self.cause_name, self.causes[r.cause]
            ),
            SectionContent::Table(t),
        )
    }

    fn regression_fit(&self, r: &RegressionResult) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        t.push(["Units used".to_owned(), num(r.n_used)]);
        if r.n_missing > 0 {
            t.push(["Rows left out (missing covariate)".to_owned(), r.n_missing.to_string()]);
        }
        t.push([format!("Failures from {}", self.causes[r.cause]), num(r.fit.n_events)]);
        t.push(["Log partial likelihood".to_owned(), num(r.fit.log_likelihood)]);
        t.push(["Log partial likelihood, no covariates".to_owned(), num(r.fit.null_log_likelihood)]);
        if let Ok((chi, df, p)) = r.fit.wald_test() {
            t.push(["Robust Wald χ² (all coefficients zero)".to_owned(), format_number(chi, 4)]);
            t.push(["DF".to_owned(), num(df)]);
            t.push(["p-value".to_owned(), format_p_value(p)]);
        }
        t.push(["Newton–Raphson iterations".to_owned(), r.fit.iterations.to_string()]);
        section(
            SectionKind::SubdistributionFit,
            "Regression Fit (other causes' failures stay in the risk set, weighted by the censoring distribution)",
            SectionContent::Table(t),
        )
    }

    fn prediction_plot(&self, r: &RegressionResult) -> ReportSection {
        let curves = r
            .profiles
            .iter()
            .map(|(label, z)| SurvivalCurveSpec {
                label: label.clone(),
                steps: r.fit.incidence_polyline(z, 0.0),
                band: Vec::new(),
                fitted: Vec::new(),
            })
            .collect();
        section(
            SectionKind::PredictedIncidencePlot,
            format!("Predicted Cumulative Incidence of {} at Covariate Settings", self.causes[r.cause]),
            SectionContent::Survival(Box::new(SurvivalPlotSpec {
                x_label: self.time_name.clone(),
                y_label: "Predicted cumulative incidence".to_owned(),
                curves,
                points: Vec::new(),
                censored: Vec::new(),
                membership: Vec::new(),
                estimate: None,
                risk_table: None,
            })),
        )
    }

    fn mode_table(&self, modes: &[std::result::Result<CensoredFit, String>]) -> ReportSection {
        let family = self.request.mode_family.unwrap_or(FitFamily::Weibull);
        let level = self.level();
        let lower = format!("Lower {level}");
        let upper = format!("Upper {level}");
        let mut t = TextTable::new([
            "Failure mode",
            "Failures",
            "Parameter",
            "Estimate",
            "Std error",
            lower.as_str(),
            upper.as_str(),
            "Log-likelihood",
            "AIC",
        ]);
        let all: Vec<CompetingObservation> = self.groups.iter().flat_map(|g| g.rows.iter().map(|r| r.1)).collect();
        for (j, (cause, fit)) in self.causes.iter().zip(modes).enumerate() {
            let failures: f64 = all.iter().filter(|o| o.cause == Some(j)).map(|o| o.weight).sum();
            match fit {
                Ok(f) => {
                    for (i, p) in f.parameters(self.request.confidence, false).iter().enumerate() {
                        let first = i == 0;
                        t.push([
                            if first { cause.clone() } else { String::new() },
                            if first { num(failures) } else { String::new() },
                            p.name.to_owned(),
                            num(p.estimate),
                            format_number(p.std_error, 4),
                            num(p.wald.0),
                            num(p.wald.1),
                            if first { num(f.log_likelihood()) } else { String::new() },
                            if first { num(f.aic()) } else { String::new() },
                        ]);
                    }
                }
                Err(e) => t.push([
                    cause.clone(),
                    num(failures),
                    e.clone(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ]),
            }
        }
        section(
            SectionKind::FailureModeFits,
            format!(
                "{} Fit to Each Failure Mode (the other modes' failures treated as censored; Wald limits)",
                family.label()
            ),
            SectionContent::Table(t),
        )
    }

    fn time_span(&self) -> (f64, f64) {
        let mut hi: f64 = 0.0;
        for g in &self.groups {
            hi = hi.max(g.incidence.end);
        }
        (0.0, if hi > 0.0 { hi } else { 1.0 })
    }

    fn system_plot(&self, system: &SeriesSystem) -> ReportSection {
        let family = self.request.mode_family.unwrap_or(FitFamily::Weibull);
        let all: Vec<(usize, CompetingObservation)> = self.groups.iter().flat_map(|g| g.rows.iter().copied()).collect();
        let obs: Vec<CompetingObservation> = all.iter().map(|r| r.1).collect();
        let (_, hi) = self.time_span();
        let grid: Vec<f64> = (1..=CURVE_POINTS).map(|i| hi * i as f64 / CURVE_POINTS as f64).collect();
        let failing = |np: &NonparametricEstimate| -> Polyline {
            np.survival_polyline(0.0).into_iter().map(|[x, y]| [x, 1.0 - y]).collect()
        };
        let mut curves = Vec::new();
        let mut estimates: Vec<Option<NonparametricEstimate>> = Vec::new();
        for (j, cause) in self.causes.iter().enumerate() {
            let np = mode_life_data(&obs, j).and_then(|d| turnbull(&d)).ok();
            let fit = &system.modes[j];
            let line: Polyline = grid.iter().map(|&t| [t, fit.distribution.cdf(t)]).collect();
            curves.push(SurvivalCurveSpec {
                label: cause.clone(),
                steps: np.as_ref().map(failing).unwrap_or_default(),
                band: Vec::new(),
                fitted: vec![(format!("{} fit", family.label()), LinePattern::Dashed, line)],
            });
            estimates.push(np);
        }
        // The system: every failure counts.
        let all_failures: Vec<stat_reliability::LifeObservation> = obs
            .iter()
            .map(|o| {
                if o.cause.is_some() {
                    stat_reliability::LifeObservation::exact(o.time, o.weight)
                } else {
                    stat_reliability::LifeObservation::right(o.time, o.weight)
                }
            })
            .collect();
        let system_np = stat_reliability::LifeData::new(all_failures).and_then(|d| turnbull(&d)).ok();
        let level = self.request.confidence;
        let mut band = Vec::new();
        let mut prev = 0.0;
        for &t in &grid {
            if let Ok(p) = system.probability_interval(t, level) {
                band.push([prev, t, p.lower, p.upper]);
            }
            prev = t;
        }
        let system_line: Polyline = grid.iter().map(|&t| [t, system.failure_probability(t)]).collect();
        let system_index = curves.len();
        curves.push(SurvivalCurveSpec {
            label: "System (all modes)".to_owned(),
            steps: system_np.as_ref().map(failing).unwrap_or_default(),
            band,
            fitted: vec![("Series system fit".to_owned(), LinePattern::Solid, system_line)],
        });
        let (mut points, mut censored, mut membership) = (Vec::new(), Vec::new(), Vec::new());
        for &(row, o) in &all {
            let (curve, est) = match o.cause {
                Some(j) => (j, estimates[j].as_ref()),
                None => (system_index, system_np.as_ref()),
            };
            let y = est.and_then(|e| e.cdf_at(o.time)).unwrap_or(f64::NAN);
            if y.is_finite() {
                points.push(RowPoint { row, x: o.time, y });
                censored.push(o.cause.is_none());
                membership.push(curve);
            }
        }
        section(
            SectionKind::SeriesSystemPlot,
            format!(
                "Failure Modes and Series System (steps: Kaplan–Meier with the other modes censored; dashed: {} fit \
                 per mode; solid: system 1 − Π(1 − F_j) with pointwise {} limits)",
                family.label(),
                self.level()
            ),
            SectionContent::Survival(Box::new(SurvivalPlotSpec {
                x_label: self.time_name.clone(),
                y_label: "Fraction failing".to_owned(),
                curves,
                points,
                censored,
                membership,
                estimate: None,
                risk_table: None,
            })),
        )
    }

    fn system_times(&self, system: &SeriesSystem) -> Vec<f64> {
        if !self.request.times.is_empty() {
            return self.request.times.clone();
        }
        // The system's 1%, 10% and 50% points, rounded for reading.
        [0.01, 0.1, 0.5]
            .iter()
            .map(|&p| system.quantile(p))
            .filter(|t| t.is_finite() && *t > 0.0)
            .map(|t| {
                let m = 10f64.powf(t.log10().floor() - 1.0);
                (t / m).round() * m
            })
            .collect()
    }

    fn system_table(&self, system: &SeriesSystem) -> ReportSection {
        let level = self.level();
        let lower = format!("Lower {level}");
        let upper = format!("Upper {level}");
        let mut cols: Vec<String> =
            vec!["Time".into(), "System failure probability".into(), "Std error".into(), lower, upper];
        cols.extend(self.causes.iter().map(|c| format!("F({c})")));
        let mut t = TextTable::new(cols);
        for time in self.system_times(system) {
            let Ok(p) = system.probability_interval(time, self.request.confidence) else { continue };
            let mut row = vec![num(time), num(p.estimate), format_number(p.std_error, 4), num(p.lower), num(p.upper)];
            row.extend(system.modes.iter().map(|m| num(m.distribution.cdf(time))));
            t.push(row);
        }
        for p in [0.01, 0.1, 0.5] {
            let q = system.quantile(p);
            let mut row = vec![
                format!("{}% of systems failed by", format_number(p * 100.0, 3)),
                num(q),
                String::new(),
                String::new(),
                String::new(),
            ];
            row.extend(system.modes.iter().map(|_| String::new()));
            t.push(row);
        }
        section(
            SectionKind::SeriesSystemProbabilities,
            "Series System Failure Probabilities (delta-method limits on the logit scale; modes independent)",
            SectionContent::Table(t),
        )
    }
}

/// A usable row: (row, time, cause or `None` if censored, weight, group label).
type UsableRow = (usize, f64, Option<String>, f64, String);

/// The Fine–Gray fit on the rows with every covariate.
fn regression(
    table: &DataTable,
    request: &CompetingRisksRequest,
    usable: &[UsableRow],
    causes: &[String],
    obs_of: &dyn Fn(&UsableRow) -> CompetingObservation,
) -> Result<RegressionResult> {
    let cause = match &request.regression_cause {
        Some(c) => causes
            .iter()
            .position(|x| x == c.trim())
            .ok_or_else(|| invalid(format!("no failures from {c:?} to model")))?,
        None => 0,
    };
    enum Data {
        Num(Vec<f64>),
        Cat(Vec<String>),
    }
    let mut names = Vec::new();
    let mut data = Vec::new();
    for &c in &request.covariates {
        let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
        names.push(col.name.clone());
        data.push(match (col.as_numeric(), col.level) {
            (Some(v), MeasurementLevel::Continuous) => Data::Num(v.to_vec()),
            _ => Data::Cat((0..table.n_rows()).map(|r| col.level_text(r).trim().to_owned()).collect()),
        });
    }
    let present = |r: usize| {
        data.iter().all(|d| match d {
            Data::Num(v) => v[r].is_finite(),
            Data::Cat(v) => !v[r].is_empty(),
        })
    };
    let used: Vec<&UsableRow> = usable.iter().filter(|u| present(u.0)).collect();
    let n_missing = usable.len() - used.len();
    let rows: Vec<usize> = used.iter().map(|u| u.0).collect();
    let preds: Vec<Predictor<'_>> = names
        .iter()
        .zip(&data)
        .map(|(name, d)| Predictor {
            name,
            values: match d {
                Data::Num(v) => PredictorValues::Continuous(v),
                Data::Cat(v) => PredictorValues::Categorical(v),
            },
        })
        .collect();
    let design = ModelDesign::new(&preds, &rows)?;
    let x: Vec<Vec<f64>> = rows.iter().map(|&r| design.row(&preds, r).unwrap_or_default()).collect();
    let obs: Vec<CompetingObservation> = used.iter().map(|u| obs_of(u)).collect();
    let fit = fine_gray(&obs, &x, cause)?;
    // Profiles.
    let terms = design.terms();
    let typical: Vec<TermValue> = terms
        .iter()
        .zip(&data)
        .map(|(term, d)| match (&term.kind, d) {
            (TermKind::Categorical { levels }, Data::Cat(v)) => {
                let counts: Vec<usize> = levels.iter().map(|l| rows.iter().filter(|&&r| v[r] == *l).count()).collect();
                let best = counts.iter().copied().max().unwrap_or(0);
                TermValue::Level(counts.iter().position(|&c| c == best).unwrap_or(0))
            }
            (_, Data::Num(v)) => {
                let used: Vec<f64> = rows.iter().map(|&r| v[r]).collect();
                TermValue::Number(used.iter().sum::<f64>() / used.len().max(1) as f64)
            }
            _ => TermValue::Number(0.0),
        })
        .collect();
    let describe = |values: &[TermValue]| -> String {
        terms
            .iter()
            .zip(values)
            .map(|(term, v)| match (&term.kind, v) {
                (TermKind::Categorical { levels }, TermValue::Level(l)) => format!("{} = {}", term.name, levels[*l]),
                (_, TermValue::Number(x)) => format!("{} = {}", term.name, format_number(*x, 4)),
                _ => term.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut settings: Vec<Vec<TermValue>> = Vec::new();
    if request.profiles.is_empty() {
        match (&terms[0].kind, &data[0]) {
            (TermKind::Categorical { levels }, _) => {
                for l in 0..levels.len() {
                    let mut v = typical.clone();
                    v[0] = TermValue::Level(l);
                    settings.push(v);
                }
            }
            (_, Data::Num(values)) => {
                let mut used: Vec<f64> = rows.iter().map(|&r| values[r]).collect();
                used.sort_by(f64::total_cmp);
                for p in [0.1, 0.5, 0.9] {
                    let q = used[((used.len() - 1) as f64 * p).round() as usize];
                    let mut v = typical.clone();
                    v[0] = TermValue::Number(q);
                    settings.push(v);
                }
            }
            _ => settings.push(typical.clone()),
        }
    } else {
        for p in &request.profiles {
            if p.len() != terms.len() {
                return Err(invalid(format!(
                    "each prediction setting needs {} values (one per covariate)",
                    terms.len()
                )));
            }
            let v = terms
                .iter()
                .zip(p)
                .map(|(term, text)| match &term.kind {
                    TermKind::Categorical { levels } => levels
                        .iter()
                        .position(|l| l == text.trim())
                        .map(TermValue::Level)
                        .ok_or_else(|| invalid(format!("{} has no level {:?}", term.name, text.trim()))),
                    _ => text
                        .trim()
                        .parse::<f64>()
                        .map(TermValue::Number)
                        .map_err(|_| invalid(format!("{} needs a number, not {:?}", term.name, text.trim()))),
                })
                .collect::<Result<Vec<_>>>()?;
            settings.push(v);
        }
    }
    let profiles = settings.iter().map(|v| Ok((describe(v), design.encode(v)?))).collect::<Result<Vec<_>>>()?;
    Ok(RegressionResult {
        cause,
        names: design.column_names().to_vec(),
        n_used: obs.iter().map(|o| o.weight).sum(),
        n_missing,
        fit,
        profiles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::pump_seal_failures;

    fn col(t: &DataTable, name: &str) -> usize {
        t.find(name).unwrap()
    }

    fn request(t: &DataTable) -> CompetingRisksRequest {
        CompetingRisksRequest {
            censor_value: Some("Running".into()),
            group: Some(col(t, "Site")),
            covariates: vec![col(t, "Load (kN)"), col(t, "Site")],
            mode_family: Some(FitFamily::Weibull),
            ..CompetingRisksRequest::new(col(t, "Hours"), col(t, "Failure mode"))
        }
    }

    #[test]
    fn sample_report_has_every_section() {
        let t = pump_seal_failures();
        let rows = RowStateManager::new(t.n_rows());
        let mut r = request(&t);
        r.covariates = vec![col(&t, "Load (kN)")];
        let a = CompetingRisksAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(a.causes().len(), 3);
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        for k in [
            SectionKind::CompetingRisksSummary,
            SectionKind::CumulativeIncidencePlot,
            SectionKind::CumulativeIncidenceTable,
            SectionKind::GrayTests,
            SectionKind::SubdistributionHazards,
            SectionKind::SubdistributionFit,
            SectionKind::PredictedIncidencePlot,
            SectionKind::FailureModeFits,
            SectionKind::SeriesSystemPlot,
            SectionKind::SeriesSystemProbabilities,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing");
        }
        // Every analysed row is a point on the incidence plot.
        let s = a.sections();
        let SectionContent::Survival(plot) = &s[1].content else { panic!() };
        assert_eq!(plot.points.len(), t.n_rows());
        // Incidences per group match the engine on the same rows.
        let g = &a.groups()[0];
        let obs: Vec<CompetingObservation> = g.rows.iter().map(|r| r.1).collect();
        assert_eq!(g.incidence, aalen_johansen(&obs, 3).unwrap());
        // The sample's story: higher load raises the wear-out mode's incidence.
        let Some(Ok(reg)) = a.regression() else { panic!() };
        assert!(reg.fit.coefficients[0] > 0.0);
        assert!(a.gray_tests().iter().all(std::result::Result::is_ok));
    }

    #[test]
    fn stacked_nominal_covariate_profiles_and_errors() {
        let t = pump_seal_failures();
        let mut rows = RowStateManager::new(t.n_rows());
        let mut r = request(&t);
        r.stacked = true;
        r.group = None;
        r.covariates = vec![col(&t, "Site")];
        r.regression_cause = Some("Seal leak".into());
        let a = CompetingRisksAnalysis::run(&t, &rows, &r).unwrap();
        let Some(Ok(reg)) = a.regression() else { panic!() };
        assert_eq!(reg.profiles.len(), 2, "one profile per site");
        assert!(a.gray_tests().is_empty());
        // The top stacked curve ends at 1 − Ŝ.
        let s = a.sections();
        let SectionContent::Survival(plot) = &s[1].content else { panic!() };
        let top = plot.curves.last().unwrap().steps.last().unwrap()[1];
        let surv = a.groups()[0].incidence.steps.last().unwrap().survival;
        assert!((top - (1.0 - surv)).abs() < 1e-12);
        // Explicit profiles.
        r.profiles = vec![vec!["North".into()], vec!["South".into()]];
        let b = CompetingRisksAnalysis::run(&t, &rows, &r).unwrap();
        assert!(matches!(b.regression(), Some(Ok(x)) if x.profiles.len() == 2));
        r.profiles = vec![vec!["Nowhere".into()]];
        let c = CompetingRisksAnalysis::run(&t, &rows, &r).unwrap();
        assert!(matches!(c.regression(), Some(Err(_))));
        // Excluding rows drops them.
        rows.set_excluded([0, 1], true);
        let d = CompetingRisksAnalysis::run(&t, &rows, &request(&t)).unwrap();
        let n: usize = d.groups().iter().map(|g| g.rows.len()).sum();
        assert_eq!(n, t.n_rows() - 2);
        let same = CompetingRisksRequest { cause: r.time, ..r.clone() };
        assert!(CompetingRisksAnalysis::run(&t, &rows, &same).is_err());
    }

    #[test]
    fn json_round_trip() {
        let t = pump_seal_failures();
        let mut r = request(&t);
        r.profiles = vec![vec!["10".into(), "North".into()]];
        r.times = vec![500.0];
        r.stacked = true;
        r.regression_cause = Some("Seal leak".into());
        let json = r.to_json(&t);
        assert_eq!(CompetingRisksRequest::from_json(&json, &t).unwrap(), r);
        let minimal = CompetingRisksRequest::new(0, 1);
        assert_eq!(CompetingRisksRequest::from_json(&minimal.to_json(&t), &t).unwrap(), minimal);
    }
}
