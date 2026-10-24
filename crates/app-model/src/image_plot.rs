//! The image plot report (Visualize ▸ Image plot…): a value column drawn as
//! a picture over its `x` and `y` columns, for image tables
//! ([`crate::image_table`]) and any other evenly spaced grid. The grid comes
//! from [`crate::image_table::PixelGrid`] and is drawn as a
//! [`stat_viz::RasterSpec`] section ([`SectionContent::Raster`]), the same
//! section the image analyses use.

use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_viz::RasterSpec;
use stat_viz::palette::ColorScale;

use crate::compare::section;
use crate::format::format_number;
use crate::image_table::PixelGrid;
use crate::project::{invalid, member, s};
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};

/// A raster in long form: one row per cell with a value (`x`, `y`, value),
/// row by row from the top.
#[must_use]
pub fn raster_table(r: &RasterSpec) -> TextTable {
    let name = |t: &str, d: &str| if t.is_empty() { d.to_owned() } else { t.to_owned() };
    let mut t = TextTable::new([name(&r.x_title, "x"), name(&r.y_title, "y"), name(&r.value_title, "Value")]);
    for row in 0..r.rows {
        for col in 0..r.cols {
            let v = r.value(row, col);
            if v.is_finite() {
                let [x, y] = r.cell_center(row, col);
                t.push([format_number(x, 6), format_number(y, 6), format_number(v, 6)]);
            }
        }
    }
    t
}

/// An image plot request: which columns, and how the picture is coloured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImagePlotRequest {
    /// Horizontal position column.
    pub x: usize,
    /// Vertical position column.
    pub y: usize,
    /// The values drawn.
    pub value: usize,
    /// Colours.
    pub scale: ColorScale,
    /// Values at the ends of the colours; `None` uses the data's range.
    pub limits: Option<(f64, f64)>,
    /// The colours run from the scale's high end to its low end (for the
    /// grey scale: black for low values, white for high ones).
    pub reversed: bool,
    /// The vertical axis increases downwards (image rows); otherwise
    /// upwards.
    pub y_down: bool,
    /// One unit is as long across as up.
    pub equal_aspect: bool,
}

impl ImagePlotRequest {
    /// Grey colours from black (low) to white (high), data limits, `y`
    /// downwards, equal units: the picture as the image looked.
    #[must_use]
    pub fn new(x: usize, y: usize, value: usize) -> Self {
        Self { x, y, value, scale: ColorScale::Grey, limits: None, reversed: true, y_down: true, equal_aspect: true }
    }

    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        vec![self.x, self.y, self.value]
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        Self { x: f(self.x), y: f(self.y), value: f(self.value), ..*self }
    }

    /// Project-file form: `{"analysis": "image-plot", "x", "y", "value",
    /// "scale", "limits": [low, high] or null, "reverse", "y_down", "equal_aspect"}`.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        JsonValue::Object(vec![
            ("analysis".to_owned(), s("image-plot")),
            ("x".to_owned(), col(self.x)),
            ("y".to_owned(), col(self.y)),
            ("value".to_owned(), col(self.value)),
            ("scale".to_owned(), s(self.scale.key())),
            (
                "limits".to_owned(),
                self.limits.map_or(JsonValue::Null, |(a, b)| {
                    JsonValue::Array(vec![JsonValue::number(a), JsonValue::number(b)])
                }),
            ),
            ("reverse".to_owned(), JsonValue::Bool(self.reversed)),
            ("y_down".to_owned(), JsonValue::Bool(self.y_down)),
            ("equal_aspect".to_owned(), JsonValue::Bool(self.equal_aspect)),
        ])
    }

    /// Read [`ImagePlotRequest::to_json`]'s form (`scale`, `limits`,
    /// `reverse`, `y_down` and `equal_aspect` optional).
    ///
    /// # Errors
    /// When a column is missing or a setting is not understood.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find = |key: &str| -> Result<usize> {
            let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let mut r = Self::new(find("x")?, find("y")?, find("value")?);
        if let Some(key) = v.get("scale").and_then(JsonValue::as_str) {
            r.scale = ColorScale::from_key(key).ok_or_else(|| invalid(format!("unknown colour scale {key:?}")))?;
        }
        r.limits = match v.get("limits") {
            None | Some(JsonValue::Null) => None,
            Some(JsonValue::Array(a)) if a.len() == 2 => {
                let (lo, hi) = (a[0].as_f64(), a[1].as_f64());
                match (lo, hi) {
                    (Some(lo), Some(hi)) if lo < hi => Some((lo, hi)),
                    _ => return Err(invalid("limits: expected [low, high] with low below high")),
                }
            }
            Some(_) => return Err(invalid("limits: expected [low, high]")),
        };
        if let Some(b) = v.get("reverse").and_then(JsonValue::as_bool) {
            r.reversed = b;
        }
        if let Some(b) = v.get("y_down").and_then(JsonValue::as_bool) {
            r.y_down = b;
        }
        if let Some(b) = v.get("equal_aspect").and_then(JsonValue::as_bool) {
            r.equal_aspect = b;
        }
        Ok(r)
    }
}

/// A computed image plot.
#[derive(Debug, Clone)]
pub struct ImagePlotAnalysis {
    names: [String; 3],
    grid: PixelGrid,
    raster: RasterSpec,
    excluded: usize,
}

impl ImagePlotAnalysis {
    /// Put the value column on the grid of `x` and `y` (rows excluded or
    /// hidden are left blank) and colour it.
    ///
    /// # Errors
    /// When a column is not numeric or the positions do not form an evenly
    /// spaced grid ([`PixelGrid::from_columns`]).
    pub fn run(table: &DataTable, rows: &RowStateManager, request: &ImagePlotRequest) -> Result<Self> {
        let numeric = |i: usize| -> Result<(&[f64], String)> {
            let c = table.column(i).ok_or_else(|| invalid("a column of the image plot is missing"))?;
            let v = c.as_numeric().ok_or_else(|| invalid(format!("{} is not a numeric column", c.name)))?;
            Ok((v, c.name.clone()))
        };
        let (x, xn) = numeric(request.x)?;
        let (y, yn) = numeric(request.y)?;
        let (v, vn) = numeric(request.value)?;
        let excluded_row = |r: usize| r < rows.len() && rows.is_excluded(r);
        let grid = PixelGrid::from_columns(x, y, v, &|r| !excluded_row(r))?;
        let excluded = (0..v.len()).filter(|&r| excluded_row(r) && v[r].is_finite()).count();
        let mut raster = grid.raster(request.y_down);
        raster.x_title.clone_from(&xn);
        raster.y_title.clone_from(&yn);
        raster.value_title.clone_from(&vn);
        raster.scale = request.scale;
        raster.reversed = request.reversed;
        raster.limits = request.limits;
        raster.equal_aspect = request.equal_aspect;
        Ok(Self { names: [xn, yn, vn], grid, raster, excluded })
    }

    /// The drawn grid.
    #[must_use]
    pub fn raster(&self) -> &RasterSpec {
        &self.raster
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Image Plot: {}", self.names[2])
    }

    /// The picture, then a summary of the grid.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let g = &self.grid;
        let (lo, hi) = crate::image_table::value_range(&g.values);
        let (clo, chi) = self.raster.color_limits();
        let mut t = TextTable::new(["Quantity", "Value"]);
        t.push(["Cells across × down".to_owned(), format!("{} × {}", g.cols, g.rows)]);
        t.push([format!("Spacing of {}", self.names[0]), format_number(g.x.1, 6)]);
        t.push([format!("Spacing of {}", self.names[1]), format_number(g.y.1, 6)]);
        t.push(["Cells with a value".to_owned(), g.filled().to_string()]);
        t.push(["Blank cells".to_owned(), (g.cols * g.rows - g.filled()).to_string()]);
        t.push(["Least value".to_owned(), format_number(lo, 6)]);
        t.push(["Greatest value".to_owned(), format_number(hi, 6)]);
        t.push(["Colour scale from".to_owned(), format_number(clo, 6)]);
        t.push(["Colour scale to".to_owned(), format_number(chi, 6)]);
        let mut out = vec![
            section(
                SectionKind::ImagePlot,
                format!("{} over {} and {}", self.names[2], self.names[0], self.names[1]),
                SectionContent::Raster(Box::new(self.raster.clone())),
            ),
            section(SectionKind::ImageSummary, "Grid", SectionContent::Table(t)),
        ];
        if self.excluded > 0 {
            out.push(section(
                SectionKind::ImageSummary,
                "Excluded rows",
                SectionContent::Note(format!(
                    "{} excluded rows are left blank in the picture.",
                    self.excluded
                )),
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        let x: Vec<f64> = (0..6).map(|k| f64::from(k % 3)).collect();
        let y: Vec<f64> = (0..6).map(|k| f64::from(k / 3)).collect();
        let v: Vec<f64> = (0..6).map(f64::from).collect();
        DataTable::new(
            "img",
            vec![Column::numeric("x", x), Column::numeric("y", y), Column::numeric("intensity", v)],
        )
        .unwrap()
    }

    #[test]
    fn the_plot_draws_the_grid_with_excluded_rows_blank() {
        let t = table();
        let mut rows = RowStateManager::new(6);
        rows.set_excluded([4], true);
        let a = ImagePlotAnalysis::run(&t, &rows, &ImagePlotRequest::new(0, 1, 2)).unwrap();
        let r = a.raster();
        assert_eq!((r.cols, r.rows, r.y_down), (3, 2, true));
        assert_eq!(r.values[0], 0.0, "y = 0 at the top");
        assert!(r.values[4].is_nan());
        assert_eq!(r.row_index.as_ref().unwrap()[5], 5);
        assert_eq!((r.x_title.as_str(), r.value_title.as_str()), ("x", "intensity"));
        let s = a.sections();
        assert!(matches!(s[0].content, SectionContent::Raster(_)));
        assert_eq!(s[0].kind, SectionKind::ImagePlot);
        let SectionContent::Table(grid) = &s[1].content else { panic!() };
        assert_eq!(grid.rows[0][1], "3 × 2");
        assert_eq!(grid.rows[4][1], "1");
        assert!(matches!(&s[2].content, SectionContent::Note(n) if n.starts_with("1 excluded")));
        assert_eq!(a.title(), "Image Plot: intensity");
    }

    #[test]
    fn requests_round_trip_through_project_json() {
        let t = table();
        let r = ImagePlotRequest {
            scale: ColorScale::Heat,
            limits: Some((1.0, 4.0)),
            reversed: false,
            y_down: false,
            equal_aspect: false,
            ..ImagePlotRequest::new(0, 1, 2)
        };
        let back = ImagePlotRequest::from_json(&r.to_json(&t), &t).unwrap();
        assert_eq!(back, r);
        let minimal = stat_data::parse_json(r#"{"x": "x", "y": "y", "value": "intensity"}"#).unwrap();
        assert_eq!(ImagePlotRequest::from_json(&minimal, &t).unwrap(), ImagePlotRequest::new(0, 1, 2));
        let bad = stat_data::parse_json(r#"{"x": "x", "y": "y", "value": "intensity", "limits": [3, 1]}"#).unwrap();
        assert!(ImagePlotRequest::from_json(&bad, &t).is_err());
        assert_eq!(r.map_columns(&|c| c + 1).columns(), [1, 2, 3]);
    }
}
