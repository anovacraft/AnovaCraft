//! The 3-D scatter plot (Visualize ▸ 3-D scatter plot…): rows placed by three
//! continuous columns in a rotatable box, with optional colour, size and
//! marker-shape channels and drop lines to the floor.
//!
//! This module holds the GUI-independent part: which rows are plotted, their
//! positions and channel values, the axis ranges and the summary table. The
//! projection is [`crate::surface_profiler::Camera`]; the desktop view draws
//! the points back to front and handles rotation and brushing.
//!
//! Rows with a missing position or channel value are left out. Excluded rows
//! are kept (front ends draw them hollow); hidden rows are for the front end
//! to leave out, as on the graph canvas.

use crate::counts::column_levels;
use crate::format::format_number;
use crate::project::{invalid, obj, s};
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use crate::surface_profiler::Camera;
use stat_core::Result;
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager};

/// Columns of a 3-D scatter plot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scatter3dRequest {
    /// Column across the floor of the box.
    pub x: usize,
    /// Column into the depth of the box.
    pub y: usize,
    /// Column up the box.
    pub z: usize,
    /// Column colouring the points: a continuous column maps to a sequential
    /// ramp, any other to one colour per level.
    pub color: Option<usize>,
    /// Numeric column sizing the points (marker area proportional to value).
    pub size: Option<usize>,
    /// Column whose levels set the marker shape.
    pub shape: Option<usize>,
}

impl Scatter3dRequest {
    /// Positions only, no channels.
    #[must_use]
    pub fn new(x: usize, y: usize, z: usize) -> Self {
        Self { x, y, z, color: None, size: None, shape: None }
    }

    /// Every column used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        [self.x, self.y, self.z].into_iter().chain(self.color).chain(self.size).chain(self.shape).collect()
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: impl Fn(usize) -> usize) -> Self {
        Self {
            x: f(self.x),
            y: f(self.y),
            z: f(self.z),
            color: self.color.map(&f),
            size: self.size.map(&f),
            shape: self.shape.map(&f),
        }
    }
}

/// Display options of a 3-D scatter plot (saved with the project).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Scatter3dOptions {
    /// A line from each point down to the floor of the box.
    pub drop_lines: bool,
    /// Viewing angle.
    pub camera: Camera,
}

/// The project-file form (`"analysis": "scatter-3d"`).
#[must_use]
pub fn scatter3d_to_json(request: &Scatter3dRequest, options: &Scatter3dOptions, table: &DataTable) -> JsonValue {
    let name = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
    let optional = |c: Option<usize>| c.map_or(JsonValue::Null, name);
    obj([
        ("analysis", s("scatter-3d")),
        ("x", name(request.x)),
        ("y", name(request.y)),
        ("z", name(request.z)),
        ("color", optional(request.color)),
        ("size", optional(request.size)),
        ("shape", optional(request.shape)),
        ("drop_lines", JsonValue::Bool(options.drop_lines)),
        ("camera", camera_json(options.camera)),
    ])
}

/// Read the project-file form.
///
/// # Errors
/// Missing or unknown columns.
pub fn scatter3d_from_json(v: &JsonValue, table: &DataTable) -> Result<(Scatter3dRequest, Scatter3dOptions)> {
    let column = |key: &str| -> Result<Option<usize>> {
        match v.get(key) {
            None | Some(JsonValue::Null) => Ok(None),
            Some(n) => {
                let name = n.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
                table.find(name).map(Some).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            }
        }
    };
    let required = |key: &str| column(key)?.ok_or_else(|| invalid(format!("3-D scatter plot: {key:?} is required")));
    let request = Scatter3dRequest {
        x: required("x")?,
        y: required("y")?,
        z: required("z")?,
        color: column("color")?,
        size: column("size")?,
        shape: column("shape")?,
    };
    let options = Scatter3dOptions {
        drop_lines: v.get("drop_lines").and_then(JsonValue::as_bool).unwrap_or(false),
        camera: camera_from_json(v.get("camera")),
    };
    Ok((request, options))
}

/// `{"turn": azimuth, "tilt": elevation}` in degrees.
pub(crate) fn camera_json(camera: Camera) -> JsonValue {
    obj([("turn", JsonValue::Number(camera.azimuth)), ("tilt", JsonValue::Number(camera.elevation))])
}

/// The camera in `v`, or the default view when absent or malformed.
pub(crate) fn camera_from_json(v: Option<&JsonValue>) -> Camera {
    let d = Camera::default();
    let angle = |key: &str, default: f64| {
        v.and_then(|c| c.get(key)).and_then(JsonValue::as_f64).filter(|a| a.is_finite()).unwrap_or(default)
    };
    let mut camera = Camera { azimuth: 0.0, elevation: 0.0 };
    // Rotating from zero wraps the angles into their ranges.
    camera.rotate(angle("turn", d.azimuth), angle("tilt", d.elevation));
    camera
}

/// What a point's colour shows.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorChannel {
    /// No colour column.
    None,
    /// One colour per level of a categorical column.
    Levels {
        /// Column name.
        name: String,
        /// Level labels, in order.
        levels: Vec<String>,
    },
    /// A sequential ramp over a continuous column.
    Ramp {
        /// Column name.
        name: String,
        /// Smallest and largest value plotted.
        range: (f64, f64),
    },
}

/// A point's value on the colour channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorValue {
    /// No colour column.
    None,
    /// Level index.
    Level(usize),
    /// Value on a continuous column.
    Value(f64),
}

/// One plotted row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scatter3dPoint {
    /// Dataset row.
    pub row: usize,
    /// Position `(x, y, z)` in data units.
    pub at: [f64; 3],
    /// Colour channel value.
    pub color: ColorValue,
    /// Size channel value.
    pub size: Option<f64>,
    /// Shape level index.
    pub shape: Option<usize>,
}

/// The rows of a 3-D scatter plot and their channels.
#[derive(Debug, Clone, PartialEq)]
pub struct Scatter3dAnalysis {
    request: Scatter3dRequest,
    /// Names of the x, y and z columns.
    pub names: [String; 3],
    /// Plotted rows.
    pub points: Vec<Scatter3dPoint>,
    /// Axis ranges of the box (the data range with 6% padding at each end).
    pub ranges: [(f64, f64); 3],
    /// The colour channel.
    pub color: ColorChannel,
    /// Size column name and the smallest and largest value plotted.
    pub size: Option<(String, (f64, f64))>,
    /// Shape column name and its levels.
    pub shape: Option<(String, Vec<String>)>,
    /// Rows left out for a missing value.
    pub n_missing: usize,
    /// Plotted rows that are excluded.
    pub n_excluded: usize,
}

/// Padding added to each end of an axis, as a share of the data range.
const PAD: f64 = 0.06;

impl Scatter3dAnalysis {
    /// Collect the rows of `request` from `table`.
    ///
    /// # Errors
    /// [`stat_core::StatError::InvalidArgument`] for a missing column, a
    /// position or size column that is not numeric, or no complete rows.
    pub fn run(table: &DataTable, rows: &RowStateManager, request: &Scatter3dRequest) -> Result<Self> {
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let numeric = |i: usize| -> Result<(&str, &[f64])> {
            let c = column(i)?;
            Ok((c.name.as_str(), c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?))
        };
        let (xn, xs) = numeric(request.x)?;
        let (yn, ys) = numeric(request.y)?;
        let (zn, zs) = numeric(request.z)?;
        if request.x == request.y || request.x == request.z || request.y == request.z {
            return Err(invalid("choose three different columns for the axes"));
        }
        let levels_of = |i: usize| -> Result<(String, Vec<String>, Vec<Option<usize>>)> {
            let c = column(i)?;
            let levels = column_levels(table, i);
            let index =
                (0..table.n_rows()).map(|r| levels.iter().position(|l| *l == c.level_text(r))).collect::<Vec<_>>();
            Ok((c.name.clone(), levels, index))
        };
        let (color, color_of): (ColorChannel, Box<dyn Fn(usize) -> Option<ColorValue>>) = match request.color {
            None => (ColorChannel::None, Box::new(|_| Some(ColorValue::None))),
            Some(i) => {
                let c = column(i)?;
                match c.as_numeric().filter(|_| c.level == MeasurementLevel::Continuous) {
                    Some(v) => (
                        ColorChannel::Ramp { name: c.name.clone(), range: (0.0, 0.0) },
                        Box::new(move |r| v[r].is_finite().then_some(ColorValue::Value(v[r]))),
                    ),
                    None => {
                        let (name, levels, index) = levels_of(i)?;
                        (ColorChannel::Levels { name, levels }, Box::new(move |r| index[r].map(ColorValue::Level)))
                    }
                }
            }
        };
        let size_values = request.size.map(numeric).transpose()?;
        let shape = request.shape.map(levels_of).transpose()?;
        let mut points = Vec::new();
        let mut n_missing = 0;
        for r in 0..table.n_rows() {
            let at = [xs[r], ys[r], zs[r]];
            let size = size_values.map(|(_, v)| v[r]);
            let shape_level = shape.as_ref().map(|(_, _, index)| index[r]);
            let complete = at.iter().all(|v| v.is_finite())
                && size.is_none_or(f64::is_finite)
                && shape_level.is_none_or(|l| l.is_some());
            match color_of(r) {
                Some(color) if complete => {
                    points.push(Scatter3dPoint { row: r, at, color, size, shape: shape_level.flatten() });
                }
                _ => n_missing += 1,
            }
        }
        if points.is_empty() {
            return Err(invalid("no row has values in all the chosen columns"));
        }
        let span = |f: &dyn Fn(&Scatter3dPoint) -> f64| {
            points.iter().map(f).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
        };
        let pad = |(lo, hi): (f64, f64)| {
            if hi > lo {
                let d = PAD * (hi - lo);
                (lo - d, hi + d)
            } else {
                let d = if lo == 0.0 { 1.0 } else { 0.1 * lo.abs() };
                (lo - d, hi + d)
            }
        };
        let ranges = [pad(span(&|p| p.at[0])), pad(span(&|p| p.at[1])), pad(span(&|p| p.at[2]))];
        let color = match color {
            ColorChannel::Ramp { name, .. } => ColorChannel::Ramp {
                name,
                range: span(&|p| if let ColorValue::Value(v) = p.color { v } else { f64::NAN }),
            },
            other => other,
        };
        let size = size_values.map(|(name, _)| (name.to_owned(), span(&|p| p.size.unwrap_or(f64::NAN))));
        let n_excluded = points.iter().filter(|p| rows.is_excluded(p.row)).count();
        Ok(Self {
            request: request.clone(),
            names: [xn.to_owned(), yn.to_owned(), zn.to_owned()],
            points,
            ranges,
            color,
            size,
            shape: shape.map(|(name, levels, _)| (name, levels)),
            n_missing,
            n_excluded,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &Scatter3dRequest {
        &self.request
    }

    /// Report heading.
    #[must_use]
    pub fn title(&self) -> String {
        format!("3-D Scatter Plot: {} by {} and {}", self.names[2], self.names[0], self.names[1])
    }

    /// Marker radius in screen points for a size value: marker area
    /// proportional to the value's place in the plotted range, from
    /// `small` to `large`.
    #[must_use]
    pub fn radius(&self, value: Option<f64>, small: f32, large: f32) -> f32 {
        match (value, &self.size) {
            (Some(v), Some((_, (lo, hi)))) if hi > lo => {
                let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32;
                (small * small + t * (large * large - small * small)).sqrt()
            }
            _ => 0.5 * (small + large),
        }
    }

    /// The report sections: a table of what each channel shows.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut t = TextTable::new(["Role", "Column", "Shows"]);
        let range = |(lo, hi): (f64, f64)| format!("{} to {}", format_number(lo, 5), format_number(hi, 5));
        let data_range = |k: usize| {
            let (lo, hi) = self
                .points
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.at[k]), hi.max(p.at[k])));
            range((lo, hi))
        };
        for (k, role) in ["Across (x)", "Depth (y)", "Up (z)"].into_iter().enumerate() {
            t.push([role.to_owned(), self.names[k].clone(), data_range(k)]);
        }
        match &self.color {
            ColorChannel::None => {}
            ColorChannel::Levels { name, levels } => {
                t.push(["Colour".to_owned(), name.clone(), format!("{} levels", levels.len())]);
            }
            ColorChannel::Ramp { name, range: r } => t.push(["Colour".to_owned(), name.clone(), range(*r)]),
        }
        if let Some((name, r)) = &self.size {
            t.push(["Size".to_owned(), name.clone(), range(*r)]);
        }
        if let Some((name, levels)) = &self.shape {
            t.push(["Shape".to_owned(), name.clone(), format!("{} levels", levels.len())]);
        }
        let mut note = format!("{} rows plotted", self.points.len());
        if self.n_excluded > 0 {
            note.push_str(&format!(" ({} of them excluded, drawn hollow)", self.n_excluded));
        }
        if self.n_missing > 0 {
            note.push_str(&format!("; {} left out for a missing value", self.n_missing));
        }
        note.push('.');
        vec![
            ReportSection {
                kind: SectionKind::Scatter3dColumns,
                title: "Plotted Columns".to_owned(),
                content: SectionContent::Table(t),
            },
            ReportSection {
                kind: SectionKind::Scatter3dNote,
                title: "Rows".to_owned(),
                content: SectionContent::Note(note),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0, f64::NAN]),
                Column::numeric("y", vec![0.0, 1.0, 0.0, 1.0, 1.0]),
                Column::numeric("z", vec![10.0, 20.0, 15.0, 30.0, 5.0]),
                Column::text("g", ["a", "b", "a", "", "b"].map(String::from).to_vec()),
                Column::numeric("w", vec![1.0, 4.0, 2.0, 3.0, 2.0]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn rows_need_every_chosen_value() {
        let t = table();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([1], true);
        let a = Scatter3dAnalysis::run(&t, &rows, &Scatter3dRequest::new(0, 1, 2)).unwrap();
        assert_eq!(a.points.len(), 4);
        assert_eq!(a.n_missing, 1);
        assert_eq!(a.n_excluded, 1);
        // 6% padding at each end.
        let (lo, hi) = a.ranges[2];
        assert!((lo - (10.0 - 1.2)).abs() < 1e-12 && (hi - 31.2).abs() < 1e-12);
        let request =
            Scatter3dRequest { color: Some(3), size: Some(4), shape: Some(3), ..Scatter3dRequest::new(0, 1, 2) };
        let a = Scatter3dAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.points.iter().map(|p| p.row).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(a.color, ColorChannel::Levels { name: "g".into(), levels: vec!["a".into(), "b".into()] });
        assert_eq!(a.points[1].color, ColorValue::Level(1));
        assert_eq!(a.size, Some(("w".into(), (1.0, 4.0))));
        // Marker area grows linearly with the value.
        let (r0, r1, mid) =
            (a.radius(Some(1.0), 3.0, 9.0), a.radius(Some(4.0), 3.0, 9.0), a.radius(Some(2.5), 3.0, 9.0));
        assert!((r0 - 3.0).abs() < 1e-6 && (r1 - 9.0).abs() < 1e-6);
        assert!((mid * mid - 0.5 * (9.0 + 81.0)).abs() < 1e-3);
        let s = a.sections();
        let SectionContent::Table(tab) = &s[0].content else { panic!() };
        assert_eq!(tab.rows.len(), 6);
    }

    #[test]
    fn continuous_colour_is_a_ramp_and_axes_must_differ() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let request = Scatter3dRequest { color: Some(4), ..Scatter3dRequest::new(0, 1, 2) };
        let a = Scatter3dAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.color, ColorChannel::Ramp { name: "w".into(), range: (1.0, 4.0) });
        assert!(Scatter3dAnalysis::run(&t, &rows, &Scatter3dRequest::new(0, 0, 2)).is_err());
        assert!(Scatter3dAnalysis::run(&t, &rows, &Scatter3dRequest::new(0, 3, 2)).is_err());
    }

    #[test]
    fn json_round_trip_keeps_the_view() {
        let t = table();
        let request = Scatter3dRequest { color: Some(3), size: Some(4), ..Scatter3dRequest::new(0, 1, 2) };
        let options = Scatter3dOptions { drop_lines: true, camera: Camera { azimuth: 50.0, elevation: -20.0 } };
        let v = scatter3d_to_json(&request, &options, &t);
        let (r2, o2) = scatter3d_from_json(&v, &t).unwrap();
        assert_eq!(r2, request);
        assert!(o2.drop_lines);
        assert!((o2.camera.azimuth - 50.0).abs() < 1e-12 && (o2.camera.elevation + 20.0).abs() < 1e-12);
        // Older or hand-written files without a view get the default one.
        let v = obj([("analysis", s("scatter-3d")), ("x", s("x")), ("y", s("y")), ("z", s("z"))]);
        let (_, o3) = scatter3d_from_json(&v, &t).unwrap();
        assert_eq!(o3, Scatter3dOptions::default());
    }
}
