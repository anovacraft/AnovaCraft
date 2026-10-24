//! Mixture-model analysis: Scheffé canonical polynomials fitted to blends
//! whose component proportions sum to one ([`stat_doe::ScheffeModel`];
//! Cornell, *Experiments with Mixtures*, 3rd ed., ch. 2–3).
//!
//! Rows with a response and every component are used (excluded rows are left
//! out). Component values are rescaled to proportions of their row total, so
//! amounts or percentages work as well as fractions; rows whose total differs
//! from that of the first row by more than 1% are reported.
//!
//! The model has no intercept (the proportions sum to one), so R² and the
//! ANOVA use the total sum of squares about the mean (Cornell §2.8), with
//! `q − 1` model degrees of freedom for the linear blending part. The test of
//! **nonlinear blending** compares the fitted model with the linear one;
//! each nonlinear term also gets a partial F-test. For three components the
//! fitted surface is drawn as a contour plot on the simplex (a ternary plot).

use crate::format::{format_number, format_p_value};
use crate::report::{
    Annotations, ContourMap, PlotSpec, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use stat_core::distributions::{ContinuousDistribution, FisherSnedecor};
use stat_core::{Matrix, Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_doe::ScheffeModel;
use stat_linear::{LinearModelFit, OlsOptions, TermValue, fit_linear_model};

/// Significant digits in report tables.
const DIGITS: usize = 5;
/// √3 / 2, the height of the unit simplex drawn as an equilateral triangle.
const HEIGHT: f64 = stat_viz::forms::ternary::TERNARY_HEIGHT;
/// Divisions of each side of the simplex for the ternary contour map.
const TERNARY_LATTICE: usize = 60;

/// What to fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixtureRequest {
    /// Response column.
    pub response: usize,
    /// Component columns (two to twelve).
    pub components: Vec<usize>,
    /// Scheffé model.
    pub model: ScheffeModel,
}

/// A fitted mixture model.
#[derive(Debug, Clone)]
pub struct MixtureAnalysis {
    request: MixtureRequest,
    response_name: String,
    names: Vec<String>,
    fit: LinearModelFit,
    /// (row, proportions, response) of each run used.
    runs: Vec<(usize, Vec<f64>, f64)>,
    sst: f64,
    sse: f64,
    linear_sse: f64,
    term_tests: Vec<(String, f64, Option<f64>, Option<f64>)>,
    uneven_totals: usize,
    n_excluded: usize,
    exclusion_revision: u64,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn sse_of(rows: &[Vec<f64>], y: &[f64], columns: &[usize]) -> Result<f64> {
    let cols: Vec<Vec<f64>> = columns.iter().map(|&j| rows.iter().map(|r| r[j]).collect()).collect();
    let refs: Vec<&[f64]> = cols.iter().map(Vec::as_slice).collect();
    let fit = fit_linear_model(&Matrix::from_columns(&refs)?, y, &OlsOptions::without_intercept())?;
    Ok(fit.anova().error.sum_of_squares)
}

impl MixtureAnalysis {
    /// Fit the model.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for missing, repeated or non-numeric
    /// columns, fewer than two or more than twelve components, negative
    /// amounts, a run whose components sum to zero, or fewer runs than terms;
    /// fitting errors (e.g. a design that cannot estimate the model).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &MixtureRequest) -> Result<Self> {
        let q = request.components.len();
        if !(2..=stat_doe::mixture::MAX_COMPONENTS).contains(&q) {
            return Err(invalid("choose between 2 and 12 components"));
        }
        let mut all = request.components.clone();
        all.push(request.response);
        all.sort_unstable();
        if all.windows(2).any(|w| w[0] == w[1]) {
            return Err(invalid("the response and components must be different columns"));
        }
        let numeric = |i: usize| -> Result<(String, &[f64])> {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?;
            Ok((c.name.clone(), v))
        };
        let (response_name, y_all) = numeric(request.response)?;
        let mut names = Vec::with_capacity(q);
        let mut comps = Vec::with_capacity(q);
        for &c in &request.components {
            let (n, v) = numeric(c)?;
            names.push(n);
            comps.push(v);
        }
        let complete: Vec<usize> =
            (0..table.n_rows()).filter(|&r| y_all[r].is_finite() && comps.iter().all(|v| v[r].is_finite())).collect();
        let used: Vec<usize> = complete.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        let mut runs = Vec::with_capacity(used.len());
        let mut first_total = None;
        let mut uneven_totals = 0;
        for &r in &used {
            let amounts: Vec<f64> = comps.iter().map(|v| v[r]).collect();
            if amounts.iter().any(|&a| a < 0.0) {
                return Err(invalid(format!("row {} has a negative component amount", r + 1)));
            }
            let total: f64 = amounts.iter().sum();
            if total <= 0.0 {
                return Err(invalid(format!("the components of row {} sum to zero", r + 1)));
            }
            let reference = *first_total.get_or_insert(total);
            if (total - reference).abs() > 0.01 * reference {
                uneven_totals += 1;
            }
            runs.push((r, amounts.iter().map(|a| a / total).collect::<Vec<f64>>(), y_all[r]));
        }
        let model = request.model;
        let p = model.n_terms(q);
        if runs.len() <= p {
            return Err(invalid(format!(
                "the {} model in {q} components has {p} terms and needs more runs than that ({} available)",
                model.label().to_lowercase(),
                runs.len()
            )));
        }
        let rows: Vec<Vec<f64>> = runs.iter().map(|r| model.expand(&r.1)).collect();
        let y: Vec<f64> = runs.iter().map(|r| r.2).collect();
        let labels = model.labels(&names);
        let fit = fit_linear_model(
            &Matrix::from_rows(&rows)?,
            &y,
            &OlsOptions { predictor_names: labels.clone(), ..OlsOptions::without_intercept() },
        )?;
        let sse = fit.anova().error.sum_of_squares;
        let mean = y.iter().sum::<f64>() / y.len() as f64;
        let sst = y.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>();
        let linear_sse = sse_of(&rows, &y, &(0..q).collect::<Vec<_>>())?;
        // Partial F-test of each nonlinear term.
        let df_error = (y.len() - p) as f64;
        let mse = sse / df_error;
        let mut term_tests = Vec::new();
        for j in q..p {
            let keep: Vec<usize> = (0..p).filter(|&k| k != j).collect();
            let reduced = sse_of(&rows, &y, &keep)?;
            let ss = (reduced - sse).max(0.0);
            let f = (mse > 0.0).then(|| ss / mse);
            let pv = match f {
                Some(f) => Some(FisherSnedecor::new(1.0, df_error)?.sf(f)?),
                None => None,
            };
            term_tests.push((labels[j].clone(), ss, f, pv));
        }
        Ok(Self {
            request: request.clone(),
            response_name,
            names,
            fit,
            runs,
            sst,
            sse,
            linear_sse,
            term_tests,
            uneven_totals,
            n_excluded: complete.len() - used.len(),
            exclusion_revision: states.exclusion_revision(),
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &MixtureRequest {
        &self.request
    }

    /// Fitted coefficients, in the model's term order.
    #[must_use]
    pub fn coefficients(&self) -> Vec<f64> {
        self.fit.coefficients()
    }

    /// R² about the mean.
    #[must_use]
    pub fn r_squared(&self) -> f64 {
        if self.sst > 0.0 { 1.0 - self.sse / self.sst } else { f64::NAN }
    }

    /// `true` if row exclusion has changed since the fit.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Mixture Model: {} ~ {}", self.response_name, self.names.join(" + "))
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        if self.request.model != ScheffeModel::Linear {
            out.extend(
                crate::report::EffectPareto::new(
                    self.term_tests.iter().map(|(name, _, _, p)| (name.clone(), p.unwrap_or(f64::NAN))),
                    "F tests of nonlinear blending",
                )
                .section(),
            );
        }
        out.push(self.coefficient_table());
        if self.request.model != ScheffeModel::Linear {
            out.push(self.blending_tests());
        }
        if self.names.len() == 3 {
            out.push(self.ternary_plot());
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let n = self.runs.len();
        let p = self.fit.coefficients().len();
        let mut t = TextTable::new(["Statistic", "Value"]);
        t.push(["Model".to_owned(), format!("{} Scheffé polynomial", self.request.model.label())]);
        t.push(["Components".to_owned(), self.names.join(", ")]);
        t.push(["Runs used".to_owned(), n.to_string()]);
        if self.n_excluded > 0 {
            t.push(["Rows excluded".to_owned(), self.n_excluded.to_string()]);
        }
        t.push(["R² (about the mean)".to_owned(), format_number(self.r_squared(), DIGITS)]);
        let adj = 1.0 - (self.sse / (n - p) as f64) / (self.sst / (n - 1) as f64);
        t.push(["Adjusted R²".to_owned(), format_number(adj, DIGITS)]);
        t.push(["Root mean square error".to_owned(), format_number((self.sse / (n - p) as f64).sqrt(), DIGITS)]);
        let df_model = p - 1;
        let ss_model = self.sst - self.sse;
        let f = (ss_model / df_model as f64) / (self.sse / (n - p) as f64);
        let pv = FisherSnedecor::new(df_model as f64, (n - p) as f64).and_then(|d| d.sf(f)).unwrap_or(f64::NAN);
        t.push([
            format!("Model F ({df_model}, {})", n - p),
            format!("{} (p = {})", format_number(f, 4), format_p_value(pv)),
        ]);
        let title = if self.uneven_totals > 0 {
            format!(
                "Model Summary ({} runs' component totals differ from the first by over 1%; each run was rescaled to proportions)",
                self.uneven_totals
            )
        } else {
            "Model Summary".to_owned()
        };
        section(SectionKind::MixtureSummary, title, SectionContent::Table(t))
    }

    fn coefficient_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Term", "Estimate", "Std Error", "t Ratio", "p-value"]);
        for p in self.fit.parameters() {
            t.push([
                p.name.clone(),
                format_number(p.estimate, DIGITS),
                format_number(p.std_error, DIGITS),
                p.t_statistic.map_or_else(|| "—".to_owned(), |v| format_number(v, 4)),
                p.p_value.map_or_else(|| "—".to_owned(), format_p_value),
            ]);
        }
        section(
            SectionKind::MixtureCoefficients,
            "Scheffé Coefficients (a pure-component coefficient is the predicted response of that pure \
             component; its test against 0 is rarely meaningful)",
            SectionContent::Table(t),
        )
    }

    fn blending_tests(&self) -> ReportSection {
        let q = self.names.len();
        let n = self.runs.len();
        let p = self.fit.coefficients().len();
        let df_error = (n - p) as f64;
        let mse = self.sse / df_error;
        let mut t = TextTable::new(["Source", "DF", "Sum of Squares", "F", "p-value"]);
        let df_nl = p - q;
        let ss_nl = (self.linear_sse - self.sse).max(0.0);
        let f = ss_nl / df_nl as f64 / mse;
        let pv = FisherSnedecor::new(df_nl as f64, df_error).and_then(|d| d.sf(f)).unwrap_or(f64::NAN);
        t.push([
            "All nonlinear blending terms".to_owned(),
            df_nl.to_string(),
            format_number(ss_nl, DIGITS),
            format_number(f, 4),
            format_p_value(pv),
        ]);
        for (name, ss, f, pv) in &self.term_tests {
            t.push([
                name.clone(),
                "1".to_owned(),
                format_number(*ss, DIGITS),
                f.map_or_else(|| "—".to_owned(), |v| format_number(v, 4)),
                pv.map_or_else(|| "—".to_owned(), format_p_value),
            ]);
        }
        section(
            SectionKind::MixtureBlending,
            "Tests of Nonlinear Blending (synergy or antagonism between components)",
            SectionContent::Table(t),
        )
    }

    /// Contour plot of the fitted response on the simplex (three components).
    fn ternary_plot(&self) -> ReportSection {
        let coef = self.fit.coefficients();
        let model = self.request.model;
        let value =
            |a: f64, b: f64, c: f64| -> f64 { model.expand(&[a, b, c]).iter().zip(&coef).map(|(x, k)| x * k).sum() };
        // Simplex corner A at (0, 0), B at (1, 0), C at (½, √3/2).
        let to_plot = |p: &[f64]| stat_viz::forms::ternary_position([p[0], p[1], p[2]]);
        let field = stat_viz::ContourField::simplex(TERNARY_LATTICE, |p| value(p[0], p[1], p[2]), |p| to_plot(&p));
        let map = ContourMap::new(field, 8, format!("Predicted {}", self.response_name));
        let segments: Vec<Polyline> = vec![vec![[0.0, 0.0], [1.0, 0.0], [0.5, HEIGHT], [0.0, 0.0]]];
        let labels = vec![
            (0.0, -0.04, format!("{} = 1", self.names[0])),
            (1.0, -0.04, format!("{} = 1", self.names[1])),
            (0.5, HEIGHT + 0.04, format!("{} = 1", self.names[2])),
        ];
        let points = self.runs.iter().map(|(row, p, _)| {
            let [x, y] = to_plot(p);
            RowPoint { row: *row, x, y }
        });
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: String::new(),
            y_label: String::new(),
            points: points.collect(),
            line: None,
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: Some(Annotations { groups: Vec::new(), items: Vec::new(), labels }),
            segments,
            simplex: true,
            contour: Some(map),
            ..PlotSpec::default()
        };
        section(
            SectionKind::MixtureContours,
            format!("Ternary Contour Plot of Predicted {}", self.response_name),
            SectionContent::Plot(Box::new(spec)),
        )
    }
}

impl MixtureAnalysis {
    /// Smallest and largest proportion of component `j` over the runs used.
    fn proportion_range(&self, j: usize) -> (f64, f64) {
        let values = self.runs.iter().map(|r| r.1[j]);
        let lo = values.clone().fold(f64::INFINITY, f64::min);
        let hi = values.fold(f64::NEG_INFINITY, f64::max);
        if lo < hi { (lo, hi) } else { (0.0, 1.0) }
    }

    /// Proportions of a settings vector, rescaled to sum to one.
    fn blend(&self, s: &[TermValue]) -> Result<Vec<f64>> {
        if s.len() != self.names.len() {
            return Err(invalid("the settings do not match the components"));
        }
        let x: Vec<f64> = s
            .iter()
            .map(|v| match v {
                TermValue::Number(x) if *x >= 0.0 => Ok(*x),
                _ => Err(invalid("component proportions are non-negative numbers")),
            })
            .collect::<Result<_>>()?;
        let total: f64 = x.iter().sum();
        if total <= 0.0 {
            return Err(invalid("the components sum to zero"));
        }
        Ok(x.iter().map(|v| v / total).collect())
    }
}

impl crate::surface_profiler::SurfaceModel for MixtureAnalysis {
    fn response_name(&self) -> &str {
        &self.response_name
    }

    fn profile_factors(&self) -> Vec<crate::surface_profiler::ProfileFactor> {
        (0..self.names.len())
            .map(|j| crate::surface_profiler::ProfileFactor {
                name: self.names[j].clone(),
                kind: crate::surface_profiler::ProfileFactorKind::Continuous { range: self.proportion_range(j) },
            })
            .collect()
    }

    /// The centroid of the runs used (average proportions).
    fn default_profile_settings(&self) -> Vec<TermValue> {
        let n = self.runs.len().max(1) as f64;
        (0..self.names.len()).map(|j| TermValue::Number(self.runs.iter().map(|r| r.1[j]).sum::<f64>() / n)).collect()
    }

    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        Ok(self.predict_with_limits(points)?.into_iter().map(|p| p.0).collect())
    }

    fn predict_with_limits(&self, points: &[Vec<TermValue>]) -> Result<Vec<crate::surface_profiler::Limited>> {
        if points.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<Vec<f64>> =
            points.iter().map(|s| Ok(self.request.model.expand(&self.blend(s)?))).collect::<Result<_>>()?;
        Ok(self
            .fit
            .predict(&Matrix::from_rows(&rows)?)?
            .into_iter()
            .map(|p| (p.fitted, Some(p.confidence_interval)))
            .collect())
    }

    fn confidence_level(&self) -> Option<f64> {
        Some(self.fit.confidence_level())
    }

    /// Cox-direction change: component `j` moves to `value` and the others
    /// keep their relative proportions, so the blend still sums to one.
    fn with_setting(&self, settings: &[TermValue], j: usize, value: TermValue) -> Vec<TermValue> {
        let TermValue::Number(target) = value else { return settings.to_vec() };
        let target = target.clamp(0.0, 1.0);
        let x: Vec<f64> =
            settings.iter().map(|v| if let TermValue::Number(x) = v { x.max(0.0) } else { 0.0 }).collect();
        let rest: f64 = x.iter().enumerate().filter(|&(k, _)| k != j).map(|(_, v)| v).sum();
        let others = (x.len() - 1).max(1) as f64;
        x.iter()
            .enumerate()
            .map(|(k, &v)| {
                TermValue::Number(if k == j {
                    target
                } else if rest > 1e-12 {
                    v * (1.0 - target) / rest
                } else {
                    (1.0 - target) / others
                })
            })
            .collect()
    }

    fn independent_factors(&self) -> bool {
        false
    }

    fn profile_note(&self) -> Option<String> {
        Some(
            "Moving one component rescales the others in proportion (the Cox direction), so the blend always sums \
             to 1; each panel's range is that component's range in the runs."
                .to_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// Cornell's yarn-elongation data (simplex-lattice {3, 2} with replicates);
    /// published fit 11.7x₁ + 9.4x₂ + 16.4x₃ + 19.0x₁x₂ + 11.4x₁x₃ − 9.6x₂x₃.
    fn yarn() -> DataTable {
        let pts: [([f64; 3], &[f64]); 6] = [
            ([1.0, 0.0, 0.0], &[11.0, 12.4]),
            ([0.5, 0.5, 0.0], &[15.0, 14.8, 16.1]),
            ([0.0, 1.0, 0.0], &[8.8, 10.0]),
            ([0.0, 0.5, 0.5], &[10.0, 9.7, 11.8]),
            ([0.0, 0.0, 1.0], &[16.8, 16.0]),
            ([0.5, 0.0, 0.5], &[17.7, 16.4, 16.6]),
        ];
        let mut cols = vec![Vec::new(); 4];
        for (x, ys) in pts {
            for &y in ys {
                // Percentages, to exercise the rescaling.
                for k in 0..3 {
                    cols[k].push(100.0 * x[k]);
                }
                cols[3].push(y);
            }
        }
        let names = ["Polyethylene", "Polystyrene", "Polypropylene", "Elongation"];
        DataTable::new("yarn", names.iter().zip(cols).map(|(n, v)| Column::numeric(*n, v)).collect()).unwrap()
    }

    #[test]
    fn reproduces_the_published_quadratic() {
        let t = yarn();
        let rows = RowStateManager::new(t.n_rows());
        let request = MixtureRequest { response: 3, components: vec![0, 1, 2], model: ScheffeModel::Quadratic };
        let a = MixtureAnalysis::run(&t, &rows, &request).unwrap();
        for (c, e) in a.coefficients().iter().zip([11.7, 9.4, 16.4, 19.0, 11.4, -9.6]) {
            assert!((c - e).abs() < 1e-9, "{c} vs {e}");
        }
        // R² about the mean and the nonlinear-blending F from NumPy/SciPy.
        assert!((a.r_squared() - 0.951_355_519_962_033_6).abs() < 1e-12);
        let s = a.sections();
        assert_eq!(s.len(), 5);
        let SectionContent::Table(bl) = &s[3].content else { panic!() };
        assert_eq!(bl.cell(0, "F"), Some("32.32"));
        assert_eq!(bl.rows.len(), 4, "overall and three binary terms");
        let SectionContent::Plot(p) = &s[4].content else { panic!() };
        assert_eq!(p.points.len(), 15);
        assert_eq!(p.segments.len(), 1, "the triangle");
        // Every contour line and band vertex lies inside the triangle.
        let d = p.contour.as_ref().expect("a contour map").drawing(false);
        assert!(d.lines.len() > 3 && d.bands.len() > 3);
        let lines = d.lines.iter().flat_map(|l| l.1.iter().flatten());
        let bands = d.bands.iter().flat_map(|b| b.1.rings.iter().flatten());
        for &[x, y] in lines.chain(bands) {
            assert!((-1e-9..=HEIGHT + 1e-9).contains(&y));
            assert!(y <= 2.0 * HEIGHT * x + 1e-9 && y <= 2.0 * HEIGHT * (1.0 - x) + 1e-9);
        }
        assert_eq!(a.title(), "Mixture Model: Elongation ~ Polyethylene + Polystyrene + Polypropylene");
    }

    #[test]
    fn linear_model_and_errors() {
        let t = yarn();
        let rows = RowStateManager::new(t.n_rows());
        let linear = MixtureRequest { response: 3, components: vec![0, 1, 2], model: ScheffeModel::Linear };
        let a = MixtureAnalysis::run(&t, &rows, &linear).unwrap();
        assert_eq!(a.sections().len(), 3, "no blending test for the linear model");
        let mut few = RowStateManager::new(t.n_rows());
        few.set_excluded(0..10, true);
        let cubic = MixtureRequest { model: ScheffeModel::SpecialCubic, ..linear.clone() };
        assert!(MixtureAnalysis::run(&t, &few, &cubic).is_err(), "too few runs");
        assert!(MixtureAnalysis::run(&t, &rows, &MixtureRequest { components: vec![0], ..linear.clone() }).is_err());
        assert!(MixtureAnalysis::run(&t, &rows, &MixtureRequest { components: vec![0, 3], ..linear }).is_err());
    }
}
