//! Non-compartmental pharmacokinetics bound to a dataset
//! ([`stat_inference::nca`]): per subject (and treatment) the peak
//! concentration and its time, the areas under the concentration and first
//! moment curves to the last sample and to infinity, the terminal rate
//! constant with its regression, half-life, mean residence time, and, with
//! a dose, clearance and volume; then each parameter summarized by
//! treatment (geometric mean and coefficient of variation, arithmetic mean,
//! standard deviation, median and range), and the profiles on linear and
//! semi-logarithmic axes.
//!
//! **Layout:** long data, one row per sample, with subject, time and
//! concentration columns, an optional treatment (or period) column whose
//! levels give separate profiles of the same subject, and an optional dose
//! column (the first value of each profile is used) or a single dose.
//! Concentrations below the limit of quantification should be entered as
//! 0. Excluded rows and rows with a missing time or concentration are left
//! out.

use crate::format::format_number;
use crate::project::{invalid as json_invalid, member, s};
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_inference::nca::{
    Administration, AucMethod, GeometricSummary, LambdaZRule, NcaResult, NcaSettings, geometric_summary,
    non_compartmental,
};

const DIGITS: usize = 5;
/// Most profiles drawn with their own line.
const MAX_LINES: usize = 200;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

fn opt(v: Option<f64>) -> String {
    v.map_or_else(|| "—".to_owned(), num)
}

/// Where the dose comes from.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum PkDose {
    /// No dose: clearance and volume are not reported.
    #[default]
    None,
    /// The same dose for every profile.
    Value(f64),
    /// A column; each profile uses its first value.
    Column(usize),
}

/// Columns and settings of a non-compartmental analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct PkRequest {
    /// Subject identifier.
    pub subject: usize,
    /// Sampling time.
    pub time: usize,
    /// Concentration.
    pub concentration: usize,
    /// Optional treatment or period: one profile per subject and level.
    pub treatment: Option<usize>,
    /// The dose.
    pub dose: PkDose,
    /// Area rule.
    pub method: AucMethod,
    /// Route of administration.
    pub administration: Administration,
    /// Terminal-phase point selection.
    pub lambda_rule: LambdaZRule,
}

impl PkRequest {
    /// Extravascular dosing, linear-up/log-down areas and the best-fit
    /// terminal phase, without a dose.
    #[must_use]
    pub fn new(subject: usize, time: usize, concentration: usize) -> Self {
        Self {
            subject,
            time,
            concentration,
            treatment: None,
            dose: PkDose::None,
            method: AucMethod::LinearUpLogDown,
            administration: Administration::Extravascular,
            lambda_rule: LambdaZRule::BestFit,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = vec![self.subject, self.time, self.concentration];
        out.extend(self.treatment);
        if let PkDose::Column(c) = self.dose {
            out.push(c);
        }
        out
    }

    /// Renumbers the request's columns with `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.subject = f(self.subject);
        self.time = f(self.time);
        self.concentration = f(self.concentration);
        self.treatment = self.treatment.map(f);
        if let PkDose::Column(c) = &mut self.dose {
            *c = f(*c);
        }
    }

    /// Project-file form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let mut m: Vec<(String, JsonValue)> = vec![
            ("analysis".into(), s("pharmacokinetics")),
            ("subject".into(), col(self.subject)),
            ("time".into(), col(self.time)),
            ("concentration".into(), col(self.concentration)),
        ];
        if let Some(t) = self.treatment {
            m.push(("treatment".into(), col(t)));
        }
        match self.dose {
            PkDose::None => {}
            PkDose::Value(v) => m.push(("dose".into(), JsonValue::number(v))),
            PkDose::Column(c) => m.push(("dose_column".into(), col(c))),
        }
        m.push(("auc_method".into(), s(self.method.key())));
        let (route, duration) = match self.administration {
            Administration::Extravascular => ("extravascular", None),
            Administration::IntravenousBolus => ("iv-bolus", None),
            Administration::IntravenousInfusion { duration } => ("iv-infusion", Some(duration)),
        };
        m.push(("administration".into(), s(route)));
        if let Some(d) = duration {
            m.push(("infusion_duration".into(), JsonValue::number(d)));
        }
        match self.lambda_rule {
            LambdaZRule::BestFit => m.push(("terminal".into(), s("best-fit"))),
            LambdaZRule::LastPoints(k) => {
                m.push(("terminal".into(), s("last-points")));
                m.push(("terminal_points".into(), JsonValue::number(k as f64)));
            }
            LambdaZRule::TimeRange(a, b) => {
                m.push(("terminal".into(), s("time-range")));
                m.push(("terminal_range".into(), JsonValue::Array(vec![JsonValue::number(a), JsonValue::number(b)])));
            }
        }
        JsonValue::Object(m)
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing columns or unknown settings.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find = |key: &str| -> Result<usize> {
            let name =
                member(v, key)?.as_str().ok_or_else(|| json_invalid(format!("{key}: expected a column name")))?;
            table.find(name).ok_or_else(|| json_invalid(format!("report refers to a missing column {name:?}")))
        };
        let optional = |key: &str| -> Result<Option<usize>> {
            match v.get(key) {
                None | Some(JsonValue::Null) => Ok(None),
                Some(_) => find(key).map(Some),
            }
        };
        let text = |key: &str| v.get(key).and_then(JsonValue::as_str);
        let dose = match (v.get("dose").and_then(JsonValue::as_f64), optional("dose_column")?) {
            (_, Some(c)) => PkDose::Column(c),
            (Some(d), None) => PkDose::Value(d),
            (None, None) => PkDose::None,
        };
        let method = match text("auc_method") {
            None => AucMethod::LinearUpLogDown,
            Some(k) => AucMethod::from_key(k).ok_or_else(|| json_invalid(format!("unknown AUC method {k:?}")))?,
        };
        let administration = match text("administration") {
            None | Some("extravascular") => Administration::Extravascular,
            Some("iv-bolus") => Administration::IntravenousBolus,
            Some("iv-infusion") => Administration::IntravenousInfusion {
                duration: v
                    .get("infusion_duration")
                    .and_then(JsonValue::as_f64)
                    .ok_or_else(|| json_invalid("an infusion needs infusion_duration"))?,
            },
            Some(k) => return Err(json_invalid(format!("unknown administration {k:?}"))),
        };
        let lambda_rule = match text("terminal") {
            None | Some("best-fit") => LambdaZRule::BestFit,
            Some("last-points") => LambdaZRule::LastPoints(
                v.get("terminal_points")
                    .and_then(JsonValue::as_usize)
                    .ok_or_else(|| json_invalid("terminal_points"))?,
            ),
            Some("time-range") => {
                let r: Vec<f64> = v
                    .get("terminal_range")
                    .and_then(JsonValue::as_array)
                    .map(|a| a.iter().filter_map(JsonValue::as_f64).collect())
                    .unwrap_or_default();
                match r.as_slice() {
                    [a, b] => LambdaZRule::TimeRange(*a, *b),
                    _ => return Err(json_invalid("terminal_range must be [start, end]")),
                }
            }
            Some(k) => return Err(json_invalid(format!("unknown terminal-phase rule {k:?}"))),
        };
        Ok(Self {
            subject: find("subject")?,
            time: find("time")?,
            concentration: find("concentration")?,
            treatment: optional("treatment")?,
            dose,
            method,
            administration,
            lambda_rule,
        })
    }
}

/// One concentration–time profile.
#[derive(Debug, Clone)]
struct Profile {
    subject: String,
    treatment: String,
    /// `(row, time, concentration)` sorted by time.
    samples: Vec<(usize, f64, f64)>,
    dose: Option<f64>,
    result: std::result::Result<NcaResult, String>,
}

/// A computed non-compartmental analysis.
#[derive(Debug, Clone)]
pub struct PkAnalysis {
    request: PkRequest,
    concentration_name: String,
    time_name: String,
    treatment_name: Option<String>,
    profiles: Vec<Profile>,
    n_skipped: usize,
}

/// The parameters reported, as (heading, getter, summarized geometrically).
type Getter = fn(&NcaResult) -> Option<f64>;
const PARAMETERS: [(&str, Getter, bool); 15] = [
    ("Cmax", |r| Some(r.cmax), true),
    ("Tmax", |r| Some(r.tmax), false),
    ("Clast", |r| Some(r.clast), true),
    ("Tlast", |r| Some(r.tlast), false),
    ("AUC 0–t", |r| Some(r.auc_last), true),
    ("AUMC 0–t", |r| Some(r.aumc_last), true),
    ("λz", |r| r.lambda_z.map(|l| l.lambda), true),
    ("t½", |r| r.half_life, true),
    ("AUC 0–∞", |r| r.auc_inf, true),
    ("AUC extrapolated (%)", |r| r.auc_pct_extrapolated, false),
    ("AUMC 0–∞", |r| r.aumc_inf, true),
    ("MRT", |r| r.mrt, true),
    ("CL", |r| r.clearance, true),
    ("Vz", |r| r.volume_z, true),
    ("λz points", |r| r.lambda_z.map(|l| l.n_points as f64), false),
];

impl PkAnalysis {
    /// Compute every profile.
    ///
    /// # Errors
    /// Missing or non-numeric time, concentration or dose columns, a column
    /// used twice, a dose that is not positive, or no usable profile.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &PkRequest) -> Result<Self> {
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("choose a different column for each role"));
        }
        let numeric = |c: usize| -> Result<&[f64]> {
            let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
            col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", col.name)))
        };
        let times = numeric(request.time)?;
        let conc = numeric(request.concentration)?;
        let dose_col = match request.dose {
            PkDose::Column(c) => Some(numeric(c)?),
            _ => None,
        };
        if let PkDose::Value(d) = request.dose {
            if !(d.is_finite() && d > 0.0) {
                return Err(invalid("the dose must be positive"));
            }
        }
        let subject = table.column(request.subject).ok_or_else(|| invalid("no subject column"))?;
        let treatment =
            request.treatment.map(|c| table.column(c).ok_or_else(|| invalid("no treatment column"))).transpose()?;
        let mut profiles: Vec<Profile> = Vec::new();
        let mut used = 0;
        for r in 0..table.n_rows() {
            if states.is_excluded(r) || !times[r].is_finite() || !conc[r].is_finite() {
                continue;
            }
            let id = subject.level_text(r);
            if id.is_empty() {
                continue;
            }
            let tr = treatment.map_or_else(String::new, |c| c.level_text(r));
            if treatment.is_some() && tr.is_empty() {
                continue;
            }
            used += 1;
            let p = match profiles.iter_mut().position(|p| p.subject == id && p.treatment == tr) {
                Some(i) => &mut profiles[i],
                None => {
                    profiles.push(Profile {
                        subject: id,
                        treatment: tr,
                        samples: Vec::new(),
                        dose: None,
                        result: Err(String::new()),
                    });
                    profiles.last_mut().expect("just pushed")
                }
            };
            p.samples.push((r, times[r], conc[r]));
            if p.dose.is_none() {
                p.dose = match (request.dose, dose_col) {
                    (PkDose::Value(d), _) => Some(d),
                    (PkDose::Column(_), Some(d)) if d[r].is_finite() && d[r] > 0.0 => Some(d[r]),
                    _ => None,
                };
            }
        }
        if profiles.is_empty() {
            return Err(invalid("no rows with a subject, time and concentration"));
        }
        for p in &mut profiles {
            p.samples.sort_by(|a, b| a.1.total_cmp(&b.1));
            let t: Vec<f64> = p.samples.iter().map(|s| s.1).collect();
            let c: Vec<f64> = p.samples.iter().map(|s| s.2).collect();
            let settings = NcaSettings {
                method: request.method,
                administration: request.administration,
                lambda_rule: request.lambda_rule,
                dose: p.dose,
            };
            p.result = non_compartmental(&t, &c, &settings).map_err(|e| e.to_string());
        }
        if profiles.iter().all(|p| p.result.is_err()) {
            let e = profiles[0].result.as_ref().err().cloned().unwrap_or_default();
            return Err(invalid(format!("no profile could be analysed: {e}")));
        }
        let name = |c: usize| table.column(c).map_or_else(String::new, |c| c.name.clone());
        Ok(Self {
            request: request.clone(),
            concentration_name: name(request.concentration),
            time_name: name(request.time),
            treatment_name: request.treatment.map(name),
            profiles,
            n_skipped: table.n_rows() - used,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &PkRequest {
        &self.request
    }

    /// Results per profile: `(subject, treatment, result)`.
    #[must_use]
    pub fn results(&self) -> Vec<(&str, &str, std::result::Result<&NcaResult, &str>)> {
        self.profiles
            .iter()
            .map(|p| (p.subject.as_str(), p.treatment.as_str(), p.result.as_ref().map_err(String::as_str)))
            .collect()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Pharmacokinetic Parameters: {} by {}", self.concentration_name, self.time_name)
    }

    fn has_dose(&self) -> bool {
        self.profiles.iter().any(|p| p.dose.is_some())
    }

    fn shown_parameters(&self) -> Vec<(&'static str, Getter, bool)> {
        let dose = self.has_dose();
        PARAMETERS.into_iter().filter(|(n, _, _)| dose || !matches!(*n, "CL" | "Vz")).collect()
    }

    fn treatments(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in &self.profiles {
            if !out.contains(&p.treatment) {
                out.push(p.treatment.clone());
            }
        }
        out.sort_by(|a, b| stat_data::natural_order(a, b));
        out
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![
            self.settings_section(),
            self.parameter_table(),
            self.summary_table(),
            self.terminal_table(),
            self.profile_plot(false),
            self.profile_plot(true),
        ]
    }

    fn settings_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Setting", "Value"]);
        let route = match self.request.administration {
            Administration::Extravascular => "Extravascular (CL and Vz are apparent: CL/F, Vz/F)".to_owned(),
            Administration::IntravenousBolus => "Intravenous bolus (C0 back-extrapolated when not sampled)".to_owned(),
            Administration::IntravenousInfusion { duration } => format!("Intravenous infusion over {}", num(duration)),
        };
        t.push(["Administration".to_owned(), route]);
        t.push(["Area rule".to_owned(), self.request.method.label().to_owned()]);
        let rule = match self.request.lambda_rule {
            LambdaZRule::BestFit => {
                "Best fit: largest adjusted R² over the last 3 or more points after Cmax (ties within 0.0001: most points)"
                    .to_owned()
            }
            LambdaZRule::LastPoints(k) => format!("The last {k} positive concentrations"),
            LambdaZRule::TimeRange(a, b) => format!("Positive concentrations from {} to {}", num(a), num(b)),
        };
        t.push(["Terminal phase".to_owned(), rule]);
        let dose = match self.request.dose {
            PkDose::None => "None (clearance and volume not reported)".to_owned(),
            PkDose::Value(d) => num(d),
            PkDose::Column(_) => "From the dose column (first value of each profile)".to_owned(),
        };
        t.push(["Dose".to_owned(), dose]);
        t.push(["Profiles".to_owned(), self.profiles.len().to_string()]);
        if self.n_skipped > 0 {
            t.push(["Rows left out (excluded or incomplete)".to_owned(), self.n_skipped.to_string()]);
        }
        section(SectionKind::PkSettings, "Analysis Settings", SectionContent::Table(t))
    }

    fn id_headings(&self) -> Vec<String> {
        let mut heads = vec!["Subject".to_owned()];
        if let Some(t) = &self.treatment_name {
            heads.push(t.clone());
        }
        heads
    }

    fn parameter_table(&self) -> ReportSection {
        let shown = self.shown_parameters();
        let mut heads = self.id_headings();
        heads.extend(shown.iter().map(|p| p.0.to_owned()));
        let mut t = TextTable::new(heads);
        for p in &self.profiles {
            let mut row = vec![p.subject.clone()];
            if self.treatment_name.is_some() {
                row.push(p.treatment.clone());
            }
            match &p.result {
                Ok(r) => row.extend(shown.iter().map(|(n, g, _)| {
                    if *n == "λz points" {
                        g(r).map_or_else(|| "—".to_owned(), |v| format!("{v}"))
                    } else {
                        opt(g(r))
                    }
                })),
                Err(e) => {
                    row.push(e.clone());
                    row.extend(std::iter::repeat_n(String::new(), shown.len() - 1));
                }
            }
            t.push(row);
        }
        section(SectionKind::PkParameters, "Parameters by Subject (non-compartmental)", SectionContent::Table(t))
    }

    fn summary_table(&self) -> ReportSection {
        let mut heads = vec!["Parameter".to_owned()];
        if let Some(t) = &self.treatment_name {
            heads.push(t.clone());
        }
        heads.extend(
            ["N", "Geometric mean", "Geometric CV (%)", "Mean", "Std dev", "Median", "Minimum", "Maximum"]
                .map(String::from),
        );
        let mut t = TextTable::new(heads);
        for (name, get, geometric) in self.shown_parameters() {
            if name == "λz points" {
                continue;
            }
            for tr in self.treatments() {
                let values: Vec<f64> = self
                    .profiles
                    .iter()
                    .filter(|p| p.treatment == tr)
                    .filter_map(|p| p.result.as_ref().ok().and_then(get))
                    .filter(|v| v.is_finite())
                    .collect();
                let g: GeometricSummary = geometric_summary(&values);
                let mut row = vec![name.to_owned()];
                if self.treatment_name.is_some() {
                    row.push(tr.clone());
                }
                let (gm, gcv) = if geometric {
                    (opt(g.geometric_mean), opt(g.geometric_cv.map(|c| 100.0 * c)))
                } else {
                    ("—".to_owned(), "—".to_owned())
                };
                row.extend([
                    g.n.to_string(),
                    gm,
                    gcv,
                    opt(g.arithmetic_mean),
                    opt(g.sd),
                    opt(g.median),
                    opt(g.min),
                    opt(g.max),
                ]);
                t.push(row);
            }
        }
        let by = self.treatment_name.as_ref().map_or_else(String::new, |n| format!(" by {n}"));
        section(
            SectionKind::PkSummary,
            format!("Summary{by} (geometric CV = √(exp(s²) − 1) of the log values)"),
            SectionContent::Table(t),
        )
    }

    fn terminal_table(&self) -> ReportSection {
        let mut heads = self.id_headings();
        heads.extend(["Points", "From", "To", "λz", "Intercept (ln C)", "R²", "Adjusted R²"].map(String::from));
        let mut t = TextTable::new(heads);
        for p in &self.profiles {
            let mut row = vec![p.subject.clone()];
            if self.treatment_name.is_some() {
                row.push(p.treatment.clone());
            }
            match p.result.as_ref().ok().and_then(|r| r.lambda_z) {
                Some(l) => row.extend([
                    l.n_points.to_string(),
                    num(l.start),
                    num(l.end),
                    num(l.lambda),
                    num(l.intercept),
                    format_number(l.r_squared, 6),
                    format_number(l.adj_r_squared, 6),
                ]),
                None => {
                    row.push("No terminal phase estimated".to_owned());
                    row.extend(std::iter::repeat_n(String::new(), 6));
                }
            }
            t.push(row);
        }
        section(
            SectionKind::PkTerminalPhase,
            "Terminal Phase Regressions (ln C = intercept − λz·t)",
            SectionContent::Table(t),
        )
    }

    fn profile_plot(&self, log: bool) -> ReportSection {
        let treatments = self.treatments();
        let mut points = Vec::new();
        let mut membership = Vec::new();
        let mut segments: Vec<Polyline> = Vec::new();
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let y = |c: f64| if log { c.ln() } else { c };
        for p in &self.profiles {
            let g = treatments.iter().position(|t| *t == p.treatment).unwrap_or(0);
            let mut line = Vec::new();
            for &(row, t, c) in &p.samples {
                if log && c <= 0.0 {
                    continue;
                }
                points.push(RowPoint { row, x: t, y: y(c) });
                membership.push(g);
                line.push([t, y(c)]);
                if c > 0.0 {
                    (lo, hi) = (lo.min(c), hi.max(c));
                }
            }
            if segments.len() < MAX_LINES {
                segments.push(line);
            }
            if log {
                if let Some(l) = p.result.as_ref().ok().and_then(|r| r.lambda_z) {
                    segments.push(vec![
                        [l.start, l.intercept - l.lambda * l.start],
                        [l.end, l.intercept - l.lambda * l.end],
                    ]);
                }
            }
        }
        let labels: Vec<String> =
            if self.treatment_name.is_some() { treatments.clone() } else { vec!["Samples".to_owned()] };
        let y_ticks = (log && lo.is_finite()).then(|| {
            crate::life_stress::log_ticks(lo, hi).into_iter().map(|v| (v.ln(), format_number(v, 4))).collect()
        });
        let spec = PlotSpec {
            x_label: self.time_name.clone(),
            y_label: if log {
                format!("{} (log scale)", self.concentration_name)
            } else {
                self.concentration_name.clone()
            },
            points,
            groups: Some(PointGroups {
                title: self.treatment_name.clone().unwrap_or_default(),
                lines: vec![Vec::new(); labels.len()],
                labels,
                membership,
            }),
            segments,
            y_ticks,
            ..PlotSpec::default()
        };
        if log {
            section(
                SectionKind::PkSemiLogPlot,
                "Profiles on a Log Scale (positive concentrations; terminal regressions drawn over their points)",
                SectionContent::Plot(Box::new(spec)),
            )
        } else {
            section(SectionKind::PkProfilePlot, "Concentration–Time Profiles", SectionContent::Plot(Box::new(spec)))
        }
    }
}

#[cfg(test)]
mod tests;
