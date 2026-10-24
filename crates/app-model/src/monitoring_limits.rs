//! Environmental monitoring limits, bound to the row state
//! ([`stat_quality::monitoring_limits`]): alert and action levels for colony
//! counts, positive samples or continuous readings, set per limit group
//! (all locations together, each location, or the levels of a grouping column
//! such as a room grade) from the best-fitting distribution, from the mean
//! and standard deviation, typed in, or read from columns; each location
//! charted against its levels in time order with the early-warning rules; and
//! a summary of excursions by location.
//!
//! Rows missing the reading (or, for positives, the number of samples taken)
//! and excluded rows are left out. A missing location or limit-group value
//! is a level of its own, "(missing)". Each chart point is one dataset row,
//! so brushing links the charts with every other view.

use crate::attribute_chart::SubgroupSize;
use crate::format::format_number;
use crate::project::{invalid, member, obj, s};
use crate::report::{ChartPoint, ControlChartSpec, LimitStep, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_core::distribution_fit::FitFamily;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_quality::{
    Candidate, CountFamily, MonitoringFit, ReadingKind, WarningRules, best_candidate, fit_candidates, fit_counts,
    fit_positives, warning_rules,
};

const DIGITS: usize = 5;

/// Name of the level a missing location or group value forms.
pub const MISSING_LEVEL: &str = "(missing)";

/// The level used for every group without its own typed levels.
pub const ANY_GROUP: &str = "";

/// What the readings are, with stable keys.
#[must_use]
pub fn reading_kind_key(kind: ReadingKind) -> &'static str {
    match kind {
        ReadingKind::Counts => "counts",
        ReadingKind::Positives => "positives",
        ReadingKind::Continuous => "continuous",
    }
}

/// Display name of a reading kind.
#[must_use]
pub fn reading_kind_label(kind: ReadingKind) -> &'static str {
    match kind {
        ReadingKind::Counts => "Counts (e.g. colony-forming units)",
        ReadingKind::Positives => "Positive samples out of samples taken",
        ReadingKind::Continuous => "Continuous readings (zero or more)",
    }
}

/// Every reading kind, in menu order.
pub const READING_KINDS: [ReadingKind; 3] = [ReadingKind::Counts, ReadingKind::Positives, ReadingKind::Continuous];

/// The families a reading kind can use: `(key, label)`.
#[must_use]
pub fn families(kind: ReadingKind) -> Vec<(&'static str, &'static str)> {
    match kind {
        ReadingKind::Counts => CountFamily::COUNTS.iter().map(|f| (f.key(), f.label())).collect(),
        ReadingKind::Positives => CountFamily::POSITIVES.iter().map(|f| (f.key(), f.label())).collect(),
        ReadingKind::Continuous => FitFamily::ALL.iter().map(|f| (f.key(), f.label())).collect(),
    }
}

/// Which rows share one set of levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LimitsBy {
    /// One set for every location.
    All,
    /// Each location its own.
    #[default]
    Location,
    /// Each level of this column (e.g. a room grade).
    Column(usize),
}

/// Levels typed in for one limit group.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedLevels {
    /// The group ([`ANY_GROUP`] for every group without its own).
    pub group: String,
    /// Alert level.
    pub alert: f64,
    /// Action level.
    pub action: f64,
}

/// How the alert and action levels are set.
#[derive(Debug, Clone, PartialEq)]
pub enum LevelMethod {
    /// Quantiles of the fitted distribution.
    Quantiles {
        /// Probability of the alert level.
        alert: f64,
        /// Probability of the action level.
        action: f64,
    },
    /// The mean plus multiples of the standard deviation.
    Sigma {
        /// Multiple for the alert level.
        alert: f64,
        /// Multiple for the action level.
        action: f64,
    },
    /// Typed in, per limit group.
    Typed(Vec<TypedLevels>),
    /// Read from two columns (each row's value, else the first value in its
    /// limit group).
    Columns {
        /// Alert-level column.
        alert: usize,
        /// Action-level column.
        action: usize,
    },
}

impl LevelMethod {
    /// The default: the 0.975 and 0.9985 quantiles.
    pub const DEFAULT: Self = Self::Quantiles { alert: 0.975, action: 0.9985 };

    fn key(&self) -> &'static str {
        match self {
            Self::Quantiles { .. } => "quantiles",
            Self::Sigma { .. } => "sigma",
            Self::Typed(_) => "typed",
            Self::Columns { .. } => "columns",
        }
    }
}

/// What to analyse.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitoringRequest {
    /// Reading column (counts, positives or continuous values).
    pub value: usize,
    /// What the readings are.
    pub kind: ReadingKind,
    /// Samples taken per reading (positives only).
    pub trials: SubgroupSize,
    /// Location column (`None`: one location).
    pub location: Option<usize>,
    /// Time-order column (numeric or date); `None` keeps table order.
    pub order: Option<usize>,
    /// Which rows share levels.
    pub limits_by: LimitsBy,
    /// A family key to use instead of the best by AIC.
    pub family: Option<String>,
    /// How the levels are set.
    pub method: LevelMethod,
    /// Early-warning rules 3 and 4.
    pub rules: WarningRules,
    /// Chart only the locations with a signal.
    pub signals_only: bool,
}

impl MonitoringRequest {
    /// Counts in `value` by `location`, with quantile levels per location.
    #[must_use]
    pub fn new(value: usize, location: Option<usize>) -> Self {
        Self {
            value,
            kind: ReadingKind::Counts,
            trials: SubgroupSize::None,
            location,
            order: None,
            limits_by: if location.is_some() { LimitsBy::Location } else { LimitsBy::All },
            family: None,
            method: LevelMethod::DEFAULT,
            rules: WarningRules::default(),
            signals_only: false,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.value];
        if let SubgroupSize::Column(t) = self.trials {
            c.push(t);
        }
        c.extend(self.location);
        c.extend(self.order);
        if let LimitsBy::Column(g) = self.limits_by {
            c.push(g);
        }
        if let LevelMethod::Columns { alert, action } = self.method {
            c.extend([alert, action]);
        }
        c
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.value = f(self.value);
        if let SubgroupSize::Column(t) = &mut self.trials {
            *t = f(*t);
        }
        self.location = self.location.map(f);
        self.order = self.order.map(f);
        if let LimitsBy::Column(g) = &mut self.limits_by {
            *g = f(*g);
        }
        if let LevelMethod::Columns { alert, action } = &mut self.method {
            *alert = f(*alert);
            *action = f(*action);
        }
    }

    /// The project-file form (`"analysis": "monitoring-limits"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let mut m: Vec<(String, JsonValue)> = vec![
            ("analysis".into(), s("monitoring-limits")),
            ("value".into(), col(self.value)),
            ("readings".into(), s(reading_kind_key(self.kind))),
        ];
        match self.trials {
            SubgroupSize::Column(c) => m.push(("samples".into(), col(c))),
            SubgroupSize::Constant(n) => m.push(("samples".into(), JsonValue::number(n))),
            SubgroupSize::None => {}
        }
        if let Some(l) = self.location {
            m.push(("location".into(), col(l)));
        }
        if let Some(o) = self.order {
            m.push(("order".into(), col(o)));
        }
        m.push((
            "limits_by".into(),
            match self.limits_by {
                LimitsBy::All => s("all"),
                LimitsBy::Location => s("location"),
                LimitsBy::Column(c) => col(c),
            },
        ));
        if let Some(f) = &self.family {
            m.push(("family".into(), s(f)));
        }
        m.push(("levels".into(), s(self.method.key())));
        match &self.method {
            LevelMethod::Quantiles { alert, action } | LevelMethod::Sigma { alert, action } => {
                m.push(("alert".into(), JsonValue::number(*alert)));
                m.push(("action".into(), JsonValue::number(*action)));
            }
            LevelMethod::Typed(levels) => m.push((
                "typed".into(),
                JsonValue::Array(
                    levels
                        .iter()
                        .map(|l| {
                            obj([
                                ("group", s(&l.group)),
                                ("alert", JsonValue::number(l.alert)),
                                ("action", JsonValue::number(l.action)),
                            ])
                        })
                        .collect(),
                ),
            )),
            LevelMethod::Columns { alert, action } => {
                m.push(("alert".into(), col(*alert)));
                m.push(("action".into(), col(*action)));
            }
        }
        let pair = |p: Option<(usize, usize)>| {
            p.map_or(JsonValue::Null, |(a, b)| {
                JsonValue::Array(vec![JsonValue::number(a as f64), JsonValue::number(b as f64)])
            })
        };
        m.push(("alerts_in_window".into(), pair(self.rules.alerts_in_window)));
        m.push(("rising_run".into(), self.rules.rising_run.map_or(JsonValue::Null, |k| JsonValue::number(k as f64))));
        m.push(("signals_only".into(), JsonValue::Bool(self.signals_only)));
        JsonValue::Object(m)
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, unknown keys.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find = |c: &JsonValue| -> Result<usize> {
            let name = c.as_str().ok_or_else(|| invalid("monitoring limits: expected a column name"))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let opt_col = |key: &str| -> Result<Option<usize>> {
            match v.get(key) {
                None | Some(JsonValue::Null) => Ok(None),
                Some(c) => find(c).map(Some),
            }
        };
        let kind = match v.get("readings").and_then(JsonValue::as_str) {
            None | Some("counts") => ReadingKind::Counts,
            Some("positives") => ReadingKind::Positives,
            Some("continuous") => ReadingKind::Continuous,
            Some(other) => return Err(invalid(format!("monitoring limits: unknown readings {other:?}"))),
        };
        let trials = match v.get("samples") {
            None | Some(JsonValue::Null) => SubgroupSize::None,
            Some(JsonValue::String(_)) => SubgroupSize::Column(find(member(v, "samples")?)?),
            Some(n) => SubgroupSize::Constant(
                n.as_f64().ok_or_else(|| invalid("monitoring limits: samples must be a column or a number"))?,
            ),
        };
        let location = opt_col("location")?;
        let limits_by = match v.get("limits_by") {
            None | Some(JsonValue::Null) => {
                if location.is_some() {
                    LimitsBy::Location
                } else {
                    LimitsBy::All
                }
            }
            Some(JsonValue::String(k)) if k == "all" => LimitsBy::All,
            Some(JsonValue::String(k)) if k == "location" => LimitsBy::Location,
            Some(c) => LimitsBy::Column(find(c)?),
        };
        let num = |key: &str, d: f64| v.get(key).and_then(JsonValue::as_f64).unwrap_or(d);
        let method = match v.get("levels").and_then(JsonValue::as_str) {
            None | Some("quantiles") => {
                LevelMethod::Quantiles { alert: num("alert", 0.975), action: num("action", 0.9985) }
            }
            Some("sigma") => LevelMethod::Sigma { alert: num("alert", 2.0), action: num("action", 3.0) },
            Some("typed") => LevelMethod::Typed(
                member(v, "typed")?
                    .as_array()
                    .ok_or_else(|| invalid("monitoring limits: typed levels must be a list"))?
                    .iter()
                    .map(|e| {
                        let n = |k: &str| {
                            e.get(k)
                                .and_then(JsonValue::as_f64)
                                .ok_or_else(|| invalid(format!("typed levels: missing {k}")))
                        };
                        Ok(TypedLevels {
                            group: e.get("group").and_then(JsonValue::as_str).unwrap_or(ANY_GROUP).to_owned(),
                            alert: n("alert")?,
                            action: n("action")?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            ),
            Some("columns") => {
                LevelMethod::Columns { alert: find(member(v, "alert")?)?, action: find(member(v, "action")?)? }
            }
            Some(other) => return Err(invalid(format!("monitoring limits: unknown levels {other:?}"))),
        };
        let d = WarningRules::default();
        let alerts_in_window = match v.get("alerts_in_window") {
            None => d.alerts_in_window,
            Some(JsonValue::Array(a)) if a.len() == 2 => {
                Some((a[0].as_usize().unwrap_or(2), a[1].as_usize().unwrap_or(5)))
            }
            Some(_) => None,
        };
        let rising_run = match v.get("rising_run") {
            None => d.rising_run,
            Some(x) => x.as_usize(),
        };
        Ok(Self {
            value: find(member(v, "value")?)?,
            kind,
            trials,
            location,
            order: opt_col("order")?,
            limits_by,
            family: v.get("family").and_then(JsonValue::as_str).map(str::to_owned),
            method,
            rules: WarningRules { alerts_in_window, rising_run },
            signals_only: v.get("signals_only").and_then(JsonValue::as_bool).unwrap_or(false),
        })
    }
}

/// One limit group's levels and how they were set.
#[derive(Debug, Clone)]
struct GroupLevels {
    name: String,
    /// Readings in the group.
    n: usize,
    /// Every candidate family (quantile method only).
    candidates: Vec<Candidate>,
    /// The fit used (quantile method only).
    chosen: Option<usize>,
    /// Mean and standard deviation of the readings (on the plotted scale).
    mean: f64,
    sd: f64,
    /// Levels for a reading with the typical number of samples taken.
    alert: f64,
    action: f64,
}

/// One location's readings in time order.
#[derive(Debug, Clone)]
struct Location {
    name: String,
    group: usize,
    rows: Vec<usize>,
    values: Vec<f64>,
    alert: Vec<f64>,
    action: Vec<f64>,
    rules: Vec<Vec<u8>>,
}

impl Location {
    fn count(&self, rule: u8) -> usize {
        self.rules.iter().filter(|r| r.contains(&rule)).count()
    }

    fn has_signal(&self) -> bool {
        self.rules.iter().any(|r| !r.is_empty())
    }
}

/// A computed environmental monitoring study.
#[derive(Debug, Clone)]
pub struct MonitoringAnalysis {
    request: MonitoringRequest,
    value_name: String,
    location_name: Option<String>,
    group_name: Option<String>,
    order_labels: Vec<String>,
    groups: Vec<GroupLevels>,
    locations: Vec<Location>,
    /// Samples taken behind the "typical" levels of positives.
    typical_trials: f64,
    n_excluded: usize,
    n_missing: usize,
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn level_name(text: String) -> String {
    if text.is_empty() { MISSING_LEVEL.to_owned() } else { text }
}

/// The fit of `key` (a family of `kind`) to the readings.
fn fit_family(key: &str, values: &[f64], trials: &[f64], kind: ReadingKind) -> Result<MonitoringFit> {
    match kind {
        ReadingKind::Counts | ReadingKind::Positives => {
            let list = if kind == ReadingKind::Counts { &CountFamily::COUNTS[..] } else { &CountFamily::POSITIVES[..] };
            let f = list
                .iter()
                .find(|f| f.key() == key)
                .ok_or_else(|| invalid(format!("{key:?} is not a family for these readings")))?;
            if kind == ReadingKind::Counts { fit_counts(values, *f) } else { fit_positives(values, trials, *f) }
                .map(MonitoringFit::Count)
        }
        ReadingKind::Continuous => {
            let f = FitFamily::from_key(key).ok_or_else(|| invalid(format!("{key:?} is not a continuous family")))?;
            stat_core::distribution_fit::fit_distribution(values, f).map(MonitoringFit::Continuous)
        }
    }
}

fn median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    if n == 0 {
        f64::NAN
    } else if n % 2 == 1 {
        s[n / 2]
    } else {
        0.5 * (s[n / 2 - 1] + s[n / 2])
    }
}

impl MonitoringAnalysis {
    /// Set the levels and evaluate every location.
    ///
    /// # Errors
    /// Missing or non-numeric columns, positives without the samples taken,
    /// a text order column, no usable rows, a limit group whose distribution
    /// cannot be fitted, missing typed levels, an alert level above its action
    /// level, and errors from [`stat_quality::monitoring_limits`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &MonitoringRequest) -> Result<Self> {
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let numeric = |i: usize| -> Result<&[f64]> {
            let c = column(i)?;
            c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))
        };
        let value_col = column(request.value)?;
        let raw = numeric(request.value)?;
        let positives = request.kind == ReadingKind::Positives;
        let trials_all: Option<&[f64]> = match (positives, request.trials) {
            (false, _) => None,
            (true, SubgroupSize::Column(c)) if c == request.value => {
                return Err(invalid("the samples-taken column must differ from the positives column"));
            }
            (true, SubgroupSize::Column(c)) => Some(numeric(c)?),
            (true, SubgroupSize::Constant(_)) => None,
            (true, SubgroupSize::None) => {
                return Err(invalid("positive samples need the number of samples taken per reading"));
            }
        };
        let trials_of = |r: usize| match (positives, request.trials, trials_all) {
            (true, _, Some(t)) => t[r],
            (true, SubgroupSize::Constant(n), None) => n,
            _ => 1.0,
        };
        let order_vals = request.order.map(numeric).transpose()?;
        let location_col = request.location.map(column).transpose()?;
        let group_col = match request.limits_by {
            LimitsBy::Column(c) => Some(column(c)?),
            _ => None,
        };
        let level_cols = match request.method {
            LevelMethod::Columns { alert, action } => Some((numeric(alert)?, numeric(action)?)),
            _ => None,
        };
        let (mut n_excluded, mut n_missing) = (0, 0);
        let mut usable = Vec::new();
        for r in 0..table.n_rows() {
            let ok = raw[r].is_finite()
                && (!positives || trials_of(r).is_finite())
                && order_vals.is_none_or(|o| o[r].is_finite());
            if !ok {
                n_missing += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                usable.push(r);
            }
        }
        if usable.is_empty() {
            return Err(invalid("no usable readings"));
        }
        if let Some(o) = order_vals {
            usable.sort_by(|a, b| o[*a].total_cmp(&o[*b]).then(a.cmp(b)));
        }
        // Locations and limit groups, in order of first appearance.
        let mut loc_names: Vec<String> = Vec::new();
        let mut loc_of = Vec::with_capacity(usable.len());
        for &r in &usable {
            let name = location_col.map_or_else(|| value_col.name.clone(), |c| level_name(c.level_text(r)));
            let k = loc_names.iter().position(|n| *n == name).unwrap_or_else(|| {
                loc_names.push(name);
                loc_names.len() - 1
            });
            loc_of.push(k);
        }
        let mut group_names: Vec<String> = Vec::new();
        let mut group_of = Vec::with_capacity(usable.len());
        for (i, &r) in usable.iter().enumerate() {
            let name = match request.limits_by {
                LimitsBy::All => "All locations".to_owned(),
                LimitsBy::Location => loc_names[loc_of[i]].clone(),
                LimitsBy::Column(_) => level_name(group_col.map(|c| c.level_text(r)).unwrap_or_default()),
            };
            let g = group_names.iter().position(|n| *n == name).unwrap_or_else(|| {
                group_names.push(name);
                group_names.len() - 1
            });
            group_of.push(g);
        }
        // Plotted values (proportions for positives).
        let plotted: Vec<f64> =
            usable.iter().map(|&r| if positives { raw[r] / trials_of(r) } else { raw[r] }).collect();
        let typical_trials =
            if positives { median(&usable.iter().map(|&r| trials_of(r)).collect::<Vec<_>>()) } else { 1.0 };
        let mut alert = vec![f64::NAN; usable.len()];
        let mut action = vec![f64::NAN; usable.len()];
        let mut groups = Vec::new();
        for (g, name) in group_names.iter().enumerate() {
            let members: Vec<usize> = (0..usable.len()).filter(|&i| group_of[i] == g).collect();
            let values: Vec<f64> = members.iter().map(|&i| raw[usable[i]]).collect();
            let trials: Vec<f64> = members.iter().map(|&i| trials_of(usable[i])).collect();
            let shown: Vec<f64> = members.iter().map(|&i| plotted[i]).collect();
            let n = members.len();
            let mean = shown.iter().sum::<f64>() / n as f64;
            let sd = if n > 1 {
                (shown.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt()
            } else {
                f64::NAN
            };
            let mut level = GroupLevels {
                name: name.clone(),
                n,
                candidates: Vec::new(),
                chosen: None,
                mean,
                sd,
                alert: f64::NAN,
                action: f64::NAN,
            };
            let at = |l: f64, t: f64| if positives { l / t } else { l };
            match &request.method {
                LevelMethod::Quantiles { alert: qa, action: qx } => {
                    if !(0.0 < *qa && qa < qx && *qx < 1.0) {
                        return Err(invalid("the alert and action probabilities must satisfy 0 < alert < action < 1"));
                    }
                    let candidates = match &request.family {
                        None => fit_candidates(&values, &trials, request.kind),
                        Some(key) => {
                            let fit = fit_family(key, &values, &trials, request.kind)
                                .map_err(|e| invalid(format!("{name}: {e}")))?;
                            vec![Candidate { label: fit.label(), fit: Ok(fit) }]
                        }
                    };
                    let best = best_candidate(&candidates).ok_or_else(|| {
                        invalid(format!("{name}: no distribution could be fitted to its {n} readings"))
                    })?;
                    let fit = candidates[best].fit.as_ref().expect("best is a fit").clone();
                    for (k, &i) in members.iter().enumerate() {
                        alert[i] = at(fit.quantile(*qa, trials[k]), trials[k]);
                        action[i] = at(fit.quantile(*qx, trials[k]), trials[k]);
                    }
                    level.alert = at(fit.quantile(*qa, typical_trials), typical_trials);
                    level.action = at(fit.quantile(*qx, typical_trials), typical_trials);
                    level.candidates = candidates;
                    level.chosen = Some(best);
                }
                LevelMethod::Sigma { alert: ka, action: kx } => {
                    if !sd.is_finite() {
                        return Err(invalid(format!("{name}: needs at least two readings for a standard deviation")));
                    }
                    let sigma = |k: f64, t: f64| {
                        if positives { mean + k * (mean * (1.0 - mean) / t).sqrt() } else { mean + k * sd }
                    };
                    for (k, &i) in members.iter().enumerate() {
                        alert[i] = sigma(*ka, trials[k]);
                        action[i] = sigma(*kx, trials[k]);
                    }
                    level.alert = sigma(*ka, typical_trials);
                    level.action = sigma(*kx, typical_trials);
                }
                LevelMethod::Typed(typed) => {
                    let entry = typed
                        .iter()
                        .find(|t| t.group == *name)
                        .or_else(|| typed.iter().find(|t| t.group == ANY_GROUP))
                        .ok_or_else(|| invalid(format!("no alert and action levels were typed for {name}")))?;
                    for &i in &members {
                        alert[i] = entry.alert;
                        action[i] = entry.action;
                    }
                    level.alert = entry.alert;
                    level.action = entry.action;
                }
                LevelMethod::Columns { .. } => {
                    let (ca, cx) = level_cols.expect("level columns");
                    let first = |c: &[f64]| members.iter().map(|&i| c[usable[i]]).find(|v| v.is_finite());
                    let (fa, fx) = (first(ca), first(cx));
                    let (Some(fa), Some(fx)) = (fa, fx) else {
                        return Err(invalid(format!("{name}: no alert and action levels in the level columns")));
                    };
                    for &i in &members {
                        let r = usable[i];
                        alert[i] = if ca[r].is_finite() { ca[r] } else { fa };
                        action[i] = if cx[r].is_finite() { cx[r] } else { fx };
                    }
                    level.alert = fa;
                    level.action = fx;
                }
            }
            if members.iter().any(|&i| alert[i] > action[i]) {
                return Err(invalid(format!("{name}: the alert level must not exceed the action level")));
            }
            groups.push(level);
        }
        let mut locations: Vec<Location> = loc_names
            .iter()
            .map(|n| Location {
                name: n.clone(),
                group: 0,
                rows: Vec::new(),
                values: Vec::new(),
                alert: Vec::new(),
                action: Vec::new(),
                rules: Vec::new(),
            })
            .collect();
        let mut order_labels = vec![String::new(); table.n_rows()];
        for (i, &r) in usable.iter().enumerate() {
            let l = &mut locations[loc_of[i]];
            l.group = group_of[i];
            l.rows.push(r);
            l.values.push(plotted[i]);
            l.alert.push(alert[i]);
            l.action.push(action[i]);
            if let Some(o) = request.order {
                order_labels[r] = table.column(o).map(|c| c.level_text(r)).unwrap_or_default();
            }
        }
        for l in &mut locations {
            l.rules = warning_rules(&l.values, &l.alert, &l.action, &request.rules);
        }
        Ok(Self {
            request: request.clone(),
            value_name: value_col.name.clone(),
            location_name: location_col.map(|c| c.name.clone()),
            group_name: group_col.map(|c| c.name.clone()),
            order_labels,
            groups,
            locations,
            typical_trials,
            n_excluded,
            n_missing,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &MonitoringRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.location_name {
            Some(l) => format!("Monitoring Limits: {} by {l}", self.value_name),
            None => format!("Monitoring Limits: {}", self.value_name),
        }
    }

    /// Names of the locations, with the alert and action levels of their
    /// limit group (for a typical reading).
    #[must_use]
    pub fn location_levels(&self) -> Vec<(String, f64, f64)> {
        self.locations
            .iter()
            .map(|l| (l.name.clone(), self.groups[l.group].alert, self.groups[l.group].action))
            .collect()
    }

    /// Dataset rows that break any rule.
    #[must_use]
    pub fn signal_rows(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self
            .locations
            .iter()
            .flat_map(|l| l.rows.iter().zip(&l.rules).filter(|(_, r)| !r.is_empty()).map(|(row, _)| *row))
            .collect();
        v.sort_unstable();
        v
    }

    fn fmt(&self, v: f64) -> String {
        format_number(v, DIGITS)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        if matches!(self.request.method, LevelMethod::Quantiles { .. }) {
            out.push(self.fits());
            out.push(self.candidates());
        }
        out.push(self.levels());
        out.push(self.by_location());
        out.push(self.excursions());
        out.push(self.rules_note());
        let charted: Vec<&Location> =
            self.locations.iter().filter(|l| !self.request.signals_only || l.has_signal()).collect();
        if charted.is_empty() {
            out.push(section(
                SectionKind::MonitoringCharts,
                "Location Charts",
                SectionContent::Note("No location has a signal, so there are no charts to show.".to_owned()),
            ));
        }
        for l in charted {
            out.push(self.chart(l));
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        push("Readings", reading_kind_label(self.request.kind).to_owned());
        push("Reading column", self.value_name.clone());
        if self.request.kind == ReadingKind::Positives {
            push(
                "Samples taken",
                match self.request.trials {
                    SubgroupSize::Constant(n) => format_number(n, 6),
                    _ => format!("from a column (typical {})", format_number(self.typical_trials, 6)),
                },
            );
        }
        if let Some(l) = &self.location_name {
            push("Location column", l.clone());
        }
        push("Locations", self.locations.len().to_string());
        push(
            "Levels set for",
            match (&self.request.limits_by, &self.group_name) {
                (LimitsBy::All, _) => "all locations together".to_owned(),
                (LimitsBy::Location, _) => "each location".to_owned(),
                (LimitsBy::Column(_), Some(g)) => format!("each level of {g}"),
                (LimitsBy::Column(_), None) => "each group".to_owned(),
            },
        );
        push(
            "Levels from",
            match &self.request.method {
                LevelMethod::Quantiles { alert, action } => {
                    let fam = self.request.family.as_deref().map_or_else(
                        || "the family with the smallest AIC".to_owned(),
                        |k| {
                            families(self.request.kind)
                                .into_iter()
                                .find(|f| f.0 == k)
                                .map_or_else(|| k.to_owned(), |f| format!("the {} family", f.1))
                        },
                    );
                    format!("{} and {} quantiles of {fam}", format_number(*alert, 6), format_number(*action, 6))
                }
                LevelMethod::Sigma { alert, action } => {
                    format!("mean + {} and {} standard deviations", format_number(*alert, 4), format_number(*action, 4))
                }
                LevelMethod::Typed(_) => "typed in".to_owned(),
                LevelMethod::Columns { .. } => "level columns in the data".to_owned(),
            },
        );
        if self.n_excluded > 0 {
            push("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            push("Rows with a missing value", self.n_missing.to_string());
        }
        let total = |rule: u8| self.locations.iter().map(|l| l.count(rule)).sum::<usize>().to_string();
        push("Action excursions", total(1));
        push("Alert excursions", total(2));
        push("Locations with a signal", self.locations.iter().filter(|l| l.has_signal()).count().to_string());
        section(SectionKind::MonitoringSummary, "Summary", SectionContent::Table(t))
    }

    fn fits(&self) -> ReportSection {
        let mut t = TextTable::new(["Limit group", "Readings", "Family", "Parameters", "AIC"]);
        for g in &self.groups {
            let Some(fit) = g.chosen.and_then(|k| g.candidates[k].fit.as_ref().ok()) else { continue };
            let params = fit
                .parameters()
                .iter()
                .map(|(n, v)| format!("{n} = {}", format_number(*v, 4)))
                .collect::<Vec<_>>()
                .join(", ");
            t.push([g.name.clone(), g.n.to_string(), fit.label().to_owned(), params, self.fmt(fit.aic())]);
        }
        section(SectionKind::MonitoringFits, "Fitted Distributions", SectionContent::Table(t))
    }

    fn candidates(&self) -> ReportSection {
        let mut t = TextTable::new(["Limit group", "Family", "Log-likelihood", "AIC", "ΔAIC", "Note"]);
        for g in &self.groups {
            let best = g.chosen.and_then(|k| g.candidates[k].fit.as_ref().ok()).map_or(f64::NAN, MonitoringFit::aic);
            let mut rows: Vec<&Candidate> = g.candidates.iter().collect();
            rows.sort_by(|a, b| {
                let key = |c: &Candidate| c.fit.as_ref().map_or(f64::INFINITY, MonitoringFit::aic);
                key(a).total_cmp(&key(b))
            });
            for c in rows {
                match &c.fit {
                    Ok(f) => t.push([
                        g.name.clone(),
                        c.label.to_owned(),
                        self.fmt(f.log_likelihood()),
                        self.fmt(f.aic()),
                        format_number(f.aic() - best, 4),
                        if f.aic() == best { "chosen".to_owned() } else { String::new() },
                    ]),
                    Err(e) => t.push([
                        g.name.clone(),
                        c.label.to_owned(),
                        "—".to_owned(),
                        "—".to_owned(),
                        "—".to_owned(),
                        format!("not fitted: {e}"),
                    ]),
                }
            }
        }
        section(SectionKind::MonitoringCandidates, "Families Compared (smallest AIC first)", SectionContent::Table(t))
    }

    fn levels(&self) -> ReportSection {
        let scale = if self.request.kind == ReadingKind::Positives { " (proportion)" } else { "" };
        let mut t = TextTable::new([
            "Limit group".to_owned(),
            "Readings".to_owned(),
            format!("Mean{scale}"),
            "SD".to_owned(),
            format!("Alert level{scale}"),
            format!("Action level{scale}"),
        ]);
        for g in &self.groups {
            t.push([
                g.name.clone(),
                g.n.to_string(),
                self.fmt(g.mean),
                if g.sd.is_finite() { self.fmt(g.sd) } else { "—".to_owned() },
                self.fmt(g.alert),
                self.fmt(g.action),
            ]);
        }
        let mut title = "Alert and Action Levels".to_owned();
        if self.request.kind == ReadingKind::Positives
            && matches!(self.request.method, LevelMethod::Quantiles { .. } | LevelMethod::Sigma { .. })
        {
            title.push_str(&format!(" (for {} samples taken)", format_number(self.typical_trials, 6)));
        }
        section(SectionKind::MonitoringLevels, title, SectionContent::Table(t))
    }

    fn by_location(&self) -> ReportSection {
        let mut t = TextTable::new([
            "Location",
            "Limit group",
            "Readings",
            "Mean",
            "Maximum",
            "Alert excursions",
            "Action excursions",
            "Early warnings",
            "Status",
        ]);
        for l in &self.locations {
            let max = l.values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let mean = l.values.iter().sum::<f64>() / l.values.len() as f64;
            let (alerts, actions) = (l.count(2), l.count(1));
            let warnings = l.rules.iter().filter(|r| r.contains(&3) || r.contains(&4)).count();
            let status = if actions > 0 {
                "action"
            } else if alerts > 0 {
                "alert"
            } else if warnings > 0 {
                "early warning"
            } else {
                "within levels"
            };
            t.push([
                l.name.clone(),
                self.groups[l.group].name.clone(),
                l.values.len().to_string(),
                self.fmt(mean),
                self.fmt(max),
                alerts.to_string(),
                actions.to_string(),
                warnings.to_string(),
                status.to_owned(),
            ]);
        }
        section(SectionKind::MonitoringByLocation, "Summary by Location", SectionContent::Table(t))
    }

    fn excursions(&self) -> ReportSection {
        let title = "Excursions and Early Warnings";
        let mut t = TextTable::new(["Location", "Reading", "Row", "When", "Value", "Alert", "Action", "Rules"]);
        for l in &self.locations {
            for (k, rules) in l.rules.iter().enumerate() {
                if rules.is_empty() {
                    continue;
                }
                let r = l.rows[k];
                t.push([
                    l.name.clone(),
                    (k + 1).to_string(),
                    (r + 1).to_string(),
                    if self.order_labels[r].is_empty() { "—".to_owned() } else { self.order_labels[r].clone() },
                    self.fmt(l.values[k]),
                    self.fmt(l.alert[k]),
                    self.fmt(l.action[k]),
                    rules.iter().map(u8::to_string).collect::<Vec<_>>().join(", "),
                ]);
            }
        }
        if t.rows.is_empty() {
            return section(
                SectionKind::MonitoringExcursions,
                title,
                SectionContent::Note("None: every reading is at or below its alert level.".to_owned()),
            );
        }
        section(SectionKind::MonitoringExcursions, title, SectionContent::Table(t))
    }

    fn rules_note(&self) -> ReportSection {
        let mut lines = vec![
            "1 — action excursion: a reading above its action level.".to_owned(),
            "2 — alert excursion: a reading above its alert level (but not its action level).".to_owned(),
        ];
        if let Some((n, m)) = self.request.rules.alerts_in_window {
            lines.push(format!("3 — {n} or more readings above the alert level within {m} consecutive readings."));
        }
        if let Some(k) = self.request.rules.rising_run {
            lines.push(format!("4 — {k} increases in a row (a rising trend)."));
        }
        let text = format!(
            "Each location's readings are checked in time order. Rule numbers:\n{}\nA reading equal to a level \
             is not an excursion.",
            lines.join("\n")
        );
        section(SectionKind::MonitoringRules, "Early-Warning Rules", SectionContent::Note(text))
    }

    fn chart(&self, l: &Location) -> ReportSection {
        let points: Vec<ChartPoint> = l
            .values
            .iter()
            .enumerate()
            .map(|(k, v)| ChartPoint {
                sample: k + 1,
                value: *v,
                rows: vec![l.rows[k]],
                rules: l.rules[k].clone(),
                label: Some(self.order_labels[l.rows[k]].clone()).filter(|s| !s.is_empty()),
                note: None,
                signal: false,
            })
            .collect();
        let varies = l.alert.windows(2).any(|w| w[0] != w[1]) || l.action.windows(2).any(|w| w[0] != w[1]);
        let steps = if varies {
            (0..l.values.len())
                .map(|k| LimitStep {
                    from: k as f64 + 0.5,
                    to: k as f64 + 1.5,
                    center: f64::NAN,
                    lower: l.alert[k],
                    upper: l.action[k],
                    zone_sigma: 0.0,
                })
                .collect()
        } else {
            Vec::new()
        };
        let y_label = match self.request.kind {
            ReadingKind::Positives => format!("Proportion positive ({})", self.value_name),
            _ => self.value_name.clone(),
        };
        let spec = ControlChartSpec {
            x_label: "Reading (time order)".to_owned(),
            y_label,
            x_ticks: None,
            flag_label: None,
            limit_label: Some("Alert level (lower) and action level (upper)".to_owned()),
            points,
            center: f64::NAN,
            lower: l.alert.first().copied().unwrap_or(f64::NAN),
            upper: l.action.first().copied().unwrap_or(f64::NAN),
            zone_sigma: 0.0,
            steps,
            phases: Vec::new(),
        };
        let g = &self.groups[l.group];
        let title = if varies {
            format!("{}: alert {} and action {} for a typical reading", l.name, self.fmt(g.alert), self.fmt(g.action))
        } else {
            format!("{}: alert level {}, action level {}", l.name, self.fmt(l.alert[0]), self.fmt(l.action[0]))
        };
        section(SectionKind::MonitoringCharts, title, SectionContent::ControlChart(spec))
    }
}

#[cfg(test)]
mod tests;
