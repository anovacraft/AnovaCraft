//! A multivariate control chart from a principal-components model, bound to
//! the row state ([`stat_quality::pca_chart`]): T² on the retained component
//! scores and the squared prediction error (SPE) of what the model leaves
//! out, with variable contributions to every signal, a parallel-coordinates
//! view of the signalling rows, and a change-point estimate.
//!
//! Rows are taken in table order, skipping excluded rows and rows missing a
//! value in any measurement column. The first `reference` rows (all when
//! `None`) fit the model; later rows are monitored against it. Every plotted
//! point is one dataset row, so brushing the charts selects it.

use crate::format::format_number;
use crate::project::{invalid, member, obj, s};
use crate::report::{
    Annotations, ChartPoint, ControlChartSpec, LimitStep, PhaseMark, PlotSpec, PointGroups, ReportSection, RowPoint,
    SectionContent, SectionKind, TextTable,
};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_quality::{
    ChangePoint, ChartPhase, ComponentChoice, MultivariatePoint, PcaChart, PcaChartOptions, SpeLimitMethod,
    mean_change_point, pca_chart,
};

const DIGITS: usize = 5;

/// Signals shown in each contribution and parallel-coordinates plot (the
/// largest statistics).
pub const PCA_PLOT_POINTS: usize = 8;

/// Permutations behind the change-point p-value.
const PERMUTATIONS: usize = 999;

/// What to chart.
#[derive(Debug, Clone, PartialEq)]
pub struct PcaChartRequest {
    /// Measurement columns (two or more).
    pub columns: Vec<usize>,
    /// Column naming each point.
    pub label: Option<usize>,
    /// Rows (after exclusions and missing values) in the reference period,
    /// from the start; `None` uses every row.
    pub reference: Option<usize>,
    /// Standardize the variables (correlation model).
    pub standardize: bool,
    /// Components retained; `None` chooses them by cross-validation.
    pub components: Option<usize>,
    /// Cross-validation folds.
    pub folds: usize,
    /// False-alarm probability of each limit.
    pub alpha: f64,
    /// Approximation for the SPE limit.
    pub spe_method: SpeLimitMethod,
}

impl PcaChartRequest {
    /// Default false-alarm probability (that of three-sigma limits).
    pub const DEFAULT_ALPHA: f64 = 0.0027;
    /// Default number of cross-validation folds.
    pub const DEFAULT_FOLDS: usize = 7;

    /// A correlation model of `columns` with components chosen by
    /// cross-validation and every row in the reference period.
    #[must_use]
    pub fn new(columns: Vec<usize>) -> Self {
        Self {
            columns,
            label: None,
            reference: None,
            standardize: true,
            components: None,
            folds: Self::DEFAULT_FOLDS,
            alpha: Self::DEFAULT_ALPHA,
            spe_method: SpeLimitMethod::JacksonMudholkar,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn all_columns(&self) -> Vec<usize> {
        let mut c = self.columns.clone();
        c.extend(self.label);
        c
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.columns.iter_mut().for_each(|c| *c = f(*c));
        self.label = self.label.map(f);
    }

    /// The project-file form (`"analysis": "pca-chart"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let opt = |v: Option<usize>| v.map_or(JsonValue::Null, |n| JsonValue::number(n as f64));
        obj([
            ("analysis", s("pca-chart")),
            ("columns", JsonValue::Array(self.columns.iter().map(|&c| col(c)).collect())),
            ("label", self.label.map_or(JsonValue::Null, col)),
            ("reference", opt(self.reference)),
            ("standardize", JsonValue::Bool(self.standardize)),
            ("components", opt(self.components)),
            ("folds", JsonValue::number(self.folds as f64)),
            ("alpha", JsonValue::number(self.alpha)),
            ("spe_limit", s(self.spe_method.key())),
        ])
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, an unknown SPE method.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find = |c: &JsonValue| -> Result<usize> {
            let name = c.as_str().ok_or_else(|| invalid("PCA chart: expected a column name"))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let columns = member(v, "columns")?
            .as_array()
            .ok_or_else(|| invalid("PCA chart: columns must be a list"))?
            .iter()
            .map(find)
            .collect::<Result<Vec<_>>>()?;
        let label = match v.get("label") {
            None | Some(JsonValue::Null) => None,
            Some(c) => Some(find(c)?),
        };
        let spe_method = match v.get("spe_limit").and_then(JsonValue::as_str) {
            None => SpeLimitMethod::JacksonMudholkar,
            Some(k) => {
                SpeLimitMethod::from_key(k).ok_or_else(|| invalid(format!("PCA chart: unknown SPE limit {k:?}")))?
            }
        };
        let d = Self::new(columns);
        Ok(Self {
            label,
            reference: v.get("reference").and_then(JsonValue::as_usize),
            standardize: v.get("standardize").and_then(JsonValue::as_bool).unwrap_or(true),
            components: v.get("components").and_then(JsonValue::as_usize),
            folds: v.get("folds").and_then(JsonValue::as_usize).unwrap_or(d.folds),
            alpha: v.get("alpha").and_then(JsonValue::as_f64).unwrap_or(d.alpha),
            spe_method,
            ..d
        })
    }

    fn options(&self) -> PcaChartOptions {
        PcaChartOptions {
            standardize: self.standardize,
            components: match self.components {
                Some(a) => ComponentChoice::Fixed(a),
                None => ComponentChoice::CrossValidation { folds: self.folds },
            },
            alpha: self.alpha,
            spe_method: self.spe_method,
        }
    }
}

/// A computed principal-components monitoring chart.
#[derive(Debug, Clone)]
pub struct PcaChartAnalysis {
    request: PcaChartRequest,
    names: Vec<String>,
    chart: PcaChart,
    /// Dataset row behind each point.
    rows: Vec<usize>,
    labels: Vec<Option<String>>,
    change: Option<std::result::Result<ChangePoint, String>>,
    n_excluded: usize,
    n_missing: usize,
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

/// Which statistic a section is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Statistic {
    T2,
    Spe,
}

impl Statistic {
    fn label(self) -> &'static str {
        match self {
            Self::T2 => "T²",
            Self::Spe => "SPE",
        }
    }
}

impl PcaChartAnalysis {
    /// Fit the model and chart every usable row.
    ///
    /// # Errors
    /// Fewer than two measurement columns, a missing or non-numeric column,
    /// and errors from [`stat_quality::pca_chart`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &PcaChartRequest) -> Result<Self> {
        if request.columns.len() < 2 {
            return Err(invalid("choose at least two measurement columns"));
        }
        let mut names = Vec::new();
        let mut values: Vec<&[f64]> = Vec::new();
        for &i in &request.columns {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            values.push(c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?);
            names.push(c.name.clone());
        }
        let label_col =
            request.label.map(|i| table.column(i).ok_or_else(|| invalid(format!("no column {i}")))).transpose()?;
        let (mut n_excluded, mut n_missing) = (0, 0);
        let mut rows = Vec::new();
        for r in 0..table.n_rows() {
            if !values.iter().all(|v| v[r].is_finite()) {
                n_missing += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                rows.push(r);
            }
        }
        let data: Vec<Vec<f64>> = rows.iter().map(|&r| values.iter().map(|v| v[r]).collect()).collect();
        let chart = pca_chart(&data, request.reference, &request.options())?;
        let labels = rows.iter().map(|&r| label_col.map(|c| c.level_text(r)).filter(|s| !s.is_empty())).collect();
        let any_signal = chart.t2.iter().chain(&chart.spe).any(|p| p.signal);
        let change =
            any_signal.then(|| mean_change_point(&chart.scores, PERMUTATIONS, 0x5EED).map_err(|e| e.to_string()));
        Ok(Self { request: request.clone(), names, chart, rows, labels, change, n_excluded, n_missing })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &PcaChartRequest {
        &self.request
    }

    /// The fitted chart.
    #[must_use]
    pub fn chart(&self) -> &PcaChart {
        &self.chart
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        if self.names.len() <= 3 {
            format!("Principal-Components Control Chart of {}", self.names.join(", "))
        } else {
            format!("Principal-Components Control Chart of {} Variables", self.names.len())
        }
    }

    /// Indices of the points outside the T² or SPE limit.
    #[must_use]
    pub fn signals(&self) -> Vec<usize> {
        (0..self.rows.len()).filter(|&i| self.chart.t2[i].signal || self.chart.spe[i].signal).collect()
    }

    /// Dataset rows of the points outside a limit.
    #[must_use]
    pub fn signal_rows(&self) -> Vec<usize> {
        self.signals().into_iter().map(|i| self.rows[i]).collect()
    }

    fn points(&self, s: Statistic) -> &[MultivariatePoint] {
        match s {
            Statistic::T2 => &self.chart.t2,
            Statistic::Spe => &self.chart.spe,
        }
    }

    fn contributions(&self, s: Statistic, i: usize) -> Vec<f64> {
        match s {
            Statistic::T2 => self.chart.t2_contributions(i),
            Statistic::Spe => self.chart.spe_contributions(i),
        }
        .unwrap_or_default()
    }

    fn point_name(&self, i: usize) -> String {
        match &self.labels[i] {
            Some(l) => format!("{} ({l})", i + 1),
            None => (i + 1).to_string(),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        if !self.chart.press.is_empty() {
            out.push(self.cross_validation());
        }
        out.push(self.control_chart(Statistic::T2));
        out.push(self.control_chart(Statistic::Spe));
        out.push(self.signal_table());
        for s in [Statistic::T2, Statistic::Spe] {
            out.extend(self.contribution_sections(s));
        }
        if !self.signals().is_empty() {
            out.push(self.parallel_coordinates());
        }
        out.extend(self.change_point_sections());
        out.push(self.loadings());
        out
    }

    fn summary(&self) -> ReportSection {
        let c = &self.chart;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        let total = self.rows.len();
        push("Variables", self.names.join(", "));
        push("Observations", total.to_string());
        push("Reference observations (phase I)", c.reference_count.to_string());
        if c.reference_count < total {
            push("Monitored observations (phase II)", (total - c.reference_count).to_string());
        }
        if self.n_excluded > 0 {
            push("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            push("Rows with a missing value", self.n_missing.to_string());
        }
        push(
            "Model",
            if c.options.standardize { "Correlations (standardized)" } else { "Covariances (centred)" }.into(),
        );
        let how = match c.options.components {
            ComponentChoice::Fixed(_) => "set".to_owned(),
            ComponentChoice::CrossValidation { folds } => format!("smallest PRESS, {folds}-fold cross-validation"),
        };
        push("Components retained", format!("{} ({how})", c.components));
        push("Variance explained", format!("{}%", format_number(100.0 * c.explained(), 4)));
        push("False-alarm probability α", format_number(c.options.alpha, DIGITS));
        push("T² limit, reference (scaled beta)", format_number(c.t2_reference_limit, DIGITS));
        if c.t2_monitoring_limit.is_finite() {
            push("T² limit, monitored (scaled F)", format_number(c.t2_monitoring_limit, DIGITS));
        }
        if c.spe_limit.is_finite() {
            let mut method = c.spe_method_used.label().to_owned();
            if c.spe_method_used != c.options.spe_method {
                method.push_str(&format!(", as {} could not be computed", c.options.spe_method.label()));
            }
            push(&format!("SPE limit ({method})"), format_number(c.spe_limit, DIGITS));
        } else {
            push("SPE limit", "none: every component is retained, so nothing is left unexplained".into());
        }
        let count = |s: &[MultivariatePoint]| s.iter().filter(|p| p.signal).count().to_string();
        push("Points above the T² limit", count(&c.t2));
        push("Points above the SPE limit", count(&c.spe));
        section(SectionKind::PcaChartSummary, "Chart Summary", SectionContent::Table(t))
    }

    fn cross_validation(&self) -> ReportSection {
        let c = &self.chart;
        let mut t = TextTable::new(["Components", "PRESS", "Cumulative variance (%)", "Chosen"]);
        let total: f64 = c.eigenvalues.iter().sum();
        let mut acc = 0.0;
        for (a, press) in c.press.iter().enumerate() {
            acc += c.eigenvalues[a];
            t.push([
                (a + 1).to_string(),
                format_number(*press, DIGITS),
                format_number(100.0 * acc / total, 4),
                if a + 1 == c.components { "◀ smallest PRESS".to_owned() } else { String::new() },
            ]);
        }
        section(SectionKind::PcaChartCrossValidation, "Choosing the Number of Components", SectionContent::Table(t))
    }

    fn control_chart(&self, s: Statistic) -> ReportSection {
        let pts = self.points(s);
        let total = pts.len();
        let m = self.chart.reference_count;
        let points = pts
            .iter()
            .enumerate()
            .map(|(i, p)| ChartPoint {
                sample: i + 1,
                value: p.statistic,
                rows: vec![self.rows[i]],
                rules: if p.signal { vec![1] } else { Vec::new() },
                label: self.labels[i].clone(),
                note: None,
                signal: false,
            })
            .collect();
        let step = |from: usize, to: usize, p: &MultivariatePoint| LimitStep {
            from: from as f64 + 0.5,
            to: to as f64 + 0.5,
            center: f64::NAN,
            lower: f64::NAN,
            upper: p.upper,
            zone_sigma: 0.0,
        };
        let split = m < total && s == Statistic::T2;
        let (steps, phases) = if m < total {
            (
                if split { vec![step(0, m, &pts[0]), step(m, total, &pts[m])] } else { Vec::new() },
                vec![
                    PhaseMark { at: 0.5, label: "Reference".to_owned() },
                    PhaseMark { at: m as f64 + 0.5, label: "Monitoring".to_owned() },
                ],
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let spec = ControlChartSpec {
            x_label: "Observation".to_owned(),
            y_label: s.label().to_owned(),
            x_ticks: None,
            flag_label: Some("Above the limit".to_owned()),
            limit_label: Some("Upper limit".to_owned()),
            points,
            center: f64::NAN,
            lower: f64::NAN,
            upper: pts[0].upper,
            zone_sigma: 0.0,
            steps,
            phases,
        };
        let (kind, title) = match s {
            Statistic::T2 => (SectionKind::PcaChartT2, "T² Chart (variation within the model)"),
            Statistic::Spe => (SectionKind::PcaChartSpe, "SPE Chart (variation the model does not explain)"),
        };
        section(kind, title, SectionContent::ControlChart(spec))
    }

    fn signal_table(&self) -> ReportSection {
        let title = "Points Above the Limits";
        let signals = self.signals();
        if signals.is_empty() {
            return section(
                SectionKind::PcaChartSignals,
                title,
                SectionContent::Note("None: every point lies within both limits.".to_owned()),
            );
        }
        let mut t = TextTable::new(["Point", "Row", "Phase", "T²", "SPE", "Signals on", "Largest contribution"]);
        for i in signals {
            let (t2, spe) = (&self.chart.t2[i], &self.chart.spe[i]);
            let on = match (t2.signal, spe.signal) {
                (true, true) => "T² and SPE",
                (true, false) => "T²",
                _ => "SPE",
            };
            let stat = if spe.signal && (!t2.signal || spe.statistic / spe.upper > t2.statistic / t2.upper) {
                Statistic::Spe
            } else {
                Statistic::T2
            };
            let top = self
                .contributions(stat, i)
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or_else(|| "—".to_owned(), |(j, _)| format!("{} ({})", self.names[j], stat.label()));
            t.push([
                self.point_name(i),
                (self.rows[i] + 1).to_string(),
                match t2.phase {
                    ChartPhase::Reference => "reference",
                    ChartPhase::Monitoring => "monitoring",
                }
                .to_owned(),
                format_number(t2.statistic, DIGITS),
                format_number(spe.statistic, DIGITS),
                on.to_owned(),
                top,
            ]);
        }
        section(SectionKind::PcaChartSignals, title, SectionContent::Table(t))
    }

    /// The signals of `s`, largest first, at most [`PCA_PLOT_POINTS`], then
    /// in point order.
    fn largest(&self, s: Statistic) -> Vec<usize> {
        let pts = self.points(s);
        let mut v: Vec<usize> = (0..pts.len()).filter(|&i| pts[i].signal).collect();
        v.sort_by(|a, b| pts[*b].statistic.total_cmp(&pts[*a].statistic));
        v.truncate(PCA_PLOT_POINTS);
        v.sort_unstable();
        v
    }

    fn contribution_sections(&self, s: Statistic) -> Vec<ReportSection> {
        let (table_kind, plot_kind) = match s {
            Statistic::T2 => (SectionKind::PcaChartT2Contributions, SectionKind::PcaChartT2ContributionPlot),
            Statistic::Spe => (SectionKind::PcaChartSpeContributions, SectionKind::PcaChartSpeContributionPlot),
        };
        let label = s.label();
        let pts = self.points(s);
        let signals: Vec<usize> = (0..pts.len()).filter(|&i| pts[i].signal).collect();
        if signals.is_empty() {
            return Vec::new();
        }
        let mut header = vec!["Point".to_owned(), label.to_owned()];
        header.extend(self.names.iter().cloned());
        let mut t = TextTable::new(header);
        for &i in &signals {
            let c = self.contributions(s, i);
            let mut row = vec![self.point_name(i), format_number(pts[i].statistic, DIGITS)];
            row.extend(c.iter().map(|v| format_number(*v, 4)));
            t.push(row);
        }
        let note = match s {
            Statistic::T2 => {
                "Each variable's share of T²: its standardized value times its weight in the retained scores \
                 (Σₐ tₐ pⱼₐ / λₐ). The shares add up to T²; a large positive share marks a variable that moves \
                 the point along the model, and a negative share one that pulls it back."
            }
            Statistic::Spe => {
                "Each variable's squared residual eⱼ²: how far its standardized value is from what the model \
                 predicts from the other variables' pattern. The residuals add up to SPE; a large one marks a \
                 variable that broke the usual correlation structure."
            }
        };
        let shown = self.largest(s);
        let p = self.names.len();
        let mut points = Vec::new();
        let mut membership = Vec::new();
        let mut lines = Vec::new();
        let mut labels = Vec::new();
        for (g, &i) in shown.iter().enumerate() {
            let c = self.contributions(s, i);
            let line: Vec<[f64; 2]> = c.iter().enumerate().map(|(j, v)| [(j + 1) as f64, *v]).collect();
            for &[x, y] in &line {
                points.push(RowPoint { row: self.rows[i], x, y });
                membership.push(g);
            }
            lines.push(line);
            labels.push(format!("Point {}", self.point_name(i)));
        }
        let zero = (s == Statistic::T2).then(|| vec![[0.5, 0.0], [p as f64 + 0.5, 0.0]]);
        let plot = PlotSpec {
            x_label: "Variable".to_owned(),
            y_label: format!("Contribution to {label}"),
            points,
            groups: Some(PointGroups { title: "Point".to_owned(), labels, membership, lines }),
            x_ticks: Some(self.names.iter().enumerate().map(|(j, n)| ((j + 1) as f64, n.clone())).collect()),
            segments: zero.into_iter().collect(),
            ..PlotSpec::default()
        };
        let plot_title = if signals.len() > PCA_PLOT_POINTS {
            format!("{label} Contribution Plot (the {PCA_PLOT_POINTS} largest signals)")
        } else {
            format!("{label} Contribution Plot")
        };
        let table_title = format!("Contributions to {label} Signals");
        vec![
            section(table_kind, table_title, SectionContent::Table(t)),
            section(plot_kind, plot_title, SectionContent::Plot(Box::new(plot))),
            section(
                match s {
                    Statistic::T2 => SectionKind::PcaChartT2ContributionNote,
                    Statistic::Spe => SectionKind::PcaChartSpeContributionNote,
                },
                format!("About {label} Contributions"),
                SectionContent::Note(note.to_owned()),
            ),
        ]
    }

    /// The signalling rows' standardized values across the variables, against
    /// the reference mean (0) and ±3 reference standard deviations.
    fn parallel_coordinates(&self) -> ReportSection {
        let c = &self.chart;
        let p = self.names.len();
        // Reference standard deviation of each standardized variable (1 for a
        // correlation model).
        let m = c.reference_count;
        let sd: Vec<f64> = (0..p)
            .map(|j| {
                let ss: f64 = c.standardized[..m].iter().map(|z| z[j] * z[j]).sum();
                (ss / (m - 1) as f64).sqrt()
            })
            .collect();
        let mut shown: Vec<usize> = self.signals();
        shown.sort_by(|a, b| {
            let r = |i: usize| (c.t2[i].statistic / c.t2[i].upper).max(c.spe[i].statistic / c.spe[i].upper);
            r(*b).total_cmp(&r(*a))
        });
        shown.truncate(PCA_PLOT_POINTS);
        shown.sort_unstable();
        let mut points = Vec::new();
        let mut membership = Vec::new();
        let mut lines = Vec::new();
        let mut labels = Vec::new();
        for (g, &i) in shown.iter().enumerate() {
            let line: Vec<[f64; 2]> = (0..p)
                .map(|j| [(j + 1) as f64, if sd[j] > 0.0 { c.standardized[i][j] / sd[j] } else { 0.0 }])
                .collect();
            for &[x, y] in &line {
                points.push(RowPoint { row: self.rows[i], x, y });
                membership.push(g);
            }
            lines.push(line);
            labels.push(format!("Point {}", self.point_name(i)));
        }
        let right = p as f64 + 0.5;
        let rule = |y: f64| vec![[0.5, y], [right, y]];
        let plot = PlotSpec {
            x_label: "Variable".to_owned(),
            y_label: "Standardized value (reference SDs from the reference mean)".to_owned(),
            points,
            groups: Some(PointGroups { title: "Point".to_owned(), labels, membership, lines }),
            x_ticks: Some(self.names.iter().enumerate().map(|(j, n)| ((j + 1) as f64, n.clone())).collect()),
            segments: vec![rule(0.0), rule(-3.0), rule(3.0)],
            annotations: Some(Annotations {
                groups: Vec::new(),
                items: Vec::new(),
                labels: vec![
                    (right, 0.0, "Reference mean".to_owned()),
                    (right, 3.0, "+3 SD".to_owned()),
                    (right, -3.0, "−3 SD".to_owned()),
                ],
            }),
            ..PlotSpec::default()
        };
        let title = if self.signals().len() > PCA_PLOT_POINTS {
            format!("Signalling Points against the Reference (the {PCA_PLOT_POINTS} largest)")
        } else {
            "Signalling Points against the Reference".to_owned()
        };
        section(SectionKind::PcaChartParallel, title, SectionContent::Plot(Box::new(plot)))
    }

    fn change_point_sections(&self) -> Vec<ReportSection> {
        let title = "Change Point in the Mean";
        let Some(change) = &self.change else { return Vec::new() };
        let cp = match change {
            Ok(cp) => cp,
            Err(e) => {
                return vec![section(
                    SectionKind::PcaChartChangePoint,
                    title,
                    SectionContent::Note(format!("No change-point estimate: {e}")),
                )];
            }
        };
        let mut t = TextTable::new(["Statistic", "Value"]);
        let k = cp.before;
        t.push(["Last point before the shift".to_owned(), self.point_name(k - 1)]);
        t.push(["First point after the shift".to_owned(), self.point_name(k)]);
        t.push(["Row of the first point after".to_owned(), (self.rows[k] + 1).to_string()]);
        t.push(["Likelihood-ratio statistic".to_owned(), format_number(cp.statistic, DIGITS)]);
        t.push([
            "p-value (permutation)".to_owned(),
            format!("{} ({} permutations)", format_number(cp.p_value, 4), cp.permutations),
        ]);
        let note = format!(
            "The most likely single point at which the mean of the {} retained scores shifted, over all \
             observations: the split that maximizes the likelihood-ratio statistic for a change in the mean \
             vector with a common covariance (Sullivan & Woodall 2000). A small p-value says that a shift is \
             unlikely to be chance; a shift in a direction outside the model shows on the SPE chart instead.",
            self.chart.components
        );
        let points: Vec<RowPoint> = cp
            .statistics
            .iter()
            .enumerate()
            .map(|(i, v)| RowPoint { row: self.rows[i], x: (i + 1) as f64, y: *v })
            .collect();
        let line: Vec<[f64; 2]> = points.iter().map(|p| [p.x, p.y]).collect();
        let plot = PlotSpec {
            x_label: "Last observation before the split".to_owned(),
            y_label: "Likelihood-ratio statistic".to_owned(),
            points,
            line: Some(line),
            line_name: Some("Statistic".to_owned()),
            point_name: Some("Split".to_owned()),
            ..PlotSpec::default()
        };
        vec![
            section(SectionKind::PcaChartChangePoint, title, SectionContent::Table(t)),
            section(SectionKind::PcaChartChangePointNote, "About the Change Point", SectionContent::Note(note)),
            section(SectionKind::PcaChartChangePointPlot, "Change-Point Scan", SectionContent::Plot(Box::new(plot))),
        ]
    }

    fn loadings(&self) -> ReportSection {
        let c = &self.chart;
        let mut header = vec!["Variable".to_owned(), "Reference mean".to_owned()];
        if c.options.standardize {
            header.push("Reference SD".to_owned());
        }
        header.extend((1..=c.components).map(|a| format!("Loading {a}")));
        let mut t = TextTable::new(header);
        for (j, name) in self.names.iter().enumerate() {
            let mut row = vec![name.clone(), format_number(c.means[j], DIGITS)];
            if c.options.standardize {
                row.push(format_number(c.scales[j], DIGITS));
            }
            row.extend(c.loadings[j].iter().map(|v| format_number(*v, 4)));
            t.push(row);
        }
        let mut row = vec!["Eigenvalue".to_owned(), String::new()];
        if c.options.standardize {
            row.push(String::new());
        }
        row.extend(c.eigenvalues[..c.components].iter().map(|v| format_number(*v, DIGITS)));
        t.push(row);
        section(SectionKind::PcaChartLoadings, "Reference Model: Means and Loadings", SectionContent::Table(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sensor_batches;

    #[test]
    fn sample_signals_and_brushing_rows() {
        let t = sensor_batches();
        let rows = RowStateManager::new(t.n_rows());
        let cols: Vec<usize> = (2..t.n_cols()).collect();
        let request = PcaChartRequest { reference: Some(60), label: Some(1), ..PcaChartRequest::new(cols) };
        let a = PcaChartAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.chart().components, 2, "two latent drivers: {:?}", a.chart().press);
        let signals = a.signal_rows();
        assert!(!signals.is_empty() && signals.iter().all(|&r| r >= 60), "{signals:?}");
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        for k in [
            SectionKind::PcaChartCrossValidation,
            SectionKind::PcaChartT2,
            SectionKind::PcaChartSpe,
            SectionKind::PcaChartSpeContributionPlot,
            SectionKind::PcaChartParallel,
            SectionKind::PcaChartChangePoint,
        ] {
            assert!(kinds.contains(&k), "{k:?}");
        }
        let spe = s.iter().find(|x| x.kind == SectionKind::PcaChartSpe).unwrap();
        let SectionContent::ControlChart(c) = &spe.content else { panic!() };
        assert_eq!(c.points[70].rows, [70]);
        assert_eq!(c.phases.len(), 2);
        // The SPE signals point at the sensor whose correlation broke.
        let table = s.iter().find(|x| x.kind == SectionKind::PcaChartSignals).unwrap();
        let SectionContent::Table(tt) = &table.content else { panic!() };
        assert!(tt.rows.iter().any(|r| r[6].starts_with("Coolant temperature")), "{:?}", tt.rows);
        assert!(a.title().starts_with("Principal-Components Control Chart of"));
    }

    #[test]
    fn json_round_trip_exclusion_and_errors() {
        let t = sensor_batches();
        let mut rows = RowStateManager::new(t.n_rows());
        let r = PcaChartRequest {
            components: Some(3),
            standardize: false,
            spe_method: SpeLimitMethod::ScaledChiSquare,
            reference: Some(50),
            label: Some(1),
            ..PcaChartRequest::new(vec![2, 3, 4, 5])
        };
        let back = PcaChartRequest::from_json(&r.to_json(&t), &t).unwrap();
        assert_eq!(back, r);
        rows.set_excluded([0, 1], true);
        let a = PcaChartAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(a.rows.len(), t.n_rows() - 2);
        assert_eq!(a.chart().components, 3);
        assert!(a.sections().iter().all(|s| s.kind != SectionKind::PcaChartCrossValidation));
        assert!(PcaChartAnalysis::run(&t, &rows, &PcaChartRequest::new(vec![2])).is_err());
        assert!(PcaChartAnalysis::run(&t, &rows, &PcaChartRequest::new(vec![1, 2])).is_err(), "text column");
    }
}
