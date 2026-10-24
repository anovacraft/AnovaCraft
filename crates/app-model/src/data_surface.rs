//! Contour and surface plots of data (Visualize ▸ Contour and surface
//! plot…): a response column over two others, made into a surface through
//! the scattered observations ([`stat_viz::interpolate`]) by Delaunay
//! triangulation with linear interpolation, or by a thin-plate spline that
//! interpolates or smooths (smoothing chosen by generalized
//! cross-validation; Wahba 1990).
//!
//! The surface is evaluated on a regular grid once, when the report runs.
//! The report shows a table describing the fit, a contour plot of the grid
//! with the observations (brushable), and — drawn by front ends from
//! [`DataSurfaceAnalysis::mesh`] — a rotatable 3-D surface, filled or as a
//! wireframe, with the observations as points.
//!
//! Excluded rows are left out of the surface, as for analyses.

use crate::format::format_number;
use crate::project::{invalid, obj, s};
use crate::report::{ContourMap, PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use crate::scatter3d::{camera_from_json, camera_json};
use crate::surface_profiler::{Camera, ObservedPoint, SurfaceMesh, mesh_from_grid};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_viz::interpolate::{LinearSurface, ScatteredPoints, SplineSmoothing, ThinPlateSpline};

/// How the surface is made from the observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceFit {
    /// Delaunay triangulation, linear within each triangle; passes through
    /// the data and stops at their convex hull.
    Triangulation,
    /// Thin-plate spline through every observation.
    Spline,
    /// Thin-plate spline smoothed by generalized cross-validation.
    SmoothingSpline,
}

impl SurfaceFit {
    /// Every method, in menu order.
    pub const ALL: [Self; 3] = [Self::Triangulation, Self::Spline, Self::SmoothingSpline];

    /// Name shown in dialogs and reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Triangulation => "Triangulation (linear)",
            Self::Spline => "Thin-plate spline (through the data)",
            Self::SmoothingSpline => "Smoothing thin-plate spline (cross-validated)",
        }
    }

    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Triangulation => "triangulation",
            Self::Spline => "spline",
            Self::SmoothingSpline => "smoothing-spline",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Most grid divisions along each axis.
pub const MAX_GRID: usize = 120;
/// Fewest grid divisions along each axis.
pub const MIN_GRID: usize = 8;

/// Columns and method of a contour and surface plot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataSurfaceRequest {
    /// Column across.
    pub x: usize,
    /// Column in depth (the contour plot's vertical axis).
    pub y: usize,
    /// Response column.
    pub z: usize,
    /// How the surface is made.
    pub method: SurfaceFit,
    /// Grid divisions along each axis ([`MIN_GRID`] to [`MAX_GRID`]).
    pub grid: usize,
}

impl DataSurfaceRequest {
    /// A triangulated surface on a 40 × 40 grid.
    #[must_use]
    pub fn new(x: usize, y: usize, z: usize) -> Self {
        Self { x, y, z, method: SurfaceFit::Triangulation, grid: 40 }
    }

    /// Every column used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        vec![self.x, self.y, self.z]
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: impl Fn(usize) -> usize) -> Self {
        Self { x: f(self.x), y: f(self.y), z: f(self.z), ..*self }
    }
}

/// How the surface is drawn (saved with the project).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DataSurfaceOptions {
    /// About this many contour levels, at round values.
    pub levels: usize,
    /// The contour plot section.
    pub contour_plot: bool,
    /// The 3-D surface section.
    pub surface: bool,
    /// Filled (shaded) surface; a wireframe when `false`.
    pub filled: bool,
    /// Contour lines on the 3-D surface.
    pub surface_contours: bool,
    /// Contour map on the floor of the 3-D box.
    pub floor_map: bool,
    /// The observations in the 3-D view.
    pub points: bool,
    /// Viewing angle of the 3-D surface.
    pub camera: Camera,
}

impl Default for DataSurfaceOptions {
    fn default() -> Self {
        Self {
            levels: 10,
            contour_plot: true,
            surface: true,
            filled: true,
            surface_contours: true,
            floor_map: true,
            points: true,
            camera: Camera::default(),
        }
    }
}

/// The project-file form (`"analysis": "data-surface"`).
#[must_use]
pub fn data_surface_to_json(request: &DataSurfaceRequest, o: &DataSurfaceOptions, table: &DataTable) -> JsonValue {
    let name = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
    obj([
        ("analysis", s("data-surface")),
        ("x", name(request.x)),
        ("y", name(request.y)),
        ("z", name(request.z)),
        ("method", s(request.method.key())),
        ("grid", JsonValue::Number(request.grid as f64)),
        ("levels", JsonValue::Number(o.levels as f64)),
        ("contour_plot", JsonValue::Bool(o.contour_plot)),
        ("surface", JsonValue::Bool(o.surface)),
        ("filled", JsonValue::Bool(o.filled)),
        ("surface_contours", JsonValue::Bool(o.surface_contours)),
        ("floor_map", JsonValue::Bool(o.floor_map)),
        ("points", JsonValue::Bool(o.points)),
        ("camera", camera_json(o.camera)),
    ])
}

/// Read the project-file form.
///
/// # Errors
/// Missing or unknown columns, or an unknown method.
pub fn data_surface_from_json(v: &JsonValue, table: &DataTable) -> Result<(DataSurfaceRequest, DataSurfaceOptions)> {
    let column = |key: &str| -> Result<usize> {
        let name = v
            .get(key)
            .and_then(JsonValue::as_str)
            .ok_or_else(|| invalid(format!("contour and surface plot: {key:?} must name a column")))?;
        table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
    };
    let method = match v.get("method").and_then(JsonValue::as_str) {
        None => SurfaceFit::Triangulation,
        Some(k) => SurfaceFit::from_key(k).ok_or_else(|| invalid(format!("unknown surface method {k:?}")))?,
    };
    let d = DataSurfaceOptions::default();
    let flag = |key: &str, default: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(default);
    let count = |key: &str, default: usize| v.get(key).and_then(JsonValue::as_usize).unwrap_or(default);
    let request = DataSurfaceRequest {
        x: column("x")?,
        y: column("y")?,
        z: column("z")?,
        method,
        grid: count("grid", 40).clamp(MIN_GRID, MAX_GRID),
    };
    let options = DataSurfaceOptions {
        levels: count("levels", d.levels).min(40),
        contour_plot: flag("contour_plot", d.contour_plot),
        surface: flag("surface", d.surface),
        filled: flag("filled", d.filled),
        surface_contours: flag("surface_contours", d.surface_contours),
        floor_map: flag("floor_map", d.floor_map),
        points: flag("points", d.points),
        camera: camera_from_json(v.get("camera")),
    };
    Ok((request, options))
}

/// The fitted surface.
#[derive(Debug, Clone, PartialEq)]
enum Fitted {
    Linear(LinearSurface),
    Spline(Box<ThinPlateSpline>),
}

impl Fitted {
    fn value(&self, x: f64, y: f64) -> f64 {
        match self {
            Self::Linear(s) => s.value(x, y),
            Self::Spline(s) => s.value(x, y),
        }
    }
}

/// A surface made from the observations, on its grid.
#[derive(Debug, Clone, PartialEq)]
pub struct DataSurfaceAnalysis {
    request: DataSurfaceRequest,
    /// Names of the x, y and response columns.
    pub names: [String; 3],
    /// Used rows and their `(x, y, z)`.
    pub observations: Vec<(usize, [f64; 3])>,
    /// The surface at each used row's location.
    pub at_observations: Vec<f64>,
    /// Grid positions along x.
    pub xs: Vec<f64>,
    /// Grid positions along y.
    pub ys: Vec<f64>,
    /// `grid[j][i]`: the surface at `(xs[i], ys[j])` (NaN outside the data
    /// for a triangulation).
    pub grid: Vec<Vec<f64>>,
    fitted: Fitted,
    n_locations: usize,
    n_excluded: usize,
    n_missing: usize,
}

impl DataSurfaceAnalysis {
    /// Make the surface of `request` from the included rows of `table`.
    ///
    /// # Errors
    /// [`stat_core::StatError::InvalidArgument`] for columns that are not
    /// numeric or are repeated, fewer than three distinct locations,
    /// locations on one line, or more locations than a thin-plate spline
    /// allows.
    pub fn run(table: &DataTable, rows: &RowStateManager, request: &DataSurfaceRequest) -> Result<Self> {
        if request.x == request.y || request.x == request.z || request.y == request.z {
            return Err(invalid("choose three different columns"));
        }
        let numeric = |i: usize| -> Result<(&str, &[f64])> {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            Ok((c.name.as_str(), c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?))
        };
        let (xn, xv) = numeric(request.x)?;
        let (yn, yv) = numeric(request.y)?;
        let (zn, zv) = numeric(request.z)?;
        let (mut observations, mut n_excluded, mut n_missing) = (Vec::new(), 0, 0);
        for r in 0..table.n_rows() {
            let p = [xv[r], yv[r], zv[r]];
            if !p.iter().all(|v| v.is_finite()) {
                n_missing += 1;
            } else if rows.is_excluded(r) {
                n_excluded += 1;
            } else {
                observations.push((r, p));
            }
        }
        let col = |k: usize| observations.iter().map(|(_, p)| p[k]).collect::<Vec<f64>>();
        let points = ScatteredPoints::new(&col(0), &col(1), &col(2))?;
        let n_locations = points.len();
        let fitted = match request.method {
            SurfaceFit::Triangulation => Fitted::Linear(LinearSurface::new(points)?),
            SurfaceFit::Spline => Fitted::Spline(Box::new(ThinPlateSpline::fit(points, SplineSmoothing::Interpolate)?)),
            SurfaceFit::SmoothingSpline => {
                if n_locations <= 3 {
                    return Err(invalid("a smoothing spline needs more than three distinct locations"));
                }
                Fitted::Spline(Box::new(ThinPlateSpline::fit(points, SplineSmoothing::CrossValidated)?))
            }
        };
        let n = request.grid.clamp(MIN_GRID, MAX_GRID);
        let axis = |(lo, hi): (f64, f64)| (0..=n).map(|k| lo + (hi - lo) * k as f64 / n as f64).collect::<Vec<_>>();
        let (x_range, y_range) = match &fitted {
            Fitted::Linear(s) => (s.points.x_range, s.points.y_range),
            Fitted::Spline(s) => (s.points.x_range, s.points.y_range),
        };
        let (xs, ys) = (axis(x_range), axis(y_range));
        let grid = match &fitted {
            Fitted::Linear(s) => s.grid(&xs, &ys),
            Fitted::Spline(s) => s.grid(&xs, &ys),
        };
        let at_observations = observations.iter().map(|(_, p)| fitted.value(p[0], p[1])).collect();
        Ok(Self {
            request: *request,
            names: [xn.to_owned(), yn.to_owned(), zn.to_owned()],
            observations,
            at_observations,
            xs,
            ys,
            grid,
            fitted,
            n_locations,
            n_excluded,
            n_missing,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &DataSurfaceRequest {
        &self.request
    }

    /// Report heading.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Contour and Surface: {} over {} and {}", self.names[2], self.names[0], self.names[1])
    }

    /// The surface at `(x, y)` (NaN outside the data for a triangulation).
    #[must_use]
    pub fn value(&self, x: f64, y: f64) -> f64 {
        self.fitted.value(x, y)
    }

    /// `true` when the surface passes through every observation (a
    /// triangulation or an interpolating spline; rows averaged at a shared
    /// location aside).
    #[must_use]
    pub fn interpolates(&self) -> bool {
        self.request.method != SurfaceFit::SmoothingSpline
    }

    /// The grid as a 3-D mesh with about `levels` contour levels and the
    /// observations as points.
    ///
    /// # Errors
    /// When the grid has no finite value (not expected for a fitted surface).
    pub fn mesh(&self, levels: usize) -> Result<SurfaceMesh> {
        let names = [self.names[2].as_str(), self.names[0].as_str(), self.names[1].as_str()];
        let mut mesh = mesh_from_grid(names, self.xs.clone(), self.ys.clone(), self.grid.clone(), levels)?;
        mesh.points = self
            .observations
            .iter()
            .zip(&self.at_observations)
            .map(|(&(row, p), &surface)| ObservedPoint {
                row,
                x: p[0],
                y: p[1],
                observed: p[2],
                surface: if surface.is_finite() { surface } else { p[2] },
                fitted: if surface.is_finite() { surface } else { p[2] },
            })
            .collect();
        for p in &mesh.points {
            mesh.z_range.0 = mesh.z_range.0.min(p.observed);
            mesh.z_range.1 = mesh.z_range.1.max(p.observed);
        }
        Ok(mesh)
    }

    /// The report sections: how the surface was made, and the contour plot
    /// when `options` show it.
    #[must_use]
    pub fn sections(&self, options: &DataSurfaceOptions) -> Vec<ReportSection> {
        let mut out = vec![self.fit_table()];
        if options.contour_plot {
            out.push(self.contour_plot(options.levels));
        }
        out
    }

    fn fit_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Quantity", "Value"]);
        let mut row = |k: &str, v: String| t.push([k.to_owned(), v]);
        row("Method", self.request.method.label().to_owned());
        row("Rows used", self.observations.len().to_string());
        if self.n_locations < self.observations.len() {
            row("Distinct locations (rows at one location averaged)", self.n_locations.to_string());
        }
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            row("Rows with a missing value", self.n_missing.to_string());
        }
        row("Grid", format!("{0} × {0} divisions", self.xs.len() - 1));
        match &self.fitted {
            Fitted::Linear(s) => {
                row("Triangles", s.triangles.len().to_string());
                let cells = self.grid.iter().flatten().count();
                let inside = self.grid.iter().flatten().filter(|v| v.is_finite()).count();
                row(
                    "Grid inside the data's convex hull",
                    format!("{}%", format_number(100.0 * inside as f64 / cells as f64, 3)),
                );
            }
            Fitted::Spline(s) => {
                if self.request.method == SurfaceFit::SmoothingSpline {
                    row("Smoothing parameter λ (coordinates scaled to [0, 1])", format_number(s.lambda, 5));
                    row("Effective degrees of freedom", format_number(s.effective_df, 5));
                    if let Some(g) = s.gcv {
                        row("Generalized cross-validation score", format_number(g, 5));
                    }
                    // The spline is fitted to the location means, so its
                    // residuals, like λ, the effective df and GCV, are those
                    // of the distinct locations (Wahba 1990, ch. 4).
                    let n = s.points.len() as f64;
                    let rss = s.residual_ss();
                    let at = if self.n_locations < self.observations.len() { " (distinct locations)" } else { "" };
                    row(&format!("Residual sum of squares{at}"), format_number(rss, 5));
                    if n > s.effective_df {
                        row(
                            &format!("Residual standard deviation{at}"),
                            format_number((rss / (n - s.effective_df)).sqrt(), 5),
                        );
                    }
                }
            }
        }
        let (lo, hi) = self.z_span();
        row("Surface range", format!("{} to {}", format_number(lo, 5), format_number(hi, 5)));
        ReportSection {
            kind: SectionKind::DataSurfaceFit,
            title: "Surface Fit".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn z_span(&self) -> (f64, f64) {
        self.grid
            .iter()
            .flatten()
            .filter(|v| v.is_finite())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)))
    }

    /// Contour lines of the grid with each level's value written on its
    /// longest line, and the observations as brushable points.
    fn contour_plot(&self, levels: usize) -> ReportSection {
        let (lo, hi) = self.z_span();
        let map = ContourMap::new(
            stat_viz::ContourField::from_grid(&self.xs, &self.ys, &self.grid),
            levels,
            self.names[2].clone(),
        );
        let (_, step) = map.levels();
        let spec = PlotSpec {
            x_label: self.names[0].clone(),
            y_label: self.names[1].clone(),
            points: self.observations.iter().map(|&(row, p)| RowPoint { row, x: p[0], y: p[1] }).collect(),
            point_name: Some("Observations".to_owned()),
            contour: Some(map),
            ..PlotSpec::default()
        };
        let title = if levels > 0 && hi > lo {
            format!("Contour Plot of {} (contours every {})", self.names[2], format_number(step, 6))
        } else {
            format!("Contour Plot of {}", self.names[2])
        };
        ReportSection { kind: SectionKind::DataSurfaceContours, title, content: SectionContent::Plot(Box::new(spec)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// A plane z = 2x − y + 1 on a 5 × 4 grid plus one row off the grid.
    fn plane() -> DataTable {
        let (mut x, mut y) = (Vec::new(), Vec::new());
        for j in 0..4 {
            for i in 0..5 {
                x.push(f64::from(i));
                y.push(f64::from(j) * 10.0);
            }
        }
        x.push(2.5);
        y.push(15.0);
        let z = x.iter().zip(&y).map(|(a, b)| 2.0 * a - 0.1 * b + 1.0).collect();
        DataTable::new("plane", vec![Column::numeric("x", x), Column::numeric("y", y), Column::numeric("z", z)])
            .unwrap()
    }

    #[test]
    fn every_method_reproduces_a_plane() {
        let t = plane();
        let rows = RowStateManager::new(t.n_rows());
        for method in SurfaceFit::ALL {
            let request = DataSurfaceRequest { method, grid: 10, ..DataSurfaceRequest::new(0, 1, 2) };
            let a = DataSurfaceAnalysis::run(&t, &rows, &request).unwrap();
            for (j, &v) in a.ys.iter().enumerate() {
                for (i, &u) in a.xs.iter().enumerate() {
                    let want = 2.0 * u - 0.1 * v + 1.0;
                    assert!((a.grid[j][i] - want).abs() < 1e-7, "{method:?} at ({u}, {v}): {}", a.grid[j][i]);
                }
            }
            let mesh = a.mesh(6).unwrap();
            assert_eq!(mesh.points.len(), 21);
            assert!(!mesh.levels.is_empty());
            let s = a.sections(&DataSurfaceOptions::default());
            assert_eq!(s.len(), 2);
            let SectionContent::Plot(p) = &s[1].content else { panic!() };
            assert_eq!(p.points.len(), 21);
            assert!(p.contour.as_ref().is_some_and(|m| !m.drawing(false).bands.is_empty()));
        }
    }

    #[test]
    fn triangulation_leaves_holes_outside_the_hull() {
        // Points on a triangle: the grid's far corner is outside.
        let t = DataTable::new(
            "triangle",
            vec![
                Column::numeric("x", vec![0.0, 1.0, 0.0, 0.3]),
                Column::numeric("y", vec![0.0, 0.0, 1.0, 0.3]),
                Column::numeric("z", vec![1.0, 2.0, 3.0, 1.0]),
            ],
        )
        .unwrap();
        let rows = RowStateManager::new(4);
        let a =
            DataSurfaceAnalysis::run(&t, &rows, &DataSurfaceRequest { grid: 10, ..DataSurfaceRequest::new(0, 1, 2) })
                .unwrap();
        assert!(a.grid[10][10].is_nan());
        assert!((a.grid[0][0] - 1.0).abs() < 1e-12);
        let mesh = a.mesh(4).unwrap();
        // Cells with a missing corner are not drawn.
        let drawn = mesh.cells_with_depth(&Camera::default()).len();
        assert!((40..100).contains(&drawn), "{drawn}");
    }

    #[test]
    fn excluded_rows_are_left_out_and_errors_are_clear() {
        let t = plane();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([20], true);
        let a = DataSurfaceAnalysis::run(&t, &rows, &DataSurfaceRequest::new(0, 1, 2)).unwrap();
        assert_eq!(a.observations.len(), 20);
        let SectionContent::Table(tab) = &a.sections(&DataSurfaceOptions::default())[0].content else { panic!() };
        assert!(tab.rows.iter().any(|r| r[0] == "Rows excluded" && r[1] == "1"));
        assert!(DataSurfaceAnalysis::run(&t, &rows, &DataSurfaceRequest::new(0, 0, 2)).is_err());
    }

    #[test]
    fn smoothing_spline_reports_its_fit() {
        let mut t = plane();
        let z: Vec<f64> = t.column(2).unwrap().as_numeric().unwrap().to_vec();
        let noisy: Vec<f64> = z.iter().enumerate().map(|(i, v)| v + 0.3 * ((i * 7 % 5) as f64 - 2.0)).collect();
        t.insert_column(3, Column::numeric("noisy", noisy)).unwrap();
        let rows = RowStateManager::new(t.n_rows());
        let request = DataSurfaceRequest { method: SurfaceFit::SmoothingSpline, ..DataSurfaceRequest::new(0, 1, 3) };
        let a = DataSurfaceAnalysis::run(&t, &rows, &request).unwrap();
        assert!(!a.interpolates());
        let SectionContent::Table(tab) = &a.sections(&DataSurfaceOptions::default())[0].content else { panic!() };
        for key in ["Effective degrees of freedom", "Generalized cross-validation score", "Residual sum of squares"] {
            assert!(tab.rows.iter().any(|r| r[0] == key), "{key}");
        }
        // A repeated location is averaged first, so the residual figures
        // say they are over the distinct locations.
        let col = |j: usize| t.column(j).unwrap().as_numeric().unwrap().to_vec();
        let (mut x, mut y, mut z) = (col(0), col(1), col(3));
        x.push(x[0]);
        y.push(y[0]);
        z.push(z[0] + 0.5);
        let t2 =
            DataTable::new("repeat", vec![Column::numeric("x", x), Column::numeric("y", y), Column::numeric("z", z)])
                .unwrap();
        let request = DataSurfaceRequest { method: SurfaceFit::SmoothingSpline, ..DataSurfaceRequest::new(0, 1, 2) };
        let a = DataSurfaceAnalysis::run(&t2, &RowStateManager::new(t2.n_rows()), &request).unwrap();
        let SectionContent::Table(tab) = &a.sections(&DataSurfaceOptions::default())[0].content else { panic!() };
        for key in [
            "Distinct locations (rows at one location averaged)",
            "Residual sum of squares (distinct locations)",
            "Residual standard deviation (distinct locations)",
        ] {
            assert!(tab.rows.iter().any(|r| r[0] == key), "{key}: {tab:?}");
        }
    }

    #[test]
    fn json_round_trip() {
        let t = plane();
        let request =
            DataSurfaceRequest { method: SurfaceFit::SmoothingSpline, grid: 25, ..DataSurfaceRequest::new(1, 0, 2) };
        let options = DataSurfaceOptions {
            levels: 7,
            contour_plot: false,
            filled: false,
            camera: Camera { azimuth: 10.0, elevation: 60.0 },
            ..DataSurfaceOptions::default()
        };
        let v = data_surface_to_json(&request, &options, &t);
        let (r2, o2) = data_surface_from_json(&v, &t).unwrap();
        assert_eq!(r2, request);
        assert_eq!(o2, options);
    }
}
