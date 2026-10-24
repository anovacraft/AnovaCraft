//! Dose–response (probit) analysis bound to the row state
//! ([`stat_discrete::dose_response`]): the proportion responding against a
//! (log) dose, with effective doses such as the ED50 and their fiducial
//! limits.
//!
//! Each row is a group of subjects (a count responding out of a number of
//! trials) or, without a trials column, one subject with a 0/1 response.
//! Excluded rows and rows missing a value are left out; rows with a zero or
//! negative dose on a log scale (untreated controls) are counted and left out.

use crate::format::{format_number, format_p_value};
use crate::report::{PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_discrete::{DoseResponseFit, DoseResponseOptions, DoseScale, Link, fit_dose_response};

const DIGITS: usize = 6;
const CURVE_POINTS: usize = 120;

/// What to analyse.
#[derive(Debug, Clone, PartialEq)]
pub struct DoseResponseRequest {
    /// Dose column.
    pub dose: usize,
    /// Number responding per row (or 0/1 without a trials column).
    pub response: usize,
    /// Number of subjects per row; `None` for one subject per row.
    pub trials: Option<usize>,
    /// Probit, logit or complementary log-log.
    pub link: Link,
    /// Scale of the dose in the model.
    pub scale: DoseScale,
    /// Response probabilities whose effective doses are reported.
    pub probabilities: Vec<f64>,
    /// Fiducial (confidence) level.
    pub confidence_level: f64,
    /// Level of the goodness-of-fit test above which the heterogeneity
    /// factor is applied.
    pub heterogeneity_alpha: f64,
}

impl DoseResponseRequest {
    /// Probit analysis on log₁₀ dose with the default effective doses
    /// (10%, 50%, 90%), 95% fiducial limits and heterogeneity level 0.05.
    #[must_use]
    pub fn probit(dose: usize, response: usize, trials: Option<usize>) -> Self {
        let d = DoseResponseOptions::default();
        Self {
            dose,
            response,
            trials,
            link: d.link,
            scale: d.scale,
            probabilities: d.probabilities,
            confidence_level: d.confidence_level,
            heterogeneity_alpha: d.heterogeneity_alpha,
        }
    }

    /// Links offered.
    pub const LINKS: [Link; 3] = [Link::Probit, Link::Logit, Link::ComplementaryLogLog];

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.dose, self.response];
        c.extend(self.trials);
        c
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.dose = f(self.dose);
        self.response = f(self.response);
        self.trials = self.trials.map(f);
    }
}

/// A computed dose–response analysis.
#[derive(Debug, Clone)]
pub struct DoseResponseAnalysis {
    request: DoseResponseRequest,
    dose_name: String,
    response_name: String,
    fit: DoseResponseFit,
    /// Used rows with their dose and observed proportion.
    points: Vec<(usize, f64, f64)>,
    n_excluded: usize,
    n_missing: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn percent(p: f64) -> String {
    format_number(100.0 * p, 4)
}

impl DoseResponseAnalysis {
    /// Fit the model.
    ///
    /// # Errors
    /// Missing or non-numeric columns, no usable rows, and errors from
    /// [`fit_dose_response`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &DoseResponseRequest) -> Result<Self> {
        let numeric = |i: usize| -> Result<(&str, &[f64])> {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            Ok((c.name.as_str(), c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?))
        };
        let (dose_name, dose) = numeric(request.dose)?;
        let (response_name, response) = numeric(request.response)?;
        let trials = request.trials.map(numeric).transpose()?.map(|(_, v)| v);
        let (mut d, mut r, mut m, mut points) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut n_excluded, mut n_missing) = (0, 0);
        for row in 0..table.n_rows() {
            let n = trials.map_or(1.0, |t| t[row]);
            if !(dose[row].is_finite() && response[row].is_finite() && n.is_finite()) {
                n_missing += 1;
            } else if states.is_excluded(row) {
                n_excluded += 1;
            } else {
                d.push(dose[row]);
                r.push(response[row]);
                m.push(n);
                points.push((row, dose[row], response[row] / n));
            }
        }
        if d.is_empty() {
            return Err(invalid("no usable rows"));
        }
        if trials.is_none() && r.iter().any(|&v| v != 0.0 && v != 1.0) {
            return Err(invalid("without a trials column each row is one subject and the response must be 0 or 1"));
        }
        let options = DoseResponseOptions {
            link: request.link,
            scale: request.scale,
            probabilities: request.probabilities.clone(),
            confidence_level: request.confidence_level,
            heterogeneity_alpha: request.heterogeneity_alpha,
        };
        let fit = fit_dose_response(&d, &r, &m, &options)?;
        Ok(Self {
            request: request.clone(),
            dose_name: dose_name.to_owned(),
            response_name: response_name.to_owned(),
            fit,
            points,
            n_excluded,
            n_missing,
        })
    }

    /// The fit.
    #[must_use]
    pub fn fit(&self) -> &DoseResponseFit {
        &self.fit
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let kind = match self.request.link {
            Link::Probit => "Probit Analysis",
            Link::Logit => "Dose–Response Analysis (logit)",
            _ => "Dose–Response Analysis (complementary log-log)",
        };
        format!("{kind}: {} by {}", self.response_name, self.dose_name)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![self.summary(), self.estimates(), self.effective_doses(), self.groups(), self.plot()]
    }

    fn summary(&self) -> ReportSection {
        let f = &self.fit;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Model", format!("{} link on {}", f.options.link.label(), f.options.scale.label()));
        row("Dose groups", f.groups.len().to_string());
        let subjects: f64 = f.groups.iter().map(|g| g.trials).sum();
        let responded: f64 = f.groups.iter().map(|g| g.responded).sum();
        row("Subjects (responding)", format!("{} ({})", format_number(subjects, 8), format_number(responded, 8)));
        if f.rows_left_out > 0 {
            row("Rows left out (dose not positive on a log scale)", f.rows_left_out.to_string());
        }
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            row("Rows with missing values", self.n_missing.to_string());
        }
        if f.df > 0 {
            row(
                "Goodness of fit (Pearson χ²)",
                format!("{} on {} df, p = {}", format_number(f.chi_square, DIGITS), f.df, format_p_value(f.p_value)),
            );
        } else {
            row("Goodness of fit (Pearson χ²)", "not available: two dose groups".to_owned());
        }
        row("Residual deviance", format_number(f.glm.deviance, DIGITS));
        if f.heterogeneity_applied {
            row(
                "Heterogeneity factor",
                format!(
                    "{} applied (χ² significant at {}); limits use t on {} df",
                    format_number(f.heterogeneity, DIGITS),
                    format_number(f.options.heterogeneity_alpha, 3),
                    f.df
                ),
            );
        } else {
            row("Heterogeneity factor", "not applied (1)".to_owned());
        }
        row("g = t²·var(slope)/slope²", format_number(f.g, 4));
        for w in &f.glm.warnings {
            row("Warning", w.clone());
        }
        section(SectionKind::DoseResponseSummary, "Model Summary", SectionContent::Table(t))
    }

    fn estimates(&self) -> ReportSection {
        let f = &self.fit;
        let level = percent(f.options.confidence_level);
        let mut t = TextTable::new([
            "Term",
            "Estimate",
            "Std Error",
            "z",
            "p-value",
            &format!("Lower {level}%"),
            &format!("Upper {level}%"),
        ]);
        for c in &f.glm.coefficients {
            let name = if c.name == "Intercept" { "Intercept".to_owned() } else { format!("Slope ({})", c.name) };
            t.push([
                name,
                format_number(c.estimate, DIGITS),
                format_number(c.std_error, DIGITS),
                format_number(c.statistic, 4),
                format_p_value(c.p_value),
                format_number(c.confidence_interval.0, DIGITS),
                format_number(c.confidence_interval.1, DIGITS),
            ]);
        }
        section(
            SectionKind::DoseResponseEstimates,
            "Parameter Estimates (Wald, without the heterogeneity factor)",
            SectionContent::Table(t),
        )
    }

    fn effective_doses(&self) -> ReportSection {
        let f = &self.fit;
        let level = percent(f.options.confidence_level);
        let model_scale = f.options.scale != DoseScale::Linear;
        let mut heads = vec![
            "Response %".to_owned(),
            format!("Dose ({})", self.dose_name),
            format!("Fiducial lower {level}%"),
            format!("Fiducial upper {level}%"),
            format!("Delta-method lower {level}%"),
            format!("Delta-method upper {level}%"),
        ];
        if model_scale {
            heads.push(f.options.scale.label().to_owned());
            heads.push(format!("Std Error ({})", f.options.scale.label()));
        } else {
            heads.push("Std Error".to_owned());
        }
        let mut t = TextTable::new(heads);
        for e in &f.effective_doses {
            let (lo, hi) = e.fiducial.map_or_else(
                || ("unbounded".to_owned(), "unbounded".to_owned()),
                |(a, b)| (format_number(a, DIGITS), format_number(b, DIGITS)),
            );
            let mut row = vec![
                percent(e.probability),
                format_number(e.dose, DIGITS),
                lo,
                hi,
                format_number(e.delta.0, DIGITS),
                format_number(e.delta.1, DIGITS),
            ];
            if model_scale {
                row.push(format_number(e.x, DIGITS));
            }
            row.push(format_number(e.std_error, DIGITS));
            t.push(row);
        }
        let title = format!("Effective Doses with Fiducial Limits ({level}%)");
        section(SectionKind::EffectiveDoses, title, SectionContent::Table(t))
    }

    fn groups(&self) -> ReportSection {
        let mut t = TextTable::new([
            "Dose",
            "Subjects",
            "Responding",
            "Observed %",
            "Expected %",
            "Expected responding",
            "Pearson residual",
        ]);
        for g in &self.fit.groups {
            t.push([
                format_number(g.dose, DIGITS),
                format_number(g.trials, 8),
                format_number(g.responded, 8),
                percent(g.responded / g.trials),
                percent(g.expected_probability),
                format_number(g.trials * g.expected_probability, 4),
                format_number(g.pearson_residual, 4),
            ]);
        }
        section(SectionKind::DoseGroups, "Observed and Expected Responses", SectionContent::Table(t))
    }

    fn plot(&self) -> ReportSection {
        let f = &self.fit;
        let scale = f.options.scale;
        let points: Vec<RowPoint> =
            self.points.iter().filter_map(|&(row, d, p)| scale.forward(d).map(|x| RowPoint { row, x, y: p })).collect();
        let (lo, hi) = f.groups.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), g| (a.min(g.x), b.max(g.x)));
        let pad = 0.1 * (hi - lo).max(1e-9);
        let (lo, hi) = (lo - pad, hi + pad);
        let (a, b) = f.coefficients();
        let line: Vec<[f64; 2]> = (0..CURVE_POINTS)
            .map(|i| {
                let x = lo + (hi - lo) * i as f64 / (CURVE_POINTS - 1) as f64;
                [x, f.options.link.inverse(a + b * x)]
            })
            .collect();
        // Each effective dose: a dashed drop line and its fiducial interval.
        let mut segments = Vec::new();
        for e in &f.effective_doses {
            segments.push(vec![[lo, e.probability], [e.x, e.probability], [e.x, 0.0]]);
            if let Some((l, h)) = e.fiducial {
                if let (Some(l), Some(h)) = (scale.forward(l), scale.forward(h)) {
                    segments.push(vec![[l, e.probability], [h, e.probability]]);
                }
            }
        }
        let x_label = match scale {
            DoseScale::Linear => self.dose_name.clone(),
            DoseScale::Log10 => format!("log₁₀ {}", self.dose_name),
            DoseScale::Ln => format!("ln {}", self.dose_name),
        };
        let spec = PlotSpec {
            x_label,
            y_label: format!("Proportion responding ({})", self.response_name),
            points,
            line: Some(line),
            line_name: Some("Fitted dose–response curve".to_owned()),
            segments,
            ..PlotSpec::default()
        };
        section(SectionKind::DoseResponsePlot, "Dose–Response Curve", SectionContent::Plot(Box::new(spec)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// Martin's rotenone data (Finney 1971) with the untreated control.
    fn table() -> DataTable {
        DataTable::new(
            "rotenone",
            vec![
                Column::numeric("Dose", vec![10.2, 7.7, 5.1, 3.8, 2.6, 0.0]),
                Column::numeric("Exposed", vec![50.0, 49.0, 46.0, 48.0, 50.0, 49.0]),
                Column::numeric("Killed", vec![44.0, 42.0, 24.0, 16.0, 6.0, 0.0]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn probit_report_sections() {
        let t = table();
        let req = DoseResponseRequest::probit(0, 2, Some(1));
        let a = DoseResponseAnalysis::run(&t, &RowStateManager::new(6), &req).unwrap();
        assert_eq!(a.title(), "Probit Analysis: Killed by Dose");
        let s = a.sections();
        assert_eq!(s.len(), 5);
        let SectionContent::Table(ed) = &s[2].content else { panic!() };
        assert_eq!(ed.cell(1, "Response %"), Some("50"));
        // ED50 = 4.84549 mg/l (see stat-discrete's reference test).
        assert!(ed.cell(1, "Dose (Dose)").unwrap().starts_with("4.8454"), "{:?}", ed.cell(1, "Dose (Dose)"));
        let SectionContent::Table(sum) = &s[0].content else { panic!() };
        assert!(sum.rows.iter().any(|r| r[0].starts_with("Rows left out")));
        let SectionContent::Plot(p) = &s[4].content else { panic!() };
        assert_eq!(p.points.len(), 5, "the control has no log dose");
        assert!(p.line.as_ref().unwrap().windows(2).all(|w| w[1][1] > w[0][1]));
    }

    #[test]
    fn excluded_rows_and_bad_requests() {
        let t = table();
        let mut rows = RowStateManager::new(6);
        rows.set_excluded([4], true);
        let a = DoseResponseAnalysis::run(&t, &rows, &DoseResponseRequest::probit(0, 2, Some(1))).unwrap();
        assert_eq!(a.fit().groups.len(), 4);
        // Counts without trials are refused.
        assert!(
            DoseResponseAnalysis::run(&t, &RowStateManager::new(6), &DoseResponseRequest::probit(0, 2, None)).is_err()
        );
        let mut cols = DoseResponseRequest::probit(0, 2, Some(1));
        cols.map_columns(&|c| c + 1);
        assert_eq!(cols.columns(), vec![1, 3, 2]);
    }
}
