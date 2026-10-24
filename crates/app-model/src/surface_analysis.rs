//! Response-surface analysis: fit the full second-order model to a response
//! and continuous factors, then locate and describe its optimum.
//!
//! Each factor is coded so that the data run from −1 to +1
//! (`x = (value − centre) / half-range`); the model
//! `ŷ = b₀ + Σ bᵢxᵢ + Σ bᵢⱼxᵢxⱼ + Σ bᵢᵢxᵢ²` is fitted by least squares to the
//! rows that are complete and not excluded. From the fit
//! ([`stat_doe::QuadraticSurface`]) the report gives the stationary point and
//! its shape, the canonical analysis (eigenvalues and eigenvectors), ridge paths
//! of greatest and least predicted response out to the edge of the data, and a
//! contour plot over any two factors with the others held fixed.

use crate::format::{format_number, format_p_value};
use crate::report::{
    Annotations, ContourMap, PlotSpec, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use stat_core::{Matrix, Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_doe::{CanonicalAnalysis, ModelFormula, QuadraticSurface, RidgeDirection};
use stat_linear::{LinearModelFit, OlsOptions, fit_linear_model};

/// Significant digits in report tables.
const DIGITS: usize = 5;
/// Grid cells along each side of the contour plot.
const CONTOUR_GRID: usize = 60;
/// Contour levels aimed for (the step is rounded to 1, 2, 2.5 or 5 × 10ᵏ).
const CONTOUR_LEVELS: usize = 9;
/// Radii (coded units) of the ridge tables.
const RIDGE_STEPS: usize = 10;

/// Which columns enter the analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceRequest {
    /// Response column.
    pub response: usize,
    /// Continuous factor columns (at least one).
    pub factors: Vec<usize>,
}

/// Report options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceOptions {
    /// Show the coefficient table.
    pub show_coefficients: bool,
    /// Show the ridge tables.
    pub show_ridge: bool,
    /// Factor (index into the request's factors) on the contour plot's x axis.
    pub contour_x: usize,
    /// Factor on the contour plot's y axis.
    pub contour_y: usize,
    /// Show the Response Explorer (one slice panel per factor).
    pub show_profiler: bool,
    /// Show the 3-D surface profiler.
    pub show_surface_profiler: bool,
    /// Show observed data in the 3-D profiler.
    pub show_surface_observed: bool,
}

impl Default for SurfaceOptions {
    fn default() -> Self {
        Self {
            show_coefficients: true,
            show_ridge: true,
            contour_x: 0,
            contour_y: 1,
            show_profiler: false,
            show_surface_profiler: false,
            show_surface_observed: false,
        }
    }
}

impl SurfaceOptions {
    /// These options for a surface in `k` factors: contour axes that are not
    /// two different factors among the `k` (kept from a report that had more
    /// factors) go back to the first two.
    #[must_use]
    pub fn for_factors(mut self, k: usize) -> Self {
        if self.contour_x >= k || self.contour_y >= k || self.contour_x == self.contour_y {
            self.contour_x = 0;
            self.contour_y = 1;
        }
        self
    }
}

/// A fitted response surface and what the report needs from it.
#[derive(Debug, Clone)]
pub struct SurfaceAnalysis {
    request: SurfaceRequest,
    response_name: String,
    factor_names: Vec<String>,
    /// Centre and half-range of each factor (coded value = (x − c) / h).
    coding: Vec<(f64, f64)>,
    fit: LinearModelFit,
    surface: QuadraticSurface,
    canonical: CanonicalAnalysis,
    /// Included rows with their factor values and response.
    used_points: Vec<(usize, Vec<f64>, f64)>,
    n_excluded: usize,
    n_missing: usize,
    exclusion_revision: u64,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

impl SurfaceAnalysis {
    /// Fit the second-order model.
    ///
    /// # Errors
    /// * [`StatError::InvalidArgument`] for missing or repeated columns, a
    ///   non-numeric column, a factor with a single value, or fewer rows than
    ///   model terms.
    /// * Any error from the least-squares fit (e.g. too few distinct levels for
    ///   the quadratic terms).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SurfaceRequest) -> Result<Self> {
        if request.factors.is_empty() {
            return Err(invalid("choose at least one factor"));
        }
        let mut seen = request.factors.clone();
        seen.push(request.response);
        seen.sort_unstable();
        if seen.windows(2).any(|w| w[0] == w[1]) {
            return Err(invalid("the response and factors must be different columns"));
        }
        let numeric = |i: usize| -> Result<(String, &[f64])> {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?;
            Ok((c.name.clone(), v))
        };
        let (response_name, y_all) = numeric(request.response)?;
        let mut factor_names = Vec::new();
        let mut factor_values = Vec::new();
        for &f in &request.factors {
            let (name, v) = numeric(f)?;
            factor_names.push(name);
            factor_values.push(v);
        }
        let complete: Vec<usize> = (0..table.n_rows())
            .filter(|&r| y_all[r].is_finite() && factor_values.iter().all(|v| v[r].is_finite()))
            .collect();
        let used: Vec<usize> = complete.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        let k = request.factors.len();
        let formula = ModelFormula::quadratic(k);
        if used.len() <= formula.n_terms() {
            return Err(invalid(format!(
                "a second-order model in {k} factor{} has {} terms and needs more rows than that ({} available)",
                if k == 1 { "" } else { "s" },
                formula.n_terms(),
                used.len()
            )));
        }
        let mut coding = Vec::with_capacity(k);
        for (name, v) in factor_names.iter().zip(&factor_values) {
            let lo = used.iter().map(|&r| v[r]).fold(f64::INFINITY, f64::min);
            let hi = used.iter().map(|&r| v[r]).fold(f64::NEG_INFINITY, f64::max);
            if hi <= lo {
                return Err(invalid(format!("{name} has a single value in the rows used")));
            }
            coding.push((0.5 * (lo + hi), 0.5 * (hi - lo)));
        }
        let code =
            |r: usize| -> Vec<f64> { factor_values.iter().zip(&coding).map(|(v, &(c, h))| (v[r] - c) / h).collect() };
        let rows: Vec<Vec<f64>> = used.iter().map(|&r| formula.expand(&code(r))[1..].to_vec()).collect();
        let x = Matrix::from_rows(&rows)?;
        let y: Vec<f64> = used.iter().map(|&r| y_all[r]).collect();
        let names: Vec<String> = formula.terms()[1..].iter().map(|t| t.label(&factor_names)).collect();
        let fit = fit_linear_model(&x, &y, &OlsOptions { predictor_names: names, ..OlsOptions::default() })?;
        let surface = QuadraticSurface::from_formula(&formula, &fit.coefficients())?;
        let canonical = surface.canonical_analysis();
        let used_points = used.iter().map(|&r| (r, factor_values.iter().map(|v| v[r]).collect(), y_all[r])).collect();
        Ok(Self {
            request: request.clone(),
            response_name,
            factor_names,
            coding,
            fit,
            surface,
            canonical,
            used_points,
            n_excluded: complete.len() - used.len(),
            n_missing: table.n_rows() - complete.len(),
            exclusion_revision: states.exclusion_revision(),
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &SurfaceRequest {
        &self.request
    }

    /// Factor names, in request order.
    #[must_use]
    pub fn factor_names(&self) -> &[String] {
        &self.factor_names
    }

    /// The fitted surface (coded units).
    #[must_use]
    pub fn surface(&self) -> &QuadraticSurface {
        &self.surface
    }

    /// Stationary point, eigenvalues and shape.
    #[must_use]
    pub fn canonical(&self) -> &CanonicalAnalysis {
        &self.canonical
    }

    /// `true` if row exclusion has changed since the fit.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Response Surface: {} ~ {}", self.response_name, self.factor_names.join(", "))
    }

    /// Actual value of factor `j` at coded value `x`.
    #[must_use]
    pub fn actual(&self, j: usize, x: f64) -> f64 {
        let (c, h) = self.coding[j];
        c + h * x
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self, options: &SurfaceOptions) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        out.extend(
            crate::report::EffectPareto::new(
                self.fit
                    .parameters()
                    .iter()
                    .filter(|p| p.name != stat_linear::INTERCEPT_NAME)
                    .map(|p| (p.name.clone(), p.p_value.unwrap_or(f64::NAN))),
                "t tests of the coded coefficients",
            )
            .section(),
        );
        if options.show_coefficients {
            out.push(self.coefficients());
        }
        out.push(self.stationary());
        out.push(self.canonical_table());
        if options.show_ridge {
            out.push(self.ridge(RidgeDirection::Maximum));
            out.push(self.ridge(RidgeDirection::Minimum));
        }
        if self.factor_names.len() >= 2 {
            out.push(self.contours(options.contour_x, options.contour_y));
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let s = self.fit.summary();
        let mut t = TextTable::new(["Statistic", "Value"]);
        t.push(["R²".to_owned(), format_number(s.r_squared, DIGITS)]);
        t.push(["Adjusted R²".to_owned(), format_number(s.adj_r_squared, DIGITS)]);
        t.push(["Root mean square error".to_owned(), format_number(s.root_mean_square_error, DIGITS)]);
        t.push(["Mean of response".to_owned(), format_number(s.mean_response, DIGITS)]);
        t.push(["Observations used".to_owned(), s.n_observations.to_string()]);
        if self.n_excluded > 0 {
            t.push(["Rows excluded".to_owned(), self.n_excluded.to_string()]);
        }
        if self.n_missing > 0 {
            t.push(["Rows with missing values".to_owned(), self.n_missing.to_string()]);
        }
        for (j, name) in self.factor_names.iter().enumerate() {
            let (lo, hi) = (self.actual(j, -1.0), self.actual(j, 1.0));
            t.push([
                format!("Coding of {name}"),
                format!("{} → −1, {} → +1", format_number(lo, DIGITS), format_number(hi, DIGITS)),
            ]);
        }
        section(SectionKind::SurfaceSummary, "Model Summary (full second-order model)", SectionContent::Table(t))
    }

    fn coefficients(&self) -> ReportSection {
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
        section(SectionKind::SurfaceCoefficients, "Coefficients (coded factors)", SectionContent::Table(t))
    }

    fn stationary(&self) -> ReportSection {
        let c = &self.canonical;
        let Some(xs) = &c.stationary_point else {
            return section(
                SectionKind::StationaryPoint,
                "Stationary Point",
                SectionContent::Note(
                    "The quadratic part of the model is singular, so there is no unique stationary point; the \
                     surface is a ridge. See the ridge tables for the best settings within the data."
                        .to_owned(),
                ),
            );
        };
        let mut t = TextTable::new(["Factor", "Coded", "Actual"]);
        for (j, name) in self.factor_names.iter().enumerate() {
            t.push([name.clone(), format_number(xs[j], DIGITS), format_number(self.actual(j, xs[j]), DIGITS)]);
        }
        t.push([
            format!("Predicted {}", self.response_name),
            String::new(),
            format_number(c.stationary_value.unwrap_or(f64::NAN), DIGITS),
        ]);
        let outside = xs.iter().any(|v| v.abs() > 1.0);
        let place = if outside { ", outside the range of the data" } else { "" };
        section(
            SectionKind::StationaryPoint,
            format!("Stationary Point ({}{place})", c.kind.label()),
            SectionContent::Table(t),
        )
    }

    fn canonical_table(&self) -> ReportSection {
        let c = &self.canonical;
        let mut columns = vec!["Eigenvalue".to_owned()];
        columns.extend(self.factor_names.iter().cloned());
        let mut t = TextTable::new(columns);
        for (l, v) in c.eigenvalues.iter().zip(&c.eigenvectors) {
            let mut row = vec![format_number(*l, DIGITS)];
            row.extend(v.iter().map(|x| format_number(*x, 4)));
            t.push(row);
        }
        let biggest = c.eigenvalues.iter().fold(0.0_f64, |m, l| m.max(l.abs()));
        let flat = c.eigenvalues.iter().any(|l| l.abs() < 0.05 * biggest);
        let title = if flat {
            "Canonical Analysis (eigenvectors in coded units; a near-zero eigenvalue means a ridge)"
        } else {
            "Canonical Analysis (eigenvectors in coded units)"
        };
        section(SectionKind::CanonicalAnalysis, title, SectionContent::Table(t))
    }

    fn ridge(&self, direction: RidgeDirection) -> ReportSection {
        let radii: Vec<f64> = (0..=RIDGE_STEPS).map(|i| i as f64 / RIDGE_STEPS as f64).collect();
        let mut columns = vec!["Radius (coded)".to_owned()];
        columns.extend(self.factor_names.iter().cloned());
        columns.push(format!("Predicted {}", self.response_name));
        let mut t = TextTable::new(columns);
        if let Ok(path) = self.surface.ridge_path(direction, &radii) {
            for p in path {
                let mut row = vec![format_number(p.radius, 3)];
                row.extend(p.x.iter().enumerate().map(|(j, &x)| format_number(self.actual(j, x), DIGITS)));
                row.push(format_number(p.value, DIGITS));
                t.push(row);
            }
        }
        let (kind, title) = match direction {
            RidgeDirection::Maximum => (SectionKind::RidgeMaximum, "Ridge of Maximum Response"),
            RidgeDirection::Minimum => (SectionKind::RidgeMinimum, "Ridge of Minimum Response"),
        };
        section(kind, format!("{title} (best point at each distance from the center)"), SectionContent::Table(t))
    }

    /// Coded settings of the factors not on the contour plot: the stationary
    /// point when it lies within the data, otherwise the centre.
    fn held_settings(&self) -> Vec<f64> {
        match &self.canonical.stationary_point {
            Some(xs) if xs.iter().all(|v| v.abs() <= 1.0) => xs.clone(),
            _ => vec![0.0; self.factor_names.len()],
        }
    }

    fn contours(&self, ix: usize, iy: usize) -> ReportSection {
        let k = self.factor_names.len();
        let (ix, iy) = if ix < k && iy < k && ix != iy { (ix, iy) } else { (0, 1) };
        let held = self.held_settings();
        let eval = |u: f64, v: f64| {
            let mut x = held.clone();
            x[ix] = u;
            x[iy] = v;
            self.surface.value(&x)
        };
        let n = CONTOUR_GRID;
        let coord = |i: usize| -1.0 + 2.0 * i as f64 / n as f64;
        let grid: Vec<Vec<f64>> = (0..=n).map(|j| (0..=n).map(|i| eval(coord(i), coord(j))).collect()).collect();
        let xs: Vec<f64> = (0..=n).map(|i| self.actual(ix, coord(i))).collect();
        let ys: Vec<f64> = (0..=n).map(|j| self.actual(iy, coord(j))).collect();
        let map = ContourMap::new(
            stat_viz::ContourField::from_grid(&xs, &ys, &grid),
            CONTOUR_LEVELS,
            format!("Predicted {}", self.response_name),
        );
        let (_, step) = map.levels();
        // Only the rows the fit used: excluded rows are not brushable here.
        let points = self.used_points.iter().map(|(row, v, _)| RowPoint { row: *row, x: v[ix], y: v[iy] }).collect();
        let mut items = Vec::new();
        if let (Some(xs), Some(ys)) = (&self.canonical.stationary_point, self.canonical.stationary_value) {
            if xs[ix].abs() <= 1.0 && xs[iy].abs() <= 1.0 {
                let label = format!("{} {}", self.canonical.kind.label(), format_number(ys, DIGITS));
                items.push((self.actual(ix, xs[ix]), self.actual(iy, xs[iy]), label, 0));
            }
        }
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: self.factor_names[ix].clone(),
            y_label: self.factor_names[iy].clone(),
            points,
            line: None,
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: Some(Annotations { groups: vec!["Stationary point".to_owned()], items, labels: Vec::new() }),
            contour: Some(map),
            ..PlotSpec::default()
        };
        let others: Vec<String> = (0..k)
            .filter(|&j| j != ix && j != iy)
            .map(|j| format!("{} = {}", self.factor_names[j], format_number(self.actual(j, held[j]), 4)))
            .collect();
        let mut title =
            format!("Contour Plot of Predicted {} (contours every {}", self.response_name, format_number(step, 6));
        if !others.is_empty() {
            title.push_str(&format!(
                "; {} held at {}",
                if others.len() == 1 { "factor" } else { "factors" },
                others.join(", ")
            ));
        }
        title.push(')');
        section(SectionKind::SurfaceContours, title, SectionContent::Plot(Box::new(spec)))
    }
}

/// Contour lines of `grid` (values at `coord(i), coord(j)` for column `i`, row
/// `j`) at `level`, by marching squares (Lorensen & Cline 1987, in two
/// dimensions), with the two ambiguous cell cases resolved by the cell's mean
/// value. Segments are joined into polylines. Cells with a NaN or infinite
/// corner are skipped, so contours end at holes in the grid.
pub fn contour_lines(grid: &[Vec<f64>], level: f64, coord: impl Fn(usize) -> f64) -> Vec<Polyline> {
    let rows = grid.len();
    let cols = grid.first().map_or(0, Vec::len);
    if rows < 2 || cols < 2 {
        return Vec::new();
    }
    // Edge crossing points, identified by (edge kind, i, j) so shared edges match.
    // Kind 0: horizontal edge from (i, j) to (i+1, j); kind 1: vertical from (i, j) to (i, j+1).
    type Key = (u8, usize, usize);
    let point = |key: Key| -> [f64; 2] {
        let (kind, i, j) = key;
        let (i2, j2) = if kind == 0 { (i + 1, j) } else { (i, j + 1) };
        let (a, b) = (grid[j][i], grid[j2][i2]);
        let t = if b == a { 0.5 } else { ((level - a) / (b - a)).clamp(0.0, 1.0) };
        [coord(i) + t * (coord(i2) - coord(i)), coord(j) + t * (coord(j2) - coord(j))]
    };
    let mut segments: Vec<(Key, Key)> = Vec::new();
    for j in 0..rows - 1 {
        for i in 0..cols - 1 {
            let (v00, v10, v11, v01) = (grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]);
            if !(v00.is_finite() && v10.is_finite() && v11.is_finite() && v01.is_finite()) {
                continue; // a hole in the surface (outside the data)
            }
            let case = usize::from(v00 >= level)
                | usize::from(v10 >= level) << 1
                | usize::from(v11 >= level) << 2
                | usize::from(v01 >= level) << 3;
            let bottom: Key = (0, i, j);
            let right: Key = (1, i + 1, j);
            let top: Key = (0, i, j + 1);
            let left: Key = (1, i, j);
            let centre_high = 0.25 * (v00 + v10 + v11 + v01) >= level;
            match case {
                0 | 15 => {}
                1 | 14 => segments.push((left, bottom)),
                2 | 13 => segments.push((bottom, right)),
                3 | 12 => segments.push((left, right)),
                4 | 11 => segments.push((right, top)),
                6 | 9 => segments.push((bottom, top)),
                7 | 8 => segments.push((left, top)),
                5 => {
                    if centre_high {
                        segments.push((left, top));
                        segments.push((bottom, right));
                    } else {
                        segments.push((left, bottom));
                        segments.push((right, top));
                    }
                }
                10 => {
                    if centre_high {
                        segments.push((left, bottom));
                        segments.push((right, top));
                    } else {
                        segments.push((left, top));
                        segments.push((bottom, right));
                    }
                }
                _ => unreachable!("four bits"),
            }
        }
    }
    // Join segments sharing an end point.
    let mut by_end: std::collections::HashMap<Key, Vec<usize>> = std::collections::HashMap::new();
    for (s, &(a, b)) in segments.iter().enumerate() {
        by_end.entry(a).or_default().push(s);
        by_end.entry(b).or_default().push(s);
    }
    let mut used = vec![false; segments.len()];
    let mut out = Vec::new();
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (a, b) = segments[start];
        let mut chain: std::collections::VecDeque<Key> = std::collections::VecDeque::from([a, b]);
        // Extend forwards from the back, then backwards from the front.
        for forward in [true, false] {
            loop {
                let end = if forward { *chain.back().unwrap_or(&a) } else { *chain.front().unwrap_or(&a) };
                let next = by_end.get(&end).and_then(|list| list.iter().copied().find(|&s| !used[s]));
                let Some(s) = next else { break };
                used[s] = true;
                let (p, q) = segments[s];
                let other = if p == end { q } else { p };
                if forward {
                    chain.push_back(other);
                } else {
                    chain.push_front(other);
                }
            }
        }
        out.push(chain.into_iter().map(point).collect());
    }
    out
}

impl crate::surface_profiler::SurfaceModel for SurfaceAnalysis {
    fn response_name(&self) -> &str {
        &self.response_name
    }

    fn profile_factors(&self) -> Vec<crate::surface_profiler::ProfileFactor> {
        self.factor_names
            .iter()
            .enumerate()
            .map(|(j, name)| crate::surface_profiler::ProfileFactor {
                name: name.clone(),
                kind: crate::surface_profiler::ProfileFactorKind::Continuous {
                    range: (self.actual(j, -1.0), self.actual(j, 1.0)),
                },
            })
            .collect()
    }

    /// The stationary point when it lies within the data, otherwise the centre.
    fn default_profile_settings(&self) -> Vec<stat_linear::TermValue> {
        self.held_settings()
            .iter()
            .enumerate()
            .map(|(j, &u)| stat_linear::TermValue::Number(self.actual(j, u)))
            .collect()
    }

    fn predict_settings(&self, points: &[Vec<stat_linear::TermValue>]) -> Result<Vec<f64>> {
        points.iter().map(|s| Ok(self.surface.value(&self.coded(s)?))).collect()
    }

    fn predict_with_limits(
        &self,
        points: &[Vec<stat_linear::TermValue>],
    ) -> Result<Vec<crate::surface_profiler::Limited>> {
        let formula = ModelFormula::quadratic(self.coding.len());
        let rows: Vec<Vec<f64>> =
            points.iter().map(|s| Ok(formula.expand(&self.coded(s)?)[1..].to_vec())).collect::<Result<_>>()?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }
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

    fn observations(&self) -> Vec<crate::surface_profiler::Observation> {
        self.used_points
            .iter()
            .map(|(row, x, y)| crate::surface_profiler::Observation {
                row: *row,
                settings: x.iter().map(|&v| stat_linear::TermValue::Number(v)).collect(),
                response: *y,
            })
            .collect()
    }
}

impl SurfaceAnalysis {
    /// Coded factor values of a settings vector (all numbers).
    fn coded(&self, s: &[stat_linear::TermValue]) -> Result<Vec<f64>> {
        if s.len() != self.coding.len() {
            return Err(invalid("the settings do not match the factors"));
        }
        s.iter()
            .zip(&self.coding)
            .map(|(v, &(c, h))| match v {
                stat_linear::TermValue::Number(x) => Ok((x - c) / h),
                stat_linear::TermValue::Level(_) => Err(invalid("response-surface factors are continuous")),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// Montgomery's Example 11.2 central composite design in actual units
    /// (time 80–90 min, temperature 170–180 °F at the factorial points, axial
    /// distance √2).
    fn yield_table() -> DataTable {
        let r = 5.0 * std::f64::consts::SQRT_2;
        let time = [80.0, 90.0, 80.0, 90.0, 85.0, 85.0, 85.0, 85.0, 85.0, 85.0 + r, 85.0 - r, 85.0, 85.0];
        let temp = [170.0, 170.0, 180.0, 180.0, 175.0, 175.0, 175.0, 175.0, 175.0, 175.0, 175.0, 175.0 + r, 175.0 - r];
        let y = [76.5, 78.0, 77.0, 79.5, 79.9, 80.3, 80.0, 79.7, 79.8, 78.4, 75.6, 78.5, 77.0];
        let col = |name: &str, v: &[f64]| Column::numeric(name, v.to_vec());
        DataTable::new("yield", vec![col("Time", &time), col("Temperature", &temp), col("Yield", &y)]).unwrap()
    }

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    #[test]
    fn reproduces_the_published_yield_optimum() {
        let table = yield_table();
        let rows = RowStateManager::new(table.n_rows());
        let a = SurfaceAnalysis::run(&table, &rows, &SurfaceRequest { response: 2, factors: vec![0, 1] }).unwrap();
        // Coefficients in range coding (NumPy least squares on the same data).
        let coef = a.fit.coefficients();
        for (c, e) in coef.iter().zip([79.94, 1.407_106_78, 0.728_553_39, 0.5, -2.7525, -2.0025]) {
            close(*c, e, 1e-8);
        }
        let xs = a.canonical().stationary_point.clone().unwrap();
        // Published: 86.95 min, 176.53 °F, predicted yield 80.21 (a maximum).
        close(a.actual(0, xs[0]), 86.946_301_88, 1e-9);
        close(a.actual(1, xs[1]), 176.529_288_73, 1e-9);
        close(a.canonical().stationary_value.unwrap(), 80.212_435_73, 1e-9);
        assert_eq!(a.canonical().kind, stat_doe::StationaryKind::Maximum);

        let s = a.sections(&SurfaceOptions::default());
        let kinds: Vec<SectionKind> = s.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::SurfaceSummary,
                SectionKind::EffectPareto,
                SectionKind::SurfaceCoefficients,
                SectionKind::StationaryPoint,
                SectionKind::CanonicalAnalysis,
                SectionKind::RidgeMaximum,
                SectionKind::RidgeMinimum,
                SectionKind::SurfaceContours
            ]
        );
        assert!(s[3].title.contains("Maximum"));
        let SectionContent::Table(t) = &s[3].content else { panic!() };
        assert_eq!(t.cell(0, "Actual"), Some("86.946"));
        let SectionContent::Table(t) = &s[5].content else { panic!() };
        assert_eq!(t.rows.len(), RIDGE_STEPS + 1);
        let SectionContent::Plot(p) = &s[7].content else { panic!() };
        assert_eq!(p.points.len(), 13);
        let notes = p.annotations.as_ref().unwrap();
        assert_eq!(notes.items.len(), 1, "stationary point marked");
        let drawing = p.contour.as_ref().expect("a contour map").drawing(false);
        assert!(!drawing.lines.is_empty() && !drawing.labels.is_empty() && !drawing.bands.is_empty());
        // Every level line lies on its level (to the map's linear interpolation).
        let span = drawing.levels.last().unwrap() - drawing.levels[0];
        for (level, lines) in &drawing.lines {
            for [x, y] in lines.iter().flatten() {
                let coded =
                    [(x - 85.0) / (5.0 * std::f64::consts::SQRT_2), (y - 175.0) / (5.0 * std::f64::consts::SQRT_2)];
                close(a.surface().value(&coded), *level, 0.02 * span);
            }
        }
        assert!(SurfaceOptions { show_ridge: false, show_coefficients: false, ..SurfaceOptions::default() }.eq(
            &SurfaceOptions {
                show_ridge: false,
                show_coefficients: false,
                contour_x: 0,
                contour_y: 1,
                show_profiler: false,
                show_surface_profiler: false,
                show_surface_observed: false
            }
        ));
        assert_eq!(
            a.sections(&SurfaceOptions { show_ridge: false, show_coefficients: false, ..SurfaceOptions::default() })
                .len(),
            5
        );
    }

    #[test]
    fn contour_plot_leaves_out_excluded_rows() {
        let table = yield_table();
        let mut rows = RowStateManager::new(table.n_rows());
        rows.set_excluded([12], true);
        let a = SurfaceAnalysis::run(&table, &rows, &SurfaceRequest { response: 2, factors: vec![0, 1] }).unwrap();
        let s = a.sections(&SurfaceOptions::default());
        let p = s.iter().find_map(|s| match (&s.kind, &s.content) {
            (SectionKind::SurfaceContours, SectionContent::Plot(p)) => Some(p),
            _ => None,
        });
        let p = p.unwrap();
        assert_eq!(p.points.len(), 12);
        assert!(p.points.iter().all(|pt| pt.row != 12));
    }

    #[test]
    fn contour_axes_fit_fewer_factors() {
        let three = SurfaceOptions { contour_x: 2, contour_y: 0, ..SurfaceOptions::default() };
        assert_eq!(three.for_factors(3), three, "valid axes are kept");
        let two = three.for_factors(2);
        assert_eq!((two.contour_x, two.contour_y), (0, 1), "an axis beyond the factors resets");
        let same = SurfaceOptions { contour_x: 1, contour_y: 1, ..SurfaceOptions::default() };
        assert_eq!((same.for_factors(3).contour_x, same.for_factors(3).contour_y), (0, 1));
    }

    #[test]
    fn rejects_bad_requests() {
        let table = yield_table();
        let rows = RowStateManager::new(table.n_rows());
        let run =
            |response, factors: Vec<usize>| SurfaceAnalysis::run(&table, &rows, &SurfaceRequest { response, factors });
        assert!(run(2, vec![]).is_err());
        assert!(run(2, vec![0, 0]).is_err());
        assert!(run(0, vec![0, 1]).is_err());
        let mut few = RowStateManager::new(table.n_rows());
        few.set_excluded(0..8, true);
        assert!(SurfaceAnalysis::run(&table, &few, &SurfaceRequest { response: 2, factors: vec![0, 1] }).is_err());
    }

    #[test]
    fn contours_of_a_cone_are_closed_circles() {
        // r² = u² + v² on [−1, 1]²: the 0.25 contour is the circle of radius 0.5.
        let n = 80;
        let coord = |i: usize| -1.0 + 2.0 * i as f64 / n as f64;
        let grid: Vec<Vec<f64>> =
            (0..=n).map(|j| (0..=n).map(|i| coord(i).powi(2) + coord(j).powi(2)).collect()).collect();
        let lines = contour_lines(&grid, 0.25, coord);
        assert_eq!(lines.len(), 1, "one joined loop");
        let l = &lines[0];
        assert_eq!(l.first(), l.last(), "closed");
        for p in l {
            assert!((p[0].hypot(p[1]) - 0.5).abs() < 2e-3);
        }
        // A level above the corners' value gives nothing.
        assert!(contour_lines(&grid, 3.0, coord).is_empty());
    }
}
