//! Plot styles in project files, dashboards, scripts and graph files.
//!
//! A [`PlotStyle`] (see `stat_viz::plot_style`) is written as one JSON object:
//!
//! ```text
//! {
//!   "x": {axis}, "y": {axis}, "y2": {axis},
//!   "equal_scale": false, "category_labels": "automatic",
//!   "hover_coordinates": true, "show_key": true,
//!   "title": "…" | null, "show_title": true,
//!   "scale_ratio": 1, "print_scale": millimetres | null,
//!   "export": {…}, "reference_lines": {"lines": [line, …], "bands": [band, …]},
//!   "annotations": {…}, "key": {…}
//! }
//! line = {"orientation": "horizontal" | "vertical", "value": value, "label": "…", "show_value": true,
//!         "color": "#rrggbb" | "#rrggbbaa" | null, "width": 1.5,
//!         "pattern": "solid" | "dashed" | "dotted" | "dash-dot", "scope": "whole" | "panel" | "group"}
//! band = {"orientation": …, "from": value, "to": value, "label": "…", "color": … | null,
//!         "opacity": 0.18, "edges": true, "edge_labels": true, "scope": …}
//! value = number | {"stat": "mean" | "median" | "quantile" | "mean-sd" | "individuals-centre"
//!                            | "individuals-upper" | "individuals-lower", "parameter": p or k}
//! axis = {"major": "auto" | interval, "minor": "none" | "auto" | {"subdivisions": n} | {"every": interval},
//!         "origin": 0, "grid": "major" | "major-and-minor" | "none",
//!         "tick_marks": true, "tick_labels": true,
//!         "format": "auto" | {"decimals": n} | {"scientific": n} | {"percent": n},
//!         "title": "…" | null, "show_title": true, "side": "standard" | "opposite",
//!         "break": [from, to] | null,
//!         "breaks": {"at": [[from, to], …], "mark": "slashes" | "zigzag",
//!                    "segments": [{"scale": "log10" | null, "share": percent | null}, …]},
//!         "tick_direction": "inside" | "outside" | "both",
//!         "major_mark": {"length": 5, "width": 1}, "minor_mark": {"length": 3, "width": 1},
//!         "offset": points, "extend_to_ticks": false,
//!         "custom_ticks": [{"value": v, "label": "\\pi"}, …], "range": [min, max] | null}
//! ```
//!
//! Every settings member is written, so a style read back is the style
//! written whatever the plot's own defaults are; members left out of a file
//! keep the value they have in the style the file is read onto. The four
//! formatting parts are written only when they differ from their defaults.
//! Graph canvases keep their titles and breaks in their own `"axes"` member,
//! so their style is written without them ([`StyleTitles::Without`]).
//!
//! `"break"` is an axis's first break, kept for files read by earlier
//! versions; `"breaks"` (written when anything beyond one plain break is set)
//! holds every break, each piece's scale (`null`: the axis's own) and share
//! of the axis in percent (`null`: automatic), and the break-mark style, and
//! wins when both are present.

use stat_data::JsonValue;
use stat_viz::CategoryLabels;
use stat_viz::axis_settings::{
    AxisSettings, AxisSide, CustomTick, GridLines, MajorTicks, MinorTicks, TickDirection, TickFormat, TickMarkSize,
};
use stat_viz::element::reference::{
    ReferenceBand, ReferenceLine, ReferenceLook, ReferenceOrientation, ReferenceScope, ReferenceStat, ReferenceValue,
};
use stat_viz::{Annotation, AnnotationAnchor, AnnotationShape, LinePattern, Rgba};
pub use stat_viz::plot_style::{AnnotationLook, ExportLook, FigureSize, KeyLook, PlotFont, PlotStyle, user_axes};
use stat_viz::{AxisBreaks, BreakMark};
use stat_viz::figure_page::LengthUnit;

use crate::project::invalid;

/// Whether titles and axis breaks are part of a written style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleTitles {
    /// Write them (report plots).
    With,
    /// Leave them out (graph canvases, which keep them elsewhere).
    Without,
}

fn s(text: &str) -> JsonValue {
    JsonValue::String(text.to_owned())
}

fn obj(members: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn opt_text(t: Option<&String>) -> JsonValue {
    t.map_or(JsonValue::Null, |t| JsonValue::String(t.clone()))
}

fn pair(p: Option<[f64; 2]>) -> JsonValue {
    p.map_or(JsonValue::Null, |[lo, hi]| JsonValue::Array(vec![JsonValue::number(lo), JsonValue::number(hi)]))
}

fn mark_to_json(m: TickMarkSize) -> JsonValue {
    obj(vec![("length", JsonValue::number(m.length)), ("width", JsonValue::number(m.width))])
}

fn direction_name(d: TickDirection) -> &'static str {
    match d {
        TickDirection::Inside => "inside",
        TickDirection::Outside => "outside",
        TickDirection::Both => "both",
    }
}

fn axis_to_json(a: &AxisSettings, titles: StyleTitles) -> JsonValue {
    let major = match a.major {
        MajorTicks::Auto => s("auto"),
        MajorTicks::Every(v) => JsonValue::number(v),
    };
    let minor = match a.minor {
        MinorTicks::None => s("none"),
        MinorTicks::Auto => s("auto"),
        MinorTicks::Subdivisions(n) => obj(vec![("subdivisions", JsonValue::number(f64::from(n)))]),
        MinorTicks::Every(v) => obj(vec![("every", JsonValue::number(v))]),
    };
    let grid = match a.grid {
        GridLines::MajorAndMinor => "major-and-minor",
        GridLines::MajorOnly => "major",
        GridLines::None => "none",
    };
    let format = match a.format {
        TickFormat::Auto => s("auto"),
        TickFormat::Decimals(n) => obj(vec![("decimals", JsonValue::number(f64::from(n)))]),
        TickFormat::Scientific(n) => obj(vec![("scientific", JsonValue::number(f64::from(n)))]),
        TickFormat::Percent(n) => obj(vec![("percent", JsonValue::number(f64::from(n)))]),
    };
    let side = match a.side {
        AxisSide::Standard => "standard",
        AxisSide::Opposite => "opposite",
    };
    let mut m = vec![
        ("major", major),
        ("minor", minor),
        ("origin", JsonValue::number(a.origin)),
        ("grid", s(grid)),
        ("tick_marks", JsonValue::Bool(a.tick_marks)),
        ("tick_labels", JsonValue::Bool(a.tick_labels)),
        ("format", format),
        ("show_title", JsonValue::Bool(a.show_title)),
        ("side", s(side)),
        ("tick_direction", s(direction_name(a.tick_direction))),
        ("major_mark", mark_to_json(a.major_mark)),
        ("minor_mark", mark_to_json(a.minor_mark)),
        ("offset", JsonValue::number(a.offset)),
        ("extend_to_ticks", JsonValue::Bool(a.extend_to_ticks)),
        (
            "custom_ticks",
            JsonValue::Array(
                a.custom_ticks
                    .iter()
                    .map(|c| obj(vec![("value", JsonValue::number(c.value)), ("label", s(&c.label))]))
                    .collect(),
            ),
        ),
        ("range", pair(a.range)),
    ];
    if titles == StyleTitles::With {
        m.push(("title", opt_text(a.title.as_ref())));
        m.push(("break", pair(a.breaks.breaks.first().map(|b| [b.from, b.to]))));
        if !is_plain_break(&a.breaks) {
            m.push(("breaks", breaks_to_json(&a.breaks)));
        }
    }
    obj(m)
}

/// The JSON form of `style` (see the module documentation).
#[must_use]
pub fn plot_style_to_json(style: &PlotStyle, titles: StyleTitles) -> JsonValue {
    let a = &style.axes;
    let labels = match a.category_labels {
        CategoryLabels::Automatic => "automatic",
        CategoryLabels::Horizontal => "horizontal",
        CategoryLabels::Slanted => "slanted",
        CategoryLabels::Vertical => "vertical",
    };
    let mut m = vec![
        ("x", axis_to_json(&a.x, titles)),
        ("y", axis_to_json(&a.y, titles)),
        ("y2", axis_to_json(&a.y2, titles)),
        ("equal_scale", JsonValue::Bool(a.equal_scale)),
        ("category_labels", s(labels)),
        ("hover_coordinates", JsonValue::Bool(a.hover_coordinates)),
        ("show_key", JsonValue::Bool(a.show_key)),
        ("show_title", JsonValue::Bool(a.show_title)),
        ("scale_ratio", JsonValue::number(a.scale_ratio)),
        ("print_scale", a.print_scale.map_or(JsonValue::Null, JsonValue::number)),
    ];
    if titles == StyleTitles::With {
        m.push(("title", opt_text(a.title.as_ref())));
    }
    // The formatting parts, each only when set (their members are added with
    // the parts' settings).
    if !style.export.is_default() {
        m.push(("export", export_to_json(&style.export)));
    }
    if !style.reference.is_default() {
        m.push(("reference_lines", reference_to_json(&style.reference)));
    }
    if !style.annotations.is_default() {
        m.push(("annotations", annotations_to_json(&style.annotations)));
    }
    if !style.key.is_default() {
        m.push(("key", key_to_json(&style.key)));
    }
    obj(m)
}

/// The JSON form of a figure look:
/// `{"font": "sans" | "serif", "text_size": 1, "transparent": false,
///   "size": {"width": w, "height": h, "unit": "mm" | "in"} | null,
///   "lock_ratio": true, "ppi": 300}`.
fn export_to_json(e: &ExportLook) -> JsonValue {
    let size = e.size.map_or(JsonValue::Null, |z| {
        obj(vec![
            ("width", JsonValue::number(z.width)),
            ("height", JsonValue::number(z.height)),
            ("unit", s(z.unit.label())),
        ])
    });
    obj(vec![
        ("font", s(e.font.key())),
        ("text_size", JsonValue::number(e.text_size)),
        ("transparent", JsonValue::Bool(e.transparent)),
        ("size", size),
        ("lock_ratio", JsonValue::Bool(e.lock_ratio)),
        ("ppi", JsonValue::number(e.ppi)),
    ])
}

fn export_from_json(v: &JsonValue, e: &mut ExportLook) -> stat_core::Result<()> {
    if !matches!(v, JsonValue::Object(_)) {
        return Err(invalid("plot style: export must be an object"));
    }
    if let Some(m) = present(v, "font") {
        e.font = m
            .as_str()
            .and_then(PlotFont::from_key)
            .ok_or_else(|| invalid("plot style: font must be sans or serif"))?;
    }
    if let Some(m) = present(v, "text_size") {
        e.text_size = Some(finite(m, "text_size")?)
            .filter(|x| ExportLook::TEXT_SIZES.contains(x))
            .ok_or_else(|| invalid("plot style: text_size must be from 0.5 to 3"))?;
    }
    read_bool(v, "transparent", &mut e.transparent)?;
    if let Some(m) = v.get("size") {
        e.size = match m {
            JsonValue::Null => None,
            _ => {
                let side = |key: &str| {
                    present(m, key)
                        .ok_or_else(|| invalid(format!("plot style: size needs a {key}")))
                        .and_then(|x| finite(x, key))
                        .and_then(|x| (x > 0.0).then_some(x).ok_or_else(|| invalid(format!("plot style: bad {key}"))))
                };
                let unit = match present(m, "unit").and_then(JsonValue::as_str) {
                    Some("mm") => LengthUnit::Millimetres,
                    Some("in") => LengthUnit::Inches,
                    _ => return Err(invalid("plot style: size unit must be mm or in")),
                };
                Some(FigureSize { width: side("width")?, height: side("height")?, unit })
            }
        };
    }
    read_bool(v, "lock_ratio", &mut e.lock_ratio)?;
    if let Some(m) = present(v, "ppi") {
        e.ppi = Some(finite(m, "ppi")?)
            .filter(|x| ExportLook::PPI.contains(x))
            .ok_or_else(|| invalid("plot style: ppi must be from 36 to 1200"))?;
    }
    Ok(())
}

/// A member that is present and not null.
fn present<'a>(v: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    v.get(key).filter(|m| !matches!(m, JsonValue::Null))
}

fn read_bool(v: &JsonValue, key: &str, slot: &mut bool) -> stat_core::Result<()> {
    if let Some(m) = present(v, key) {
        *slot = m.as_bool().ok_or_else(|| invalid(format!("plot style: {key} must be true or false")))?;
    }
    Ok(())
}

/// A whole number 0–`max` from `v`.
fn count(v: &JsonValue, what: &str, max: u32) -> stat_core::Result<u32> {
    v.as_f64()
        .filter(|n| n.fract() == 0.0 && (0.0..=f64::from(max)).contains(n))
        .map(|n| n as u32)
        .ok_or_else(|| invalid(format!("plot style: {what} must be a whole number from 0 to {max}")))
}

fn finite(v: &JsonValue, what: &str) -> stat_core::Result<f64> {
    v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| invalid(format!("plot style: {what} must be a number")))
}

fn read_axis(v: &JsonValue, a: &mut AxisSettings, titles: StyleTitles) -> stat_core::Result<()> {
    if let Some(m) = present(v, "major") {
        a.major = match m.as_str() {
            Some("auto") => MajorTicks::Auto,
            _ => MajorTicks::Every(
                Some(finite(m, "major")?).filter(|x| *x > 0.0).ok_or_else(|| invalid("plot style: bad major"))?,
            ),
        };
    }
    if let Some(m) = present(v, "minor") {
        a.minor = match m.as_str() {
            Some("none") => MinorTicks::None,
            Some("auto") => MinorTicks::Auto,
            _ => {
                if let Some(n) = present(m, "subdivisions") {
                    MinorTicks::Subdivisions(count(n, "minor subdivisions", 100)?.max(2))
                } else if let Some(x) = present(m, "every") {
                    MinorTicks::Every(finite(x, "minor interval")?.abs())
                } else {
                    return Err(invalid("plot style: bad minor"));
                }
            }
        };
    }
    if let Some(m) = present(v, "origin") {
        a.origin = finite(m, "origin")?;
    }
    if let Some(m) = present(v, "grid") {
        a.grid = match m.as_str() {
            Some("major-and-minor") => GridLines::MajorAndMinor,
            Some("major") => GridLines::MajorOnly,
            Some("none") => GridLines::None,
            _ => return Err(invalid("plot style: grid must be major, major-and-minor or none")),
        };
    }
    read_bool(v, "tick_marks", &mut a.tick_marks)?;
    read_bool(v, "tick_labels", &mut a.tick_labels)?;
    if let Some(m) = present(v, "format") {
        a.format = if m.as_str() == Some("auto") {
            TickFormat::Auto
        } else if let Some(n) = present(m, "decimals") {
            TickFormat::Decimals(count(n, "decimals", 15)?)
        } else if let Some(n) = present(m, "scientific") {
            TickFormat::Scientific(count(n, "scientific decimals", 15)?)
        } else if let Some(n) = present(m, "percent") {
            TickFormat::Percent(count(n, "percent decimals", 15)?)
        } else {
            return Err(invalid("plot style: bad format"));
        };
    }
    read_bool(v, "show_title", &mut a.show_title)?;
    if let Some(m) = present(v, "side") {
        a.side = match m.as_str() {
            Some("standard") => AxisSide::Standard,
            Some("opposite") => AxisSide::Opposite,
            _ => return Err(invalid("plot style: side must be standard or opposite")),
        };
    }
    if let Some(m) = present(v, "tick_direction") {
        a.tick_direction = match m.as_str() {
            Some("inside") => TickDirection::Inside,
            Some("outside") => TickDirection::Outside,
            Some("both") => TickDirection::Both,
            _ => return Err(invalid("plot style: tick_direction must be inside, outside or both")),
        };
    }
    for (key, slot) in [("major_mark", &mut a.major_mark), ("minor_mark", &mut a.minor_mark)] {
        if let Some(m) = present(v, key) {
            if !matches!(m, JsonValue::Object(_)) {
                return Err(invalid(format!("plot style: {key} must be an object")));
            }
            if let Some(l) = present(m, "length") {
                slot.length = finite(l, "tick mark length")?;
            }
            if let Some(w) = present(m, "width") {
                slot.width = finite(w, "tick mark width")?;
            }
            *slot = slot.clamped();
        }
    }
    if let Some(m) = present(v, "offset") {
        a.offset = finite(m, "offset")?.clamp(0.0, stat_viz::axis_settings::MAX_AXIS_OFFSET);
    }
    read_bool(v, "extend_to_ticks", &mut a.extend_to_ticks)?;
    if let Some(m) = present(v, "custom_ticks") {
        let list = m.as_array().ok_or_else(|| invalid("plot style: custom_ticks must be a list"))?;
        a.custom_ticks = list
            .iter()
            .map(|c| {
                let value = finite(c.get("value").unwrap_or(&JsonValue::Null), "custom tick value")?;
                let label = c
                    .get("label")
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| invalid("plot style: each custom tick needs a label"))?;
                Ok(CustomTick { value, label: label.to_owned() })
            })
            .collect::<stat_core::Result<_>>()?;
    }
    if let Some(r) = v.get("range") {
        a.range = read_pair(r, "range")?.filter(|[lo, hi]| lo < hi);
    }
    if titles == StyleTitles::With {
        if let Some(t) = v.get("title") {
            a.title = t.as_str().map(str::to_owned);
        }
        if let Some(b) = present(v, "breaks") {
            a.breaks = breaks_from_json(b)?;
        } else if let Some(b) = v.get("break") {
            a.breaks = read_pair(b, "break")?.map_or_else(AxisBreaks::default, |[from, to]| AxisBreaks::single(from, to));
        }
    }
    Ok(())
}

/// `true` when `b` is nothing or one break with nothing else set (which the
/// `"break"` pair holds in full).
pub(crate) fn is_plain_break(b: &AxisBreaks) -> bool {
    b.breaks.len() <= 1 && b.segments.iter().all(stat_viz::AxisSegment::is_default) && b.mark == BreakMark::default()
}

/// The JSON form of an axis's breaks (see the module documentation).
pub(crate) fn breaks_to_json(b: &AxisBreaks) -> JsonValue {
    let at = b
        .breaks
        .iter()
        .map(|x| JsonValue::Array(vec![JsonValue::number(x.from), JsonValue::number(x.to)]))
        .collect();
    let segments = b
        .segments
        .iter()
        .map(|g| {
            obj(vec![
                ("scale", g.scale.map_or(JsonValue::Null, |sc| s(sc.key()))),
                ("share", g.share.map_or(JsonValue::Null, JsonValue::number)),
            ])
        })
        .collect();
    obj(vec![("at", JsonValue::Array(at)), ("mark", s(b.mark.key())), ("segments", JsonValue::Array(segments))])
}

/// Read [`breaks_to_json`].
///
/// # Errors
/// A member of the wrong kind, an unknown scale or mark, or a share that is
/// not a positive number.
pub(crate) fn breaks_from_json(v: &JsonValue) -> stat_core::Result<AxisBreaks> {
    if !matches!(v, JsonValue::Object(_)) {
        return Err(invalid("plot style: breaks must be an object"));
    }
    let mut out = AxisBreaks::default();
    if let Some(at) = present(v, "at") {
        let list = at.as_array().ok_or_else(|| invalid("plot style: breaks.at must be a list"))?;
        for b in list {
            if let Some([from, to]) = read_pair(b, "break")? {
                out.breaks.push(stat_viz::AxisBreak { from: from.min(to), to: from.max(to) });
            }
        }
    }
    if let Some(m) = present(v, "mark") {
        out.mark = m
            .as_str()
            .and_then(BreakMark::from_key)
            .ok_or_else(|| invalid("plot style: break mark must be slashes or zigzag"))?;
    }
    if let Some(list) = present(v, "segments") {
        let list = list.as_array().ok_or_else(|| invalid("plot style: breaks.segments must be a list"))?;
        for g in list {
            let scale = match present(g, "scale") {
                None => None,
                Some(sc) => Some(
                    sc.as_str()
                        .and_then(stat_viz::AxisScale::from_key)
                        .ok_or_else(|| invalid("plot style: unknown segment scale"))?,
                ),
            };
            let share = match present(g, "share") {
                None => None,
                Some(x) => Some(finite(x, "segment share")?).filter(|x| *x > 0.0).map(|x| x.min(100.0)),
            };
            out.segments.push(stat_viz::AxisSegment { scale, share });
        }
    }
    Ok(out)
}

/// `null` or `[a, b]` of finite numbers.
fn read_pair(v: &JsonValue, what: &str) -> stat_core::Result<Option<[f64; 2]>> {
    match v {
        JsonValue::Null => Ok(None),
        JsonValue::Array(ends) if ends.len() == 2 => Ok(Some([finite(&ends[0], what)?, finite(&ends[1], what)?])),
        _ => Err(invalid(format!("plot style: {what} must be [from, to]"))),
    }
}

/// Read [`plot_style_to_json`] onto `style`: members left out keep their
/// value in `style`.
///
/// # Errors
/// A member of the wrong kind or out of range.
pub fn plot_style_from_json(v: &JsonValue, style: &mut PlotStyle, titles: StyleTitles) -> stat_core::Result<()> {
    if !matches!(v, JsonValue::Object(_)) {
        return Err(invalid("plot style must be an object"));
    }
    let a = &mut style.axes;
    for (key, axis) in [("x", &mut a.x), ("y", &mut a.y), ("y2", &mut a.y2)] {
        if let Some(m) = present(v, key) {
            read_axis(m, axis, titles)?;
        }
    }
    read_bool(v, "equal_scale", &mut a.equal_scale)?;
    if let Some(m) = present(v, "category_labels") {
        a.category_labels = match m.as_str() {
            Some("automatic") => CategoryLabels::Automatic,
            Some("horizontal") => CategoryLabels::Horizontal,
            Some("slanted") => CategoryLabels::Slanted,
            Some("vertical") => CategoryLabels::Vertical,
            _ => return Err(invalid("plot style: bad category_labels")),
        };
    }
    read_bool(v, "hover_coordinates", &mut a.hover_coordinates)?;
    read_bool(v, "show_key", &mut a.show_key)?;
    read_bool(v, "show_title", &mut a.show_title)?;
    if let Some(m) = present(v, "scale_ratio") {
        a.scale_ratio = Some(finite(m, "scale_ratio")?)
            .filter(|r| *r > 0.0)
            .ok_or_else(|| invalid("plot style: scale_ratio must be above 0"))?;
    }
    if let Some(m) = v.get("print_scale") {
        a.print_scale = match m {
            JsonValue::Null => None,
            m => Some(
                Some(finite(m, "print_scale")?)
                    .filter(|r| *r > 0.0)
                    .ok_or_else(|| invalid("plot style: print_scale must be above 0"))?,
            ),
        };
    }
    if titles == StyleTitles::With {
        if let Some(t) = v.get("title") {
            a.title = t.as_str().map(str::to_owned);
        }
    }
    if let Some(m) = present(v, "export") {
        export_from_json(m, &mut style.export)?;
    }
    if let Some(m) = present(v, "annotations") {
        style.annotations = annotations_from_json(m)?;
    }
    if let Some(m) = present(v, "key") {
        style.key = key_from_json(m)?;
    }
    if let Some(r) = present(v, "reference_lines") {
        style.reference = reference_from_json(r)?;
    }
    Ok(())
}

fn point(p: [f64; 2]) -> JsonValue {
    JsonValue::Array(vec![JsonValue::number(p[0]), JsonValue::number(p[1])])
}

fn strings(items: &[String]) -> JsonValue {
    JsonValue::Array(items.iter().map(|t| s(t)).collect())
}

/// `{"items": [{"shape": "text" | "arrow" | "line" | "rectangle",
/// "anchor": "data" | "frame", "panel": 0, "at": [x, y], "to": [x, y],
/// "text": "…"}]}`
fn annotations_to_json(a: &AnnotationLook) -> JsonValue {
    let items = a
        .items
        .iter()
        .map(|n| {
            obj(vec![
                ("shape", s(n.shape.key())),
                ("anchor", s(n.anchor.key())),
                ("panel", JsonValue::number(n.panel as f64)),
                ("at", point(n.at)),
                ("to", point(n.to)),
                ("text", s(&n.text)),
            ])
        })
        .collect();
    obj(vec![("items", JsonValue::Array(items))])
}

fn read_point(v: &JsonValue, what: &str) -> stat_core::Result<[f64; 2]> {
    match v.as_array() {
        Some([a, b]) => Ok([finite(a, what)?, finite(b, what)?]),
        _ => Err(invalid(format!("plot style: {what} must be [x, y]"))),
    }
}

fn annotations_from_json(v: &JsonValue) -> stat_core::Result<AnnotationLook> {
    let mut out = AnnotationLook::default();
    for item in present(v, "items").and_then(JsonValue::as_array).unwrap_or_default() {
        let shape = present(item, "shape")
            .and_then(JsonValue::as_str)
            .and_then(AnnotationShape::from_key)
            .ok_or_else(|| invalid("plot style: an annotation's shape must be text, arrow, line or rectangle"))?;
        let anchor = match present(item, "anchor") {
            None => AnnotationAnchor::Data,
            Some(a) => a
                .as_str()
                .and_then(AnnotationAnchor::from_key)
                .ok_or_else(|| invalid("plot style: an annotation's anchor must be data or frame"))?,
        };
        let at = read_point(present(item, "at").ok_or_else(|| invalid("plot style: an annotation needs at"))?, "at")?;
        let to = match present(item, "to") {
            Some(t) => read_point(t, "to")?,
            None => at,
        };
        let panel = match present(item, "panel") {
            Some(p) => count(p, "annotation panel", 10_000)? as usize,
            None => 0,
        };
        let text = present(item, "text").and_then(JsonValue::as_str).unwrap_or_default().to_owned();
        out.items.push(Annotation { shape, anchor, panel, at, to, text });
    }
    Ok(out)
}

fn color_json(c: Option<Rgba>) -> JsonValue {
    c.map_or(JsonValue::Null, |c| s(&c.hex()))
}

fn value_json(v: ReferenceValue) -> JsonValue {
    match v {
        ReferenceValue::Fixed(x) => JsonValue::number(x),
        ReferenceValue::Statistic(st) => {
            let mut m = vec![("stat", s(st.key()))];
            if let Some(p) = st.parameter() {
                m.push(("parameter", JsonValue::number(p)));
            }
            obj(m)
        }
    }
}

/// The JSON form of a plot's reference lines and bands.
#[must_use]
pub fn reference_to_json(r: &ReferenceLook) -> JsonValue {
    let lines = r
        .lines
        .iter()
        .map(|l| {
            obj(vec![
                ("orientation", s(l.orientation.key())),
                ("value", value_json(l.value)),
                ("label", s(&l.label)),
                ("show_value", JsonValue::Bool(l.show_value)),
                ("color", color_json(l.color)),
                ("width", JsonValue::number(f64::from(l.width))),
                ("pattern", s(l.pattern.key())),
                ("scope", s(l.scope.key())),
            ])
        })
        .collect();
    let bands = r
        .bands
        .iter()
        .map(|b| {
            obj(vec![
                ("orientation", s(b.orientation.key())),
                ("from", value_json(b.from)),
                ("to", value_json(b.to)),
                ("label", s(&b.label)),
                ("color", color_json(b.color)),
                ("opacity", JsonValue::number(f64::from(b.opacity))),
                ("edges", JsonValue::Bool(b.edges)),
                ("edge_labels", JsonValue::Bool(b.edge_labels)),
                ("scope", s(b.scope.key())),
            ])
        })
        .collect();
    obj(vec![("lines", JsonValue::Array(lines)), ("bands", JsonValue::Array(bands))])
}

fn read_key<T>(v: &JsonValue, key: &str, slot: &mut T, parse: impl Fn(&str) -> Option<T>) -> stat_core::Result<()> {
    if let Some(m) = present(v, key) {
        *slot = m.as_str().and_then(parse).ok_or_else(|| invalid(format!("reference lines: bad {key}")))?;
    }
    Ok(())
}

fn read_color(v: &JsonValue, key: &str, slot: &mut Option<Rgba>) -> stat_core::Result<()> {
    if let Some(m) = v.get(key) {
        *slot = match m {
            JsonValue::Null => None,
            _ => Some(
                m.as_str()
                    .and_then(Rgba::from_hex)
                    .ok_or_else(|| invalid(format!("reference lines: {key} must be a colour such as #6d28d9")))?,
            ),
        };
    }
    Ok(())
}

fn read_value(v: &JsonValue, key: &str, slot: &mut ReferenceValue) -> stat_core::Result<()> {
    let Some(m) = present(v, key) else { return Ok(()) };
    *slot = if let Some(x) = m.as_f64() {
        ReferenceValue::Fixed(Some(x).filter(|x| x.is_finite()).ok_or_else(|| invalid("reference lines: bad value"))?)
    } else {
        let stat = present(m, "stat").and_then(JsonValue::as_str).unwrap_or_default();
        let parameter = present(m, "parameter").and_then(JsonValue::as_f64);
        ReferenceValue::Statistic(
            ReferenceStat::from_key(stat, parameter)
                .ok_or_else(|| invalid(format!("reference lines: bad statistic {stat:?}")))?,
        )
    };
    Ok(())
}

fn read_f32(v: &JsonValue, key: &str, slot: &mut f32, range: std::ops::RangeInclusive<f32>) -> stat_core::Result<()> {
    if let Some(m) = present(v, key) {
        let x = finite(m, key)? as f32;
        *slot = Some(x).filter(|x| range.contains(x)).ok_or_else(|| invalid(format!("reference lines: {key} out of range")))?;
    }
    Ok(())
}

fn read_text(v: &JsonValue, key: &str, slot: &mut String) {
    if let Some(t) = present(v, key).and_then(JsonValue::as_str) {
        t.clone_into(slot);
    }
}

/// Read [`reference_to_json`].
///
/// # Errors
/// A member of the wrong kind or out of range.
pub fn reference_from_json(v: &JsonValue) -> stat_core::Result<ReferenceLook> {
    let list = |key: &str| -> stat_core::Result<&[JsonValue]> {
        match present(v, key) {
            None => Ok(&[]),
            Some(m) => m.as_array().ok_or_else(|| invalid(format!("reference lines: {key} must be a list"))),
        }
    };
    let mut out = ReferenceLook::default();
    for l in list("lines")? {
        let mut line = ReferenceLine::default();
        read_key(l, "orientation", &mut line.orientation, ReferenceOrientation::from_key)?;
        read_value(l, "value", &mut line.value)?;
        read_text(l, "label", &mut line.label);
        read_bool(l, "show_value", &mut line.show_value)?;
        read_color(l, "color", &mut line.color)?;
        read_f32(l, "width", &mut line.width, 0.25..=12.0)?;
        read_key(l, "pattern", &mut line.pattern, LinePattern::from_key)?;
        read_key(l, "scope", &mut line.scope, ReferenceScope::from_key)?;
        out.lines.push(line);
    }
    for b in list("bands")? {
        let mut band = ReferenceBand::default();
        read_key(b, "orientation", &mut band.orientation, ReferenceOrientation::from_key)?;
        read_value(b, "from", &mut band.from)?;
        read_value(b, "to", &mut band.to)?;
        read_text(b, "label", &mut band.label);
        read_color(b, "color", &mut band.color)?;
        read_f32(b, "opacity", &mut band.opacity, 0.0..=1.0)?;
        read_bool(b, "edges", &mut band.edges)?;
        read_bool(b, "edge_labels", &mut band.edge_labels)?;
        read_key(b, "scope", &mut band.scope, ReferenceScope::from_key)?;
        out.bands.push(band);
    }
    Ok(out)
}

/// `{"renamed": [{"id": "…", "text": "…"}], "hidden": ["…"], "order": ["…"]}`
fn key_to_json(k: &KeyLook) -> JsonValue {
    let renamed = k.renamed.iter().map(|(id, text)| obj(vec![("id", s(id)), ("text", s(text))])).collect();
    obj(vec![("renamed", JsonValue::Array(renamed)), ("hidden", strings(&k.hidden)), ("order", strings(&k.order))])
}

fn key_from_json(v: &JsonValue) -> stat_core::Result<KeyLook> {
    let list = |key: &str| -> stat_core::Result<Vec<String>> {
        present(v, key)
            .and_then(JsonValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|t| t.as_str().map(str::to_owned).ok_or_else(|| invalid(format!("plot style: key {key} must be text"))))
            .collect()
    };
    let mut renamed = Vec::new();
    for r in present(v, "renamed").and_then(JsonValue::as_array).unwrap_or_default() {
        match (present(r, "id").and_then(JsonValue::as_str), present(r, "text").and_then(JsonValue::as_str)) {
            (Some(id), Some(text)) => renamed.push((id.to_owned(), text.to_owned())),
            _ => return Err(invalid("plot style: a renamed key entry needs id and text")),
        }
    }
    Ok(KeyLook { renamed, hidden: list("hidden")?, order: list("order")? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_viz::axis_settings::PlotAxes;

    fn busy() -> PlotStyle {
        let mut axes = PlotAxes::default();
        axes.x.major = MajorTicks::Every(2.5);
        axes.x.minor = MinorTicks::Subdivisions(4);
        axes.x.origin = 1.0;
        axes.x.grid = GridLines::MajorAndMinor;
        axes.x.title = Some("Dose (mg)".into());
        axes.x.breaks = AxisBreaks::single(10.0, 90.0);
        axes.y.breaks = AxisBreaks {
            breaks: vec![stat_viz::AxisBreak { from: 5.0, to: 20.0 }, stat_viz::AxisBreak { from: 40.0, to: 400.0 }],
            segments: vec![
                stat_viz::AxisSegment { scale: Some(stat_viz::AxisScale::Log10), share: Some(30.0) },
                stat_viz::AxisSegment::default(),
                stat_viz::AxisSegment { scale: None, share: Some(20.0) },
            ],
            mark: BreakMark::Zigzag,
        };
        axes.y.minor = MinorTicks::Every(0.5);
        axes.y.format = TickFormat::Percent(1);
        axes.y.tick_marks = false;
        axes.y.side = AxisSide::Opposite;
        axes.y2.format = TickFormat::Scientific(2);
        axes.y2.show_title = false;
        axes.equal_scale = true;
        axes.category_labels = CategoryLabels::Slanted;
        axes.hover_coordinates = false;
        axes.show_key = false;
        axes.title = Some("Response at \\alpha".into());
        axes.x.tick_direction = TickDirection::Both;
        axes.x.major_mark = TickMarkSize { length: 8.0, width: 1.5 };
        axes.y.minor_mark = TickMarkSize { length: 2.0, width: 0.5 };
        axes.y.offset = 6.0;
        axes.y.extend_to_ticks = true;
        axes.x.custom_ticks = vec![CustomTick { value: std::f64::consts::PI, label: "\\pi".into() }];
        axes.x.range = Some([-1.0, 7.5]);
        axes.scale_ratio = 2.0;
        axes.print_scale = Some(10.0);
        let mut style = PlotStyle::of(&axes);
        style.export = ExportLook {
            font: PlotFont::Serif,
            text_size: 1.25,
            transparent: true,
            size: Some(FigureSize { width: 89.0, height: 61.5, unit: LengthUnit::Millimetres }),
            lock_ratio: false,
            ppi: 600.0,
        };
        style.annotations.items = vec![
            Annotation {
                shape: AnnotationShape::Arrow,
                anchor: AnnotationAnchor::Data,
                panel: 0,
                at: [1.5, 20.0],
                to: [3.0, 25.5],
                text: "peak at \\beta_1".into(),
            },
            Annotation {
                shape: AnnotationShape::Rectangle,
                anchor: AnnotationAnchor::Frame,
                panel: 2,
                at: [0.1, 0.2],
                to: [0.4, 0.6],
                text: String::new(),
            },
        ];
        style.key.renamed = vec![("fit/Fit".into(), "Least squares".into())];
        style.key.hidden = vec!["color/Site/A".into()];
        style.key.order = vec!["fit/Fit".into(), "color/Site/B".into()];
        style.reference = ReferenceLook {
            lines: vec![
                ReferenceLine { value: ReferenceValue::Fixed(2.5), label: "Target".into(), ..ReferenceLine::default() },
                ReferenceLine {
                    orientation: ReferenceOrientation::Vertical,
                    value: ReferenceValue::Statistic(ReferenceStat::Quantile(0.25)),
                    show_value: false,
                    color: Some(Rgba([1, 2, 3, 200])),
                    width: 3.0,
                    pattern: LinePattern::DashDot,
                    scope: ReferenceScope::Group,
                    ..ReferenceLine::default()
                },
            ],
            bands: vec![ReferenceBand {
                from: ReferenceValue::Statistic(ReferenceStat::MeanSd(-3.0)),
                to: ReferenceValue::Statistic(ReferenceStat::IndividualsUpper),
                label: "Limits".into(),
                color: Some(Rgba::rgb(9, 9, 9)),
                opacity: 0.4,
                edges: false,
                edge_labels: false,
                scope: ReferenceScope::Whole,
                ..ReferenceBand::default()
            }],
        };
        style
    }

    #[test]
    fn styles_round_trip() {
        let style = busy();
        let mut back = PlotStyle::default();
        plot_style_from_json(&plot_style_to_json(&style, StyleTitles::With), &mut back, StyleTitles::With).unwrap();
        assert_eq!(back, style);
    }

    #[test]
    fn styles_without_titles_keep_the_titles_they_are_read_onto() {
        let style = busy();
        let json = plot_style_to_json(&style, StyleTitles::Without);
        assert!(json.get("title").is_none() && json.get("x").unwrap().get("break").is_none());
        let mut back = PlotStyle::default();
        back.axes.x.title = Some("kept".into());
        plot_style_from_json(&json, &mut back, StyleTitles::Without).unwrap();
        assert_eq!(back.axes.x.title.as_deref(), Some("kept"));
        assert_eq!(back.axes.x.major, MajorTicks::Every(2.5));
        assert!(back.axes.equal_scale);
        // Typed ranges and tick labels are kept on graphs too.
        assert_eq!(back.axes.x.range, Some([-1.0, 7.5]));
        assert_eq!(back.axes.x.custom_ticks.len(), 1);
    }

    #[test]
    fn figure_looks_are_written_only_when_set_and_read_in_part() {
        let json = plot_style_to_json(&PlotStyle::default(), StyleTitles::With);
        assert!(json.get("export").is_none());
        let mut style = PlotStyle::default();
        let v = stat_data::parse_json(r#"{"export": {"font": "serif", "size": {"width": 3.5, "height": 2, "unit": "in"}}}"#)
            .unwrap();
        plot_style_from_json(&v, &mut style, StyleTitles::With).unwrap();
        assert_eq!(style.export.font, PlotFont::Serif);
        assert_eq!(style.export.size.unwrap().points(), (252.0, 144.0));
        assert_eq!(style.export.ppi, 300.0, "members left out keep their value");
        let v = stat_data::parse_json(r#"{"export": {"size": null}}"#).unwrap();
        plot_style_from_json(&v, &mut style, StyleTitles::With).unwrap();
        assert_eq!(style.export.size, None);
    }

    #[test]
    fn missing_members_keep_their_values_and_bad_ones_are_refused() {
        let mut style = busy();
        let before = style.clone();
        plot_style_from_json(&stat_data::parse_json("{}").unwrap(), &mut style, StyleTitles::With).unwrap();
        assert_eq!(style, before);
        for bad in [
            r#"{"x": {"grid": "some"}}"#,
            r#"{"x": {"major": -1}}"#,
            r#"{"y": {"minor": {"subdivisions": 2.5}}}"#,
            r#"{"y": {"format": {"decimals": 99}}}"#,
            r#"{"equal_scale": 1}"#,
            r#"{"x": {"break": [1]}}"#,
            r#"{"export": {"font": "mono"}}"#,
            r#"{"export": {"text_size": 7}}"#,
            r#"{"export": {"size": {"width": 10, "height": -1, "unit": "mm"}}}"#,
            r#"{"export": {"size": {"width": 10, "height": 5, "unit": "cm"}}}"#,
            r#"{"export": {"ppi": 5}}"#,
            r#"{"x": {"tick_direction": "up"}}"#,
            r#"{"x": {"custom_ticks": [{"value": 1}]}}"#,
            r#"{"y": {"range": [1, 2, 3]}}"#,
            r#"{"scale_ratio": 0}"#,
            r#"{"annotations": {"items": [{"shape": "star", "at": [0, 0]}]}}"#,
            r#"{"annotations": {"items": [{"shape": "text", "at": [0]}]}}"#,
            r#"{"key": {"hidden": [1]}}"#,
            r#"{"reference_lines": {"lines": [{"pattern": "wavy"}]}}"#,
            r#"{"reference_lines": {"lines": [{"value": {"stat": "mode"}}]}}"#,
            r#"{"reference_lines": {"bands": [{"opacity": 2}]}}"#,
            r#"{"reference_lines": {"lines": [{"color": "violet"}]}}"#,
            r#"{"reference_lines": {"lines": {}}}"#,
            r#"[]"#,
        ] {
            let mut s = PlotStyle::default();
            assert!(
                plot_style_from_json(&stat_data::parse_json(bad).unwrap(), &mut s, StyleTitles::With).is_err(),
                "{bad}"
            );
        }
    }
}
