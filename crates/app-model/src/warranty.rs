//! Warranty returns bound to a dataset ([`stat_reliability::warranty`]):
//! shipments per period and returns by period of shipment and period of
//! return become interval- and right-censored life data (ages in periods), to
//! which life distributions are fitted and compared (the probability plot and
//! estimates come from [`crate::life_distribution`]); the chosen fit forecasts
//! returns per future calendar period from the units still in service, plus
//! any planned shipments, with prediction limits.
//!
//! **Layouts.** *Triangular* (a "Nevada" table): one row per shipment period
//! in calendar order, a column of units shipped, and one column per calendar
//! period of return, the first being the first row's period (cells before a
//! row's own period are empty). The number of return columns is the number
//! of periods observed. *Long*: one row per count, with numeric shipment and
//! return period numbers, the count returned, and the units shipped in that
//! row's shipment period (repeated on its rows; a row with an empty return
//! period carries only the shipment). Excluded rows are left out.

use crate::format::format_number;
use crate::life_distribution::{LifeDataLayout, LifeDistributionAnalysis, LifeDistributionRequest};
use crate::project::{invalid, member, s};
use crate::report::{Annotations, PlotSpec, Polyline, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_core::distribution_fit::FitFamily;
use stat_data::{Column, DataTable, JsonValue, RowStateManager};
use stat_reliability::warranty::{WarrantyCohort, WarrantyData, WarrantyForecast, forecast_returns};

const DIGITS: usize = 5;

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

/// Where the shipment and return counts come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarrantyLayout {
    /// One row per shipment period; returns by calendar period across columns.
    Triangular {
        /// Units shipped.
        shipped: usize,
        /// Returns in each calendar period, in order (the first is the first
        /// row's shipment period).
        returns: Vec<usize>,
        /// Optional period labels.
        label: Option<usize>,
    },
    /// One row per (shipment period, return period) count.
    Long {
        /// Shipment period number.
        ship_period: usize,
        /// Return period number (empty: a shipment-only row).
        return_period: usize,
        /// Units returned.
        returns: usize,
        /// Units shipped in the row's shipment period.
        shipped: usize,
    },
}

/// Columns and settings of a warranty-returns analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct WarrantyRequest {
    /// Data layout.
    pub layout: WarrantyLayout,
    /// Families fitted and compared.
    pub families: Vec<FitFamily>,
    /// Family used for the forecast (`None`: the best by AIC).
    pub detail: Option<FitFamily>,
    /// Future periods forecast.
    pub horizon: usize,
    /// Planned shipments in future periods 1, 2, ….
    pub planned: Vec<f64>,
    /// Warranty length in periods (`None`: returns accepted at any age).
    pub warranty: Option<f64>,
    /// Confidence level of intervals and prediction limits.
    pub confidence: f64,
}

impl WarrantyRequest {
    /// Default confidence level.
    pub const DEFAULT_CONFIDENCE: f64 = 0.95;
    /// Default forecast horizon (periods).
    pub const DEFAULT_HORIZON: usize = 12;
    /// Families compared by default.
    pub const DEFAULT_FAMILIES: [FitFamily; 4] =
        [FitFamily::Weibull, FitFamily::Lognormal, FitFamily::Exponential, FitFamily::LogLogistic];

    /// Counts in `layout` with default settings (Weibull forecast).
    #[must_use]
    pub fn new(layout: WarrantyLayout) -> Self {
        Self {
            layout,
            families: Self::DEFAULT_FAMILIES.to_vec(),
            detail: Some(FitFamily::Weibull),
            horizon: Self::DEFAULT_HORIZON,
            planned: Vec::new(),
            warranty: None,
            confidence: Self::DEFAULT_CONFIDENCE,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match &self.layout {
            WarrantyLayout::Triangular { shipped, returns, label } => {
                let mut out = vec![*shipped];
                out.extend(returns);
                out.extend(*label);
                out
            }
            WarrantyLayout::Long { ship_period, return_period, returns, shipped } => {
                vec![*ship_period, *return_period, *returns, *shipped]
            }
        }
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        match &mut self.layout {
            WarrantyLayout::Triangular { shipped, returns, label } => {
                *shipped = f(*shipped);
                for r in returns.iter_mut() {
                    *r = f(*r);
                }
                *label = label.map(f);
            }
            WarrantyLayout::Long { ship_period, return_period, returns, shipped } => {
                *ship_period = f(*ship_period);
                *return_period = f(*return_period);
                *returns = f(*returns);
                *shipped = f(*shipped);
            }
        }
    }

    /// The project-file (JSON) form, columns by name.
    pub(crate) fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let n = JsonValue::number;
        let mut m: Vec<(String, JsonValue)> = vec![("analysis".into(), s("warranty-returns"))];
        match &self.layout {
            WarrantyLayout::Triangular { shipped, returns, label } => {
                m.push(("layout".into(), s("triangular")));
                m.push(("shipped".into(), col(*shipped)));
                m.push(("returns".into(), JsonValue::Array(returns.iter().map(|&c| col(c)).collect())));
                if let Some(c) = label {
                    m.push(("label".into(), col(*c)));
                }
            }
            WarrantyLayout::Long { ship_period, return_period, returns, shipped } => {
                m.push(("layout".into(), s("long")));
                m.push(("ship_period".into(), col(*ship_period)));
                m.push(("return_period".into(), col(*return_period)));
                m.push(("returns".into(), col(*returns)));
                m.push(("shipped".into(), col(*shipped)));
            }
        }
        m.push(("families".into(), JsonValue::Array(self.families.iter().map(|f| s(f.key())).collect())));
        if let Some(f) = self.detail {
            m.push(("detail".into(), s(f.key())));
        }
        m.push(("horizon".into(), n(self.horizon as f64)));
        if !self.planned.is_empty() {
            m.push(("planned".into(), JsonValue::Array(self.planned.iter().map(|&x| n(x)).collect())));
        }
        if let Some(w) = self.warranty {
            m.push(("warranty".into(), n(w)));
        }
        m.push(("confidence".into(), n(self.confidence)));
        JsonValue::Object(m)
    }

    /// Read the JSON form (see [`to_json`](Self::to_json)).
    pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let name_of = |x: &JsonValue, key: &str| -> Result<usize> {
            let name = x.as_str().ok_or_else(|| invalid(format!("warranty {key}: expected a column name")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let col = |key: &str| name_of(member(v, key)?, key);
        let text = |key: &str| v.get(key).and_then(JsonValue::as_str);
        let layout = match text("layout") {
            None | Some("triangular") => WarrantyLayout::Triangular {
                shipped: col("shipped")?,
                returns: member(v, "returns")?
                    .as_array()
                    .ok_or_else(|| invalid("warranty returns: expected a list of columns"))?
                    .iter()
                    .map(|x| name_of(x, "returns"))
                    .collect::<Result<_>>()?,
                label: match v.get("label") {
                    None | Some(JsonValue::Null) => None,
                    Some(x) => Some(name_of(x, "label")?),
                },
            },
            Some("long") => WarrantyLayout::Long {
                ship_period: col("ship_period")?,
                return_period: col("return_period")?,
                returns: col("returns")?,
                shipped: col("shipped")?,
            },
            Some(other) => return Err(invalid(format!("warranty: unknown layout {other:?}"))),
        };
        let family =
            |k: &str| FitFamily::from_key(k).ok_or_else(|| invalid(format!("warranty: unknown distribution {k:?}")));
        let mut r = Self::new(layout);
        if let Some(items) = v.get("families").and_then(JsonValue::as_array) {
            r.families = items.iter().map(|x| family(x.as_str().unwrap_or_default())).collect::<Result<_>>()?;
        }
        r.detail = text("detail").map(family).transpose()?;
        r.horizon = v.get("horizon").and_then(JsonValue::as_usize).unwrap_or(Self::DEFAULT_HORIZON);
        if let Some(items) = v.get("planned").and_then(JsonValue::as_array) {
            r.planned = items.iter().filter_map(JsonValue::as_f64).collect();
        }
        r.warranty = v.get("warranty").and_then(JsonValue::as_f64);
        r.confidence = v.get("confidence").and_then(JsonValue::as_f64).unwrap_or(Self::DEFAULT_CONFIDENCE);
        Ok(r)
    }
}

/// A computed warranty analysis.
#[derive(Debug, Clone)]
pub struct WarrantyAnalysis {
    request: WarrantyRequest,
    /// Label of each cohort's shipment period.
    labels: Vec<String>,
    /// The counts as cohorts.
    data: WarrantyData,
    life: LifeDistributionAnalysis,
    /// Dataset row of each life-data row, for brushing the probability plot.
    life_rows: Vec<usize>,
    forecast: std::result::Result<(FitFamily, WarrantyForecast), String>,
    n_skipped: usize,
}

fn value(table: &DataTable, col: usize, row: usize) -> f64 {
    table.column(col).and_then(|c| c.as_numeric()).map_or(f64::NAN, |v| v[row])
}

/// Cohorts with their labels and the dataset row standing for each cohort
/// and for each (cohort, age) cell.
struct Gathered {
    cohorts: Vec<WarrantyCohort>,
    labels: Vec<String>,
    periods: usize,
    cohort_row: Vec<usize>,
    /// `(cohort, age) → row`.
    cell_row: Vec<((usize, usize), usize)>,
    n_skipped: usize,
}

fn gather_triangular(
    table: &DataTable,
    states: &RowStateManager,
    shipped: usize,
    returns: &[usize],
    label: Option<usize>,
) -> Result<Gathered> {
    if returns.is_empty() {
        return Err(invalid("choose the return columns (one per calendar period)"));
    }
    let periods = returns.len();
    let label_col = label.and_then(|c| table.column(c));
    let mut g = Gathered {
        cohorts: Vec::new(),
        labels: Vec::new(),
        periods,
        cohort_row: Vec::new(),
        cell_row: Vec::new(),
        n_skipped: 0,
    };
    let mut period = 0;
    for r in 0..table.n_rows() {
        let n = value(table, shipped, r);
        if !n.is_finite() {
            g.n_skipped += 1;
            continue;
        }
        let s = period;
        period += 1;
        if s >= periods {
            return Err(invalid(format!(
                "there are more shipment rows than return columns ({periods}): each row is a shipment period and \
                 each return column a calendar period, the first being the first row's"
            )));
        }
        if states.is_excluded(r) {
            g.n_skipped += 1;
            continue;
        }
        let mut rets = Vec::new();
        for (j, &c) in returns.iter().enumerate() {
            let v = value(table, c, r);
            if j < s {
                if v.is_finite() && v != 0.0 {
                    return Err(invalid(format!(
                        "row {}: returns before the shipment period (column {} is an earlier calendar period)",
                        r + 1,
                        table.column(c).map_or("", |x| x.name.as_str())
                    )));
                }
                continue;
            }
            rets.push(if v.is_finite() { v } else { 0.0 });
        }
        while rets.last() == Some(&0.0) {
            rets.pop();
        }
        g.labels.push(label_col.map_or_else(|| format!("Period {}", s + 1), |c| c.level_text(r)));
        g.cohort_row.push(r);
        let k = g.cohorts.len();
        for a in 0..rets.len() {
            g.cell_row.push(((k, a), r));
        }
        g.cohorts.push(WarrantyCohort { period: s, shipped: n, returns: rets });
    }
    Ok(g)
}

fn gather_long(
    table: &DataTable,
    states: &RowStateManager,
    ship_period: usize,
    return_period: usize,
    returns: usize,
    shipped: usize,
) -> Result<Gathered> {
    // (ship period number, shipped, rows, returns by return period number).
    type ShipGroup = (f64, Option<f64>, Vec<usize>, Vec<(f64, f64, usize)>);
    let mut by_ship: Vec<ShipGroup> = Vec::new();
    let mut n_skipped = 0;
    let integral = |x: f64| x.is_finite() && (x - x.round()).abs() < 1e-9;
    for r in 0..table.n_rows() {
        let sp = value(table, ship_period, r);
        if states.is_excluded(r) || !integral(sp) {
            n_skipped += 1;
            continue;
        }
        let rp = value(table, return_period, r);
        let count = value(table, returns, r);
        let n = value(table, shipped, r);
        let k = match by_ship.iter().position(|b| b.0 == sp) {
            Some(k) => k,
            None => {
                by_ship.push((sp, None, Vec::new(), Vec::new()));
                by_ship.len() - 1
            }
        };
        let entry = &mut by_ship[k];
        entry.2.push(r);
        if n.is_finite() {
            match entry.1 {
                Some(prev) if (prev - n).abs() > 1e-9 * prev.abs().max(1.0) => {
                    return Err(invalid(format!(
                        "shipment period {sp}: the units shipped differ between rows ({prev} and {n})"
                    )));
                }
                _ => entry.1 = Some(n),
            }
        }
        if integral(rp) && count.is_finite() {
            if rp < sp {
                return Err(invalid(format!("row {}: returned in period {rp}, before shipment in {sp}", r + 1)));
            }
            entry.3.push((rp, count, r));
        }
    }
    if by_ship.is_empty() {
        return Err(invalid("no rows with a whole-number shipment period"));
    }
    let first = by_ship.iter().map(|b| b.0).fold(f64::INFINITY, f64::min);
    let last = by_ship
        .iter()
        .flat_map(|b| std::iter::once(b.0).chain(b.3.iter().map(|x| x.0)))
        .fold(f64::NEG_INFINITY, f64::max);
    let periods = (last - first) as usize + 1;
    by_ship.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut g = Gathered {
        cohorts: Vec::new(),
        labels: Vec::new(),
        periods,
        cohort_row: Vec::new(),
        cell_row: Vec::new(),
        n_skipped,
    };
    for (sp, n, rows, rets) in by_ship {
        let Some(n) = n else {
            return Err(invalid(format!("shipment period {sp}: no row gives the units shipped")));
        };
        let s = (sp - first) as usize;
        let k = g.cohorts.len();
        let mut counts = Vec::new();
        for (rp, c, r) in rets {
            let a = (rp - sp) as usize;
            if counts.len() <= a {
                counts.resize(a + 1, 0.0);
            }
            counts[a] += c;
            if !g.cell_row.iter().any(|x| x.0 == (k, a)) {
                g.cell_row.push(((k, a), r));
            }
        }
        g.labels.push(format_number(sp, 6));
        g.cohort_row.push(rows[0]);
        g.cohorts.push(WarrantyCohort { period: s, shipped: n, returns: counts });
    }
    Ok(g)
}

impl WarrantyAnalysis {
    /// Build the life data, fit the families and forecast.
    ///
    /// # Errors
    /// Missing or non-numeric columns, a column used twice, inconsistent
    /// counts (more returns than shipments, returns before shipment or after
    /// the warranty), no families, a horizon of zero, or a confidence level
    /// outside (0.5, 1).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &WarrantyRequest) -> Result<Self> {
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("choose a different column for each role"));
        }
        let label = match &request.layout {
            WarrantyLayout::Triangular { label, .. } => *label,
            WarrantyLayout::Long { .. } => None,
        };
        for &c in &cols {
            let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
            if Some(c) != label && col.as_numeric().is_none() {
                return Err(invalid(format!("{} must be numeric", col.name)));
            }
        }
        if request.families.is_empty() {
            return Err(invalid("choose at least one distribution"));
        }
        if request.horizon == 0 {
            return Err(invalid("forecast at least one period"));
        }
        if !(request.confidence > 0.5 && request.confidence < 1.0) {
            return Err(invalid("the confidence level must be between 0.5 and 1"));
        }
        let g = match &request.layout {
            WarrantyLayout::Triangular { shipped, returns, label } => {
                gather_triangular(table, states, *shipped, returns, *label)?
            }
            WarrantyLayout::Long { ship_period, return_period, returns, shipped } => {
                gather_long(table, states, *ship_period, *return_period, *returns, *shipped)?
            }
        };
        let data = WarrantyData::new(g.cohorts, g.periods, request.warranty)?;
        // The life data as a table, analysed as interval-censored life data.
        let rows = data.life_rows();
        let life_rows: Vec<usize> = rows
            .iter()
            .map(|r| {
                if r.observation.upper.is_finite() {
                    let a = r.observation.lower as usize;
                    g.cell_row.iter().find(|x| x.0 == (r.cohort, a)).map_or(g.cohort_row[r.cohort], |x| x.1)
                } else {
                    g.cohort_row[r.cohort]
                }
            })
            .collect();
        let finite = |v: f64| if v.is_finite() { v } else { f64::NAN };
        let life_table = DataTable::new(
            "Warranty life data",
            vec![
                Column::numeric("Age from (periods)", rows.iter().map(|r| finite(r.observation.lower)).collect()),
                Column::numeric("Age to (periods)", rows.iter().map(|r| finite(r.observation.upper)).collect()),
                Column::numeric("Units", rows.iter().map(|r| r.observation.weight).collect()),
            ],
        )?;
        let life_request = LifeDistributionRequest {
            frequency: Some(2),
            families: request.families.clone(),
            detail: request.detail,
            confidence: request.confidence,
            probabilities: vec![0.01, 0.05, 0.1],
            ..LifeDistributionRequest::new(LifeDataLayout::Intervals { start: 0, end: 1 })
        };
        let life = LifeDistributionAnalysis::run(&life_table, &RowStateManager::new(rows.len()), &life_request)?;
        let forecast = match life.detail() {
            None => Err("no distribution could be fitted".to_owned()),
            Some(family) => match life.groups().first().and_then(|grp| grp.fit(family)) {
                None => Err(format!("the {} distribution could not be fitted", family.label())),
                Some(fit) => forecast_returns(&data, fit, request.horizon, &request.planned, request.confidence)
                    .map(|f| (family, f))
                    .map_err(|e| e.to_string()),
            },
        };
        Ok(Self { request: request.clone(), labels: g.labels, data, life, life_rows, forecast, n_skipped: g.n_skipped })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &WarrantyRequest {
        &self.request
    }

    /// The counts as cohorts.
    #[must_use]
    pub fn data(&self) -> &WarrantyData {
        &self.data
    }

    /// The life-distribution analysis of the derived life data.
    #[must_use]
    pub fn life(&self) -> &LifeDistributionAnalysis {
        &self.life
    }

    /// The family used and its forecast, or why there is none.
    pub fn forecast(&self) -> std::result::Result<&(FitFamily, WarrantyForecast), &String> {
        self.forecast.as_ref()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let shipped: f64 = self.data.cohorts().iter().map(|c| c.shipped).sum();
        format!("Warranty Returns: {} periods, {} units shipped", self.data.periods(), num(shipped))
    }

    fn level(&self) -> String {
        format!("{}%", format_number(self.request.confidence * 100.0, 4))
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.life_data_table()];
        for s in self.life.sections() {
            match s.kind {
                SectionKind::LifeModelComparison | SectionKind::LifeParameters | SectionKind::LifePercentiles => {
                    out.push(s);
                }
                SectionKind::LifeProbabilityPlot => {
                    let mut s = s;
                    if let SectionContent::Plot(p) = &mut s.content {
                        for point in &mut p.points {
                            point.row = self.life_rows.get(point.row).copied().unwrap_or(point.row);
                        }
                        p.x_label = p.x_label.replace("Age from (periods)–Age to (periods)", "Age (periods)");
                    }
                    out.push(s);
                }
                _ => {}
            }
        }
        match &self.forecast {
            Ok((family, f)) => {
                out.push(self.forecast_plot(*family, f));
                out.push(self.forecast_table(*family, f));
                out.push(self.cumulative_table(f));
            }
            Err(e) => out.push(section(
                SectionKind::WarrantyForecastTable,
                "Forecast",
                SectionContent::Note(format!("No forecast: {e}")),
            )),
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new([
            "Shipment period",
            "Shipped",
            "Returned",
            "In service",
            "Age (periods)",
            "Fraction returned",
        ]);
        let (mut shipped, mut returned) = (0.0, 0.0);
        for (c, label) in self.data.cohorts().iter().zip(&self.labels) {
            shipped += c.shipped;
            returned += c.returned();
            t.push([
                label.clone(),
                num(c.shipped),
                num(c.returned()),
                num(c.in_service()),
                num(self.data.current_age(c)),
                format_number(if c.shipped > 0.0 { c.returned() / c.shipped } else { f64::NAN }, 4),
            ]);
        }
        t.push([
            "Total".to_owned(),
            num(shipped),
            num(returned),
            num(shipped - returned),
            String::new(),
            format_number(returned / shipped, 4),
        ]);
        let mut title = format!(
            "Shipments and Returns ({} calendar periods observed; units enter service at the start of their shipment period",
            self.data.periods()
        );
        if let Some(w) = self.data.warranty() {
            title.push_str(&format!("; warranty {} periods", num(w)));
        }
        if self.n_skipped > 0 {
            title.push_str(&format!("; {} rows excluded or empty", self.n_skipped));
        }
        title.push(')');
        section(SectionKind::WarrantySummary, title, SectionContent::Table(t))
    }

    fn life_data_table(&self) -> ReportSection {
        // Aggregated over cohorts: returns per age interval and units censored at each age.
        let mut returned: Vec<f64> = Vec::new();
        let mut censored: Vec<(f64, f64)> = Vec::new();
        for r in self.data.life_rows() {
            let o = r.observation;
            if o.upper.is_finite() {
                let a = o.lower as usize;
                if returned.len() <= a {
                    returned.resize(a + 1, 0.0);
                }
                returned[a] += o.weight;
            } else {
                match censored.iter_mut().find(|c| c.0 == o.lower) {
                    Some(c) => c.1 += o.weight,
                    None => censored.push((o.lower, o.weight)),
                }
            }
        }
        censored.sort_by(|a, b| a.0.total_cmp(&b.0));
        let max_age = censored.last().map_or(0.0, |c| c.0).max(returned.len() as f64);
        let mut t = TextTable::new([
            "Age from (periods)",
            "Age to (periods)",
            "Returned (interval-censored)",
            "Still in service at the end of this age (right-censored)",
        ]);
        let mut a = 0usize;
        while (a as f64) < max_age {
            let r = returned.get(a).copied().unwrap_or(0.0);
            let c = censored.iter().find(|c| c.0 == (a + 1) as f64).map_or(0.0, |c| c.1);
            if r > 0.0 || c > 0.0 {
                t.push([num(a as f64), num((a + 1) as f64), num(r), num(c)]);
            }
            a += 1;
        }
        section(
            SectionKind::WarrantyLifeData,
            "Life Data (ages in periods: a return in the k-th period of service failed at an age in (k − 1, k]; \
             units not returned are censored at their current age)",
            SectionContent::Table(t),
        )
    }

    fn forecast_plot(&self, family: FitFamily, f: &WarrantyForecast) -> ReportSection {
        let p = self.data.periods();
        let observed = self.data.returns_by_period();
        let fit = self
            .life
            .groups()
            .first()
            .and_then(|g| g.fit(family))
            .map(|c| self.data.fitted_by_period(&c.distribution))
            .unwrap_or_default();
        let mut line: Polyline = fit.iter().enumerate().map(|(i, &v)| [(i + 1) as f64, v]).collect();
        line.extend(f.periods.iter().map(|v| [(p + v.period) as f64, v.expected]));
        let anchor = line.get(p.saturating_sub(1)).copied();
        let mut lower: Polyline = anchor.into_iter().collect();
        let mut upper: Polyline = anchor.into_iter().collect();
        lower.extend(f.periods.iter().map(|v| [(p + v.period) as f64, v.lower]));
        upper.extend(f.periods.iter().map(|v| [(p + v.period) as f64, v.upper]));
        let top = f.periods.iter().map(|v| v.upper).chain(observed.iter().copied()).fold(0.0, f64::max);
        let annotations = Annotations {
            groups: vec!["Observed returns".to_owned()],
            items: observed.iter().enumerate().map(|(i, &v)| ((i + 1) as f64, v, String::new(), 0)).collect(),
            labels: Vec::new(),
        };
        let spec = PlotSpec {
            x_label: "Calendar period (1 = first shipment period)".to_owned(),
            y_label: "Returns per period".to_owned(),
            line: Some(line),
            line_name: Some(format!("Expected ({} fit, then forecast)", family.label())),
            band: Some((lower, upper)),
            band_name: Some(format!("{} prediction limits", self.level())),
            annotations: Some(annotations),
            segments: vec![vec![[p as f64 + 0.5, 0.0], [p as f64 + 0.5, top * 1.05]]],
            ..PlotSpec::default()
        };
        section(
            SectionKind::WarrantyForecastPlot,
            format!(
                "Returns per Period (markers: observed; line: expected under the {} fit, forecast after the thin \
                 vertical rule; dashed: {} prediction limits)",
                family.label(),
                self.level()
            ),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn forecast_table(&self, family: FitFamily, f: &WarrantyForecast) -> ReportSection {
        let level = self.level();
        let lower = format!("Lower {level}");
        let upper = format!("Upper {level}");
        let mut t = TextTable::new([
            "Future period",
            "Calendar period",
            "Expected returns",
            "From units in service",
            "From planned shipments",
            "Std deviation",
            "Parameter std error",
            lower.as_str(),
            upper.as_str(),
        ]);
        for v in &f.periods {
            t.push([
                v.period.to_string(),
                (self.data.periods() + v.period).to_string(),
                num(v.expected),
                num(v.from_installed),
                num(v.from_planned),
                format_number(v.std_error, 4),
                format_number(v.parameter_std_error, 4),
                num(v.lower),
                num(v.upper),
            ]);
        }
        section(
            SectionKind::WarrantyForecastTable,
            format!(
                "Forecast Returns per Period ({} fit; conditional failure probabilities given survival to the \
                 current age; limits: normal approximation with binomial variation plus delta-method parameter \
                 uncertainty)",
                family.label()
            ),
            SectionContent::Table(t),
        )
    }

    fn cumulative_table(&self, f: &WarrantyForecast) -> ReportSection {
        let level = self.level();
        let lower = format!("Lower {level}");
        let upper = format!("Upper {level}");
        let mut t = TextTable::new([
            "Through future period",
            "Expected returns",
            "Std deviation",
            lower.as_str(),
            upper.as_str(),
        ]);
        for v in &f.cumulative {
            t.push([v.period.to_string(), num(v.expected), format_number(v.std_error, 4), num(v.lower), num(v.upper)]);
        }
        section(
            SectionKind::WarrantyCumulativeForecast,
            "Cumulative Forecast Returns (total from the next period through each future period)",
            SectionContent::Table(t),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::monthly_warranty_returns;

    fn request(t: &DataTable) -> WarrantyRequest {
        let returns: Vec<usize> =
            (0..t.n_cols()).filter(|&c| t.column(c).unwrap().name.starts_with("Returned")).collect();
        WarrantyRequest {
            planned: vec![1500.0; 6],
            ..WarrantyRequest::new(WarrantyLayout::Triangular {
                shipped: t.find("Shipped").unwrap(),
                returns,
                label: t.find("Ship month"),
            })
        }
    }

    #[test]
    fn sample_report_has_every_section() {
        let t = monthly_warranty_returns();
        let rows = RowStateManager::new(t.n_rows());
        let a = WarrantyAnalysis::run(&t, &rows, &request(&t)).unwrap();
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        for k in [
            SectionKind::WarrantySummary,
            SectionKind::WarrantyLifeData,
            SectionKind::LifeModelComparison,
            SectionKind::LifeProbabilityPlot,
            SectionKind::LifeParameters,
            SectionKind::WarrantyForecastPlot,
            SectionKind::WarrantyForecastTable,
            SectionKind::WarrantyCumulativeForecast,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing");
        }
        let (family, f) = a.forecast().unwrap();
        assert_eq!(*family, FitFamily::Weibull);
        assert_eq!(f.periods.len(), WarrantyRequest::DEFAULT_HORIZON);
        // Planned shipments add to the forecast.
        assert!(f.periods[1].from_planned > 0.0);
        // Probability-plot points refer to dataset rows.
        let s = a.sections();
        let plot = s.iter().find(|x| x.kind == SectionKind::LifeProbabilityPlot).unwrap();
        let SectionContent::Plot(p) = &plot.content else { panic!() };
        assert!(!p.points.is_empty() && p.points.iter().all(|pt| pt.row < t.n_rows()));
        // The life data total the units shipped.
        let total: f64 = a.data().life_rows().iter().map(|r| r.observation.weight).sum();
        let shipped: f64 = a.data().cohorts().iter().map(|c| c.shipped).sum();
        assert!((total - shipped).abs() < 1e-9);
    }

    /// The same counts in the long layout give the same analysis.
    #[test]
    fn long_layout_matches_triangular() {
        let t = monthly_warranty_returns();
        let rows = RowStateManager::new(t.n_rows());
        let tri = WarrantyAnalysis::run(&t, &rows, &request(&t)).unwrap();
        let (mut ship, mut ret, mut count, mut shipped) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for c in tri.data().cohorts() {
            ship.push((c.period + 1) as f64);
            ret.push(f64::NAN);
            count.push(f64::NAN);
            shipped.push(c.shipped);
            for (a, &r) in c.returns.iter().enumerate() {
                ship.push((c.period + 1) as f64);
                ret.push((c.period + a + 1) as f64);
                count.push(r);
                shipped.push(c.shipped);
            }
        }
        // Calendar periods run to the last return column even without returns there.
        let long = DataTable::new(
            "long",
            vec![
                Column::numeric("Ship", ship),
                Column::numeric("Return", ret),
                Column::numeric("Count", count),
                Column::numeric("Shipped", shipped),
            ],
        )
        .unwrap();
        let lrows = RowStateManager::new(long.n_rows());
        let req = WarrantyRequest {
            planned: vec![1500.0; 6],
            ..WarrantyRequest::new(WarrantyLayout::Long { ship_period: 0, return_period: 1, returns: 2, shipped: 3 })
        };
        let lng = WarrantyAnalysis::run(&long, &lrows, &req).unwrap();
        assert_eq!(lng.data().cohorts(), tri.data().cohorts());
        assert_eq!(lng.data().periods(), tri.data().periods());
        let (a, b) = (&tri.forecast().unwrap().1, &lng.forecast().unwrap().1);
        assert!((a.periods[0].expected - b.periods[0].expected).abs() < 1e-9);
    }

    #[test]
    fn errors_and_json() {
        let t = monthly_warranty_returns();
        let mut rows = RowStateManager::new(t.n_rows());
        let r = request(&t);
        let json = r.to_json(&t);
        assert_eq!(WarrantyRequest::from_json(&json, &t).unwrap(), r);
        let mut other = r.clone();
        other.detail = None;
        other.warranty = Some(24.0);
        other.layout = WarrantyLayout::Long { ship_period: 1, return_period: 2, returns: 3, shipped: 4 };
        assert_eq!(WarrantyRequest::from_json(&other.to_json(&t), &t).unwrap(), other);
        // Too few return columns for the shipment rows.
        let mut short = r.clone();
        if let WarrantyLayout::Triangular { returns, .. } = &mut short.layout {
            returns.truncate(3);
        }
        assert!(WarrantyAnalysis::run(&t, &rows, &short).is_err());
        // Excluding a shipment period leaves its cohort out.
        rows.set_excluded([0], true);
        let a = WarrantyAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(a.data().cohorts().len(), t.n_rows() - 1);
        assert_eq!(a.data().cohorts()[0].period, 1);
    }
}
