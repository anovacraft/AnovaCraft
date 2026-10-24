//! Area under a sampled curve ([`stat_inference::area`]): the trapezoid
//! area of the mean response at each `x` above a baseline, Bailer's (1988)
//! standard error from replicates, the areas above and below the baseline
//! and the peaks above it, one curve per level of an optional group column.
//!
//! The report can save three transforms of the response as new columns:
//! the response minus its curve's baseline, the response as a percentage of
//! its curve's range (0 at the smallest value, 100 at the largest), and the
//! response as a fraction of its curve's total. The same transforms are
//! formula functions (`subtract_baseline`, `percent_of_range`,
//! `fraction_of_total`), which share [`curve_transform`].

use crate::compare::{DIGITS, section};
use crate::format::format_number;
use crate::project::{invalid, member, obj, s};
use crate::report::{PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_inference::area::{AreaBaseline, AreaUnderCurve, area_under_curve};

/// Most curves (group levels) one report draws a plot for.
const MAX_PLOTS: usize = 12;

/// Settings of an area-under-the-curve report.
#[derive(Debug, Clone, PartialEq)]
pub struct AreaRequest {
    /// Column of `x` values (time, dose, wavelength…).
    pub x: usize,
    /// Column of responses.
    pub y: usize,
    /// Optional column splitting the rows into one curve per level.
    pub group: Option<usize>,
    /// The baseline the area is measured from.
    pub baseline: AreaBaseline,
    /// Confidence level of the interval for the area.
    pub level: f64,
    /// Peaks lower than this share of the tallest are not listed.
    pub min_peak_share: f64,
}

impl AreaRequest {
    /// Defaults: baseline 0, 95% interval, every peak listed.
    #[must_use]
    pub fn new(x: usize, y: usize) -> Self {
        Self { x, y, group: None, baseline: AreaBaseline::Value(0.0), level: 0.95, min_peak_share: 0.0 }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        [self.x, self.y].into_iter().chain(self.group).collect()
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        Self { x: f(self.x), y: f(self.y), group: self.group.map(f), ..self.clone() }
    }

    /// Project-file form: `baseline` is a number, or `baseline_points` the
    /// number of first points whose mean is the baseline.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let (value, points) = match self.baseline {
            AreaBaseline::Value(v) => (JsonValue::number(v), JsonValue::Null),
            AreaBaseline::FirstPoints(k) => (JsonValue::Null, JsonValue::number(k as f64)),
        };
        obj([
            ("analysis", s("area-under-curve")),
            ("x", col(self.x)),
            ("y", col(self.y)),
            ("group", self.group.map_or(JsonValue::Null, col)),
            ("baseline", value),
            ("baseline_points", points),
            ("level", JsonValue::number(self.level)),
            ("min_peak_share", JsonValue::number(self.min_peak_share)),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing columns or settings out of range.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find = |key: &str| -> Result<usize> {
            let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let group = match v.get("group") {
            None | Some(JsonValue::Null) => None,
            Some(_) => Some(find("group")?),
        };
        let baseline = match v.get("baseline_points").and_then(JsonValue::as_f64) {
            Some(k) if k >= 1.0 && k.fract() == 0.0 => AreaBaseline::FirstPoints(k as usize),
            Some(_) => return Err(invalid("area under curve: baseline_points must be a whole number of at least 1")),
            None => AreaBaseline::Value(v.get("baseline").and_then(JsonValue::as_f64).unwrap_or(0.0)),
        };
        let r = Self {
            x: find("x")?,
            y: find("y")?,
            group,
            baseline,
            level: v.get("level").and_then(JsonValue::as_f64).unwrap_or(0.95),
            min_peak_share: v.get("min_peak_share").and_then(JsonValue::as_f64).unwrap_or(0.0),
        };
        if !(r.level > 0.0 && r.level < 1.0) {
            return Err(invalid("area under curve: level must lie between 0 and 1"));
        }
        if !(0.0..=1.0).contains(&r.min_peak_share) {
            return Err(invalid("area under curve: min_peak_share must lie between 0 and 1"));
        }
        Ok(r)
    }

    /// Words for the baseline, as the report and dialog show it.
    #[must_use]
    pub fn baseline_label(&self) -> String {
        match self.baseline {
            AreaBaseline::Value(v) => format!("baseline {}", format_number(v, DIGITS)),
            AreaBaseline::FirstPoints(1) => "baseline at the first point".to_owned(),
            AreaBaseline::FirstPoints(k) => format!("baseline the mean of the first {k} points"),
        }
    }
}

/// A transform of a response, computed within each curve (group).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveTransform {
    /// `100 (y − min) / (max − min)`.
    PercentOfRange,
    /// `y / Σ y`.
    FractionOfTotal,
    /// `y − b`, with `b` the mean of the group's first `k` values.
    SubtractFirst(usize),
}

/// `kind` of `values` within each group (`groups[i]` is row `i`'s group,
/// numbered from 0, in row order). Missing values (NaN) stay missing and are
/// left out of the minimum, maximum, total and baseline. A group whose range
/// or total is zero, or with fewer than `k` values, gives NaN.
#[must_use]
pub fn curve_transform(values: &[f64], groups: &[usize], kind: CurveTransform) -> Vec<f64> {
    let n_groups = groups.iter().max().map_or(0, |m| m + 1);
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); n_groups];
    for (i, (&g, v)) in groups.iter().zip(values).enumerate() {
        if !v.is_nan() {
            members[g].push(i);
        }
    }
    let mut out = vec![f64::NAN; values.len()];
    for m in &members {
        let ys: Vec<f64> = m.iter().map(|&i| values[i]).collect();
        let f: Box<dyn Fn(f64) -> f64> = match kind {
            CurveTransform::PercentOfRange => {
                let lo = ys.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                Box::new(move |y| if hi > lo { 100.0 * (y - lo) / (hi - lo) } else { f64::NAN })
            }
            CurveTransform::FractionOfTotal => {
                let total: f64 = ys.iter().sum();
                Box::new(move |y| if total == 0.0 { f64::NAN } else { y / total })
            }
            CurveTransform::SubtractFirst(k) => {
                let b = if k >= 1 && ys.len() >= k { ys[..k].iter().sum::<f64>() / k as f64 } else { f64::NAN };
                Box::new(move |y| y - b)
            }
        };
        for (&i, &y) in m.iter().zip(&ys) {
            out[i] = f(y);
        }
    }
    out
}

/// One curve of the report.
#[derive(Debug, Clone)]
struct Curve {
    name: String,
    /// Rows used, in table order.
    rows: Vec<usize>,
    result: Result<AreaUnderCurve>,
}

/// A computed area-under-the-curve report.
#[derive(Debug, Clone)]
pub struct AreaAnalysis {
    request: AreaRequest,
    x_name: String,
    y_name: String,
    group_name: Option<String>,
    n_rows: usize,
    x: Vec<f64>,
    y: Vec<f64>,
    curves: Vec<Curve>,
}

impl AreaAnalysis {
    /// Split the rows into curves and measure each one's area.
    ///
    /// # Errors
    /// Non-numeric `x` or response columns, or no usable rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &AreaRequest) -> Result<Self> {
        let numeric = |i: usize| -> Result<(&str, &[f64])> {
            let c = table.column(i).ok_or_else(|| invalid("no such column"))?;
            let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?;
            Ok((c.name.as_str(), v))
        };
        let (x_name, x) = numeric(request.x)?;
        let (y_name, y) = numeric(request.y)?;
        let group = match request.group {
            Some(g) => Some(table.column(g).ok_or_else(|| invalid("no such column"))?),
            None => None,
        };
        let rows: Vec<usize> =
            (0..table.n_rows()).filter(|&r| !states.is_excluded(r) && x[r].is_finite() && y[r].is_finite()).collect();
        if rows.is_empty() {
            return Err(invalid(format!("no rows have both {x_name} and {y_name}")));
        }
        let mut names: Vec<String> = Vec::new();
        let mut members: Vec<Vec<usize>> = Vec::new();
        for &r in &rows {
            let name = group.map_or_else(String::new, |g| g.level_text(r));
            match names.iter().position(|n| *n == name) {
                Some(i) => members[i].push(r),
                None => {
                    names.push(name);
                    members.push(vec![r]);
                }
            }
        }
        let curves = names
            .into_iter()
            .zip(members)
            .map(|(name, rows)| {
                let xs: Vec<f64> = rows.iter().map(|&r| x[r]).collect();
                let ys: Vec<f64> = rows.iter().map(|&r| y[r]).collect();
                let result = area_under_curve(&xs, &ys, request.baseline, request.level, request.min_peak_share);
                Curve { name, rows, result }
            })
            .collect();
        Ok(Self {
            request: request.clone(),
            x_name: x_name.to_owned(),
            y_name: y_name.to_owned(),
            group_name: group.map(|g| g.name.clone()),
            n_rows: table.n_rows(),
            x: x.to_vec(),
            y: y.to_vec(),
            curves,
        })
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.group_name {
            Some(g) => format!("Area under the Curve: {} by {} ({g})", self.y_name, self.x_name),
            None => format!("Area under the Curve: {} by {}", self.y_name, self.x_name),
        }
    }

    /// The area of each curve, in order of first appearance (`""` names the
    /// only curve when there is no group column).
    #[must_use]
    pub fn areas(&self) -> Vec<(&str, Option<&AreaUnderCurve>)> {
        self.curves.iter().map(|c| (c.name.as_str(), c.result.as_ref().ok())).collect()
    }

    /// The response minus its curve's baseline, as a percentage of its
    /// curve's range, and as a fraction of its curve's total, for the rows
    /// used (other rows missing).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let mut groups = vec![usize::MAX; self.n_rows];
        for (g, c) in self.curves.iter().enumerate() {
            for &r in &c.rows {
                groups[r] = g;
            }
        }
        let used: Vec<usize> = (0..self.n_rows).filter(|&r| groups[r] != usize::MAX).collect();
        let y: Vec<f64> = used.iter().map(|&r| self.y[r]).collect();
        let g: Vec<usize> = used.iter().map(|&r| groups[r]).collect();
        let spread = |v: Vec<f64>| {
            let mut out = vec![f64::NAN; self.n_rows];
            for (&r, x) in used.iter().zip(v) {
                out[r] = x;
            }
            out
        };
        let baseline: Vec<f64> = used
            .iter()
            .map(|&r| {
                let c = &self.curves[groups[r]];
                c.result.as_ref().map_or(f64::NAN, |a| self.y[r] - a.baseline)
            })
            .collect();
        vec![
            ReportColumn::continuous(
                "baseline_subtracted",
                "Save response minus baseline",
                format!("{} minus baseline", self.y_name),
                spread(baseline),
            ),
            ReportColumn::continuous(
                "percent_of_range",
                "Save response as % of its range",
                format!("{} (% of range)", self.y_name),
                spread(curve_transform(&y, &g, CurveTransform::PercentOfRange)),
            ),
            ReportColumn::continuous(
                "fraction_of_total",
                "Save response as a fraction of its total",
                format!("{} (fraction of total)", self.y_name),
                spread(curve_transform(&y, &g, CurveTransform::FractionOfTotal)),
            ),
        ]
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let r = &self.request;
        let p = format_number(100.0 * r.level, 4);
        let grouped = self.group_name.is_some();
        let curve_heading = self.group_name.clone().unwrap_or_else(|| "Curve".to_owned());
        let mut head = vec![curve_heading.clone()];
        head.extend(
            [
                "Points",
                "N",
                "Baseline",
                "Net area",
                "Std error",
                &format!("Lower {p}%"),
                &format!("Upper {p}%"),
                "Area above",
                "Area below",
            ]
            .map(str::to_owned),
        );
        let mut t = TextTable::new(head);
        let blank = |n: usize| (0..n).map(|_| String::new());
        for c in &self.curves {
            let name = if grouped { c.name.clone() } else { self.y_name.clone() };
            let mut row = vec![name];
            match &c.result {
                Ok(a) => {
                    let (lo, hi) = a.interval.map_or((String::new(), String::new()), |(l, h)| {
                        (format_number(l, DIGITS), format_number(h, DIGITS))
                    });
                    row.extend([
                        a.x.len().to_string(),
                        c.rows.len().to_string(),
                        format_number(a.baseline, DIGITS),
                        format_number(a.net_area, DIGITS),
                        a.std_error.map_or_else(|| "—".to_owned(), |s| format_number(s, DIGITS)),
                        lo,
                        hi,
                        format_number(a.area_above, DIGITS),
                        format_number(a.area_below, DIGITS),
                    ]);
                }
                Err(e) => {
                    row.push(format!("not available: {e}"));
                    row.extend(blank(8));
                }
            }
            t.push(row);
        }
        let mut out = vec![section(
            SectionKind::AreaUnderCurve,
            format!(
                "Area under the Curve (trapezoid rule on the mean {} at each {}; {}; standard error from \
                 replicates, Bailer 1988; — without two or more values at every {})",
                self.y_name,
                self.x_name,
                r.baseline_label(),
                self.x_name
            ),
            SectionContent::Table(t),
        )];
        // Peaks.
        let mut head = Vec::new();
        if grouped {
            head.push(curve_heading);
        }
        head.extend(["Start", "End", "Peak at", "Height above baseline", "Area", "% of area above"].map(str::to_owned));
        let mut peaks = TextTable::new(head);
        for c in &self.curves {
            let Ok(a) = &c.result else { continue };
            for pk in &a.peaks {
                let mut row = Vec::new();
                if grouped {
                    row.push(c.name.clone());
                }
                row.extend([
                    format_number(pk.start, DIGITS),
                    format_number(pk.end, DIGITS),
                    format_number(pk.peak_x, DIGITS),
                    format_number(pk.height, DIGITS),
                    format_number(pk.area, DIGITS),
                    if a.area_above > 0.0 { format_number(100.0 * pk.area / a.area_above, 4) } else { String::new() },
                ]);
                peaks.push(row);
            }
        }
        let share = if r.min_peak_share > 0.0 {
            format!("; peaks lower than {}% of the tallest left out", format_number(100.0 * r.min_peak_share, 4))
        } else {
            String::new()
        };
        out.push(section(
            SectionKind::AreaPeaks,
            format!("Peaks above the Baseline (regions between crossings{share})"),
            SectionContent::Table(peaks),
        ));
        for c in self.curves.iter().take(MAX_PLOTS) {
            let Ok(a) = &c.result else { continue };
            let points = c.rows.iter().map(|&row| RowPoint { row, x: self.x[row], y: self.y[row] }).collect();
            let curve: Vec<[f64; 2]> = a.x.iter().zip(&a.mean).map(|(&x, &m)| [x, m]).collect();
            let (x0, x1) = (a.x[0], a.x[a.x.len() - 1]);
            let spec = PlotSpec {
                x_label: self.x_name.clone(),
                y_label: self.y_name.clone(),
                points,
                line: Some(curve),
                line_name: Some(format!("Mean {} (joined by straight lines)", self.y_name)),
                segments: vec![vec![[x0, a.baseline], [x1, a.baseline]]],
                point_name: Some("Observations (rows)".to_owned()),
                ..PlotSpec::default()
            };
            let title = if grouped {
                format!(
                    "Curve: {} = {} (thin line: baseline {})",
                    self.group_name.as_deref().unwrap_or(""),
                    c.name,
                    format_number(a.baseline, DIGITS)
                )
            } else {
                format!("Curve (thin line: baseline {})", format_number(a.baseline, DIGITS))
            };
            out.push(section(SectionKind::AreaPlot, title, SectionContent::Plot(Box::new(spec))));
        }
        if self.curves.len() > MAX_PLOTS {
            out.push(section(
                SectionKind::AreaPlot,
                "More Curves",
                SectionContent::Note(format!(
                    "Plots are drawn for the first {MAX_PLOTS} of {} curves; the tables list them all.",
                    self.curves.len()
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

    /// Bailer's example of `stat_inference::area` (R values in
    /// `crates/stat-inference/tests/data/area_reference.R`), twice under a
    /// group column, with one excluded row that changes nothing but N.
    fn table() -> DataTable {
        let x = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 4.0, 4.0, 4.0, 8.0, 8.0, 8.0];
        let y = [2.0, 3.0, 2.5, 20.0, 24.0, 19.0, 15.0, 13.0, 17.0, 9.0, 7.0, 8.5, 3.0, 2.0, 4.0];
        let mut xs = x.to_vec();
        xs.extend(x);
        xs.push(1.0);
        let mut ys = y.to_vec();
        ys.extend(y.map(|v| 2.0 * v));
        ys.push(1000.0);
        let mut g: Vec<String> = (0..15).map(|_| "A".to_owned()).collect();
        g.extend((0..16).map(|_| "B".to_owned()));
        DataTable::new("t", vec![Column::numeric("t", xs), Column::numeric("c", ys), Column::text("g", g)]).unwrap()
    }

    #[test]
    fn areas_by_group_match_r() {
        let t = table();
        let mut rows = RowStateManager::new(31);
        rows.set_excluded([30], true);
        let req = AreaRequest { group: Some(2), ..AreaRequest::new(0, 1) };
        let a = AreaAnalysis::run(&t, &rows, &req).unwrap();
        let areas = a.areas();
        assert_eq!(areas.len(), 2);
        let (name, first) = areas[0];
        assert_eq!(name, "A");
        let first = first.unwrap();
        assert!((first.net_area - 75.25).abs() < 1e-12);
        assert!((first.std_error.unwrap() - 3.152_380_053_229_623).abs() < 1e-12);
        let second = areas[1].1.unwrap();
        assert!((second.net_area - 150.5).abs() < 1e-12, "doubling the response doubles the area");
        assert!((second.std_error.unwrap() - 2.0 * 3.152_380_053_229_623).abs() < 1e-12);
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [SectionKind::AreaUnderCurve, SectionKind::AreaPeaks, SectionKind::AreaPlot, SectionKind::AreaPlot]
        );
        let SectionContent::Table(tab) = &s[0].content else { panic!() };
        assert_eq!(tab.rows[1][0], "B");
        assert_eq!(tab.rows[1][2], "15", "the excluded row is left out");
        // One peak per curve spanning the data (all above 0).
        let SectionContent::Table(peaks) = &s[1].content else { panic!() };
        assert_eq!(peaks.rows.len(), 2);
        assert_eq!(peaks.rows[0][3], "1", "peak at x = 1");
        // Baseline from the first two points: R AUC_B = −18.75.
        let req = AreaRequest { baseline: AreaBaseline::FirstPoints(2), ..req };
        let a = AreaAnalysis::run(&t, &rows, &req).unwrap();
        assert!((a.areas()[0].1.unwrap().net_area + 18.75).abs() < 1e-12);
        let cols = a.saveable_columns();
        assert_eq!(
            cols.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
            ["baseline_subtracted", "percent_of_range", "fraction_of_total"]
        );
        // Baseline of A is (2.5 + 21) / 2 = 11.75.
        assert!((cols[0].values[0] - (2.0 - 11.75)).abs() < 1e-12);
        // A's range is 2 … 24: 24 → 100 %, 2 → 0 %.
        assert!((cols[1].values[4] - 100.0).abs() < 1e-12);
        assert!(cols[1].values[0].abs() < 1e-12);
        assert!(cols[1].values[30].is_nan(), "the excluded row");
        let total_a: f64 =
            [2.0, 3.0, 2.5, 20.0, 24.0, 19.0, 15.0, 13.0, 17.0, 9.0, 7.0, 8.5, 3.0, 2.0, 4.0].iter().sum();
        assert!((cols[2].values[3] - 20.0 / total_a).abs() < 1e-15);
    }

    #[test]
    fn transforms_by_group_and_json() {
        let v = [1.0, 3.0, f64::NAN, 5.0, 10.0, 20.0];
        let g = [0, 0, 0, 0, 1, 1];
        let pr = curve_transform(&v, &g, CurveTransform::PercentOfRange);
        assert_eq!(pr[..2], [0.0, 50.0]);
        assert!(pr[2].is_nan());
        assert_eq!(pr[3..], [100.0, 0.0, 100.0]);
        let ft = curve_transform(&v, &g, CurveTransform::FractionOfTotal);
        assert!((ft[1] - 3.0 / 9.0).abs() < 1e-15 && (ft[5] - 20.0 / 30.0).abs() < 1e-15);
        let sb = curve_transform(&v, &g, CurveTransform::SubtractFirst(2));
        assert_eq!([sb[0], sb[1], sb[3]], [-1.0, 1.0, 3.0]);
        assert_eq!([sb[4], sb[5]], [-5.0, 5.0]);
        assert!(curve_transform(&[1.0], &[0], CurveTransform::SubtractFirst(2))[0].is_nan());

        let t = table();
        let req = AreaRequest {
            group: Some(2),
            baseline: AreaBaseline::FirstPoints(3),
            level: 0.9,
            min_peak_share: 0.25,
            ..AreaRequest::new(0, 1)
        };
        assert_eq!(AreaRequest::from_json(&req.to_json(&t), &t).unwrap(), req);
        let plain = AreaRequest { baseline: AreaBaseline::Value(1.5), ..AreaRequest::new(0, 1) };
        assert_eq!(AreaRequest::from_json(&plain.to_json(&t), &t).unwrap(), plain);
        let mut bad = req.to_json(&t);
        if let JsonValue::Object(m) = &mut bad {
            m.retain(|(k, _)| k != "level");
            m.push(("level".to_owned(), JsonValue::number(2.0)));
        }
        assert!(AreaRequest::from_json(&bad, &t).is_err());
    }
}
