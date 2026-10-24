//! Comparing predictive models through the prediction columns they saved.
//!
//! Each model is a column: predicted values for a continuous response, or
//! the probability of one **event** class for a categorical response (other
//! classes count as non-events, so a model of several classes is compared
//! one class against the rest). Models are compared on the same rows: those
//! not excluded with the response and every model's prediction present.
//! An optional validation column (0/1/2 or Training/Validation/Test) splits
//! the measures by role.
//!
//! **Measures.** Continuous: RMSE, mean absolute error and R²
//! ([`ContinuousFit`]). Categorical: misclassification at probability 0.5,
//! log loss and the area under the ROC curve ([`ClassFit`] of the event
//! against the rest), and the Brier score `Σ(p̂ − y)²/n` (Brier, *Monthly
//! Weather Review* 78, 1950).
//!
//! **Plots** (on test rows when there are any, else validation rows, else
//! every row): for a categorical response, ROC curves, lift and cumulative
//! gains by depth (the share of rows ranked highest by the model; Berry &
//! Linoff, *Data Mining Techniques*, 2004, ch. 5) and a calibration plot of
//! observed event rate against mean predicted probability in tenths of the
//! predictions; for a continuous response, actual by predicted and residual
//! by predicted. Every point is a dataset row (a row appears once per model),
//! so brushing works as elsewhere.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_predictive::measures::{ClassFit, ContinuousFit};
use stat_predictive::{Role, roc_curve};

use crate::format::format_number;
use crate::prediction::Response;
use crate::report::{PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Columns compared.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelComparisonRequest {
    /// Response column.
    pub response: usize,
    /// Prediction columns, one per model.
    pub models: Vec<usize>,
    /// Event class of a categorical response (`None`: the last class in
    /// natural order, e.g. "Yes" of No/Yes).
    pub event: Option<String>,
    /// Column of validation roles.
    pub validation: Option<usize>,
}

impl ModelComparisonRequest {
    /// Columns read.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.response];
        c.extend(&self.models);
        c.extend(self.validation);
        c
    }

    /// The same request with column indices passed through `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.response = f(self.response);
        self.models.iter_mut().for_each(|c| *c = f(*c));
        self.validation = self.validation.map(&f);
        self
    }
}

/// The observed response on the compared rows.
#[derive(Debug, Clone)]
enum Observed {
    Continuous(Vec<f64>),
    /// `true` for the event class.
    Event {
        classes: Vec<String>,
        event: usize,
        is_event: Vec<bool>,
    },
}

/// A model comparison.
#[derive(Debug, Clone)]
pub struct ModelComparison {
    request: ModelComparisonRequest,
    response_name: String,
    names: Vec<String>,
    /// Compared table rows.
    rows: Vec<usize>,
    /// Role of each compared row (`None` without a validation column).
    roles: Vec<Option<Role>>,
    observed: Observed,
    /// `predictions[m][k]`: model `m` at compared row `k`.
    predictions: Vec<Vec<f64>>,
}

impl ModelComparison {
    /// Read and check the columns.
    ///
    /// # Errors
    /// Fewer than one model, the response among the models, a non-numeric
    /// model column, a probability outside [0, 1], an unknown event class, a
    /// bad validation value, or no rows with every value present.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ModelComparisonRequest) -> Result<Self> {
        if request.models.is_empty() {
            return Err(invalid("choose at least one prediction column"));
        }
        if request.models.contains(&request.response) {
            return Err(invalid("the response cannot also be a model's prediction"));
        }
        let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let response = Response::read(table, request.response, &included)?;
        let response_name = table.column(request.response).map_or(String::new(), |c| c.name.clone());
        let mut names = Vec::new();
        let mut columns = Vec::new();
        for &m in &request.models {
            let c = table.column(m).ok_or_else(|| invalid(format!("no column {m}")))?;
            let v = c.as_numeric().ok_or_else(|| invalid(format!("the model column {:?} is not numeric", c.name)))?;
            names.push(c.name.clone());
            columns.push(v.to_vec());
        }
        let role_column = request.validation.map(|v| table.column(v).ok_or_else(|| invalid(format!("no column {v}"))));
        let role_column = role_column.transpose()?;
        let mut rows = Vec::new();
        let mut roles = Vec::new();
        for &r in &included {
            if !response.present(r) || columns.iter().any(|c| !c[r].is_finite()) {
                continue;
            }
            let role = match role_column {
                None => None,
                Some(col) => {
                    let text = col.level_text(r);
                    if text.trim().is_empty() {
                        continue;
                    }
                    Some(Role::parse(&text).ok_or_else(|| {
                        invalid(format!(
                            "the validation column {:?} has the value {text:?}; use 0/1/2 or Training/Validation/Test",
                            col.name
                        ))
                    })?)
                }
            };
            rows.push(r);
            roles.push(role);
        }
        if rows.is_empty() {
            return Err(invalid("no row has the response and every model's prediction"));
        }
        let observed = match &response {
            Response::Continuous(y) => Observed::Continuous(rows.iter().map(|&r| y[r]).collect()),
            Response::Categorical { classes, codes } => {
                let event = match &request.event {
                    None => classes.len() - 1,
                    Some(e) => classes
                        .iter()
                        .position(|c| c == e)
                        .ok_or_else(|| invalid(format!("{response_name:?} has no class {e:?}")))?,
                };
                for (name, col) in names.iter().zip(&columns) {
                    if rows.iter().any(|&r| !(0.0..=1.0).contains(&col[r])) {
                        return Err(invalid(format!(
                            "{name:?} has values outside 0 to 1; for a categorical response, compare columns of \
                             the event's probability"
                        )));
                    }
                }
                Observed::Event {
                    classes: classes.clone(),
                    event,
                    is_event: rows.iter().map(|&r| codes[r] == Some(event)).collect(),
                }
            }
        };
        let predictions = columns.iter().map(|c| rows.iter().map(|&r| c[r]).collect()).collect();
        Ok(Self { request: request.clone(), response_name, names, rows, roles, observed, predictions })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &ModelComparisonRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Model Comparison: {} ({} models)", self.response_name, self.names.len())
    }

    /// The event class (categorical response).
    #[must_use]
    pub fn event(&self) -> Option<&str> {
        match &self.observed {
            Observed::Event { classes, event, .. } => Some(&classes[*event]),
            Observed::Continuous(_) => None,
        }
    }

    /// Row groups for the measures: each role present, or every row.
    fn role_groups(&self) -> Vec<(String, Vec<usize>)> {
        if self.roles.iter().all(Option::is_none) {
            return vec![("All rows".to_owned(), (0..self.rows.len()).collect())];
        }
        Role::ALL
            .iter()
            .map(|&role| {
                (role.label().to_owned(), (0..self.rows.len()).filter(|&k| self.roles[k] == Some(role)).collect())
            })
            .filter(|(_, k): &(String, Vec<usize>)| !k.is_empty())
            .collect()
    }

    /// Positions (in the compared rows) the plots use, and their description.
    fn plot_rows(&self) -> (Vec<usize>, String) {
        for role in [Role::Test, Role::Validation] {
            let k: Vec<usize> = (0..self.rows.len()).filter(|&k| self.roles[k] == Some(role)).collect();
            if !k.is_empty() {
                return (k, format!("{} rows", role.label().to_lowercase()));
            }
        }
        ((0..self.rows.len()).collect(), "all rows".to_owned())
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.measures_section()];
        match &self.observed {
            Observed::Event { is_event, .. } => {
                let (k, which) = self.plot_rows();
                out.push(self.roc_section(is_event, &k, &which));
                out.push(self.lift_section(is_event, &k, &which, false));
                out.push(self.lift_section(is_event, &k, &which, true));
                out.push(self.calibration_section(is_event, &k, &which));
            }
            Observed::Continuous(y) => {
                let (k, which) = self.plot_rows();
                out.push(self.actual_section(y, &k, &which, false));
                out.push(self.actual_section(y, &k, &which, true));
            }
        }
        out
    }

    fn measures_section(&self) -> ReportSection {
        let f = |v: f64, d: usize| if v.is_nan() { "—".to_owned() } else { format_number(v, d) };
        let (headers, title): (Vec<&str>, String) = match &self.observed {
            Observed::Continuous(_) => (
                vec!["Rows", "Model", "Observations", "RMSE", "Mean abs. error", "R²", ""],
                format!("Fit Measures ({} models on the same rows; ◀ lowest RMSE)", self.names.len()),
            ),
            Observed::Event { classes, event, .. } => (
                vec!["Rows", "Model", "Observations", "Misclassification", "Log loss", "Brier score", "AUC", ""],
                format!(
                    "Fit Measures (probability of {} against the other classes; misclassified at 0.5; ◀ highest AUC)",
                    classes[*event]
                ),
            ),
        };
        let mut t = TextTable::new(headers);
        for (label, ks) in self.role_groups() {
            let mut rows: Vec<(Vec<String>, f64)> = Vec::new();
            for (m, name) in self.names.iter().enumerate() {
                let p: Vec<f64> = ks.iter().map(|&k| self.predictions[m][k]).collect();
                match &self.observed {
                    Observed::Continuous(y) => {
                        let y: Vec<f64> = ks.iter().map(|&k| y[k]).collect();
                        if let Some(fit) = ContinuousFit::new(&y, &p) {
                            rows.push((
                                vec![fit.n.to_string(), f(fit.rmse, 6), f(fit.mae, 6), f(fit.r_squared, 4)],
                                -fit.rmse,
                            ));
                        }
                    }
                    Observed::Event { is_event, .. } => {
                        let obs: Vec<usize> = ks.iter().map(|&k| usize::from(is_event[k])).collect();
                        let probs: Vec<Vec<f64>> = p.iter().map(|&q| vec![1.0 - q, q]).collect();
                        if let Some(fit) = ClassFit::new(&obs, &probs, 2) {
                            let brier = obs.iter().zip(&p).map(|(&o, &q)| (q - o as f64).powi(2)).sum::<f64>()
                                / obs.len() as f64;
                            rows.push((
                                vec![
                                    fit.n.to_string(),
                                    f(fit.misclassification, 4),
                                    f(fit.log_loss, 4),
                                    f(brier, 4),
                                    f(fit.auc, 4),
                                ],
                                if fit.auc.is_nan() { f64::NEG_INFINITY } else { fit.auc },
                            ));
                        }
                    }
                }
                if let Some(last) = rows.last_mut().filter(|r| r.0.len() < 6) {
                    last.0.insert(0, name.clone());
                }
            }
            let best = rows.iter().map(|r| r.1).fold(f64::NEG_INFINITY, f64::max);
            for (i, (cells, score)) in rows.into_iter().enumerate() {
                let mark = if self.names.len() > 1 && score == best && score.is_finite() { "◀" } else { "" };
                let first = if i == 0 { label.clone() } else { String::new() };
                t.push(std::iter::once(first).chain(cells).chain(std::iter::once(mark.to_owned())).collect::<Vec<_>>());
            }
        }
        ReportSection { kind: SectionKind::ModelComparison, title, content: SectionContent::Table(t) }
    }

    /// One point per (row, model) at `point(m, k)`, grouped by model, with
    /// each model's line and the reference segments.
    fn plot(
        &self,
        ks: &[usize],
        labels: (&str, &str),
        point: &dyn Fn(usize, usize) -> [f64; 2],
        lines: Vec<Vec<[f64; 2]>>,
        segments: Vec<Vec<[f64; 2]>>,
    ) -> PlotSpec {
        let mut points = Vec::new();
        let mut membership = Vec::new();
        for m in 0..self.names.len() {
            for &k in ks {
                let [x, y] = point(m, k);
                if x.is_finite() && y.is_finite() {
                    points.push(RowPoint { row: self.rows[k], x, y });
                    membership.push(m);
                }
            }
        }
        PlotSpec {
            x_label: labels.0.to_owned(),
            y_label: labels.1.to_owned(),
            points,
            groups: Some(PointGroups { title: "Model".to_owned(), labels: self.names.clone(), membership, lines }),
            segments,
            ..PlotSpec::default()
        }
    }

    fn roc_section(&self, is_event: &[bool], ks: &[usize], which: &str) -> ReportSection {
        let positive: Vec<bool> = ks.iter().map(|&k| is_event[k]).collect();
        let n_pos = positive.iter().filter(|&&p| p).count().max(1) as f64;
        let n_neg = (positive.len() as f64 - positive.iter().filter(|&&p| p).count() as f64).max(1.0);
        // Each row at the curve's point for the threshold equal to its score.
        let point = |m: usize, k: usize| {
            let s = self.predictions[m][k];
            let (mut tp, mut fp) = (0.0, 0.0);
            for (&j, &pos) in ks.iter().zip(&positive) {
                if self.predictions[m][j] >= s {
                    if pos {
                        tp += 1.0;
                    } else {
                        fp += 1.0;
                    }
                }
            }
            [fp / n_neg, tp / n_pos]
        };
        let lines = (0..self.names.len())
            .map(|m| {
                let scores: Vec<f64> = ks.iter().map(|&k| self.predictions[m][k]).collect();
                roc_curve(&scores, &positive)
            })
            .collect();
        let spec = self.plot(
            ks,
            ("False positive rate (1 − specificity)", "True positive rate (sensitivity)"),
            &point,
            lines,
            vec![vec![[0.0, 0.0], [1.0, 1.0]]],
        );
        ReportSection {
            kind: SectionKind::RocCurves,
            title: format!("ROC Curves ({which}; diagonal: no better than chance)"),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    /// Ranks of `ks` by model `m`'s score, highest first: for each position,
    /// the depth (share of rows scoring at least as high) and the events
    /// among them.
    fn depth_curve(&self, m: usize, is_event: &[bool], ks: &[usize]) -> Vec<(usize, f64, f64)> {
        let mut order: Vec<usize> = ks.to_vec();
        order.sort_by(|&a, &b| self.predictions[m][b].total_cmp(&self.predictions[m][a]));
        let n = order.len() as f64;
        let mut out = Vec::with_capacity(order.len());
        let mut i = 0;
        let mut events = 0.0;
        while i < order.len() {
            let s = self.predictions[m][order[i]];
            let mut j = i;
            while j < order.len() && self.predictions[m][order[j]] == s {
                events += f64::from(u8::from(is_event[order[j]]));
                j += 1;
            }
            for &k in &order[i..j] {
                out.push((k, j as f64 / n, events));
            }
            i = j;
        }
        out
    }

    fn lift_section(&self, is_event: &[bool], ks: &[usize], which: &str, gains: bool) -> ReportSection {
        let total = ks.iter().filter(|&&k| is_event[k]).count() as f64;
        let rate = total / ks.len().max(1) as f64;
        let curves: Vec<Vec<(usize, f64, f64)>> =
            (0..self.names.len()).map(|m| self.depth_curve(m, is_event, ks)).collect();
        let value = |depth: f64, events: f64| {
            if gains {
                if total > 0.0 { events / total } else { f64::NAN }
            } else if rate > 0.0 {
                events / (depth * ks.len() as f64) / rate
            } else {
                f64::NAN
            }
        };
        let lines: Vec<Vec<[f64; 2]>> = curves
            .iter()
            .map(|c| {
                let mut pts: Vec<[f64; 2]> = c.iter().map(|&(_, d, e)| [d, value(d, e)]).collect();
                pts.dedup_by(|a, b| a[0] == b[0]);
                if gains {
                    pts.insert(0, [0.0, 0.0]);
                }
                pts
            })
            .collect();
        let at: Vec<std::collections::HashMap<usize, [f64; 2]>> =
            curves.iter().map(|c| c.iter().map(|&(k, d, e)| (k, [d, value(d, e)])).collect()).collect();
        let point = |m: usize, k: usize| at[m].get(&k).copied().unwrap_or([f64::NAN, f64::NAN]);
        let (labels, segment, kind, title) = if gains {
            (
                ("Depth (share of rows, highest predicted first)", "Share of events captured"),
                vec![[0.0, 0.0], [1.0, 1.0]],
                SectionKind::GainsChart,
                format!("Cumulative Gains ({which}; diagonal: random ordering)"),
            )
        } else {
            (
                ("Depth (share of rows, highest predicted first)", "Lift (event rate ÷ overall rate)"),
                vec![[0.0, 1.0], [1.0, 1.0]],
                SectionKind::LiftChart,
                format!("Lift ({which}; overall event rate {}; line at 1: no lift)", format_number(rate, 4)),
            )
        };
        let spec = self.plot(ks, labels, &point, lines, vec![segment]);
        ReportSection { kind, title, content: SectionContent::Plot(Box::new(spec)) }
    }

    fn calibration_section(&self, is_event: &[bool], ks: &[usize], which: &str) -> ReportSection {
        // Tenths of each model's predictions: mean predicted and observed rate.
        let bins: Vec<Vec<(Vec<usize>, f64, f64)>> = (0..self.names.len())
            .map(|m| {
                let mut order = ks.to_vec();
                order.sort_by(|&a, &b| self.predictions[m][a].total_cmp(&self.predictions[m][b]));
                let n = order.len();
                let groups = n.clamp(1, 10);
                (0..groups)
                    .map(|g| {
                        let members: Vec<usize> = order[g * n / groups..(g + 1) * n / groups].to_vec();
                        let c = members.len().max(1) as f64;
                        let mean = members.iter().map(|&k| self.predictions[m][k]).sum::<f64>() / c;
                        let observed = members.iter().filter(|&&k| is_event[k]).count() as f64 / c;
                        (members, mean, observed)
                    })
                    .filter(|b| !b.0.is_empty())
                    .collect()
            })
            .collect();
        let lines = bins.iter().map(|b| b.iter().map(|(_, x, y)| [*x, *y]).collect()).collect();
        let observed_at: Vec<std::collections::HashMap<usize, f64>> = bins
            .iter()
            .map(|b| b.iter().flat_map(|(members, _, y)| members.iter().map(move |&k| (k, *y))).collect())
            .collect();
        let point = |m: usize, k: usize| [self.predictions[m][k], observed_at[m].get(&k).copied().unwrap_or(f64::NAN)];
        let spec = self.plot(
            ks,
            ("Predicted probability", "Observed event rate (tenth of the predictions)"),
            &point,
            lines,
            vec![vec![[0.0, 0.0], [1.0, 1.0]]],
        );
        ReportSection {
            kind: SectionKind::CalibrationPlot,
            title: format!(
                "Calibration ({which}; each point at its row's prediction and the observed rate of its tenth; \
                 diagonal: perfect calibration)"
            ),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    fn actual_section(&self, y: &[f64], ks: &[usize], which: &str, residual: bool) -> ReportSection {
        let point = |m: usize, k: usize| {
            let p = self.predictions[m][k];
            [p, if residual { y[k] - p } else { y[k] }]
        };
        let xs: Vec<f64> =
            (0..self.names.len()).flat_map(|m| ks.iter().map(move |&k| self.predictions[m][k])).collect();
        let lo = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let reference = if residual { vec![[lo, 0.0], [hi, 0.0]] } else { vec![[lo, lo], [hi, hi]] };
        let name = &self.response_name;
        let (labels, kind, title) = if residual {
            (
                ("Predicted".to_owned(), format!("Residual ({name} − predicted)")),
                SectionKind::ResidualByPredicted,
                format!("Residuals vs Predicted ({which}; line at zero)"),
            )
        } else {
            (
                ("Predicted".to_owned(), name.clone()),
                SectionKind::ActualByPredicted,
                format!("Observed vs Predicted ({which}; diagonal: perfect prediction)"),
            )
        };
        let spec = self.plot(ks, (&labels.0, &labels.1), &point, vec![Vec::new(); self.names.len()], vec![reference]);
        ReportSection { kind, title, content: SectionContent::Plot(Box::new(spec)) }
    }
}

// ---- Project files and scripts ------------------------------------------------

/// The request's members after `response`, for project files.
#[must_use]
pub fn request_members_to_json(
    request: &ModelComparisonRequest,
    col: &dyn Fn(usize) -> JsonValue,
) -> Vec<(&'static str, JsonValue)> {
    vec![
        ("models", JsonValue::Array(request.models.iter().map(|&m| col(m)).collect())),
        ("event", request.event.clone().map_or(JsonValue::Null, JsonValue::String)),
        ("validation", request.validation.map_or(JsonValue::Null, col)),
    ]
}

/// Read a request from a project file's report entry.
///
/// # Errors
/// A missing or unknown column.
pub fn request_from_json(
    v: &JsonValue,
    response: usize,
    find: &dyn Fn(&JsonValue) -> Result<usize>,
) -> Result<ModelComparisonRequest> {
    let models = match v.get("models") {
        Some(JsonValue::Array(a)) => a.iter().map(find).collect::<Result<Vec<_>>>()?,
        _ => return Err(invalid("model comparison: \"models\" must list prediction columns")),
    };
    let event = v.get("event").and_then(JsonValue::as_str).map(str::to_owned);
    let validation = match v.get("validation") {
        None | Some(JsonValue::Null) => None,
        Some(c) => Some(find(c)?),
    };
    Ok(ModelComparisonRequest { response, models, event, validation })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// 200 rows: a binary outcome, a good model, a weak model, roles.
    fn table() -> DataTable {
        let mut rng = stat_core::rng::Rng::new(4);
        let n = 200;
        let x: Vec<f64> = (0..n).map(|_| rng.normal()).collect();
        let y: Vec<String> = x
            .iter()
            .map(|&v| if rng.next_f64() < 1.0 / (1.0 + (-2.0 * v).exp()) { "Yes" } else { "No" }.to_owned())
            .collect();
        let good: Vec<f64> = x.iter().map(|&v| 1.0 / (1.0 + (-2.0 * v).exp())).collect();
        let weak: Vec<f64> = x.iter().map(|&v| 1.0 / (1.0 + (-0.3 * v - 0.5 * rng.normal()).exp())).collect();
        let spend: Vec<f64> = x.iter().map(|&v| 10.0 + 3.0 * v + rng.normal()).collect();
        let fit_a: Vec<f64> = x.iter().map(|&v| 10.0 + 3.0 * v).collect();
        let fit_b: Vec<f64> = vec![10.0; n];
        let roles: Vec<String> =
            (0..n).map(|i| ["Training", "Training", "Validation", "Test"][i % 4].to_owned()).collect();
        DataTable::new(
            "t",
            vec![
                Column::text("Bought", y),
                Column::numeric("Good model", good),
                Column::numeric("Weak model", weak),
                Column::text("Role", roles),
                Column::numeric("Spend", spend),
                Column::numeric("Linear", fit_a),
                Column::numeric("Mean only", fit_b),
            ],
        )
        .unwrap()
    }

    #[test]
    fn categorical_comparison() {
        let t = table();
        let req = ModelComparisonRequest { response: 0, models: vec![1, 2], event: None, validation: Some(3) };
        let a = ModelComparison::run(&t, &RowStateManager::new(200), &req).unwrap();
        assert_eq!(a.event(), Some("Yes"));
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::ModelComparison,
                SectionKind::RocCurves,
                SectionKind::LiftChart,
                SectionKind::GainsChart,
                SectionKind::CalibrationPlot
            ]
        );
        let SectionContent::Table(m) = &s[0].content else { panic!() };
        // Three roles × two models; the good model is marked best in each.
        assert_eq!(m.rows.len(), 6);
        for pair in m.rows.chunks(2) {
            assert_eq!(pair[0][1], "Good model");
            assert_eq!(pair[0].last().unwrap(), "◀");
            assert_eq!(pair[1].last().unwrap(), "");
            let auc = |r: &Vec<String>| r[6].parse::<f64>().unwrap();
            assert!(auc(&pair[0]) > auc(&pair[1]));
        }
        // ROC points lie on their model's curve; the plots use the test rows.
        assert!(s[1].title.contains("test rows"));
        let SectionContent::Plot(roc) = &s[1].content else { panic!() };
        assert_eq!(roc.points.len(), 2 * 50);
        let g = roc.groups.as_ref().unwrap();
        for (p, &m) in roc.points.iter().zip(&g.membership) {
            assert!(g.lines[m].iter().any(|v| (v[0] - p.x).abs() < 1e-12 && (v[1] - p.y).abs() < 1e-12));
        }
        // Gains end at (1, 1); lift is 1 at full depth.
        let SectionContent::Plot(gains) = &s[3].content else { panic!() };
        assert_eq!(gains.groups.as_ref().unwrap().lines[0].last(), Some(&[1.0, 1.0]));
        let SectionContent::Plot(lift) = &s[2].content else { panic!() };
        let last = lift.groups.as_ref().unwrap().lines[0].last().unwrap();
        assert!((last[1] - 1.0).abs() < 1e-12);
        // The good model is calibrated: bins near the diagonal.
        let SectionContent::Plot(cal) = &s[4].content else { panic!() };
        let bins = &cal.groups.as_ref().unwrap().lines[0];
        assert_eq!(bins.len(), 10);
        // A probability outside [0, 1] is refused.
        let bad = ModelComparisonRequest { response: 0, models: vec![4], event: None, validation: None };
        assert!(ModelComparison::run(&t, &RowStateManager::new(200), &bad).is_err());
    }

    #[test]
    fn continuous_comparison_and_json() {
        let t = table();
        let req = ModelComparisonRequest { response: 4, models: vec![5, 6], event: None, validation: None };
        let a = ModelComparison::run(&t, &RowStateManager::new(200), &req).unwrap();
        let s = a.sections();
        assert_eq!(s.len(), 3);
        let SectionContent::Table(m) = &s[0].content else { panic!() };
        assert_eq!(m.rows[0][0], "All rows");
        assert_eq!(m.rows[0].last().unwrap(), "◀");
        let r2 = m.rows[1][5].parse::<f64>().unwrap();
        assert!(r2.abs() < 0.05, "the mean-only model has R² near 0: {r2}");
        let SectionContent::Plot(res) = &s[2].content else { panic!() };
        assert_eq!(res.points.len(), 400);
        let col = |i: usize| JsonValue::String(t.column(i).unwrap().name.clone());
        let v = JsonValue::Object(
            request_members_to_json(&req, &col).into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        );
        let find = |v: &JsonValue| t.find(v.as_str().unwrap_or_default()).ok_or_else(|| invalid("no column"));
        assert_eq!(request_from_json(&v, 4, &find).unwrap(), req);
    }
}
