//! Shelf-life estimation bound to the row state
//! ([`stat_linear::stability`]): batch poolability tests at the ICH Q1E
//! level of 0.25 and the time at which the confidence bound for the mean
//! meets the acceptance limit(s).
//!
//! Rows that are excluded or miss the time, the response or the batch are
//! left out. Without a batch column all rows are one batch.
//!
//! With [`ShelfLifeRequest::random_batches`] the batches are a random sample
//! of future production ([`stat_mixed::shelf_life`]): random intercepts and
//! slopes by batch, and the shelf life where the bound for a future batch's
//! mean (or for the population mean) meets the limit. The fixed-batch
//! analysis is still run, for its extrapolation limit and for comparison.

use crate::format::{format_number, format_p_value};
use crate::report::{
    Annotations, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_linear::{ExtrapolationRule, PoolingModel, ShelfLifeOptions, ShelfLifeResult, StabilityTimeUnit, shelf_life};
use stat_mixed::{RandomBatchOptions, RandomBatchShelfLife, random_batch_shelf_life};

const DIGITS: usize = 6;
const CURVE_POINTS: usize = 100;

/// What to analyse.
#[derive(Debug, Clone, PartialEq)]
pub struct ShelfLifeRequest {
    /// Time column (e.g. months on stability).
    pub time: usize,
    /// Measured attribute (e.g. assay).
    pub response: usize,
    /// Batch (lot) column; `None` for a single batch.
    pub batch: Option<usize>,
    /// Lower acceptance limit.
    pub lower: Option<f64>,
    /// Upper acceptance limit.
    pub upper: Option<f64>,
    /// Significance level of the poolability tests (ICH Q1E: 0.25).
    pub pooling_alpha: f64,
    /// Confidence level of the bound (0.95).
    pub confidence_level: f64,
    /// Longest time searched; `None` for twice the longest time observed.
    pub horizon: Option<f64>,
    /// Unit of the time column, for ICH Q1E's 12-month extrapolation limit;
    /// `None` when not stated (only the twice-the-data limit applies).
    pub time_unit: Option<StabilityTimeUnit>,
    /// Treat the batches as random (random intercepts and slopes) rather
    /// than testing them for poolability.
    pub random_batches: bool,
    /// With random batches: the shelf life is where the prediction bound
    /// for a future batch's mean meets the limit; otherwise where the bound
    /// for the population mean does.
    pub future_batch: bool,
}

impl ShelfLifeRequest {
    /// ICH Q1E defaults with a lower acceptance limit.
    #[must_use]
    pub fn lower(time: usize, response: usize, batch: Option<usize>, limit: f64) -> Self {
        Self {
            time,
            response,
            batch,
            lower: Some(limit),
            upper: None,
            pooling_alpha: 0.25,
            confidence_level: 0.95,
            horizon: None,
            time_unit: Some(StabilityTimeUnit::Months),
            random_batches: false,
            future_batch: true,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.time, self.response];
        c.extend(self.batch);
        c
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.time = f(self.time);
        self.response = f(self.response);
        self.batch = self.batch.map(f);
    }
}

/// A computed shelf-life analysis.
#[derive(Debug, Clone)]
pub struct ShelfLifeAnalysis {
    request: ShelfLifeRequest,
    time_name: String,
    response_name: String,
    batch_name: Option<String>,
    rows: Vec<usize>,
    time: Vec<f64>,
    response: Vec<f64>,
    batch: Vec<String>,
    result: ShelfLifeResult,
    /// The random-batch analysis, when asked for.
    random: Option<RandomBatchShelfLife>,
    n_excluded: usize,
    n_missing: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

impl ShelfLifeAnalysis {
    /// Compute the analysis.
    ///
    /// # Errors
    /// Missing or non-numeric columns, no usable rows, and errors from
    /// [`shelf_life`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ShelfLifeRequest) -> Result<Self> {
        let numeric = |i: usize| -> Result<(&str, &[f64])> {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            Ok((c.name.as_str(), c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?))
        };
        let (time_name, t) = numeric(request.time)?;
        let (response_name, y) = numeric(request.response)?;
        let batch_col =
            request.batch.map(|i| table.column(i).ok_or_else(|| invalid(format!("no column {i}")))).transpose()?;
        let (mut rows, mut time, mut response, mut batch) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut n_excluded, mut n_missing) = (0, 0);
        for r in 0..table.n_rows() {
            let label = batch_col.map_or_else(|| "All".to_owned(), |c| c.level_text(r));
            if !(t[r].is_finite() && y[r].is_finite()) || label.is_empty() {
                n_missing += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                rows.push(r);
                time.push(t[r]);
                response.push(y[r]);
                batch.push(label);
            }
        }
        if rows.is_empty() {
            return Err(invalid("no usable rows"));
        }
        let options = ShelfLifeOptions {
            lower_limit: request.lower,
            upper_limit: request.upper,
            pooling_alpha: request.pooling_alpha,
            confidence_level: request.confidence_level,
            horizon: request.horizon,
            time_unit: request.time_unit,
        };
        let result = shelf_life(&time, &response, &batch, &options)?;
        let random = if request.random_batches {
            if request.batch.is_none() {
                return Err(invalid("random batches need a batch column"));
            }
            let options = RandomBatchOptions {
                lower_limit: request.lower,
                upper_limit: request.upper,
                confidence_level: request.confidence_level,
                horizon: Some(result.horizon),
            };
            // Batches in natural order, as the fixed analysis has them.
            let order: Vec<usize> = {
                let mut idx: Vec<usize> = (0..batch.len()).collect();
                idx.sort_by_key(|&i| result.batches.iter().position(|b| *b == batch[i]).unwrap_or(0));
                idx
            };
            let pick = |v: &[f64]| order.iter().map(|&i| v[i]).collect::<Vec<f64>>();
            let labels: Vec<String> = order.iter().map(|&i| batch[i].clone()).collect();
            Some(random_batch_shelf_life(&pick(&time), &pick(&response), &labels, &options)?)
        } else {
            None
        };
        Ok(Self {
            request: request.clone(),
            time_name: time_name.to_owned(),
            response_name: response_name.to_owned(),
            batch_name: batch_col.map(|c| c.name.clone()),
            rows,
            time,
            response,
            batch,
            result,
            random,
            n_excluded,
            n_missing,
        })
    }

    /// The engine's result.
    #[must_use]
    pub fn result(&self) -> &ShelfLifeResult {
        &self.result
    }

    /// The random-batch result, when asked for.
    #[must_use]
    pub fn random_result(&self) -> Option<&RandomBatchShelfLife> {
        self.random.as_ref()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.batch_name {
            Some(b) if self.random.is_some() => {
                let bound = if self.request.future_batch { "future batch" } else { "mean of batches" };
                format!("Shelf Life: {} over {}, {b} random ({bound})", self.response_name, self.time_name)
            }
            Some(b) => format!("Shelf Life: {} over {} by {b}", self.response_name, self.time_name),
            None => format!("Shelf Life: {} over {}", self.response_name, self.time_name),
        }
    }

    fn bound_name(&self) -> String {
        let level = format_number(100.0 * self.request.confidence_level, 4);
        match (self.request.lower, self.request.upper) {
            (Some(_), Some(_)) => format!("two-sided {level}% confidence bounds"),
            (Some(_), None) => format!("one-sided {level}% lower confidence bound"),
            _ => format!("one-sided {level}% upper confidence bound"),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        if let Some(r) = &self.random {
            return self.random_sections(r);
        }
        let mut out = vec![self.summary()];
        if self.result.slope_test.is_some() || self.result.intercept_test.is_some() {
            out.push(self.tests());
        }
        out.extend([self.lines(), self.plot(), self.note()]);
        out
    }

    fn time_text(&self, t: Option<f64>) -> String {
        t.map_or_else(
            || format!("beyond {} (the horizon searched)", format_number(self.result.horizon, 4)),
            |v| format_number(v, 4),
        )
    }

    /// The extrapolation limit and the rule behind it.
    fn limit_text(&self) -> String {
        let r = &self.result;
        let value = format_number(r.extrapolation_limit, 4);
        match (r.extrapolation_rule, self.request.time_unit) {
            (ExtrapolationRule::TwelveMonthsBeyond, Some(unit)) => {
                format!("{value} {} (12 months beyond the longest time)", unit.label())
            }
            (_, Some(unit)) => format!("{value} {} (twice the longest time)", unit.label()),
            (_, None) => format!("{value} (twice the longest time; time unit not stated, so 12 months not checked)"),
        }
    }

    fn summary(&self) -> ReportSection {
        let r = &self.result;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Observations", self.rows.len().to_string());
        row("Batches", r.batches.len().to_string());
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            row("Rows with missing values", self.n_missing.to_string());
        }
        let limits = match (self.request.lower, self.request.upper) {
            (Some(l), Some(u)) => format!("{} to {}", format_number(l, DIGITS), format_number(u, DIGITS)),
            (Some(l), None) => format!("at least {}", format_number(l, DIGITS)),
            (None, Some(u)) => format!("at most {}", format_number(u, DIGITS)),
            (None, None) => String::new(),
        };
        row("Acceptance limits", limits);
        row("Bound compared", self.bound_name());
        row("Model chosen", r.model.label().to_owned());
        row("Root mean square error", format!("{} on {} df", format_number(r.rmse, DIGITS), r.df_error));
        row(&format!("Shelf life ({})", self.time_name), self.time_text(r.shelf_life));
        if let (Some(j), true) = (r.limiting_line, r.lines.len() > 1) {
            row("Limiting batch", r.lines[j].batch.clone());
        }
        row("Longest time observed", format_number(r.max_time, 4));
        row("ICH Q1E extrapolation limit", self.limit_text());
        row("Shelf life within the limit", self.time_text(r.supported_shelf_life()));
        section(SectionKind::ShelfLifeSummary, "Summary", SectionContent::Table(t))
    }

    fn tests(&self) -> ReportSection {
        let r = &self.result;
        let alpha = format_number(self.request.pooling_alpha, 3);
        let mut t = TextTable::new(["Test", "F", "DF", "p-value", "Decision"]);
        for (test, reject, keep) in [
            (r.slope_test.as_ref(), "slopes differ: separate lines", "slopes pooled"),
            (r.intercept_test.as_ref(), "intercepts differ: parallel lines", "intercepts pooled"),
        ] {
            if let Some(test) = test {
                t.push([
                    test.name.clone(),
                    format_number(test.f, 4),
                    format!("{}, {}", test.df_numerator, test.df_denominator),
                    format_p_value(test.p_value),
                    if test.p_value < self.request.pooling_alpha { reject } else { keep }.to_owned(),
                ]);
            }
        }
        section(
            SectionKind::PoolabilityTests,
            format!("Batch Poolability Tests (α = {alpha})"),
            SectionContent::Table(t),
        )
    }

    fn lines(&self) -> ReportSection {
        let r = &self.result;
        let first = if r.model == PoolingModel::Pooled { "Line" } else { "Batch" };
        let mut t = TextTable::new([first, "Intercept", "Slope", "N", "Shelf life"]);
        for l in &r.lines {
            t.push([
                l.batch.clone(),
                format_number(l.intercept, DIGITS),
                format_number(l.slope, DIGITS),
                l.n.to_string(),
                self.time_text(l.shelf_life),
            ]);
        }
        section(SectionKind::ShelfLifeEstimates, "Fitted Lines and Shelf Life", SectionContent::Table(t))
    }

    fn plot(&self) -> ReportSection {
        let r = &self.result;
        let end = r.horizon.max(r.shelf_life.unwrap_or(0.0));
        let grid: Vec<f64> = (0..CURVE_POINTS).map(|i| end * i as f64 / (CURVE_POINTS - 1) as f64).collect();
        let points: Vec<RowPoint> = self
            .rows
            .iter()
            .zip(self.time.iter().zip(&self.response))
            .map(|(&row, (&x, &y))| RowPoint { row, x, y })
            .collect();
        let pooled = r.model == PoolingModel::Pooled;
        let membership: Vec<usize> =
            self.batch.iter().map(|b| r.batches.iter().position(|x| x == b).unwrap_or(0)).collect();
        let curve = |j: usize, pick: fn((f64, f64, f64)) -> f64| -> Vec<[f64; 2]> {
            grid.iter().filter_map(|&t| r.bounds(j, t).ok().map(|b| [t, pick(b)])).collect()
        };
        let lines: Vec<Vec<[f64; 2]>> =
            (0..r.batches.len()).map(|j| if pooled { Vec::new() } else { curve(j, |b| b.0) }).collect();
        let limiting = r.limiting_line.unwrap_or(0);
        let mut segments = Vec::new();
        let mut labels = Vec::new();
        for (limit, name) in [(self.request.lower, "lower limit"), (self.request.upper, "upper limit")] {
            if let Some(v) = limit {
                segments.push(vec![[0.0, v], [end, v]]);
                labels.push((end, v, format!("{name} {}", format_number(v, DIGITS))));
            }
        }
        if let Some(s) = r.shelf_life {
            let (lo, hi) = self
                .response
                .iter()
                .chain(self.request.lower.iter())
                .chain(self.request.upper.iter())
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
            segments.push(vec![[s, lo], [s, hi]]);
            labels.push((s, hi, format!("shelf life {}", format_number(s, 4))));
        }
        let spec = PlotSpec {
            x_label: self.time_name.clone(),
            y_label: self.response_name.clone(),
            points,
            line: pooled.then(|| curve(0, |b| b.0)),
            line_name: Some("Pooled regression line".to_owned()),
            band: Some((curve(limiting, |b| b.1), curve(limiting, |b| b.2))),
            band_name: Some(upper_first(&self.bound_name())),
            groups: (r.batches.len() > 1).then(|| PointGroups {
                title: self.batch_name.clone().unwrap_or_default(),
                labels: r.batches.clone(),
                membership,
                lines,
            }),
            segments,
            annotations: Some(Annotations { groups: Vec::new(), items: Vec::new(), labels }),
            ..PlotSpec::default()
        };
        let whose = if pooled { "the pooled line".to_owned() } else { format!("batch {}", r.lines[limiting].batch) };
        section(
            SectionKind::ShelfLifePlot,
            format!("Stability Plot (band: {} of {whose})", self.bound_name()),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn note(&self) -> ReportSection {
        let r = &self.result;
        let mut text = match r.model {
            PoolingModel::Pooled => {
                "Slopes and intercepts did not differ significantly, so all batches were pooled into one line."
                    .to_owned()
            }
            PoolingModel::CommonSlope => "Slopes did not differ significantly but intercepts did: the batches \
                share a common slope, and the shelf life is that of the batch whose bound meets the limit first."
                .to_owned(),
            PoolingModel::Separate => "Slopes differ significantly: each batch has its own line (with the error \
                variance pooled across batches), and the shelf life is the shortest batch shelf life."
                .to_owned(),
        };
        if r.batches.len() == 1 {
            text = "One batch: a single line is fitted.".to_owned();
        } else if r.slope_test.is_none() {
            // The slopes were assumed common (see `notes`), not tested.
            text = if r.model == PoolingModel::Pooled {
                "Intercepts did not differ significantly, so all batches were pooled into one line.".to_owned()
            } else {
                "Intercepts differ significantly: the batches share a common slope, and the shelf life is that \
                 of the batch whose bound meets the limit first."
                    .to_owned()
            };
        }
        for note in r.notes.iter().rev() {
            text = format!("{note} {text}");
        }
        if r.shelf_life.is_none_or(|s| s > r.max_time) {
            text.push_str(&format!(
                " The shelf life extrapolates beyond the longest time observed ({}). ICH Q1E allows \
                 extrapolation up to twice the period covered by long-term data and no more than 12 months \
                 beyond it, and only when supported by the data and any accelerated studies.",
                format_number(r.max_time, 4)
            ));
            if r.shelf_life.is_none_or(|s| s > r.extrapolation_limit) {
                text.push_str(&format!(
                    " Here that limit is {}, so the shelf life within the limit is the longest these data support.",
                    format_number(r.extrapolation_limit, 4)
                ));
            }
            if self.request.time_unit.is_none() {
                text.push_str(" State the time unit to check the 12-month part of the limit.");
            }
        }
        section(SectionKind::ShelfLifeNote, "Notes", SectionContent::Note(text))
    }
}

impl ShelfLifeAnalysis {
    /// Name of the bound used with random batches.
    fn random_bound_name(&self) -> String {
        let level = format_number(100.0 * self.request.confidence_level, 4);
        let what = if self.request.future_batch {
            "prediction bound for a future batch's mean"
        } else {
            "confidence bound for the population mean"
        };
        match (self.request.lower, self.request.upper) {
            (Some(_), Some(_)) => format!("two-sided {level}% {}", what.replace("bound", "bounds")),
            (Some(_), None) => format!("one-sided {level}% lower {what}"),
            _ => format!("one-sided {level}% upper {what}"),
        }
    }

    fn random_sections(&self, r: &RandomBatchShelfLife) -> Vec<ReportSection> {
        vec![
            self.random_summary(r),
            self.random_estimates(r),
            self.random_variances(r),
            self.random_lines(r),
            self.random_plot(r),
            self.random_note(r),
        ]
    }

    /// The headline shelf life with random batches.
    fn random_life(&self, r: &RandomBatchShelfLife) -> Option<f64> {
        if self.request.future_batch { r.batch_shelf_life } else { r.mean_shelf_life }
    }

    fn random_summary(&self, r: &RandomBatchShelfLife) -> ReportSection {
        let fixed = &self.result;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Observations", self.rows.len().to_string());
        row("Batches", r.batches.len().to_string());
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            row("Rows with missing values", self.n_missing.to_string());
        }
        let limits = match (self.request.lower, self.request.upper) {
            (Some(l), Some(u)) => format!("{} to {}", format_number(l, DIGITS), format_number(u, DIGITS)),
            (Some(l), None) => format!("at least {}", format_number(l, DIGITS)),
            (None, Some(u)) => format!("at most {}", format_number(u, DIGITS)),
            (None, None) => String::new(),
        };
        row("Acceptance limits", limits);
        row("Batch model", "random intercepts and slopes (correlated), REML".to_owned());
        row("Bound compared", self.random_bound_name());
        row(&format!("Shelf life ({})", self.time_name), self.time_text(self.random_life(r)));
        row("Shelf life for a future batch", self.time_text(r.batch_shelf_life));
        row("Shelf life for the population mean", self.time_text(r.mean_shelf_life));
        row(
            "Fixed-batch shelf life (ICH Q1E pooling)",
            format!("{} ({})", self.time_text(fixed.shelf_life), fixed.model.label()),
        );
        row("Longest time observed", format_number(r.max_time, 4));
        row("ICH Q1E extrapolation limit", self.limit_text());
        let within = match self.random_life(r) {
            Some(v) => Some(v.min(fixed.extrapolation_limit)),
            None => (r.horizon >= fixed.extrapolation_limit).then_some(fixed.extrapolation_limit),
        };
        row("Shelf life within the limit", self.time_text(within));
        for w in &r.fit.warnings {
            row("Warning", w.clone());
        }
        section(SectionKind::ShelfLifeSummary, "Summary", SectionContent::Table(t))
    }

    fn random_estimates(&self, r: &RandomBatchShelfLife) -> ReportSection {
        let mut t = TextTable::new(["Term", "Estimate", "Std Error", "DF", "t", "p-value"]);
        for (c, name) in r.fit.fixed.iter().zip(["Intercept".to_owned(), self.time_name.clone()]) {
            t.push([
                name,
                format_number(c.estimate, DIGITS),
                format_number(c.std_error, DIGITS),
                c.df.map_or("—".to_owned(), |d| format_number(d, 4)),
                format_number(c.statistic, 4),
                format_p_value(c.p_value),
            ]);
        }
        section(
            SectionKind::ParameterEstimates,
            "Population Line (fixed effects, Satterthwaite degrees of freedom)",
            SectionContent::Table(t),
        )
    }

    fn random_variances(&self, r: &RandomBatchShelfLife) -> ReportSection {
        let level = format_number(100.0 * r.fit.confidence_level(), 3);
        let mut t = TextTable::new(vec![
            "Component".to_owned(),
            "Variance".to_owned(),
            "Std Dev".to_owned(),
            format!("Lower {level}%"),
            format!("Upper {level}%"),
        ]);
        let names = [
            format!("{} intercept", self.batch_name.clone().unwrap_or_default()),
            format!("{} slope", self.batch_name.clone().unwrap_or_default()),
        ];
        for (c, name) in
            r.fit.variance_components.iter().zip(names).chain(r.fit.residual.iter().zip(["Residual".to_owned()]))
        {
            t.push([
                name,
                format_number(c.variance, DIGITS),
                format_number(c.std_dev, DIGITS),
                c.interval.map_or("—".to_owned(), |v| format_number(v.0, DIGITS)),
                c.interval.map_or("—".to_owned(), |v| format_number(v.1, DIGITS)),
            ]);
        }
        if let Some(c) = r.fit.correlations.first() {
            t.push([
                "Intercept–slope correlation".to_owned(),
                format_number(c.value, 4),
                String::new(),
                String::new(),
                String::new(),
            ]);
        }
        section(SectionKind::VarianceComponents, "Batch-to-Batch Variation", SectionContent::Table(t))
    }

    fn random_lines(&self, r: &RandomBatchShelfLife) -> ReportSection {
        let mut t = TextTable::new(["Batch", "Intercept", "Slope", "N"]);
        t.push([
            "Population mean".to_owned(),
            format_number(r.fit.fixed[0].estimate, DIGITS),
            format_number(r.fit.fixed[1].estimate, DIGITS),
            self.rows.len().to_string(),
        ]);
        for (b, (i, sl)) in r.batches.iter().zip(&r.batch_lines) {
            t.push([
                b.clone(),
                format_number(*i, DIGITS),
                format_number(*sl, DIGITS),
                self.batch.iter().filter(|x| *x == b).count().to_string(),
            ]);
        }
        section(
            SectionKind::ShelfLifeEstimates,
            "Population and Batch Lines (batch lines are BLUPs)",
            SectionContent::Table(t),
        )
    }

    fn random_plot(&self, r: &RandomBatchShelfLife) -> ReportSection {
        let life = self.random_life(r);
        let end = r.horizon.max(life.unwrap_or(0.0));
        let grid: Vec<f64> = (0..CURVE_POINTS).map(|i| end * i as f64 / (CURVE_POINTS - 1) as f64).collect();
        let points: Vec<RowPoint> = self
            .rows
            .iter()
            .zip(self.time.iter().zip(&self.response))
            .map(|(&row, (&x, &y))| RowPoint { row, x, y })
            .collect();
        let membership: Vec<usize> =
            self.batch.iter().map(|b| r.batches.iter().position(|x| x == b).unwrap_or(0)).collect();
        let lines: Vec<Vec<[f64; 2]>> =
            r.batch_lines.iter().map(|(a, b)| vec![[0.0, *a], [end, a + b * end]]).collect();
        let future = self.request.future_batch;
        let curve = |pick: fn((f64, f64, f64)) -> f64| -> Vec<[f64; 2]> {
            grid.iter().filter_map(|&t| r.bounds(t, future).ok().map(|b| [t, pick(b)])).collect()
        };
        let mut segments = Vec::new();
        let mut labels = Vec::new();
        for (limit, name) in [(self.request.lower, "lower limit"), (self.request.upper, "upper limit")] {
            if let Some(v) = limit {
                segments.push(vec![[0.0, v], [end, v]]);
                labels.push((end, v, format!("{name} {}", format_number(v, DIGITS))));
            }
        }
        if let Some(s) = life {
            let (lo, hi) = self
                .response
                .iter()
                .chain(self.request.lower.iter())
                .chain(self.request.upper.iter())
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
            segments.push(vec![[s, lo], [s, hi]]);
            labels.push((s, hi, format!("shelf life {}", format_number(s, 4))));
        }
        let spec = PlotSpec {
            x_label: self.time_name.clone(),
            y_label: self.response_name.clone(),
            points,
            line: Some(curve(|b| b.0)),
            line_name: Some("Population mean line".to_owned()),
            band: Some((curve(|b| b.1), curve(|b| b.2))),
            band_name: Some(upper_first(&self.random_bound_name())),
            groups: Some(PointGroups {
                title: self.batch_name.clone().unwrap_or_default(),
                labels: r.batches.clone(),
                membership,
                lines,
            }),
            segments,
            annotations: Some(Annotations { groups: Vec::new(), items: Vec::new(), labels }),
            ..PlotSpec::default()
        };
        section(
            SectionKind::ShelfLifePlot,
            format!("Stability Plot (band: {})", self.random_bound_name()),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn random_note(&self, r: &RandomBatchShelfLife) -> ReportSection {
        let mut text = "The batches are treated as a random sample of future production: each has its own intercept \
            and slope around the population line, and the batch-to-batch variances are estimated by REML (Chow & \
            Shao 1991). The future-batch bound adds that variation to the uncertainty of the population line, so it \
            is the shorter, more conservative shelf life; the population-mean bound describes the average batch."
            .to_owned();
        if r.batches.len() < 6 {
            text.push_str(&format!(
                " With only {} batches the batch-to-batch variances are imprecise; the fixed-batch analysis \
                 (ICH Q1E pooling) is shown for comparison.",
                r.batches.len()
            ));
        }
        if self.random_life(r).is_none_or(|s| s > r.max_time) {
            text.push_str(&format!(
                " The shelf life extrapolates beyond the longest time observed ({}); ICH Q1E limits such \
                 extrapolation (see the extrapolation limit above).",
                format_number(r.max_time, 4)
            ));
        }
        section(SectionKind::ShelfLifeNote, "Notes", SectionContent::Note(text))
    }
}

/// `s` with its first letter in upper case (a key label from a phrase).
fn upper_first(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        let months = [0.0, 3.0, 6.0, 9.0, 12.0, 18.0, 24.0];
        let time: Vec<f64> = (0..3).flat_map(|_| months).collect();
        let assay = vec![
            100.7, 100.6, 100.0, 99.7, 99.1, 98.7, 98.4, 101.7, 101.6, 101.0, 100.7, 100.2, 98.9, 99.0, 100.4, 100.0,
            98.9, 98.6, 98.5, 98.0, 97.6,
        ];
        let batch: Vec<String> = ["A", "B", "C"].iter().flat_map(|b| std::iter::repeat_n((*b).to_owned(), 7)).collect();
        DataTable::new(
            "s",
            vec![Column::numeric("Month", time), Column::numeric("Assay", assay), Column::text("Batch", batch)],
        )
        .unwrap()
    }

    #[test]
    fn report_matches_engine_reference() {
        let t = table();
        let req = ShelfLifeRequest::lower(0, 1, Some(2), 95.0);
        let a = ShelfLifeAnalysis::run(&t, &RowStateManager::new(21), &req).unwrap();
        assert_eq!(a.title(), "Shelf Life: Assay over Month by Batch");
        // Engine reference (stat-linear stability tests): 39.4613 months, batch C.
        assert!((a.result().shelf_life.unwrap() - 39.461_310_015_069_216).abs() < 1e-6);
        let s = a.sections();
        assert_eq!(s.len(), 5);
        let SectionContent::Table(sum) = &s[0].content else { panic!() };
        let value = |name: &str| sum.rows.iter().find(|r| r[0] == name).map(|r| r[1].clone());
        assert_eq!(value("Model chosen").as_deref(), Some("separate intercepts, common slope"));
        // 24 months of data: ICH Q1E allows min(48, 24 + 12) = 36 months.
        assert_eq!(
            value("ICH Q1E extrapolation limit").as_deref(),
            Some("36 months (12 months beyond the longest time)")
        );
        assert_eq!(value("Shelf life within the limit").as_deref(), Some("36"));
        let SectionContent::Table(tests) = &s[1].content else { panic!() };
        assert_eq!(tests.cell(0, "Decision"), Some("slopes pooled"));
        assert_eq!(tests.cell(1, "Decision"), Some("intercepts differ: parallel lines"));
        let SectionContent::Plot(p) = &s[3].content else { panic!() };
        assert_eq!(p.groups.as_ref().unwrap().lines.len(), 3);
        assert_eq!(p.band_label(), "One-sided 95% lower confidence bound");
        let SectionContent::Note(n) = &s[4].content else { panic!() };
        assert!(n.contains("ICH Q1E allows") && n.contains("Here that limit is 36"));
        // Without a unit the 12-month part cannot be checked.
        let unitless = ShelfLifeRequest { time_unit: None, ..req };
        let a = ShelfLifeAnalysis::run(&t, &RowStateManager::new(21), &unitless).unwrap();
        let s = a.sections();
        let SectionContent::Table(sum) = &s[0].content else { panic!() };
        let limit = sum.rows.iter().find(|r| r[0] == "ICH Q1E extrapolation limit").map(|r| r[1].as_str());
        assert_eq!(limit, Some("48 (twice the longest time; time unit not stated, so 12 months not checked)"));
        let SectionContent::Note(n) = &s[4].content else { panic!() };
        assert!(n.contains("State the time unit") && !n.contains("Here that limit"));
    }

    #[test]
    fn single_batch_and_column_mapping() {
        let t = table();
        let mut rows = RowStateManager::new(21);
        rows.set_excluded(7..21, true);
        let req = ShelfLifeRequest { batch: None, ..ShelfLifeRequest::lower(0, 1, None, 95.0) };
        let a = ShelfLifeAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.result().lines.len(), 1);
        assert_eq!(a.sections().len(), 4, "no poolability tests");
        let mut r = ShelfLifeRequest::lower(0, 1, Some(2), 95.0);
        r.map_columns(&|c| c + 1);
        assert_eq!(r.columns(), vec![1, 2, 3]);
    }
}
