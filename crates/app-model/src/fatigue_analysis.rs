//! Fatigue (stress–life, S–N) curves bound to a dataset
//! ([`stat_reliability::fatigue`]): cycles to failure against stress
//! amplitude with run-outs as right-censored observations, a Basquin,
//! semi-log, quadratic log–log or endurance-limit curve with lognormal,
//! Weibull or log-logistic scatter fitted by maximum likelihood (ASTM
//! E739-10; Meeker & Escobar 1998, ch. 17; Pascual & Meeker 1999).
//!
//! The report gives the specimens per stress level, the fit, the curve's
//! coefficients with Wald and likelihood-ratio limits (and Basquin's
//! `S = A·N^b` form), the fatigue strength (the stress whose chosen life
//! quantile equals a given number of cycles), quantiles of life at chosen
//! stresses, the S–N plot with quantile curves (e.g. the 10% curve, B10)
//! and a lower confidence bound of the lowest one, standardized residuals
//! against stress, and a probability plot of the standardized residuals
//! across all stress levels (Turnbull estimate, so run-outs are handled as
//! censored) against the model's standard distribution.
//!
//! **Layout:** a cycles column, a run-out column (nonzero = the specimen did
//! not fail; optional), a stress column and an optional frequency column.
//! Excluded rows and incomplete rows are left out.

use crate::format::format_number;
use crate::life_stress::{log_ticks, plotting_position, probability_ticks};
use crate::project::{invalid as json_invalid, member, s};
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_reliability::fatigue::{FatigueData, FatigueFit, FatigueModel, SnForm, fit_fatigue};
use stat_reliability::life_regression::RegressionFamily;
use stat_reliability::{IntervalEstimate, LifeData, LifeObservation, turnbull};

const DIGITS: usize = 5;
const CURVE_POINTS: usize = 80;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

fn pct(p: f64) -> String {
    format!("{}%", format_number(p * 100.0, 4))
}

/// Columns and settings of a fatigue-curve analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct FatigueRequest {
    /// Cycles to failure or run-out.
    pub cycles: usize,
    /// Run-out flag (nonzero = did not fail); `None`: every specimen failed.
    pub runout: Option<usize>,
    /// Stress amplitude.
    pub stress: usize,
    /// Optional frequency column.
    pub frequency: Option<usize>,
    /// The S–N curve.
    pub form: SnForm,
    /// Life distribution.
    pub family: RegressionFamily,
    /// `ln σ` linear in `ln S`.
    pub stress_dependent_scale: bool,
    /// Confidence level.
    pub confidence: f64,
    /// Fractions failing for the quantile curves and tables.
    pub probabilities: Vec<f64>,
    /// Lives (cycles) whose fatigue strength is reported.
    pub lives: Vec<f64>,
    /// Stresses whose life quantiles are reported (empty: the tested levels).
    pub stresses: Vec<f64>,
}

impl FatigueRequest {
    /// Default confidence level.
    pub const DEFAULT_CONFIDENCE: f64 = 0.95;
    /// Default fractions failing: the 10% (B10) and 50% curves.
    pub const DEFAULT_PROBABILITIES: [f64; 2] = [0.1, 0.5];
    /// Default lives for the fatigue strength.
    pub const DEFAULT_LIVES: [f64; 2] = [1e6, 1e7];

    /// A lognormal Basquin curve of `cycles` against `stress`.
    #[must_use]
    pub fn new(cycles: usize, runout: Option<usize>, stress: usize) -> Self {
        Self {
            cycles,
            runout,
            stress,
            frequency: None,
            form: SnForm::Basquin,
            family: RegressionFamily::Lognormal,
            stress_dependent_scale: false,
            confidence: Self::DEFAULT_CONFIDENCE,
            probabilities: Self::DEFAULT_PROBABILITIES.to_vec(),
            lives: Self::DEFAULT_LIVES.to_vec(),
            stresses: Vec::new(),
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = vec![self.cycles, self.stress];
        out.extend(self.runout);
        out.extend(self.frequency);
        out
    }

    /// Renumbers the request's columns with `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.cycles = f(self.cycles);
        self.runout = self.runout.map(f);
        self.stress = f(self.stress);
        self.frequency = self.frequency.map(f);
    }

    /// Project-file form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let nums = |v: &[f64]| JsonValue::Array(v.iter().map(|&x| JsonValue::number(x)).collect());
        let mut m: Vec<(String, JsonValue)> = vec![
            ("analysis".into(), s("fatigue")),
            ("cycles".into(), col(self.cycles)),
            ("stress".into(), col(self.stress)),
        ];
        if let Some(c) = self.runout {
            m.push(("runout".into(), col(c)));
        }
        if let Some(c) = self.frequency {
            m.push(("frequency".into(), col(c)));
        }
        m.push(("form".into(), s(self.form.key())));
        m.push(("distribution".into(), s(self.family.key())));
        m.push(("stress_dependent_scale".into(), JsonValue::Bool(self.stress_dependent_scale)));
        m.push(("confidence".into(), JsonValue::number(self.confidence)));
        m.push(("probabilities".into(), nums(&self.probabilities)));
        m.push(("lives".into(), nums(&self.lives)));
        m.push(("stresses".into(), nums(&self.stresses)));
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
        let nums = |key: &str| -> Option<Vec<f64>> {
            v.get(key).and_then(JsonValue::as_array).map(|a| a.iter().filter_map(JsonValue::as_f64).collect())
        };
        let form = match v.get("form").and_then(JsonValue::as_str) {
            None => SnForm::Basquin,
            Some(k) => SnForm::from_key(k).ok_or_else(|| json_invalid(format!("fatigue: unknown form {k:?}")))?,
        };
        let family = match v.get("distribution").and_then(JsonValue::as_str) {
            None => RegressionFamily::Lognormal,
            Some(k) => RegressionFamily::from_key(k)
                .filter(|f| f.is_log())
                .ok_or_else(|| json_invalid(format!("fatigue: unknown distribution {k:?}")))?,
        };
        Ok(Self {
            cycles: find("cycles")?,
            runout: optional("runout")?,
            stress: find("stress")?,
            frequency: optional("frequency")?,
            form,
            family,
            stress_dependent_scale: v.get("stress_dependent_scale").and_then(JsonValue::as_bool).unwrap_or(false),
            confidence: v.get("confidence").and_then(JsonValue::as_f64).unwrap_or(Self::DEFAULT_CONFIDENCE),
            probabilities: nums("probabilities").unwrap_or_else(|| Self::DEFAULT_PROBABILITIES.to_vec()),
            lives: nums("lives").unwrap_or_else(|| Self::DEFAULT_LIVES.to_vec()),
            stresses: nums("stresses").unwrap_or_default(),
        })
    }
}

/// A computed fatigue-curve analysis.
#[derive(Debug, Clone)]
pub struct FatigueAnalysis {
    request: FatigueRequest,
    cycles_name: String,
    stress_name: String,
    /// Dataset row of each observation.
    rows: Vec<usize>,
    fit: FatigueFit,
    n_skipped: usize,
}

impl FatigueAnalysis {
    /// Fit the curve to the included rows.
    ///
    /// # Errors
    /// Missing or non-numeric columns, a column used twice, a confidence
    /// level outside (0.5, 1), fractions outside (0, 1), no usable rows, or
    /// errors of the fit.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &FatigueRequest) -> Result<Self> {
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("choose a different column for each role"));
        }
        let numeric = |c: usize| -> Result<&[f64]> {
            let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
            col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", col.name)))
        };
        let cycles = numeric(request.cycles)?;
        let stress = numeric(request.stress)?;
        let runout = request.runout.map(numeric).transpose()?;
        let freq = request.frequency.map(numeric).transpose()?;
        if !(request.confidence > 0.5 && request.confidence < 1.0) {
            return Err(invalid("the confidence level must be between 0.5 and 1"));
        }
        if request.probabilities.iter().any(|p| !(*p > 0.0 && *p < 1.0)) {
            return Err(invalid("fractions failing must lie between 0 and 1"));
        }
        let mut rows = Vec::new();
        let mut observations = Vec::new();
        let mut stresses = Vec::new();
        for r in 0..table.n_rows() {
            if states.is_excluded(r) {
                continue;
            }
            let w = freq.map_or(1.0, |f| f[r]);
            let flag = runout.map_or(0.0, |c| c[r]);
            if !(w.is_finite() && w > 0.0) || !cycles[r].is_finite() || !stress[r].is_finite() || !flag.is_finite() {
                continue;
            }
            observations.push(if flag != 0.0 {
                LifeObservation::right(cycles[r], w)
            } else {
                LifeObservation::exact(cycles[r], w)
            });
            stresses.push(stress[r]);
            rows.push(r);
        }
        if rows.is_empty() {
            return Err(invalid("no rows with usable cycles and stresses"));
        }
        let model = FatigueModel {
            form: request.form,
            family: request.family,
            stress_dependent_scale: request.stress_dependent_scale,
        };
        let fit = fit_fatigue(&FatigueData { observations, stresses }, &model)?;
        let name = |c: usize| table.column(c).map_or_else(String::new, |c| c.name.clone());
        Ok(Self {
            request: request.clone(),
            cycles_name: name(request.cycles),
            stress_name: name(request.stress),
            n_skipped: table.n_rows() - rows.len(),
            rows,
            fit,
        })
    }

    /// The fit.
    #[must_use]
    pub fn fit(&self) -> &FatigueFit {
        &self.fit
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!(
            "Fatigue Curve ({}, {}): {} by {}",
            match self.request.form {
                SnForm::Basquin => "Basquin",
                SnForm::SemiLog => "semi-log",
                SnForm::QuadraticLog => "quadratic log–log",
                SnForm::EnduranceLimit => "endurance limit",
            },
            self.request.family.label().to_lowercase(),
            self.cycles_name,
            self.stress_name
        )
    }

    fn level_text(&self) -> String {
        pct(self.request.confidence)
    }

    fn interval_headings(&self, first: &[&str]) -> Vec<String> {
        let level = self.level_text();
        let mut cols: Vec<String> = first.iter().map(|s| (*s).to_owned()).collect();
        cols.extend([
            "Std error".to_owned(),
            format!("Wald lower {level}"),
            format!("Wald upper {level}"),
            format!("Likelihood-ratio lower {level}"),
            format!("Likelihood-ratio upper {level}"),
        ]);
        cols
    }

    fn interval_cells(e: &IntervalEstimate) -> Vec<String> {
        let lr = e.likelihood_ratio.unwrap_or((f64::NAN, f64::NAN));
        vec![num(e.estimate), format_number(e.std_error, 4), num(e.wald.0), num(e.wald.1), num(lr.0), num(lr.1)]
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.levels(), self.statistics(), self.parameters()];
        if !self.request.lives.is_empty() && !self.request.probabilities.is_empty() {
            out.push(self.strength());
        }
        if !self.request.probabilities.is_empty() {
            out.push(self.lives());
        }
        out.push(self.sn_plot());
        out.push(self.residual_plot());
        out.push(self.probability_plot());
        out
    }

    fn levels(&self) -> ReportSection {
        let mut t = TextTable::new([self.stress_name.as_str(), "Specimens", "Failures", "Run-outs"]);
        for l in &self.fit.levels {
            t.push([num(l.stress), num(l.failures + l.run_outs), num(l.failures), num(l.run_outs)]);
        }
        let skipped = if self.n_skipped > 0 {
            format!("; {} rows excluded or incomplete", self.n_skipped)
        } else {
            String::new()
        };
        section(SectionKind::FatigueLevels, format!("Stress Levels{skipped}"), SectionContent::Table(t))
    }

    fn statistics(&self) -> ReportSection {
        let f = &self.fit;
        let mut t = TextTable::new(["Quantity", "Value"]);
        t.push(["Curve".to_owned(), format!("{} ({})", self.request.form.label(), self.request.form.formula())]);
        t.push(["Life distribution".to_owned(), self.request.family.label().to_owned()]);
        if !self.request.family.fixed_scale() {
            let scale = if self.request.stress_dependent_scale { "ln σ = γ₀ + γ₁ ln S" } else { "Constant" };
            t.push(["Scale σ of log life".to_owned(), scale.to_owned()]);
        }
        t.push(["Specimens".to_owned(), num(f.fit.total_weight())]);
        t.push(["Parameters".to_owned(), f.n_parameters().to_string()]);
        t.push(["Log-likelihood".to_owned(), num(f.log_likelihood())]);
        t.push(["AIC".to_owned(), num(f.aic())]);
        if let Some(e) = &f.endurance_limit {
            let (lo, hi) = e.likelihood_ratio;
            let note = if e.at_zero { " (profile largest at 0: no endurance limit supported)" } else { "" };
            t.push(["Endurance limit E".to_owned(), format!("{}{note}", num(e.estimate))]);
            t.push([
                format!("Endurance limit, likelihood-ratio {}", pct(e.level)),
                format!("{} to {}", num(lo), num(hi)),
            ]);
        }
        section(
            SectionKind::FatigueFitStatistics,
            "Model Fit (maximum likelihood; run-outs censored)",
            SectionContent::Table(t),
        )
    }

    fn parameters(&self) -> ReportSection {
        let level = self.request.confidence;
        let f = &self.fit.fit;
        let mut t = TextTable::new(self.interval_headings(&["Parameter", "Estimate"]));
        let names = self.request.form.coefficient_names();
        let p = f.location_coefficients.len();
        for (j, name) in names.iter().enumerate().take(p) {
            let mut a = vec![0.0; j + 1];
            a[j] = 1.0;
            let mut row = vec![(*name).to_owned()];
            row.extend(Self::interval_cells(&f.linear_function(&a, 0.0, level, true)));
            t.push(row);
        }
        if !self.request.family.fixed_scale() {
            for k in 0..f.scale_coefficients.len() {
                let mut a = vec![0.0; p + k + 1];
                a[p + k] = 1.0;
                let e = f.linear_function(&a, 0.0, level, true);
                if self.request.stress_dependent_scale {
                    let name = if k == 0 { "ln σ: intercept γ₀" } else { "ln σ: slope γ₁ (ln S)" };
                    let mut row = vec![name.to_owned()];
                    row.extend(Self::interval_cells(&e));
                    t.push(row);
                } else {
                    let order = |(a, b): (f64, f64)| if a <= b { (a, b) } else { (b, a) };
                    let map = |g: &dyn Fn(f64) -> f64| IntervalEstimate {
                        name: e.name,
                        estimate: g(e.estimate),
                        std_error: g(e.estimate).abs() * e.std_error,
                        wald: order((g(e.wald.0), g(e.wald.1))),
                        likelihood_ratio: e.likelihood_ratio.map(|(a, b)| order((g(a), g(b)))),
                    };
                    let mut row = vec!["Scale σ (of log cycles)".to_owned()];
                    row.extend(Self::interval_cells(&map(&f64::exp)));
                    t.push(row);
                    if self.request.family == RegressionFamily::Weibull {
                        let mut row = vec!["Weibull shape β = 1/σ".to_owned()];
                        row.extend(Self::interval_cells(&map(&|x: f64| (-x).exp())));
                        t.push(row);
                    }
                }
            }
        }
        let mut title = format!("Parameter Estimates ({})", self.request.form.formula());
        if let Some((a, b)) = self.fit.basquin_coefficients() {
            let at = if self.request.family == RegressionFamily::Weibull {
                "characteristic (63.2%) life"
            } else {
                "median life"
            };
            title = format!("{title}; as stress against {at}: S = {} · N^{}", num(a), format_number(b, 4));
        }
        section(SectionKind::FatigueParameters, title, SectionContent::Table(t))
    }

    fn strength(&self) -> ReportSection {
        let mut heads = vec![format!("Life ({})", self.cycles_name)];
        heads.extend(self.request.probabilities.iter().map(|p| format!("Stress at {} failing", pct(*p))));
        let mut t = TextTable::new(heads);
        for &n in &self.request.lives {
            let mut row = vec![num(n)];
            for &p in &self.request.probabilities {
                row.push(self.fit.stress_at_life(n, p).map_or_else(|| "not reached".to_owned(), num));
            }
            t.push(row);
        }
        section(
            SectionKind::FatigueStrength,
            "Fatigue Strength (the stress at which the given fraction of specimens fails by each life)",
            SectionContent::Table(t),
        )
    }

    fn lives(&self) -> ReportSection {
        let stresses: Vec<f64> = if self.request.stresses.is_empty() {
            self.fit.levels.iter().map(|l| l.stress).collect()
        } else {
            self.request.stresses.clone()
        };
        let mut heads = vec![self.stress_name.clone()];
        heads.extend(self.interval_headings(&["Fraction failing", "Cycles"]));
        let mut t = TextTable::new(heads);
        for &s in &stresses {
            for &p in &self.request.probabilities {
                let q = self.fit.quantile(s, p, self.request.confidence, true);
                let mut row = vec![num(s), pct(p)];
                row.extend(Self::interval_cells(&q));
                t.push(row);
            }
        }
        let note =
            if self.fit.endurance_limit.is_some() { "; conditional on the estimated endurance limit" } else { "" };
        section(
            SectionKind::FatigueLives,
            format!("Life Quantiles at Each Stress (Wald limits on log cycles{note})"),
            SectionContent::Table(t),
        )
    }

    fn log_stress_axis(&self) -> bool {
        self.request.form != SnForm::SemiLog
    }

    fn y_of(&self, s: f64) -> f64 {
        if self.log_stress_axis() { s.ln() } else { s }
    }

    fn sn_plot(&self) -> ReportSection {
        let mut points = Vec::new();
        let mut membership = Vec::new();
        let (mut n_lo, mut n_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (i, o) in self.fit.data.observations.iter().enumerate() {
            let n = o.lower;
            if n > 0.0 && n.is_finite() {
                points.push(RowPoint { row: self.rows[i], x: n.ln(), y: self.y_of(self.fit.data.stresses[i]) });
                membership.push(usize::from(o.upper.is_infinite()));
                (n_lo, n_hi) = (n_lo.min(n), n_hi.max(n));
            }
        }
        let s_lo = self.fit.levels.first().map_or(1.0, |l| l.stress);
        let s_hi = self.fit.levels.last().map_or(2.0, |l| l.stress);
        let e = self.fit.endurance();
        let start = if e > 0.0 { e + (s_lo - e) * 0.02 } else { s_lo * 0.95 };
        let grid: Vec<f64> =
            (0..=CURVE_POINTS).map(|i| start + (s_hi * 1.05 - start) * i as f64 / CURVE_POINTS as f64).collect();
        let mut labels = vec!["Failure".to_owned(), "Run-out (censored)".to_owned()];
        let mut lines: Vec<Polyline> = vec![Vec::new(), Vec::new()];
        let mut probabilities = self.request.probabilities.clone();
        if probabilities.is_empty() {
            probabilities = vec![0.5];
        }
        probabilities.truncate(4);
        let n_max = n_hi * 10.0;
        for &p in &probabilities {
            let line: Polyline = grid
                .iter()
                .filter_map(|&s| {
                    let q = self.fit.quantile(s, p, self.request.confidence, false).estimate;
                    (q > 0.0 && q.is_finite() && q < n_max).then(|| [q.ln(), self.y_of(s)])
                })
                .collect();
            labels.push(format!("{} failing", pct(p)));
            lines.push(line);
        }
        let lowest = probabilities.iter().copied().fold(f64::INFINITY, f64::min);
        let bound: Polyline = grid
            .iter()
            .filter_map(|&s| {
                let q = self.fit.quantile(s, lowest, self.request.confidence, false).wald.0;
                (q > 0.0 && q.is_finite() && q < n_max).then(|| [q.ln(), self.y_of(s)])
            })
            .collect();
        labels.push(format!("{} failing: lower {} bound", pct(lowest), self.level_text()));
        lines.push(bound);
        for l in lines.iter().skip(2) {
            for p in l {
                (n_lo, n_hi) = (n_lo.min(p[0].exp()), n_hi.max(p[0].exp()));
            }
        }
        let x_ticks = n_lo
            .is_finite()
            .then(|| log_ticks(n_lo, n_hi).into_iter().map(|v| (v.ln(), format_number(v, 3))).collect());
        let y_ticks = self
            .log_stress_axis()
            .then(|| log_ticks(start, s_hi * 1.05).into_iter().map(|v| (v.ln(), format_number(v, 4))).collect());
        let spec = PlotSpec {
            x_label: format!("{} (log scale)", self.cycles_name),
            y_label: if self.log_stress_axis() {
                format!("{} (log scale)", self.stress_name)
            } else {
                self.stress_name.clone()
            },
            points,
            groups: Some(PointGroups { title: String::new(), labels, membership, lines }),
            x_ticks,
            y_ticks,
            ..PlotSpec::default()
        };
        section(
            SectionKind::FatigueSnPlot,
            "S–N Curve (quantile curves of life; run-outs drawn at the cycles reached)",
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn residual_plot(&self) -> ReportSection {
        let residuals = self.fit.standardized_residuals();
        let mut points = Vec::new();
        let mut membership = Vec::new();
        for (i, (z, runout)) in residuals.iter().enumerate() {
            if z.is_finite() {
                points.push(RowPoint { row: self.rows[i], x: self.y_of(self.fit.data.stresses[i]), y: *z });
                membership.push(usize::from(*runout));
            }
        }
        let s_lo = self.fit.levels.first().map_or(1.0, |l| l.stress);
        let s_hi = self.fit.levels.last().map_or(2.0, |l| l.stress);
        let x_ticks = self
            .log_stress_axis()
            .then(|| self.fit.levels.iter().map(|l| (l.stress.ln(), format_number(l.stress, 4))).collect());
        let spec = PlotSpec {
            x_label: if self.log_stress_axis() {
                format!("{} (log scale)", self.stress_name)
            } else {
                self.stress_name.clone()
            },
            y_label: "Standardized residual (ln N − μ)/σ".to_owned(),
            points,
            groups: Some(PointGroups {
                title: String::new(),
                labels: vec!["Failure".to_owned(), "Run-out (lower bound)".to_owned()],
                membership,
                lines: vec![Vec::new(), Vec::new()],
            }),
            segments: vec![vec![[self.y_of(s_lo * 0.97), 0.0], [self.y_of(s_hi * 1.03), 0.0]]],
            x_ticks,
            ..PlotSpec::default()
        };
        section(
            SectionKind::FatigueResiduals,
            "Standardized Residuals by Stress (a run-out's residual is a lower bound)",
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn probability_plot(&self) -> ReportSection {
        let family = self.request.family;
        let residuals = self.fit.standardized_residuals();
        let obs: Vec<LifeObservation> = residuals
            .iter()
            .zip(&self.fit.data.observations)
            .map(
                |((z, runout), o)| {
                    if *runout { LifeObservation::right(*z, o.weight) } else { LifeObservation::exact(*z, o.weight) }
                },
            )
            .collect();
        let std_q = |p: f64| family.standard_quantile(p);
        let mut points = Vec::new();
        let mut membership = Vec::new();
        let (mut y_lo, mut y_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut z_lo, mut z_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let labels: Vec<String> =
            self.fit.levels.iter().map(|l| format!("{} {}", self.stress_name, num(l.stress))).collect();
        if let Some(np) = LifeData::new(obs).ok().and_then(|d| turnbull(&d).ok()) {
            for (i, (z, runout)) in residuals.iter().enumerate() {
                if *runout || !z.is_finite() {
                    continue;
                }
                let Some(p) = plotting_position(&np, *z).filter(|p| *p > 0.0 && *p < 1.0) else { continue };
                let y = std_q(p);
                let s = self.fit.data.stresses[i];
                let g = self.fit.levels.iter().position(|l| l.stress == s).unwrap_or(0);
                points.push(RowPoint { row: self.rows[i], x: *z, y });
                membership.push(g);
                (y_lo, y_hi) = (y_lo.min(y), y_hi.max(y));
                (z_lo, z_hi) = (z_lo.min(*z), z_hi.max(*z));
            }
        }
        if !z_lo.is_finite() {
            (z_lo, z_hi, y_lo, y_hi) = (-2.0, 2.0, -2.0, 2.0);
        }
        let a = z_lo.min(y_lo) - 0.3;
        let b = z_hi.max(y_hi) + 0.3;
        let spec = PlotSpec {
            x_label: "Standardized residual".to_owned(),
            y_label: "Fraction failing".to_owned(),
            points,
            line: Some(vec![[a, a], [b, b]]),
            line_name: Some(format!("Standard {} (the fitted model)", family.label())),
            groups: Some(PointGroups {
                title: String::new(),
                lines: vec![Vec::new(); labels.len()],
                labels,
                membership,
            }),
            y_ticks: Some(probability_ticks(&std_q, y_lo, y_hi)),
            ..PlotSpec::default()
        };
        section(
            SectionKind::FatigueProbabilityPlot,
            format!(
                "{} Probability Plot of Standardized Residuals across Stress Levels (Turnbull estimate; run-outs \
                 censored)",
                family.label()
            ),
            SectionContent::Plot(Box::new(spec)),
        )
    }
}

#[cfg(test)]
mod tests;
