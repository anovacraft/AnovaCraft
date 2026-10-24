//! Robust (parameter) design with orthogonal arrays
//! ([`stat_doe::orthogonal`]): creating an inner × outer array experiment as
//! a data table, and analysing it by signal-to-noise ratios.
//!
//! The created table has one row per inner-array run (control-factor
//! settings) and one response column per outer-array run (noise conditions,
//! named in the column heading) or per replicate. The analysis takes any
//! table laid out that way: factor columns, and the columns holding each
//! run's repeated responses.

use crate::format::format_number;
use crate::project::{invalid, member, obj, s};
use crate::report::{PlotPane, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{Column, DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_doe::{LevelMeans, Rng, SignalToNoise, StandardArray, best_levels, level_means};

/// Significant digits in report tables.
const DIGITS: usize = 5;

/// A factor assigned to an array column: its name and level labels (their
/// number must match the column's levels).
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayFactor {
    /// Column name in the created table.
    pub name: String,
    /// Level labels; numeric labels make a numeric column.
    pub levels: Vec<String>,
}

/// Everything needed to create a robust-design table.
#[derive(Debug, Clone, PartialEq)]
pub struct RobustDesignRequest {
    /// Inner array, for the control factors.
    pub inner: StandardArray,
    /// Control factors, assigned to the inner array's first columns in order.
    pub control: Vec<ArrayFactor>,
    /// Outer array, for the noise factors (`None`: plain replicates).
    pub outer: Option<StandardArray>,
    /// Noise factors, assigned to the outer array's first columns.
    pub noise: Vec<ArrayFactor>,
    /// Response columns when there is no outer array.
    pub replicates: usize,
    /// Base name of the response columns.
    pub response: String,
    /// Randomize the order of the inner runs with this seed.
    pub randomize: Option<u64>,
}

impl RobustDesignRequest {
    /// An L8 inner array with `k` two-level control factors `A, B, …` at
    /// levels 1 and 2, and an L4 outer array with two noise factors.
    #[must_use]
    pub fn new(k: usize) -> Self {
        let two = |name: String| ArrayFactor { name, levels: vec!["1".into(), "2".into()] };
        Self {
            inner: StandardArray::L8,
            control: stat_doe::default_factor_names(k).into_iter().map(two).collect(),
            outer: Some(StandardArray::L4),
            noise: vec![two("N1".into()), two("N2".into())],
            replicates: 2,
            response: "Y".to_owned(),
            randomize: None,
        }
    }
}

/// Check factors against an array's columns.
fn check_factors(array: StandardArray, factors: &[ArrayFactor], role: &str) -> Result<()> {
    let levels = array.levels();
    if factors.is_empty() {
        return Err(invalid(format!("add at least one {role} factor")));
    }
    if factors.len() > levels.len() {
        return Err(invalid(format!(
            "{} has {} columns, too few for {} {role} factors",
            array.label(),
            levels.len(),
            factors.len()
        )));
    }
    for (f, &l) in factors.iter().zip(&levels) {
        if f.name.trim().is_empty() {
            return Err(invalid(format!("every {role} factor needs a name")));
        }
        if f.levels.len() != l {
            return Err(invalid(format!(
                "{} is assigned to a {l}-level column of {}, but has {} levels",
                f.name.trim(),
                array.label(),
                f.levels.len()
            )));
        }
        if f.levels.iter().any(|v| v.trim().is_empty()) {
            return Err(invalid(format!("{} has a blank level", f.name.trim())));
        }
    }
    Ok(())
}

/// A factor's column: numeric when every label is a number.
fn factor_column(f: &ArrayFactor, settings: impl Iterator<Item = usize>) -> Column {
    let labels: Vec<&str> = f.levels.iter().map(|l| l.trim()).collect();
    let numbers: Option<Vec<f64>> = labels.iter().map(|l| l.parse::<f64>().ok()).collect();
    match numbers {
        Some(v) => {
            let mut c = Column::numeric(f.name.trim(), settings.map(|i| v[i]).collect());
            c.level = MeasurementLevel::Ordinal;
            c
        }
        None => Column::text(f.name.trim(), settings.map(|i| labels[i].to_owned()).collect()),
    }
}

/// Build the robust-design table and a one-line description.
///
/// # Errors
/// Too many factors for an array, a factor whose number of levels differs
/// from its column's, blank or repeated names, no replicates.
pub fn create_robust_design(request: &RobustDesignRequest) -> Result<(DataTable, String)> {
    check_factors(request.inner, &request.control, "control")?;
    let inner = request.inner.array();
    let response = request.response.trim();
    if response.is_empty() {
        return Err(invalid("name the response"));
    }
    let mut order: Vec<usize> = (0..inner.len()).collect();
    if let Some(seed) = request.randomize {
        Rng::new(seed).shuffle(&mut order);
    }
    let mut columns = vec![Column::numeric("Run", (1..=inner.len()).map(|i| i as f64).collect())];
    if request.randomize.is_some() {
        let mut c = Column::numeric("Std order", order.iter().map(|&i| (i + 1) as f64).collect());
        c.level = MeasurementLevel::Ordinal;
        columns.push(c);
    }
    for (j, f) in request.control.iter().enumerate() {
        columns.push(factor_column(f, order.iter().map(|&r| inner[r][j] - 1)));
    }
    let n = inner.len();
    let description = match request.outer {
        Some(outer) => {
            check_factors(outer, &request.noise, "noise")?;
            let rows = outer.array();
            for o in &rows {
                let conditions: Vec<String> = request
                    .noise
                    .iter()
                    .enumerate()
                    .map(|(j, f)| format!("{}={}", f.name.trim(), f.levels[o[j] - 1].trim()))
                    .collect();
                columns.push(Column::numeric(format!("{response} ({})", conditions.join(", ")), vec![f64::NAN; n]));
            }
            format!(
                "{} inner array ({} control factors) × {} outer array ({} noise conditions)",
                request.inner.label(),
                request.control.len(),
                outer.label(),
                rows.len()
            )
        }
        None => {
            if request.replicates == 0 {
                return Err(invalid("use at least one response column"));
            }
            for r in 1..=request.replicates {
                let name = if request.replicates == 1 { response.to_owned() } else { format!("{response} {r}") };
                columns.push(Column::numeric(name, vec![f64::NAN; n]));
            }
            format!(
                "{} array ({} control factors), {} response column{}",
                request.inner.label(),
                request.control.len(),
                request.replicates,
                if request.replicates == 1 { "" } else { "s" }
            )
        }
    };
    let table = DataTable::new(format!("Robust design {}", request.inner.key()), columns)?;
    Ok((table, description))
}

// ---------------------------------------------------------------------------
// Signal-to-noise analysis
// ---------------------------------------------------------------------------

/// A signal-to-noise analysis: each row is a run, with factor settings and
/// repeated responses.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalToNoiseRequest {
    /// Control factor columns (numeric or text).
    pub factors: Vec<usize>,
    /// Response columns, one per noise condition or replicate.
    pub responses: Vec<usize>,
    /// The signal-to-noise ratio.
    pub ratio: SignalToNoise,
}

impl SignalToNoiseRequest {
    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.factors.iter().chain(&self.responses).copied().collect()
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.factors.iter_mut().for_each(|c| *c = f(*c));
        self.responses.iter_mut().for_each(|c| *c = f(*c));
        self
    }

    /// The project-file form (`"analysis": "signal-to-noise"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let cols = |c: &[usize]| {
            JsonValue::Array(c.iter().map(|&i| s(table.column(i).map_or("", |c| c.name.as_str()))).collect())
        };
        obj([
            ("analysis", s("signal-to-noise")),
            ("factors", cols(&self.factors)),
            ("responses", cols(&self.responses)),
            ("ratio", s(self.ratio.key())),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing or unknown columns or ratio.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let ratio = match v.get("ratio").and_then(JsonValue::as_str) {
            None => SignalToNoise::SmallerTheBetter,
            Some(k) => {
                SignalToNoise::from_key(k).ok_or_else(|| invalid(format!("signal-to-noise: unknown ratio {k:?}")))?
            }
        };
        Ok(Self { factors: column_list(v, table, "factors")?, responses: column_list(v, table, "responses")?, ratio })
    }
}

/// A list of columns by name.
pub(crate) fn column_list(v: &JsonValue, table: &DataTable, key: &str) -> Result<Vec<usize>> {
    let list = member(v, key)?.as_array().ok_or_else(|| invalid(format!("{key}: expected a list of column names")))?;
    list.iter()
        .map(|c| {
            let name = c.as_str().ok_or_else(|| invalid(format!("{key}: expected column names")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        })
        .collect()
}

/// One analysed run.
#[derive(Debug, Clone, PartialEq)]
struct Run {
    row: usize,
    /// 0-based level of each factor.
    settings: Vec<usize>,
    n: usize,
    mean: f64,
    sd: f64,
    eta: f64,
}

/// A computed signal-to-noise analysis.
#[derive(Debug, Clone)]
pub struct SignalToNoiseAnalysis {
    request: SignalToNoiseRequest,
    factor_names: Vec<String>,
    /// Level labels of each factor.
    labels: Vec<Vec<String>>,
    runs: Vec<Run>,
    eta_table: Vec<LevelMeans>,
    mean_table: Vec<LevelMeans>,
    n_excluded: usize,
    n_skipped: usize,
}

impl SignalToNoiseAnalysis {
    /// Analyse the non-excluded rows that have every factor setting and at
    /// least one response (two for the nominal-the-best ratios).
    ///
    /// # Errors
    /// No factors or responses, a column used twice, non-numeric responses,
    /// fewer than two runs, a factor with one level, or a ratio that cannot be
    /// computed for some run.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SignalToNoiseRequest) -> Result<Self> {
        if request.factors.is_empty() || request.responses.is_empty() {
            return Err(invalid("choose at least one factor and one response column"));
        }
        let all = request.columns();
        if all.iter().enumerate().any(|(i, c)| all[..i].contains(c)) {
            return Err(invalid("each column can be used only once"));
        }
        let col = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let responses: Vec<&[f64]> = request
            .responses
            .iter()
            .map(|&i| {
                let c = col(i)?;
                c.as_numeric().ok_or_else(|| invalid(format!("response {} must be numeric", c.name)))
            })
            .collect::<Result<_>>()?;
        let factor_cols: Vec<&Column> = request.factors.iter().map(|&i| col(i)).collect::<Result<_>>()?;
        let min_values = match request.ratio {
            SignalToNoise::NominalTheBest | SignalToNoise::NominalVariance => 2,
            _ => 1,
        };
        let mut used: Vec<(usize, Vec<String>, Vec<f64>)> = Vec::new();
        let (mut n_excluded, mut n_skipped) = (0, 0);
        for r in 0..table.n_rows() {
            let labels: Vec<String> = factor_cols.iter().map(|c| c.level_text(r)).collect();
            let values: Vec<f64> = responses.iter().map(|v| v[r]).filter(|v| v.is_finite()).collect();
            if labels.iter().any(String::is_empty) || values.len() < min_values {
                n_skipped += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                used.push((r, labels, values));
            }
        }
        if used.len() < 2 {
            return Err(invalid(format!(
                "fewer than two runs have every factor setting and at least {min_values} response value{}",
                if min_values == 1 { "" } else { "s" }
            )));
        }
        // Levels: numeric columns in increasing order, text in order of appearance.
        let mut labels: Vec<Vec<String>> = Vec::new();
        for (j, c) in factor_cols.iter().enumerate() {
            let mut l: Vec<String> = Vec::new();
            for (_, lab, _) in &used {
                if !l.contains(&lab[j]) {
                    l.push(lab[j].clone());
                }
            }
            if let Some(v) = c.as_numeric() {
                l.sort_by(|a, b| {
                    let pos = |s: &String| used.iter().find(|u| u.1[j] == *s).map_or(f64::NAN, |u| v[u.0]);
                    pos(a).total_cmp(&pos(b))
                });
            }
            if l.len() < 2 {
                return Err(invalid(format!("factor {} has a single level in the runs used", c.name)));
            }
            labels.push(l);
        }
        let mut runs = Vec::with_capacity(used.len());
        for (row, lab, values) in &used {
            let eta = request.ratio.ratio(values).map_err(|e| {
                invalid(format!(
                    "run {}: {} Choose another ratio or check that run's response values.",
                    row + 1,
                    crate::clipboard::error_text(&e)
                ))
            })?;
            let n = values.len() as f64;
            let mean = values.iter().sum::<f64>() / n;
            let sd = if values.len() > 1 {
                (values.iter().map(|y| (y - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
            } else {
                f64::NAN
            };
            let settings =
                lab.iter().enumerate().map(|(j, l)| labels[j].iter().position(|x| x == l).unwrap_or(0)).collect();
            runs.push(Run { row: *row, settings, n: values.len(), mean, sd, eta });
        }
        let settings: Vec<Vec<usize>> =
            (0..factor_cols.len()).map(|j| runs.iter().map(|r| r.settings[j]).collect()).collect();
        let nlev: Vec<usize> = labels.iter().map(Vec::len).collect();
        let eta_table = level_means(&settings, &nlev, &runs.iter().map(|r| r.eta).collect::<Vec<_>>())?;
        let mean_table = level_means(&settings, &nlev, &runs.iter().map(|r| r.mean).collect::<Vec<_>>())?;
        Ok(Self {
            request: request.clone(),
            factor_names: factor_cols.iter().map(|c| c.name.clone()).collect(),
            labels,
            runs,
            eta_table,
            mean_table,
            n_excluded,
            n_skipped,
        })
    }

    /// Mean signal-to-noise ratio at each level of each factor.
    #[must_use]
    pub fn eta_table(&self) -> &[LevelMeans] {
        &self.eta_table
    }

    /// Signal-to-noise ratio of each run, in row order.
    #[must_use]
    pub fn ratios(&self) -> Vec<f64> {
        self.runs.iter().map(|r| r.eta).collect()
    }

    /// The level of each factor with the largest mean ratio, and the ratio
    /// and mean response the additive model predicts there.
    #[must_use]
    pub fn best_settings(&self) -> (Vec<usize>, f64, f64) {
        let n = self.runs.len() as f64;
        let overall_eta = self.runs.iter().map(|r| r.eta).sum::<f64>() / n;
        let overall_mean = self.runs.iter().map(|r| r.mean).sum::<f64>() / n;
        let (best, eta) = best_levels(&self.eta_table, overall_eta);
        let mean =
            overall_mean + self.mean_table.iter().zip(&best).map(|(f, &b)| f.means[b] - overall_mean).sum::<f64>();
        (best, eta, mean)
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Signal-to-Noise Analysis: {}", self.request.ratio.label())
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let ratio = self.request.ratio;
        let (best, eta, mean) = self.best_settings();
        let n = self.runs.len() as f64;
        let mut summary = TextTable::new(["Item", "Value"]);
        let mut row = |a: &str, b: String| summary.push([a.to_owned(), b]);
        row("Ratio", format!("{} — {} (in dB; larger is better)", ratio.label(), ratio.formula()));
        row("Runs", self.runs.len().to_string());
        row("Responses per run", self.request.responses.len().to_string());
        row("Mean ratio over the runs", format_number(self.runs.iter().map(|r| r.eta).sum::<f64>() / n, DIGITS));
        row(
            "Settings with the largest ratio",
            best.iter()
                .enumerate()
                .map(|(j, &b)| format!("{} = {}", self.factor_names[j], self.labels[j][b]))
                .collect::<Vec<_>>()
                .join(", "),
        );
        row("Predicted ratio there (additive model)", format_number(eta, DIGITS));
        row("Predicted mean response there", format_number(mean, DIGITS));
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_skipped > 0 {
            row("Rows without settings or enough responses", self.n_skipped.to_string());
        }

        let mut ranks =
            TextTable::new(["Factor", "Range of mean ratio (dB)", "Rank", "Range of mean response", "Rank (mean)"]);
        for (j, f) in self.factor_names.iter().enumerate() {
            ranks.push([
                f.clone(),
                format_number(self.eta_table[j].delta, DIGITS),
                self.eta_table[j].rank.to_string(),
                format_number(self.mean_table[j].delta, DIGITS),
                self.mean_table[j].rank.to_string(),
            ]);
        }
        let mut levels = TextTable::new(["Factor", "Level", "Runs", "Mean ratio (dB)", "Mean response"]);
        for (j, f) in self.factor_names.iter().enumerate() {
            for (l, label) in self.labels[j].iter().enumerate() {
                let count = self.runs.iter().filter(|r| r.settings[j] == l).count();
                levels.push([
                    f.clone(),
                    label.clone(),
                    count.to_string(),
                    format_number(self.eta_table[j].means[l], DIGITS),
                    format_number(self.mean_table[j].means[l], DIGITS),
                ]);
            }
        }
        let mut cols: Vec<String> = vec!["Row".into()];
        cols.extend(self.factor_names.iter().cloned());
        cols.extend(["N".into(), "Mean".into(), "Std Dev".into(), "Ratio (dB)".into()]);
        let mut by_run = TextTable::new(cols);
        for r in &self.runs {
            let mut line = vec![(r.row + 1).to_string()];
            line.extend(r.settings.iter().enumerate().map(|(j, &l)| self.labels[j][l].clone()));
            line.extend([
                r.n.to_string(),
                format_number(r.mean, DIGITS),
                format_number(r.sd, DIGITS),
                format_number(r.eta, DIGITS),
            ]);
            by_run.push(line);
        }
        vec![
            section(SectionKind::SnSummary, "Signal-to-Noise Summary", SectionContent::Table(summary)),
            section(SectionKind::SnEffectsPlot, "Main Effects on the Ratio", self.effects_plot(true)),
            section(SectionKind::SnRanking, "Factor Ranking", SectionContent::Table(ranks)),
            section(SectionKind::SnResponseTable, "Response Table", SectionContent::Table(levels)),
            section(SectionKind::SnMeanEffectsPlot, "Main Effects on the Mean", self.effects_plot(false)),
            section(SectionKind::SnRuns, "Ratio by Run", SectionContent::Table(by_run)),
        ]
    }

    /// One pane per factor; each run's ratio (or mean) at its level, and a
    /// line through the level means.
    fn effects_plot(&self, ratio: bool) -> SectionContent {
        let table = if ratio { &self.eta_table } else { &self.mean_table };
        let mut offsets = Vec::with_capacity(self.labels.len());
        let mut x = 0.0;
        for l in &self.labels {
            offsets.push(x);
            x += l.len() as f64 + 1.0;
        }
        let mut points = Vec::new();
        let mut membership = Vec::new();
        for (j, &off) in offsets.iter().enumerate() {
            for r in &self.runs {
                points.push(RowPoint {
                    row: r.row,
                    x: off + (r.settings[j] + 1) as f64,
                    y: if ratio { r.eta } else { r.mean },
                });
                membership.push(j);
            }
        }
        let lines = offsets
            .iter()
            .zip(table)
            .map(|(&off, f)| {
                f.means
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.is_finite())
                    .map(|(l, &m)| [off + (l + 1) as f64, m])
                    .collect()
            })
            .collect();
        let x_ticks = offsets
            .iter()
            .zip(&self.labels)
            .flat_map(|(&off, l)| l.iter().enumerate().map(move |(i, lab)| (off + (i + 1) as f64, lab.clone())))
            .collect();
        let panes = offsets
            .iter()
            .zip(&self.labels)
            .zip(&self.factor_names)
            .map(|((&off, l), name)| PlotPane {
                label: name.clone(),
                x_min: off + 0.5,
                x_max: off + l.len() as f64 + 0.5,
            })
            .collect();
        SectionContent::Plot(Box::new(PlotSpec {
            x_label: "Factor level".to_owned(),
            y_label: if ratio { format!("{} (dB)", self.request.ratio.label()) } else { "Mean response".to_owned() },
            points,
            groups: Some(PointGroups {
                title: "Factor".to_owned(),
                labels: self.factor_names.clone(),
                membership,
                lines,
            }),
            x_ticks: Some(x_ticks),
            panes,
            ..PlotSpec::default()
        }))
    }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inner_and_outer_arrays_make_a_wide_table() {
        let mut req = RobustDesignRequest::new(3);
        req.control[0] = ArrayFactor { name: "Supplier".into(), levels: vec!["North".into(), "South".into()] };
        req.control[1].levels = vec!["150".into(), "180".into()];
        let (t, d) = create_robust_design(&req).unwrap();
        assert_eq!(t.n_rows(), 8);
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names[..4], ["Run", "Supplier", "B", "C"]);
        assert_eq!(names[4], "Y (N1=1, N2=1)");
        assert_eq!(names[6], "Y (N1=2, N2=1)");
        assert_eq!(t.n_cols(), 8);
        assert!(d.contains("L8") && d.contains("L4"));
        // Columns follow the L8 array (Phadke, appendix C): column 1 = 1111 2222.
        assert_eq!(t.column(1).unwrap().level_text(4), "South");
        assert_eq!(
            t.column(2).unwrap().as_numeric().unwrap(),
            &[150.0, 150.0, 180.0, 180.0, 150.0, 150.0, 180.0, 180.0]
        );
        // No outer array: replicate columns; randomized order keeps the settings.
        let req = RobustDesignRequest { outer: None, replicates: 3, randomize: Some(4), ..RobustDesignRequest::new(2) };
        let (t, _) = create_robust_design(&req).unwrap();
        assert_eq!(
            t.columns().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["Run", "Std order", "A", "B", "Y 1", "Y 2", "Y 3"]
        );
        // Errors: wrong number of levels, too many factors.
        let mut bad = RobustDesignRequest::new(2);
        bad.control[0].levels.push("3".into());
        assert!(create_robust_design(&bad).is_err());
        assert!(create_robust_design(&RobustDesignRequest::new(8)).is_err());
        let mut l18 = RobustDesignRequest { inner: StandardArray::L18, outer: None, ..RobustDesignRequest::new(2) };
        l18.control[1].levels = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(create_robust_design(&l18).unwrap().0.n_rows(), 18);
    }

    /// An L4 experiment worked by hand (smaller the better): runs with
    /// responses (1, 1), (2, 2), (1, 3), (3, 3) have η = 0, −6.0206,
    /// −6.9897, −9.5424 (−10 log₁₀ of 1, 4, 5, 9). Factor A at level 1:
    /// (0 − 6.0206)/2 = −3.0103; level 2: −8.2661.
    #[test]
    fn signal_to_noise_report() {
        let t = DataTable::new(
            "L4",
            vec![
                Column::numeric("A", vec![1.0, 1.0, 2.0, 2.0]),
                Column::numeric("B", vec![1.0, 2.0, 1.0, 2.0]),
                Column::text("C", vec!["x".into(), "y".into(), "y".into(), "x".into()]),
                Column::numeric("Y1", vec![1.0, 2.0, 1.0, 3.0]),
                Column::numeric("Y2", vec![1.0, 2.0, 3.0, 3.0]),
            ],
        )
        .unwrap();
        let req = SignalToNoiseRequest {
            factors: vec![0, 1, 2],
            responses: vec![3, 4],
            ratio: SignalToNoise::SmallerTheBetter,
        };
        let a = SignalToNoiseAnalysis::run(&t, &RowStateManager::new(4), &req).unwrap();
        let eta = a.ratios();
        let expected = [0.0, -10.0 * 4f64.log10(), -10.0 * 5f64.log10(), -10.0 * 9f64.log10()];
        for (a, b) in eta.iter().zip(expected) {
            assert!((a - b).abs() < 1e-12);
        }
        let ta = &a.eta_table()[0];
        assert!((ta.means[0] - (expected[0] + expected[1]) / 2.0).abs() < 1e-12);
        assert!((ta.means[1] - (expected[2] + expected[3]) / 2.0).abs() < 1e-12);
        let (best, _, _) = a.best_settings();
        assert_eq!(best, vec![0, 0, 0], "A = 1, B = 1, C = x");
        let s = a.sections();
        assert_eq!(s.len(), 6);
        let SectionContent::Plot(p) = &s[1].content else { panic!() };
        assert_eq!(p.points.len(), 12, "each run in each of three panes");
        assert_eq!(p.panes.len(), 3);
        let back = SignalToNoiseRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
        // Nominal the best needs two values and some spread per run.
        let nominal = SignalToNoiseRequest { ratio: SignalToNoise::NominalTheBest, ..req.clone() };
        let err = SignalToNoiseAnalysis::run(&t, &RowStateManager::new(4), &nominal).unwrap_err().to_string();
        // One plain message naming the run and what to do (it read
        // "invalid argument: row 1: invalid argument: …").
        assert!(!err.contains("invalid argument"), "{err}");
        assert!(err.contains("run 1:") && err.contains("Choose another ratio"), "{err}");
        let one = SignalToNoiseRequest { responses: vec![3], ..nominal };
        assert!(SignalToNoiseAnalysis::run(&t, &RowStateManager::new(4), &one).is_err());
        let dup = SignalToNoiseRequest { factors: vec![0, 3], ..req };
        assert!(SignalToNoiseAnalysis::run(&t, &RowStateManager::new(4), &dup).is_err());
    }
}
