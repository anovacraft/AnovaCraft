//! Profilers of fitted models: the Response Explorer (one slice panel per
//! factor, [`profile`]) and the 3-D surface profiler, which shows the
//! predicted response of a fitted model over a
//! grid of two continuous factors, with every other factor held at a setting,
//! drawn as a shaded surface with contour lines on it and a contour map on the
//! floor of the box.
//!
//! This module holds the GUI-independent parts: the [`SurfaceModel`] interface
//! that fitted models implement, the grid and its contours ([`surface_mesh`]),
//! and the orthographic [`Camera`] that maps the unit cube to the screen and
//! orders the grid cells back to front for painting (the painter's algorithm).
//! Contours inside each cell are found by marching squares, with the two
//! ambiguous cases resolved by the cell's mean value, so each cell can be
//! painted together with its own contour segments and hidden parts stay hidden.

use crate::axis_settings::nice_step;
use crate::linear_analysis::{Profile, ProfilePanel};
use crate::report::Polyline;
use crate::surface_analysis::contour_lines;
use stat_core::{Result, StatError};
use stat_linear::TermValue;

/// How a factor of a [`SurfaceModel`] can be set.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileFactorKind {
    /// A number in the data's range.
    Continuous {
        /// Smallest and largest value in the data used.
        range: (f64, f64),
    },
    /// One of the levels.
    Categorical {
        /// Level labels.
        levels: Vec<String>,
    },
}

/// One factor of a [`SurfaceModel`].
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileFactor {
    /// Name.
    pub name: String,
    /// How it is set.
    pub kind: ProfileFactorKind,
}

impl ProfileFactor {
    /// `true` for a continuous factor.
    #[must_use]
    pub fn is_continuous(&self) -> bool {
        matches!(self.kind, ProfileFactorKind::Continuous { .. })
    }
}

/// A prediction with confidence limits for the mean (`None` when the model
/// gives none).
pub type Limited = (f64, Option<(f64, f64)>);

/// One observation used in a fit: its row, its setting of every factor, and
/// its response.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// Dataset row.
    pub row: usize,
    /// The row's value of each factor.
    pub settings: Vec<TermValue>,
    /// Observed response (0/1 for a binomial response).
    pub response: f64,
}

/// An observation placed in the 3-D profiler.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObservedPoint {
    /// Dataset row.
    pub row: usize,
    /// Value of the across factor.
    pub x: f64,
    /// Value of the depth factor.
    pub y: f64,
    /// Observed response.
    pub observed: f64,
    /// The drawn surface's height at this point's `(x, y)` (the other factors
    /// at the held settings): what the eye compares the point with.
    pub surface: f64,
    /// The model's fitted value for this row at the row's own values of every
    /// factor (its residual is `observed − fitted`); equal to `surface` when
    /// the held settings match the row's.
    pub fitted: f64,
}

impl ObservedPoint {
    /// `true` when the point lies above the drawn surface.
    #[must_use]
    pub fn above_surface(&self) -> bool {
        self.observed >= self.surface
    }
}

/// A fitted model whose predictions can be profiled.
pub trait SurfaceModel {
    /// Name of the predicted response.
    fn response_name(&self) -> &str;
    /// The factors, in the order of a settings vector.
    fn profile_factors(&self) -> Vec<ProfileFactor>;
    /// Starting settings (one value per factor).
    fn default_profile_settings(&self) -> Vec<TermValue>;
    /// Predicted response at each settings vector.
    ///
    /// # Errors
    /// When a settings vector does not match the factors.
    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>>;

    /// Predicted response with confidence limits for the mean, when the model
    /// provides them (the default gives none).
    ///
    /// # Errors
    /// When a settings vector does not match the factors.
    fn predict_with_limits(&self, points: &[Vec<TermValue>]) -> Result<Vec<Limited>> {
        Ok(self.predict_settings(points)?.into_iter().map(|p| (p, None)).collect())
    }

    /// Confidence level of the limits, if any.
    fn confidence_level(&self) -> Option<f64> {
        None
    }

    /// The settings after factor `j` is set to `value`. By default only that
    /// factor changes; a mixture rescales the other components so the blend
    /// still sums to one.
    fn with_setting(&self, settings: &[TermValue], j: usize, value: TermValue) -> Vec<TermValue> {
        let mut s = settings.to_vec();
        s[j] = value;
        s
    }

    /// `false` when factors cannot be varied independently (mixtures), so a
    /// surface over two of them with the rest fixed is not meaningful.
    fn independent_factors(&self) -> bool {
        true
    }

    /// A note shown with the profilers (e.g. why there are no bands).
    fn profile_note(&self) -> Option<String> {
        None
    }

    /// The observations the model was fitted to (none by default).
    fn observations(&self) -> Vec<Observation> {
        Vec::new()
    }
}

/// Which profilers a model report shows (saved with the project).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfilerOptions {
    /// The Response Explorer.
    pub show_profiler: bool,
    /// The 3-D surface profiler.
    pub show_surface_profiler: bool,
    /// Observed data in the 3-D profiler.
    pub show_observed: bool,
}

impl Default for ProfilerOptions {
    fn default() -> Self {
        Self { show_profiler: true, show_surface_profiler: false, show_observed: false }
    }
}

/// Points along each continuous panel.
pub const CURVE_POINTS: usize = 64;

/// The Response Explorer of `model` at `settings`: for each factor, the
/// prediction (and its confidence band, when the model gives limits) as that
/// factor moves through its range or levels by [`SurfaceModel::with_setting`],
/// the others staying at their settings.
///
/// # Errors
/// [`StatError::InvalidArgument`] if `settings` do not fit the model;
/// prediction errors.
pub fn profile(model: &dyn SurfaceModel, settings: &[TermValue]) -> Result<Profile> {
    if !settings_fit(model, settings) {
        return Err(invalid("the settings do not match the model's factors"));
    }
    let (predicted, current_limits) = model.predict_with_limits(&[settings.to_vec()])?[0];
    let (mut y_lo, mut y_hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut widen = |p: f64, l: Option<(f64, f64)>| {
        for v in [p, l.map_or(p, |l| l.0), l.map_or(p, |l| l.1)] {
            if v.is_finite() {
                y_lo = y_lo.min(v);
                y_hi = y_hi.max(v);
            }
        }
    };
    widen(predicted, current_limits);
    let mut panels = Vec::new();
    for (j, factor) in model.profile_factors().into_iter().enumerate() {
        match factor.kind {
            ProfileFactorKind::Continuous { range: (lo, hi) } => {
                let n = if hi > lo { CURVE_POINTS } else { 1 };
                let xs: Vec<f64> = (0..n).map(|i| lo + (hi - lo) * i as f64 / (n.max(2) - 1) as f64).collect();
                let points: Vec<Vec<TermValue>> =
                    xs.iter().map(|&x| model.with_setting(settings, j, TermValue::Number(x))).collect();
                let pred = model.predict_with_limits(&points)?;
                for &(p, l) in &pred {
                    widen(p, l);
                }
                let curve: Polyline = xs.iter().zip(&pred).map(|(&x, p)| [x, p.0]).collect();
                let band = pred.iter().all(|p| p.1.is_some()).then(|| {
                    let lower = xs.iter().zip(&pred).map(|(&x, p)| [x, p.1.map_or(p.0, |l| l.0)]).collect();
                    let upper = xs.iter().zip(&pred).map(|(&x, p)| [x, p.1.map_or(p.0, |l| l.1)]).collect();
                    (lower, upper)
                });
                let TermValue::Number(value) = settings[j] else { unreachable!("checked by settings_fit") };
                panels.push(ProfilePanel::Continuous { name: factor.name, curve, band, range: (lo, hi), value });
            }
            ProfileFactorKind::Categorical { levels } => {
                let points: Vec<Vec<TermValue>> =
                    (0..levels.len()).map(|l| model.with_setting(settings, j, TermValue::Level(l))).collect();
                let predictions = model.predict_with_limits(&points)?;
                for &(p, l) in &predictions {
                    widen(p, l);
                }
                let TermValue::Level(selected) = settings[j] else { unreachable!("checked by settings_fit") };
                panels.push(ProfilePanel::Categorical { name: factor.name, levels, predictions, selected });
            }
        }
    }
    if !(y_lo.is_finite() && y_hi.is_finite()) {
        return Err(invalid("the model gives no finite predictions at these settings"));
    }
    let pad = if y_hi > y_lo { 0.05 * (y_hi - y_lo) } else { 1.0 };
    Ok(Profile {
        response: model.response_name().to_owned(),
        panels,
        predicted,
        confidence_interval: current_limits,
        confidence_level: model.confidence_level(),
        y_range: (y_lo - pad, y_hi + pad),
        note: model.profile_note(),
    })
}

/// `true` if `settings` has one value of the right kind (and in range, for
/// levels) per factor of `model`.
#[must_use]
pub fn settings_fit(model: &dyn SurfaceModel, settings: &[TermValue]) -> bool {
    let factors = model.profile_factors();
    settings.len() == factors.len()
        && factors.iter().zip(settings).all(|(f, v)| match (&f.kind, v) {
            (ProfileFactorKind::Continuous { .. }, TermValue::Number(x)) => x.is_finite(),
            (ProfileFactorKind::Categorical { levels }, TermValue::Level(l)) => *l < levels.len(),
            _ => false,
        })
}

/// A contour segment inside one grid cell: level index and two end points
/// `(x, y, level)` in data units.
pub type CellSegment = (usize, [f64; 3], [f64; 3]);

/// The predicted response over a grid of two factors.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceMesh {
    /// Response name.
    pub response: String,
    /// Factor on the x axis.
    pub x_name: String,
    /// Factor on the y axis.
    pub y_name: String,
    /// Grid values of the x factor.
    pub xs: Vec<f64>,
    /// Grid values of the y factor.
    pub ys: Vec<f64>,
    /// `z[j][i]`: prediction at `(xs[i], ys[j])`.
    pub z: Vec<Vec<f64>>,
    /// Smallest and largest prediction on the grid.
    pub z_range: (f64, f64),
    /// Contour levels (evenly spaced at a round step).
    pub levels: Vec<f64>,
    /// Contour segments of each cell, indexed `j·(nx − 1) + i` for the cell
    /// with lower-left corner `(xs[i], ys[j])`.
    pub cell_contours: Vec<Vec<CellSegment>>,
    /// Contour lines over the whole grid as `(level index, line in (x, y))`,
    /// for the floor map.
    pub floor_contours: Vec<(usize, Polyline)>,
    /// The current setting of the two factors and its prediction.
    pub current: [f64; 3],
    /// Observations shown with the surface ([`add_observations`]); empty
    /// unless added.
    pub points: Vec<ObservedPoint>,
}

/// Most grid divisions per axis.
pub const MAX_RESOLUTION: usize = 80;
/// Fewest grid divisions per axis.
pub const MIN_RESOLUTION: usize = 4;

/// Compute the surface of `model` over factors `x` and `y` (indices into its
/// factors, both continuous) on a `resolution × resolution` grid spanning the
/// data, holding the other factors at `settings`, with about `n_levels`
/// contour levels (0 for none).
///
/// # Errors
/// [`StatError::InvalidArgument`] if `x` and `y` are equal, out of range or
/// not continuous, or `settings` do not fit the model; prediction errors.
pub fn surface_mesh(
    model: &dyn SurfaceModel,
    settings: &[TermValue],
    x: usize,
    y: usize,
    resolution: usize,
    n_levels: usize,
) -> Result<SurfaceMesh> {
    let factors = model.profile_factors();
    let range = |k: usize| match factors.get(k).map(|f| &f.kind) {
        Some(ProfileFactorKind::Continuous { range }) => Ok(*range),
        _ => Err(invalid("the surface axes must be two continuous factors")),
    };
    if x == y {
        return Err(invalid("choose two different factors for the surface axes"));
    }
    if !model.independent_factors() {
        return Err(invalid("this model's factors cannot be varied independently (a mixture)"));
    }
    let (x_range, y_range) = (range(x)?, range(y)?);
    if !settings_fit(model, settings) {
        return Err(invalid("the settings do not match the model's factors"));
    }
    let n = resolution.clamp(MIN_RESOLUTION, MAX_RESOLUTION);
    let axis = |(lo, hi): (f64, f64)| -> Vec<f64> {
        let (lo, hi) = if hi > lo { (lo, hi) } else { (lo - 0.5, lo + 0.5) };
        (0..=n).map(|k| lo + (hi - lo) * k as f64 / n as f64).collect()
    };
    let (xs, ys) = (axis(x_range), axis(y_range));
    let point = |u: f64, v: f64| {
        let mut s = settings.to_vec();
        s[x] = TermValue::Number(u);
        s[y] = TermValue::Number(v);
        s
    };
    let points: Vec<Vec<TermValue>> = ys.iter().flat_map(|&v| xs.iter().map(move |&u| point(u, v))).collect();
    let flat = model.predict_settings(&points)?;
    let z: Vec<Vec<f64>> = flat.chunks(xs.len()).map(<[f64]>::to_vec).collect();
    let at = |k: usize| match settings[k] {
        TermValue::Number(v) => v,
        TermValue::Level(_) => f64::NAN,
    };
    let (cx, cy) = (at(x), at(y));
    let current = [cx, cy, model.predict_settings(&[point(cx, cy)])?[0]];
    let names = [model.response_name(), factors[x].name.as_str(), factors[y].name.as_str()];
    let mut mesh = mesh_from_grid(names, xs, ys, z, n_levels)
        .map_err(|_| invalid("the model gives no finite predictions over this grid"))?;
    mesh.current = current;
    Ok(mesh)
}

/// A mesh of the values `z[j][i]` at `(xs[i], ys[j])` (at least two of
/// each, increasing), named `[response, x, y]`, with about `n_levels`
/// contour levels (0 for none). NaN values (outside the data, for an
/// interpolated surface) leave holes: cells with a NaN corner are neither
/// drawn nor contoured. The mesh has no marked point (`current` is NaN) and
/// no observations.
///
/// # Errors
/// [`StatError::InvalidArgument`] if the grid is too small, its sizes do not
/// match, or no value is finite.
pub fn mesh_from_grid(
    names: [&str; 3],
    xs: Vec<f64>,
    ys: Vec<f64>,
    z: Vec<Vec<f64>>,
    n_levels: usize,
) -> Result<SurfaceMesh> {
    if xs.len() < 2 || ys.len() < 2 || z.len() != ys.len() || z.iter().any(|r| r.len() != xs.len()) {
        return Err(invalid("a surface needs a grid of at least 2 × 2 values"));
    }
    let lo = z.iter().flatten().copied().filter(|v| v.is_finite()).fold(f64::INFINITY, f64::min);
    let hi = z.iter().flatten().copied().filter(|v| v.is_finite()).fold(f64::NEG_INFINITY, f64::max);
    if !(lo.is_finite() && hi.is_finite()) {
        return Err(invalid("the surface has no finite values over this grid"));
    }
    let levels = contour_levels(lo, hi, n_levels);
    let (nx, ny) = (xs.len() - 1, ys.len() - 1);
    let mut cell_contours = Vec::with_capacity(nx * ny);
    for j in 0..ny {
        for i in 0..nx {
            let corners = [
                [xs[i], ys[j], z[j][i]],
                [xs[i + 1], ys[j], z[j][i + 1]],
                [xs[i + 1], ys[j + 1], z[j + 1][i + 1]],
                [xs[i], ys[j + 1], z[j + 1][i]],
            ];
            let mut segments = Vec::new();
            if corners.iter().all(|c| c[2].is_finite()) {
                for (k, &level) in levels.iter().enumerate() {
                    for (a, b) in cell_crossings(&corners, level) {
                        segments.push((k, [a[0], a[1], level], [b[0], b[1], level]));
                    }
                }
            }
            cell_contours.push(segments);
        }
    }
    let mut floor_contours = Vec::new();
    for (k, &level) in levels.iter().enumerate() {
        for line in contour_lines(&z, level, |i| i as f64) {
            let along = |v: f64, axis: &[f64]| {
                let i = (v.floor() as usize).min(axis.len() - 2);
                axis[i] + (v - i as f64) * (axis[i + 1] - axis[i])
            };
            floor_contours.push((k, line.iter().map(|&[u, v]| [along(u, &xs), along(v, &ys)]).collect()));
        }
    }
    Ok(SurfaceMesh {
        response: names[0].to_owned(),
        x_name: names[1].to_owned(),
        y_name: names[2].to_owned(),
        xs,
        ys,
        z,
        z_range: (lo, hi),
        levels,
        cell_contours,
        floor_contours,
        current: [f64::NAN; 3],
        points: Vec::new(),
    })
}

/// Add `model`'s observations to `mesh` (drawn over factors `x` and `y` with
/// the other factors at `settings`), each with the surface height at its
/// position and its own fitted value, and widen the vertical range to include
/// them. Rows outside the grid's ranges are left out (they cannot occur for
/// data the model was fitted to).
///
/// # Errors
/// Prediction errors.
pub fn add_observations(
    mesh: &mut SurfaceMesh,
    model: &dyn SurfaceModel,
    settings: &[TermValue],
    x: usize,
    y: usize,
) -> Result<()> {
    let obs = model.observations();
    let (fitted, surface) = if obs.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        let own: Vec<Vec<TermValue>> = obs.iter().map(|o| o.settings.clone()).collect();
        let at_surface: Vec<Vec<TermValue>> = obs
            .iter()
            .map(|o| {
                let mut s = settings.to_vec();
                if let (Some(vx), Some(vy)) = (o.settings.get(x), o.settings.get(y)) {
                    s[x] = *vx;
                    s[y] = *vy;
                }
                s
            })
            .collect();
        (model.predict_settings(&own)?, model.predict_settings(&at_surface)?)
    };
    let number = |v: &TermValue| if let TermValue::Number(n) = v { *n } else { f64::NAN };
    let (x0, x1) = (mesh.xs[0], mesh.xs[mesh.xs.len() - 1]);
    let (y0, y1) = (mesh.ys[0], mesh.ys[mesh.ys.len() - 1]);
    let tolerance = |lo: f64, hi: f64| 1e-9 * (hi - lo).abs().max(1.0);
    mesh.points =
        obs.iter()
            .zip(fitted.into_iter().zip(surface))
            .filter_map(|(o, (fitted, surface))| {
                let (px, py) = (number(o.settings.get(x)?), number(o.settings.get(y)?));
                let inside = px >= x0 - tolerance(x0, x1)
                    && px <= x1 + tolerance(x0, x1)
                    && py >= y0 - tolerance(y0, y1)
                    && py <= y1 + tolerance(y0, y1);
                (inside && o.response.is_finite() && fitted.is_finite() && surface.is_finite())
                    .then_some(ObservedPoint { row: o.row, x: px, y: py, observed: o.response, surface, fitted })
            })
            .collect();
    for p in &mesh.points {
        mesh.z_range.0 = mesh.z_range.0.min(p.observed);
        mesh.z_range.1 = mesh.z_range.1.max(p.observed);
    }
    Ok(())
}

/// About `n` round contour levels strictly inside `(lo, hi)`.
fn contour_levels(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    if n == 0 || hi <= lo {
        return Vec::new();
    }
    let step = nice_step((hi - lo) / n as f64);
    let mut out = Vec::new();
    let mut k = (lo / step).floor() + 1.0;
    while k * step < hi && out.len() < 4 * n + 4 {
        out.push(k * step);
        k += 1.0;
    }
    out
}

/// Contour segments of `level` inside one cell with corners (counter-clockwise
/// from lower left) `[x, y, value]`, by marching squares.
fn cell_crossings(corners: &[[f64; 3]; 4], level: f64) -> Vec<([f64; 2], [f64; 2])> {
    let above: Vec<bool> = corners.iter().map(|c| c[2] >= level).collect();
    // Edge e joins corner e and corner e + 1.
    let crossing = |e: usize| -> Option<[f64; 2]> {
        let (a, b) = (corners[e], corners[(e + 1) % 4]);
        if above[e] == above[(e + 1) % 4] {
            return None;
        }
        let t = ((level - a[2]) / (b[2] - a[2])).clamp(0.0, 1.0);
        Some([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])])
    };
    let edges: Vec<(usize, [f64; 2])> = (0..4).filter_map(|e| crossing(e).map(|p| (e, p))).collect();
    match edges.as_slice() {
        [(_, a), (_, b)] => vec![(*a, *b)],
        [_, _, _, _] => {
            // Saddle: cut off each corner whose side differs from the centre's.
            let centre = corners.iter().map(|c| c[2]).sum::<f64>() / 4.0 >= level;
            let at = |e: usize| edges.iter().find(|(k, _)| *k == e).map(|(_, p)| *p);
            (0..4).filter(|&c| above[c] != centre).filter_map(|c| Some((at((c + 3) % 4)?, at(c)?))).collect()
        }
        _ => Vec::new(),
    }
}

/// An orthographic view of the unit cube `[−1, 1]³` (x, y factors across, the
/// response up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Rotation about the vertical axis, in degrees.
    pub azimuth: f64,
    /// Angle above the horizontal, in degrees, in (−180, 180]: 0 = side view,
    /// 90 = from above, −90 = from below; beyond ±90 the view is upside down.
    pub elevation: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self { azimuth: -35.0, elevation: 30.0 }
    }
}

impl Camera {
    /// Straight down: the floor contour map seen flat, x across and y up.
    pub const TOP: Self = Self { azimuth: 0.0, elevation: 90.0 };

    /// Turn by `d_azimuth` and tilt by `d_elevation` degrees; both wrap all
    /// the way round. A positive turn moves the side nearest the viewer to the
    /// right on screen, including when the view is upside down (the turn is
    /// reversed then), so the surface follows a horizontal drag.
    pub fn rotate(&mut self, d_azimuth: f64, d_elevation: f64) {
        let wrap = |v: f64| {
            let w = (v + 180.0).rem_euclid(360.0) - 180.0;
            if w == -180.0 { 180.0 } else { w }
        };
        let upside_down = self.elevation.to_radians().cos() < 0.0;
        self.azimuth = wrap(self.azimuth + if upside_down { -d_azimuth } else { d_azimuth });
        self.elevation = wrap(self.elevation + d_elevation);
    }

    /// `true` when the viewer is above the floor of the box (it is then
    /// behind everything else).
    #[must_use]
    pub fn sees_floor_from_above(&self) -> bool {
        self.elevation.to_radians().sin() >= 0.0
    }

    /// Screen position `(right, up)` and depth (larger is nearer the viewer) of
    /// a point of the unit cube.
    #[must_use]
    pub fn project(&self, p: [f64; 3]) -> [f64; 3] {
        let (sa, ca) = self.azimuth.to_radians().sin_cos();
        let (se, ce) = self.elevation.to_radians().sin_cos();
        let right = p[0] * ca - p[1] * sa;
        let away = p[0] * sa + p[1] * ca;
        [right, p[2] * ce + away * se, p[2] * se - away * ce]
    }
}

impl SurfaceMesh {
    /// A data point `(x, y, z)` in the unit cube `[−1, 1]³`.
    #[must_use]
    pub fn normalise(&self, p: [f64; 3]) -> [f64; 3] {
        let scale = |v: f64, lo: f64, hi: f64| if hi > lo { 2.0 * (v - lo) / (hi - lo) - 1.0 } else { 0.0 };
        let (x0, x1) = (self.xs[0], self.xs[self.xs.len() - 1]);
        let (y0, y1) = (self.ys[0], self.ys[self.ys.len() - 1]);
        [scale(p[0], x0, x1), scale(p[1], y0, y1), scale(p[2], self.z_range.0, self.z_range.1)]
    }

    /// Grid divisions per axis.
    #[must_use]
    pub fn resolution(&self) -> usize {
        self.xs.len() - 1
    }

    /// Grid divisions along the y axis (equal to [`resolution`](Self::resolution)
    /// for a model's surface).
    #[must_use]
    pub fn y_resolution(&self) -> usize {
        self.ys.len() - 1
    }

    /// The four corners of cell `(i, j)` in data units, counter-clockwise from
    /// lower left.
    #[must_use]
    pub fn cell_corners(&self, i: usize, j: usize) -> [[f64; 3]; 4] {
        let (xs, ys, z) = (&self.xs, &self.ys, &self.z);
        [
            [xs[i], ys[j], z[j][i]],
            [xs[i + 1], ys[j], z[j][i + 1]],
            [xs[i + 1], ys[j + 1], z[j + 1][i + 1]],
            [xs[i], ys[j + 1], z[j + 1][i]],
        ]
    }

    /// Every cell `(i, j)`, farthest from the viewer first.
    #[must_use]
    pub fn cells_back_to_front(&self, camera: &Camera) -> Vec<(usize, usize)> {
        self.cells_with_depth(camera).into_iter().map(|(_, i, j)| (i, j)).collect()
    }

    /// Every cell as `(depth of its centre, i, j)`, farthest first (larger
    /// depth is nearer the viewer).
    #[must_use]
    pub fn cells_with_depth(&self, camera: &Camera) -> Vec<(f64, usize, usize)> {
        let (nx, ny) = (self.resolution(), self.y_resolution());
        let mut cells: Vec<(f64, usize, usize)> = (0..ny)
            .flat_map(|j| (0..nx).map(move |i| (i, j)))
            .filter(|&(i, j)| self.cell_corners(i, j).iter().all(|c| c[2].is_finite()))
            .map(|(i, j)| {
                let depth: f64 =
                    self.cell_corners(i, j).iter().map(|&c| camera.project(self.normalise(c))[2]).sum::<f64>() / 4.0;
                (depth, i, j)
            })
            .collect();
        cells.sort_by(|a, b| a.0.total_cmp(&b.0));
        cells
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// z = 10 + x + 2y + 3g over x, y in [0, 1] and a two-level factor g.
    struct Plane;

    impl SurfaceModel for Plane {
        fn response_name(&self) -> &str {
            "z"
        }
        fn profile_factors(&self) -> Vec<ProfileFactor> {
            let c = |name: &str| ProfileFactor {
                name: name.to_owned(),
                kind: ProfileFactorKind::Continuous { range: (0.0, 1.0) },
            };
            vec![
                c("x"),
                c("y"),
                ProfileFactor {
                    name: "g".to_owned(),
                    kind: ProfileFactorKind::Categorical { levels: vec!["a".into(), "b".into()] },
                },
            ]
        }
        fn default_profile_settings(&self) -> Vec<TermValue> {
            vec![TermValue::Number(0.5), TermValue::Number(0.5), TermValue::Level(0)]
        }
        fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
            Ok(points
                .iter()
                .map(|s| match s.as_slice() {
                    [TermValue::Number(x), TermValue::Number(y), TermValue::Level(g)] => {
                        10.0 + x + 2.0 * y + 3.0 * *g as f64
                    }
                    _ => f64::NAN,
                })
                .collect())
        }
    }

    #[test]
    fn mesh_values_levels_and_contours() {
        let settings = vec![TermValue::Number(0.25), TermValue::Number(0.75), TermValue::Level(1)];
        let m = surface_mesh(&Plane, &settings, 0, 1, 10, 6).unwrap();
        assert_eq!((m.xs.len(), m.ys.len(), m.resolution()), (11, 11, 10));
        assert!((m.z[2][5] - (13.0 + 0.5 + 0.4)).abs() < 1e-12);
        assert_eq!(m.z_range, (13.0, 16.0));
        assert_eq!(m.levels, [13.5, 14.0, 14.5, 15.0, 15.5]);
        assert_eq!(m.current, [0.25, 0.75, 13.0 + 0.25 + 1.5]);
        // Every contour point lies on its level of the plane.
        for segments in &m.cell_contours {
            for (k, a, b) in segments {
                for p in [a, b] {
                    assert!((13.0 + p[0] + 2.0 * p[1] - m.levels[*k]).abs() < 1e-9);
                    assert_eq!(p[2], m.levels[*k]);
                }
            }
        }
        let total: usize = m.cell_contours.iter().map(Vec::len).sum();
        assert!(total > 0);
        assert_eq!(m.floor_contours.len(), m.levels.len(), "one straight line per level");
        for (k, line) in &m.floor_contours {
            for p in line {
                assert!((13.0 + p[0] + 2.0 * p[1] - m.levels[*k]).abs() < 1e-9);
            }
        }
        // Swapped axes and no contours.
        let s = surface_mesh(&Plane, &settings, 1, 0, 4, 0).unwrap();
        assert_eq!((s.x_name.as_str(), s.y_name.as_str()), ("y", "x"));
        assert!(s.levels.is_empty() && s.floor_contours.is_empty());
    }

    #[test]
    fn invalid_requests() {
        let d = Plane.default_profile_settings();
        assert!(surface_mesh(&Plane, &d, 0, 0, 10, 5).is_err(), "same axis");
        assert!(surface_mesh(&Plane, &d, 0, 2, 10, 5).is_err(), "categorical axis");
        assert!(surface_mesh(&Plane, &d[..2], 0, 1, 10, 5).is_err(), "short settings");
        let bad = vec![TermValue::Number(0.5), TermValue::Number(0.5), TermValue::Level(5)];
        assert!(!settings_fit(&Plane, &bad));
        assert_eq!(surface_mesh(&Plane, &d, 0, 1, 1000, 0).unwrap().resolution(), MAX_RESOLUTION);
    }

    #[test]
    fn saddle_cells_split_by_the_centre() {
        // Corners high, low, high, low: two segments either way.
        let c = [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.0, 1.0, 0.0]];
        assert_eq!(cell_crossings(&c, 0.5).len(), 2);
        // Centre (mean 0.5) counts as high: the two low corners are cut off.
        let segs = cell_crossings(&c, 0.5);
        let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).abs() < 1e-12 && (p[1] - q[1]).abs() < 1e-12;
        assert!(segs.iter().any(|(a, b)| near(*a, [0.5, 0.0]) && near(*b, [1.0, 0.5])));
        assert!(cell_crossings(&c, 2.0).is_empty());
    }

    /// The fitted models implement the interface: predictions agree with
    /// their own reports.
    #[test]
    fn fitted_models_as_surfaces() {
        use crate::{LinearModelAnalysis, SurfaceAnalysis, SurfaceRequest};
        use stat_data::RowStateManager;
        // Response surface: the stationary point (a maximum) is the default
        // setting and the highest point of the grid is near it.
        let t = crate::sample::reaction_yield();
        let rows = RowStateManager::new(t.n_rows());
        let a = SurfaceAnalysis::run(&t, &rows, &SurfaceRequest { response: 2, factors: vec![0, 1] }).unwrap();
        let d = a.default_profile_settings();
        let m = surface_mesh(&a, &d, 0, 1, 40, 8).unwrap();
        assert!((m.current[2] - a.canonical().stationary_value.unwrap()).abs() < 1e-9);
        assert!(m.z_range.1 <= m.current[2] + 1e-9, "the maximum is not exceeded on the grid");
        assert!(m.z_range.1 > m.current[2] - 0.05);
        assert_eq!(a.profile_factors()[0].name, "Time (min)");
        // Linear model with a categorical term held at a level.
        let t = crate::sample::catalyst();
        let rows = RowStateManager::new(t.n_rows());
        let spec = crate::LinearModelSpec {
            response: 3,
            predictors: vec![0, 1, 2],
            effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(2, 1)], vec![(0, 1), (1, 1)], vec![(0, 2)]],
            splines: Vec::new(),
        };
        let a = LinearModelAnalysis::run(&t, &rows, &spec).unwrap();
        let factors = a.profile_factors();
        assert!(factors[0].is_continuous() && factors[1].is_continuous() && !factors[2].is_continuous());
        let d = a.default_profile_settings();
        let m = surface_mesh(&a, &d, 0, 1, 10, 5).unwrap();
        let profile = a.profile(&d).unwrap();
        assert!((m.current[2] - profile.predicted).abs() < 1e-9);
        assert_eq!(m.xs[0], 150.0);
        assert_eq!(*m.xs.last().unwrap(), 190.0);
        assert!(surface_mesh(&a, &d, 0, 2, 10, 5).is_err(), "catalyst is categorical");
    }

    /// Response Explorers of the other fitted models agree with their fits.
    #[test]
    fn profiles_of_other_models() {
        use crate::{
            GlmAnalysis, GlmRequest, MixtureAnalysis, MixtureRequest, RegularizedAnalysis, RegularizedRequest,
            SurfaceAnalysis, SurfaceRequest,
        };
        use stat_data::RowStateManager;
        // Response surface: slice predictions are the fitted surface, with bands.
        let t = crate::sample::reaction_yield();
        let rows = RowStateManager::new(t.n_rows());
        let a = SurfaceAnalysis::run(&t, &rows, &SurfaceRequest { response: 2, factors: vec![0, 1] }).unwrap();
        let p = profile(&a, &a.default_profile_settings()).unwrap();
        assert!((p.predicted - a.canonical().stationary_value.unwrap()).abs() < 1e-9);
        let ci = p.confidence_interval.unwrap();
        assert!(ci.0 < p.predicted && p.predicted < ci.1);
        assert!(matches!(&p.panels[0], ProfilePanel::Continuous { band: Some(_), .. }));
        assert_eq!(p.confidence_level, Some(0.95));

        // Logistic regression: predictions at a data row equal the fitted
        // probability; limits stay inside (0, 1).
        let t = crate::sample::churn();
        let rows = RowStateManager::new(t.n_rows());
        let request = GlmRequest {
            response: 5,
            predictors: vec![0, 1, 2, 3],
            family: stat_discrete::Family::Binomial,
            link: None,
            event: None,
            effects: Vec::new(),
            splines: Vec::new(),
            slices: None,
            slopes: None,
        };
        let g = GlmAnalysis::run(&t, &rows, &request).unwrap();
        let factors = g.profile_factors();
        let ProfileFactorKind::Categorical { levels } = &factors[2].kind else { panic!("contract is categorical") };
        let level = levels.iter().position(|l| *l == t.column(2).unwrap().level_text(0)).unwrap();
        let num = |c: usize| TermValue::Number(t.column(c).unwrap().as_numeric().unwrap()[0]);
        let row0 = vec![num(0), num(1), TermValue::Level(level), num(3)];
        let fitted = g.predict_with_limits(&[row0]).unwrap()[0];
        assert!((fitted.0 - g.fit().fitted[0]).abs() < 1e-12);
        let (lo, hi) = fitted.1.unwrap();
        assert!(0.0 < lo && lo < fitted.0 && fitted.0 < hi && hi < 1.0);
        let p = profile(&g, &g.default_profile_settings()).unwrap();
        assert_eq!(p.response, "P(Churned = Yes)");
        assert!(p.note.is_some());
        let ProfilePanel::Categorical { predictions, .. } = &p.panels[2] else { panic!() };
        assert!(predictions.iter().all(|(m, l)| l.is_some_and(|(a, b)| a < *m && *m < b)));
        assert!(surface_mesh(&g, &g.default_profile_settings(), 0, 1, 10, 4).is_ok());

        // Penalized regression: the λ₁ₛₑ predictions, without bands.
        let request = RegularizedRequest { response: 4, predictors: vec![0, 1, 2, 3], alpha: 1.0, folds: 5, seed: 1 };
        let r = RegularizedAnalysis::run(&t, &rows, &request).unwrap();
        let p = profile(&r, &r.default_profile_settings()).unwrap();
        assert!(p.confidence_interval.is_none() && p.confidence_level.is_none());
        assert!(matches!(&p.panels[0], ProfilePanel::Continuous { band: None, .. }));
        let path = r.path();
        let l = path.index_1se;
        let ProfilePanel::Continuous { curve, .. } = &p.panels[0] else { panic!() };
        let slope = (curve[1][1] - curve[0][1]) / (curve[1][0] - curve[0][0]);
        assert!((slope - path.coefficients[l][0]).abs() < 1e-9, "tenure slope is its λ₁ₛₑ coefficient");

        // Mixture: moving one component rescales the others; at a vertex the
        // quadratic Scheffé prediction is that component's coefficient.
        let t = crate::sample::yarn();
        let rows = RowStateManager::new(t.n_rows());
        let m = MixtureAnalysis::run(
            &t,
            &rows,
            &MixtureRequest { response: 3, components: vec![0, 1, 2], model: stat_doe::ScheffeModel::Quadratic },
        )
        .unwrap();
        let d = m.default_profile_settings();
        let total =
            |s: &[TermValue]| s.iter().map(|v| if let TermValue::Number(x) = v { *x } else { 0.0 }).sum::<f64>();
        assert!((total(&d) - 1.0).abs() < 1e-12);
        let moved = m.with_setting(&d, 0, TermValue::Number(0.6));
        assert!((total(&moved) - 1.0).abs() < 1e-12);
        let ratio = |s: &[TermValue]| match (s[1], s[2]) {
            (TermValue::Number(a), TermValue::Number(b)) => a / b,
            _ => f64::NAN,
        };
        assert!((ratio(&moved) - ratio(&d)).abs() < 1e-12, "the others keep their proportions");
        let vertex = m.with_setting(&d, 0, TermValue::Number(1.0));
        let at_vertex = m.predict_settings(&[vertex]).unwrap()[0];
        assert!((at_vertex - m.coefficients()[0]).abs() < 1e-9);
        let p = profile(&m, &d).unwrap();
        let ProfilePanel::Continuous { range, .. } = &p.panels[0] else { panic!() };
        assert_eq!(*range, (0.0, 1.0));
        assert!(!m.independent_factors());
        assert!(surface_mesh(&m, &d, 0, 1, 10, 4).is_err());
    }

    /// Observations sit at their own factor values with their fitted values.
    #[test]
    fn observations_join_the_surface() {
        use crate::{GlmAnalysis, GlmRequest, LinearModelAnalysis, SurfaceAnalysis, SurfaceRequest};
        use stat_data::RowStateManager;
        let t = crate::sample::reaction_yield();
        let rows = RowStateManager::new(t.n_rows());
        let a = SurfaceAnalysis::run(&t, &rows, &SurfaceRequest { response: 2, factors: vec![0, 1] }).unwrap();
        let mut m = surface_mesh(&a, &a.default_profile_settings(), 0, 1, 20, 6).unwrap();
        assert!(m.points.is_empty());
        let before = m.z_range;
        add_observations(&mut m, &a, &a.default_profile_settings(), 0, 1).unwrap();
        assert_eq!(m.points.len(), 13);
        // With two factors the fitted value is the surface itself.
        let p = m.points[0];
        assert_eq!((p.row, p.x, p.y, p.observed), (0, 80.0, 170.0, 76.5));
        let on_surface = a.predict_settings(&[vec![TermValue::Number(80.0), TermValue::Number(170.0)]]).unwrap()[0];
        assert!((p.fitted - on_surface).abs() < 1e-12 && (p.surface - on_surface).abs() < 1e-12);
        assert!(m.z_range.0 <= before.0 && m.z_range.1 >= before.1);
        assert!(m.points.iter().all(|p| (m.z_range.0..=m.z_range.1).contains(&p.observed)));
        // Excluded rows are not shown.
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([3], true);
        let a = SurfaceAnalysis::run(&t, &rows, &SurfaceRequest { response: 2, factors: vec![0, 1] }).unwrap();
        let mut m = surface_mesh(&a, &a.default_profile_settings(), 0, 1, 20, 6).unwrap();
        add_observations(&mut m, &a, &a.default_profile_settings(), 0, 1).unwrap();
        assert!(m.points.len() == 12 && m.points.iter().all(|p| p.row != 3));
        // Linear model with a categorical term: residuals match the fit's.
        let t = crate::sample::catalyst();
        let rows = RowStateManager::new(t.n_rows());
        let spec = crate::LinearModelSpec {
            response: 3,
            predictors: vec![0, 1, 2],
            effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(2, 1)], vec![(0, 1), (1, 1)], vec![(0, 2)]],
            splines: Vec::new(),
        };
        let l = LinearModelAnalysis::run(&t, &rows, &spec).unwrap();
        let mut m = surface_mesh(&l, &l.default_profile_settings(), 0, 1, 10, 5).unwrap();
        add_observations(&mut m, &l, &l.default_profile_settings(), 0, 1).unwrap();
        assert_eq!(m.points.len(), t.n_rows());
        let residuals = l.fit().residuals();
        for (p, r) in m.points.iter().zip(residuals) {
            assert!((p.observed - p.fitted - r).abs() < 1e-9);
        }
        // Above/below is judged against the drawn surface (catalyst held at
        // A), not the row's own fit: row 5 is a catalyst-B run just below its
        // own fit but well above the drawn surface.
        let held = l.default_profile_settings();
        assert_eq!(held[2], TermValue::Level(0), "catalyst A");
        let r5 = m.points.iter().find(|p| p.row == 4).unwrap();
        assert!(r5.observed < r5.fitted && r5.above_surface());
        let mut s = held.clone();
        s[0] = TermValue::Number(r5.x);
        s[1] = TermValue::Number(r5.y);
        assert!((r5.surface - l.predict_settings(&[s]).unwrap()[0]).abs() < 1e-12);
        // Catalyst-A runs: the surface is their own fit.
        let r1 = m.points.iter().find(|p| p.row == 0).unwrap();
        assert!((r1.surface - r1.fitted).abs() < 1e-9);
        // Logistic: observed 0/1 widen the range to [0, 1].
        let t = crate::sample::churn();
        let rows = RowStateManager::new(t.n_rows());
        let request = GlmRequest {
            response: 5,
            predictors: vec![0, 1, 2, 3],
            family: stat_discrete::Family::Binomial,
            link: None,
            event: None,
            effects: Vec::new(),
            splines: Vec::new(),
            slices: None,
            slopes: None,
        };
        let g = GlmAnalysis::run(&t, &rows, &request).unwrap();
        let mut m = surface_mesh(&g, &g.default_profile_settings(), 0, 1, 10, 5).unwrap();
        add_observations(&mut m, &g, &g.default_profile_settings(), 0, 1).unwrap();
        assert_eq!(m.points.len(), 400);
        assert_eq!(m.z_range, (0.0, 1.0));
    }

    #[test]
    fn camera_projection_and_painting_order() {
        // From above, x is across, y is up, and higher points are nearer.
        let top = Camera::TOP;
        let p = top.project([0.3, -0.4, 0.9]);
        assert!((p[0] - 0.3).abs() < 1e-12 && (p[1] + 0.4).abs() < 1e-12 && (p[2] - 0.9).abs() < 1e-12);
        // From the side (azimuth 0), y points away: depth falls as y grows.
        let side = Camera { azimuth: 0.0, elevation: 0.0 };
        assert!(side.project([0.0, 1.0, 0.0])[2] < side.project([0.0, -1.0, 0.0])[2]);
        assert!((side.project([0.0, 0.0, 1.0])[1] - 1.0).abs() < 1e-12);
        // Rotation wraps both angles all the way round.
        let mut c = Camera { azimuth: 170.0, elevation: 80.0 };
        c.rotate(20.0, 30.0);
        assert!((c.azimuth + 170.0).abs() < 1e-9 && (c.elevation - 110.0).abs() < 1e-9);
        let mut c = Camera { azimuth: 0.0, elevation: 30.0 };
        c.rotate(0.0, -200.0);
        assert!((c.elevation - 190.0 + 360.0).abs() < 1e-9 && !c.sees_floor_from_above());
        let mut c = Camera { azimuth: 0.0, elevation: 30.0 };
        for _ in 0..36 {
            c.rotate(0.0, 10.0);
        }
        assert!((c.elevation - 30.0).abs() < 1e-9, "a full tilt returns to the start");
        // A positive turn moves the nearest side right, upright or upside down.
        let nearest_right = |c: &Camera| {
            let near = [[0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]]
                .into_iter()
                .max_by(|a, b| c.project(*a)[2].total_cmp(&c.project(*b)[2]))
                .unwrap();
            (near, c.project(near)[0])
        };
        for elevation in [30.0, 150.0, -40.0, -150.0] {
            let mut c = Camera { azimuth: 10.0, elevation };
            let (near, before) = nearest_right(&c);
            c.rotate(5.0, 0.0);
            assert!(c.project(near)[0] > before, "elevation {elevation}");
        }
        // Painting order: in a side view along +y, the cells at large y come first.
        let m = surface_mesh(&Plane, &Plane.default_profile_settings(), 0, 1, 4, 0).unwrap();
        let order = m.cells_back_to_front(&Camera { azimuth: 0.0, elevation: 10.0 });
        assert_eq!(order.len(), 16);
        assert_eq!(order[0].1, 3, "farthest row first");
        assert_eq!(order[15].1, 0, "nearest row last");
    }
}
