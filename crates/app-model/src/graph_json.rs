//! Graph canvases in project files: encodings by column name (one member per
//! role), axis scales, page, the graph's own row filter, column pickers and
//! each layer's settings (`null` for a layer that is off).
//!
//! ```json
//! { "analysis": "graph",
//!   "encoding": { "x": ["Dose"], "y": ["Response"], "color": "Group", "page": null, … },
//!   "x_scale": "log10", "y_scale": "linear", "page": 0,
//!   "filter": { "conditions": [ { "column": "Site", "levels": ["A", "B"] },
//!                               { "column": "Age", "min": 18, "max": 65 } ],
//!               "show_only": true, "include_only": true, "select": false },
//!   "switchers": [ { "role": "y", "slot": 0, "choices": ["Response", "Yield"] } ],
//!   "points": { "jitter": "uniform", "jitter_width": 0.25, "fade_unselected": false },
//!   "smoother": null, "fit": { "degree": 1, "band": true, "level": 0.95 }, "box_plot": null,
//!   "summary": { "stat": "mean", "error_bars": "ci", "level": 0.95,
//!                "error_look": { "color": "#334155", "width": 2, "cap": 1.5, "direction": "upper" } },
//!   "marker": { "size": 5, "color": "#1f77b4", "shape": "square",
//!               "levels": [ { "column": "Group", "level": "A", "color": "#d62728", "shape": "diamond" } ] },
//!   "label_positions": [ { "panel": 0, "id": "fit/0", "at": [0.05, 0.95] } ],
//!   "axes": { "title": "Growth of $dN/dt$", "x_title": "Time t_{0}", "y_title": null, "y2_title": "\\mu g",
//!             "x_break": null, "y_break": [40, 950], "markup_labels": false,
//!             "y_breaks": { "at": [[40, 950], [1200, 5000]], "mark": "zigzag",
//!                           "segments": [{ "scale": "log10", "share": 60 }] } },
//!   "hull": { "fill": true },
//!   "brackets": { "test": "tukey-kramer", "label": "both", "only_significant": false, "alpha": 0.05,
//!                 "pairs": [["A", "C"]], "thresholds": [0.05, 0.01, 0.001, 0.0001], "ns_label": true, "gap": 0.07 },
//!   "estimation": { "interval": "bca", "resamples": 5000, "seed": 1, "level": 0.95, "reference": null },
//!   "form": { "kind": "cluster-map", "standardize": true, "cluster_rows": true,
//!             "cluster_columns": true, "linkage": "average", "values": false } }
//! ```
//!
//! Other forms: `{ "kind": "sankey" | "chord", "size_by_sum": false, "labels": true }`,
//! `{ "kind": "network", "seed": 1, "iterations": 500, "labels": true }`,
//! `{ "kind": "estimation" }` (its settings in `estimation`).
//!
//! `layer_order` (written only when the user changed the drawing order)
//! lists layers by their member names, back to front (see
//! `GraphSpec::layer_order`). The plot style in `axes.style` carries the
//! annotations and key changes (see `crate::plot_style`).
//!
//! `encoding.y2` lists the right-axis columns (written only when there are
//! some); `axes` holds the titles typed in Plot settings (math markup), the
//! axis breaks in data units and whether column names and labels are read
//! as markup (written only when something is set).
//!
//! Members added in later versions are optional when reading, so older files
//! still open.

use stat_core::Result;
use stat_data::JsonValue;
use stat_viz::summary::{ErrorBarDirection, ErrorBarLook, ErrorBars, SummaryStat};
use stat_viz::{
    AngleUnit, PieSettings, PolarSettings, RadarSettings, RidgelineSettings, VectorSettings, WaterfallSettings,
    WindRoseSettings,
};
use stat_viz::{
    AxisBreaks, AxisScale, BarElement, BarLayout, BinElement, BoxPlotElement, ColumnSwitcher, ContourElement,
    DotPlotElement, EllipseElement, EncodingPlan, FilterCondition, FitLabels, FitLineElement, FrameOptions,
    FunctionElement, GraphAxes, GraphForm, GraphSpec, HeatmapElement, HistogramBins, HistogramElement, JitterKind,
    LabelPosition, LegendPlacement, LineConnect, LineElement, LocalFilter, MarkShape, MarkerStyle, PointsElement,
    ProbabilityPlotElement, Rgba, Role, SmoothMethod, SmootherElement, SummaryElement, TreemapSettings, ViolinElement,
};
use stat_viz::{
    BracketElement, BracketLabel, BracketTest, ClusterMapSettings, EstimationInterval, EstimationSettings,
    FlowSettings, HullElement, NetworkSettings,
};

use crate::project::{invalid, member, obj, s};

/// `v` with an `"error_look"` member when `look` is not the default.
fn with_error_look(mut v: JsonValue, look: &ErrorBarLook) -> JsonValue {
    if let (JsonValue::Object(m), false) = (&mut v, look.is_default()) {
        m.push((
            "error_look".to_owned(),
            obj([
                ("color", look.color.map_or(JsonValue::Null, |c| s(&c.hex()))),
                ("width", JsonValue::number(f64::from(look.width))),
                ("cap", JsonValue::number(look.cap)),
                ("direction", s(look.direction.key())),
            ]),
        ));
    }
    v
}

/// Read [`with_error_look`]'s member (absent or bad parts: the defaults).
fn error_look_from_json(l: &JsonValue) -> ErrorBarLook {
    let d = ErrorBarLook::default();
    let Some(v) = l.get("error_look").filter(|v| !matches!(v, JsonValue::Null)) else { return d };
    let num = |key: &str| v.get(key).and_then(JsonValue::as_f64).filter(|x| x.is_finite());
    ErrorBarLook {
        color: v.get("color").and_then(JsonValue::as_str).and_then(Rgba::from_hex),
        width: num("width").map_or(d.width, |w| w.clamp(0.25, 12.0) as f32),
        cap: num("cap").map_or(d.cap, |c| c.clamp(0.0, 10.0)),
        direction: v
            .get("direction")
            .and_then(JsonValue::as_str)
            .and_then(ErrorBarDirection::from_key)
            .unwrap_or(d.direction),
    }
}

/// A list of column names.
fn names(c: &[String]) -> JsonValue {
    JsonValue::Array(c.iter().map(|n| s(n)).collect())
}

fn encoding_to_json(e: &EncodingPlan) -> JsonValue {
    JsonValue::Object(
        Role::ALL
            .into_iter()
            // Graphs without right-axis columns are written as before.
            .filter(|role| *role != Role::Y2 || !e.y2.is_empty())
            .map(|role| {
                let cols = e.get(role);
                let value = if role.is_multi() {
                    JsonValue::Array(cols.iter().map(|c| s(c)).collect())
                } else {
                    cols.first().map_or(JsonValue::Null, |c| s(c))
                };
                (role.key().to_owned(), value)
            })
            .collect(),
    )
}

fn filter_to_json(f: &LocalFilter) -> JsonValue {
    let conditions = f
        .conditions
        .iter()
        .map(|c| match c {
            FilterCondition::Levels { column, levels } => obj([("column", s(column)), ("levels", names(levels))]),
            FilterCondition::Range { column, min, max } => {
                obj([("column", s(column)), ("min", JsonValue::number(*min)), ("max", JsonValue::number(*max))])
            }
        })
        .collect();
    obj([
        ("conditions", JsonValue::Array(conditions)),
        ("show_only", JsonValue::Bool(f.show_only)),
        ("include_only", JsonValue::Bool(f.include_only)),
        ("select", JsonValue::Bool(f.select)),
    ])
}

/// Each layer's settings as `(key, object)`, `null` for a layer that is off.
fn layers_to_json(spec: &GraphSpec) -> Vec<(&'static str, JsonValue)> {
    let number = JsonValue::number;
    let b = JsonValue::Bool;
    let n = |v: usize| JsonValue::Number(v as f64);
    let numbers = |v: &[f64]| JsonValue::Array(v.iter().map(|&x| number(x)).collect());
    let or_null = |v: Option<JsonValue>| v.unwrap_or(JsonValue::Null);
    vec![
        (
            "points",
            or_null(spec.points.map(|p| {
                obj([
                    ("jitter", s(p.jitter.key())),
                    ("jitter_width", number(p.jitter_width)),
                    ("fade_unselected", b(p.fade_unselected)),
                    ("label_selected_only", b(p.label_selected_only)),
                ])
            })),
        ),
        (
            "smoother",
            or_null(spec.smoother.map(|p| {
                obj([
                    ("method", s(p.method.key())),
                    ("lambda", number(p.lambda)),
                    ("span", number(p.span)),
                    ("local_degree", n(p.local_degree)),
                    ("band", b(p.band)),
                    ("level", number(p.level)),
                ])
            })),
        ),
        (
            "fit",
            or_null(spec.fit.map(|p| {
                obj([
                    ("degree", n(p.degree)),
                    ("band", b(p.band)),
                    ("prediction", b(p.prediction)),
                    ("level", number(p.level)),
                    ("show_equation", b(p.labels.equation)),
                    ("show_r_squared", b(p.labels.r_squared)),
                    ("show_p_value", b(p.labels.p_value)),
                ])
            })),
        ),
        ("box_plot", or_null(spec.box_plot.map(|p| obj([("outliers", b(p.outliers)), ("notches", b(p.notches))])))),
        (
            "violin",
            or_null(
                spec.violin
                    .map(|p| obj([("bandwidth_scale", number(p.bandwidth_scale)), ("quartiles", b(p.quartiles))])),
            ),
        ),
        (
            "summary",
            or_null(spec.summary.map(|p| {
                with_error_look(
                    obj([("stat", s(&p.stat.key())), ("error_bars", s(p.error_bars.key())), ("level", number(p.level))]),
                    &p.error_look,
                )
            })),
        ),
        (
            "ellipse",
            or_null(
                spec.ellipse.as_ref().map(|p| obj([("levels", numbers(&p.levels)), ("correlation", b(p.correlation))])),
            ),
        ),
        (
            "contour",
            or_null(spec.contour.as_ref().map(|p| {
                obj([
                    ("fractions", numbers(&p.fractions)),
                    ("bandwidth_scale", number(p.bandwidth_scale)),
                    ("filled", b(p.filled)),
                ])
            })),
        ),
        ("bins", or_null(spec.bins.map(|p| obj([("hexagonal", b(p.hexagonal)), ("bins", n(p.bins))])))),
        ("function", or_null(spec.function.as_ref().map(|p| obj([("formulas", names(&p.formulas))])))),
        (
            "bars",
            or_null(spec.bars.map(|p| {
                with_error_look(
                    obj([
                        ("stat", s(&p.stat.key())),
                        ("layout", s(p.layout.key())),
                        ("error_bars", s(p.error_bars.key())),
                        ("level", number(p.level)),
                        ("labels", b(p.labels)),
                    ]),
                    &p.error_look,
                )
            })),
        ),
        ("line", or_null(spec.line.map(|p| obj([("connect", s(p.connect.key())), ("stat", s(&p.stat.key()))])))),
        ("heatmap", or_null(spec.heatmap.map(|p| obj([("stat", s(&p.stat.key())), ("labels", b(p.labels))])))),
        (
            "histogram",
            or_null(spec.histogram.map(|p| {
                let bins = match p.bins {
                    HistogramBins::Auto => ("bins", s("auto")),
                    HistogramBins::Count(k) => ("bins", n(k)),
                    HistogramBins::Width(w) => ("bin_width", number(w)),
                };
                obj([bins, ("stacked", b(p.stacked)), ("density", b(p.density))])
            })),
        ),
        ("dot_plot", or_null(spec.dot_plot.map(|p| obj([("width", or_null(p.width.map(number)))])))),
        (
            "probability_plot",
            or_null(
                spec.probability_plot.map(|p| {
                    obj([("distribution", s(p.family.key())), ("band", b(p.band)), ("level", number(p.level))])
                }),
            ),
        ),
        (
            "form",
            match spec.form {
                GraphForm::Treemap(t) => {
                    obj([("kind", s(spec.form.key())), ("size_by_sum", b(t.size_by_sum)), ("labels", b(t.labels))])
                }
                GraphForm::ClusterMap(c) => obj([
                    ("kind", s(spec.form.key())),
                    ("standardize", b(c.standardize)),
                    ("cluster_rows", b(c.cluster_rows)),
                    ("cluster_columns", b(c.cluster_columns)),
                    ("linkage", s(stat_viz::cluster_map::linkage_key(c.linkage))),
                    ("values", b(c.values)),
                ]),
                GraphForm::Sankey(f) | GraphForm::Chord(f) => {
                    obj([("kind", s(spec.form.key())), ("size_by_sum", b(f.size_by_sum)), ("labels", b(f.labels))])
                }
                GraphForm::Network(w) => obj([
                    ("kind", s(spec.form.key())),
                    // Seeds are written as numbers (exact below 2⁵³).
                    ("seed", number(w.seed as f64)),
                    ("iterations", n(w.iterations)),
                    ("labels", b(w.labels)),
                ]),
                GraphForm::Pie(p) => obj([
                    ("kind", s(spec.form.key())),
                    ("donut", b(p.donut)),
                    ("size_by_sum", b(p.size_by_sum)),
                    ("labels", b(p.labels)),
                ]),
                GraphForm::Radar(r) => {
                    obj([("kind", s(spec.form.key())), ("group_means", b(r.group_means)), ("fill", b(r.fill))])
                }
                GraphForm::Polar(p) => {
                    obj([("kind", s(spec.form.key())), ("angle", s(p.angle.key())), ("connect", b(p.connect))])
                }
                GraphForm::WindRose(w) => obj([
                    ("kind", s(spec.form.key())),
                    ("sectors", n(usize::from(w.sectors))),
                    ("percent", b(w.percent)),
                ]),
                GraphForm::Waterfall(w) => {
                    obj([("kind", s(spec.form.key())), ("total", b(w.total)), ("labels", b(w.labels))])
                }
                GraphForm::Ridgeline(r) => {
                    obj([("kind", s(spec.form.key())), ("height", n(usize::from(r.height))), ("rug", b(r.rug))])
                }
                GraphForm::VectorField(v) => obj([("kind", s(spec.form.key())), ("scale", n(usize::from(v.scale)))]),
                other => obj([("kind", s(other.key()))]),
            },
        ),
        ("hull", or_null(spec.hull.map(|h| obj([("fill", b(h.fill))])))),
        (
            "brackets",
            or_null(spec.brackets.as_ref().map(|k| {
                let mut v = obj([
                    ("test", s(k.test.key())),
                    ("label", s(k.label.key())),
                    ("only_significant", b(k.only_significant)),
                    ("alpha", number(k.alpha)),
                ]);
                // Members added later, written only when set.
                if let JsonValue::Object(m) = &mut v {
                    let d = BracketElement::default();
                    if !k.pairs.is_empty() {
                        let pairs = k.pairs.iter().map(|(a, c)| JsonValue::Array(vec![s(a), s(c)])).collect();
                        m.push(("pairs".to_owned(), JsonValue::Array(pairs)));
                    }
                    if k.thresholds != d.thresholds {
                        m.push(("thresholds".to_owned(), numbers(&k.thresholds)));
                    }
                    if k.ns_label {
                        m.push(("ns_label".to_owned(), b(true)));
                    }
                    if k.gap != d.gap {
                        m.push(("gap".to_owned(), number(k.gap)));
                    }
                }
                v
            })),
        ),
        (
            "estimation",
            obj([
                ("interval", s(spec.estimation.interval.key())),
                ("resamples", n(spec.estimation.resamples)),
                ("seed", number(spec.estimation.seed as f64)),
                ("level", number(spec.estimation.level)),
                ("reference", spec.estimation.reference.as_deref().map_or(JsonValue::Null, s)),
            ]),
        ),
    ]
}

/// Marker size, color and shape, and per-level overrides.
fn style_to_json(m: &MarkerStyle) -> JsonValue {
    let num = |v: Option<f32>| v.map_or(JsonValue::Null, |v| JsonValue::Number(f64::from(v)));
    let mut keys: Vec<&String> =
        m.level_colors.keys().chain(m.level_shapes.keys()).chain(m.level_sizes.keys()).collect();
    keys.sort();
    keys.dedup();
    let levels = keys
        .into_iter()
        .filter_map(|k| {
            let (column, level) = k.split_once('\u{1f}')?;
            Some(obj([
                ("column", s(column)),
                ("level", s(level)),
                ("color", m.level_colors.get(k).map_or(JsonValue::Null, |c| s(&c.hex()))),
                ("shape", m.level_shapes.get(k).map_or(JsonValue::Null, |h| s(h.key()))),
                ("size", num(m.level_sizes.get(k).copied())),
            ]))
        })
        .collect();
    obj([
        ("size", num(m.size)),
        ("color", m.color.map_or(JsonValue::Null, |c| s(&c.hex()))),
        ("shape", m.shape.map_or(JsonValue::Null, |h| s(h.key()))),
        ("levels", JsonValue::Array(levels)),
        (
            "panels",
            JsonValue::Array(
                m.panels
                    .iter()
                    .map(|(i, p)| {
                        let mut o = style_to_json(&MarkerStyle { panels: Default::default(), ..p.clone() });
                        if let JsonValue::Object(members) = &mut o {
                            members.retain(|(k, _)| k != "panels");
                            members.insert(0, ("panel".to_owned(), JsonValue::Number(*i as f64)));
                        }
                        o
                    })
                    .collect(),
            ),
        ),
    ])
}

/// Read [`style_to_json`]; unknown colors or shapes are ignored (defaults).
fn style_from_json(v: Option<&JsonValue>) -> MarkerStyle {
    let mut m = MarkerStyle::default();
    let Some(v) = v else { return m };
    let color = |l: &JsonValue| l.get("color").and_then(JsonValue::as_str).and_then(Rgba::from_hex);
    let shape = |l: &JsonValue| l.get("shape").and_then(JsonValue::as_str).and_then(MarkShape::from_key);
    let size = |l: &JsonValue| {
        l.get("size").and_then(JsonValue::as_f64).filter(|r| r.is_finite() && *r > 0.0).map(|r| r as f32)
    };
    m.size = size(v);
    m.color = color(v);
    m.shape = shape(v);
    for l in v.get("levels").and_then(JsonValue::as_array).unwrap_or_default() {
        let (Some(column), Some(level)) =
            (l.get("column").and_then(JsonValue::as_str), l.get("level").and_then(JsonValue::as_str))
        else {
            continue;
        };
        let key = MarkerStyle::key(column, level);
        if let Some(c) = color(l) {
            m.level_colors.insert(key.clone(), c);
        }
        if let Some(h) = shape(l) {
            m.level_shapes.insert(key.clone(), h);
        }
        if let Some(r) = size(l) {
            m.level_sizes.insert(key, r);
        }
    }
    for p in v.get("panels").and_then(JsonValue::as_array).unwrap_or_default() {
        if let Some(i) = p.get("panel").and_then(JsonValue::as_usize) {
            let mut own = style_from_json(Some(p));
            own.panels.clear();
            if !own.is_default() {
                m.panels.insert(i, own);
            }
        }
    }
    m
}

/// One moved label (`None` when incomplete).
fn label_position(l: &JsonValue) -> Option<LabelPosition> {
    let at = l.get("at").and_then(JsonValue::as_array)?;
    let (x, y) = (at.first()?.as_f64()?, at.get(1)?.as_f64()?);
    if !(x.is_finite() && y.is_finite()) {
        return None;
    }
    Some(LabelPosition {
        panel: l.get("panel").and_then(JsonValue::as_usize)?,
        id: l.get("id").and_then(JsonValue::as_str)?.to_owned(),
        at: [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)],
    })
}

/// The graph as a project-file report object.
pub fn graph_to_json(spec: &GraphSpec) -> JsonValue {
    let switchers = spec
        .switchers
        .iter()
        .map(|w| {
            obj([("role", s(w.role.key())), ("slot", JsonValue::Number(w.slot as f64)), ("choices", names(&w.choices))])
        })
        .collect();
    let mut members: Vec<(String, JsonValue)> = vec![
        ("analysis".to_owned(), s("graph")),
        ("encoding".to_owned(), encoding_to_json(&spec.encoding)),
        ("x_scale".to_owned(), s(spec.options.x_scale.key())),
        ("y_scale".to_owned(), s(spec.options.y_scale.key())),
        ("page".to_owned(), JsonValue::Number(spec.options.page as f64)),
        ("filter".to_owned(), filter_to_json(&spec.options.filter)),
        ("switchers".to_owned(), JsonValue::Array(switchers)),
        ("legend".to_owned(), s(spec.legend.key())),
        ("marker".to_owned(), style_to_json(&spec.options.style)),
        (
            "label_positions".to_owned(),
            JsonValue::Array(
                spec.label_positions
                    .iter()
                    .map(|l| {
                        obj([
                            ("panel", JsonValue::Number(l.panel as f64)),
                            ("id", s(&l.id)),
                            ("at", JsonValue::Array(vec![JsonValue::Number(l.at[0]), JsonValue::Number(l.at[1])])),
                        ])
                    })
                    .collect(),
            ),
        ),
    ];
    members.extend(layers_to_json(spec).into_iter().map(|(k, v)| (k.to_owned(), v)));
    if !spec.layer_order.is_empty() {
        let order = spec.layer_order.iter().map(|k| s(k.key())).collect();
        members.push(("layer_order".to_owned(), JsonValue::Array(order)));
    }
    if !spec.axes.is_default() {
        members.push(("axes".to_owned(), axes_to_json(&spec.axes)));
    }
    JsonValue::Object(members)
}

/// Titles, breaks and markup ([`GraphAxes`]).
fn axes_to_json(a: &GraphAxes) -> JsonValue {
    let text = |t: &Option<String>| t.as_deref().map_or(JsonValue::Null, s);
    // The first break as a pair (what earlier versions read), and every
    // break with segment settings when there is more than that.
    let brk = |b: &AxisBreaks| {
        b.breaks
            .first()
            .map_or(JsonValue::Null, |b| JsonValue::Array(vec![JsonValue::number(b.from), JsonValue::number(b.to)]))
    };
    let full = |b: &AxisBreaks| {
        if crate::plot_style::is_plain_break(b) { JsonValue::Null } else { crate::plot_style::breaks_to_json(b) }
    };
    obj([
        ("title", text(&a.title)),
        ("x_title", text(&a.x_title)),
        ("y_title", text(&a.y_title)),
        ("y2_title", text(&a.y2_title)),
        ("x_break", brk(&a.x_breaks)),
        ("y_break", brk(&a.y_breaks)),
        ("x_breaks", full(&a.x_breaks)),
        ("y_breaks", full(&a.y_breaks)),
        ("markup_labels", JsonValue::Bool(a.markup_labels)),
        (
            "style",
            a.style.as_ref().map_or(JsonValue::Null, |st| {
                crate::plot_style::plot_style_to_json(st, crate::plot_style::StyleTitles::Without)
            }),
        ),
    ])
}

/// Read [`axes_to_json`] (absent: nothing set; bad entries are dropped).
fn axes_from_json(v: Option<&JsonValue>) -> GraphAxes {
    let Some(v) = v.filter(|v| !matches!(v, JsonValue::Null)) else { return GraphAxes::default() };
    let text = |key: &str| v.get(key).and_then(JsonValue::as_str).filter(|t| !t.trim().is_empty()).map(str::to_owned);
    let brk = |key: &str| {
        let list = v.get(key).and_then(JsonValue::as_array)?;
        let (from, to) = (list.first()?.as_f64()?, list.get(1)?.as_f64()?);
        (from.is_finite() && to.is_finite() && from != to).then(|| AxisBreaks::single(from, to))
    };
    // The full form wins over the pair.
    let breaks = |full: &str, pair: &str| {
        v.get(full)
            .filter(|b| !matches!(b, JsonValue::Null))
            .and_then(|b| crate::plot_style::breaks_from_json(b).ok())
            .or_else(|| brk(pair))
            .unwrap_or_default()
    };
    GraphAxes {
        title: text("title"),
        x_title: text("x_title"),
        y_title: text("y_title"),
        y2_title: text("y2_title"),
        x_breaks: breaks("x_breaks", "x_break"),
        y_breaks: breaks("y_breaks", "y_break"),
        markup_labels: v.get("markup_labels").and_then(JsonValue::as_bool).unwrap_or(false),
        style: v.get("style").filter(|st| !matches!(st, JsonValue::Null)).and_then(|st| {
            let mut style = crate::plot_style::PlotStyle::default();
            crate::plot_style::plot_style_from_json(st, &mut style, crate::plot_style::StyleTitles::Without)
                .ok()
                .map(|()| style)
        }),
    }
}

/// Column names in `v[key]` (absent or `null`: none).
fn name_list(v: &JsonValue, key: &str) -> Result<Vec<String>> {
    match v.get(key) {
        None | Some(JsonValue::Null) => Ok(Vec::new()),
        Some(list) => list
            .as_array()
            .ok_or_else(|| invalid(format!("graph: {key} must be a list of column names")))?
            .iter()
            .map(|n| n.as_str().map(str::to_owned).ok_or_else(|| invalid("graph: column names are text")))
            .collect(),
    }
}

fn encoding_from_json(enc: &JsonValue) -> Result<EncodingPlan> {
    let mut plan = EncodingPlan::default();
    for role in Role::ALL {
        if role.is_multi() {
            for c in name_list(enc, role.key())? {
                plan.assign(role, &c);
            }
        } else if let Some(c) = enc.get(role.key()).and_then(JsonValue::as_str) {
            // Set directly: `assign` would let wrap and the grid clear each other.
            match role {
                Role::GroupX => plan.group_x = Some(c.to_owned()),
                Role::GroupY => plan.group_y = Some(c.to_owned()),
                Role::Wrap => plan.wrap = Some(c.to_owned()),
                _ => plan.assign(role, c),
            }
        }
    }
    Ok(plan)
}

fn filter_from_json(v: Option<&JsonValue>) -> Result<LocalFilter> {
    let d = LocalFilter::default();
    let Some(v) = v.filter(|v| !matches!(v, JsonValue::Null)) else { return Ok(d) };
    let b = |key: &str, default: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(default);
    let mut conditions = Vec::new();
    if let Some(list) = v.get("conditions").and_then(JsonValue::as_array) {
        for c in list {
            let column =
                member(c, "column")?.as_str().ok_or_else(|| invalid("graph filter: column names are text"))?.to_owned();
            let condition = if c.get("levels").is_some() {
                FilterCondition::Levels { column, levels: name_list(c, "levels")? }
            } else {
                let num = |key: &str| -> Result<f64> {
                    member(c, key)?.as_f64().ok_or_else(|| invalid(format!("graph filter: {key} must be a number")))
                };
                FilterCondition::Range { column, min: num("min")?, max: num("max")? }
            };
            conditions.push(condition);
        }
    }
    Ok(LocalFilter {
        conditions,
        show_only: b("show_only", d.show_only),
        include_only: b("include_only", d.include_only),
        select: b("select", d.select),
    })
}

fn switchers_from_json(v: Option<&JsonValue>) -> Result<Vec<ColumnSwitcher>> {
    let Some(list) = v.and_then(JsonValue::as_array) else { return Ok(Vec::new()) };
    list.iter()
        .map(|w| {
            let key = member(w, "role")?.as_str().unwrap_or("");
            let role = Role::from_key(key).ok_or_else(|| invalid(format!("graph: unknown role {key:?}")))?;
            Ok(ColumnSwitcher {
                role,
                slot: w.get("slot").and_then(JsonValue::as_usize).unwrap_or(0),
                choices: name_list(w, "choices")?,
            })
        })
        .collect()
}

/// Read a graph saved by [`graph_to_json`] (or by an earlier version).
pub fn graph_from_json(v: &JsonValue) -> Result<GraphSpec> {
    let encoding = encoding_from_json(member(v, "encoding")?)?;
    let scale = |key: &str| -> Result<AxisScale> {
        match v.get(key).and_then(JsonValue::as_str) {
            None => Ok(AxisScale::Linear),
            Some(k) => AxisScale::from_key(k).ok_or_else(|| invalid(format!("graph: unknown scale {k:?}"))),
        }
    };
    let layer = |key: &str| v.get(key).filter(|l| !matches!(l, JsonValue::Null));
    let f = |l: &JsonValue, key: &str, default: f64| l.get(key).and_then(JsonValue::as_f64).unwrap_or(default);
    let b = |l: &JsonValue, key: &str, default: bool| l.get(key).and_then(JsonValue::as_bool).unwrap_or(default);
    let u = |l: &JsonValue, key: &str, default: usize| l.get(key).and_then(JsonValue::as_usize).unwrap_or(default);
    let probabilities = |l: &JsonValue, key: &str, default: &[f64]| -> Vec<f64> {
        match l.get(key).and_then(JsonValue::as_array) {
            Some(list) => list.iter().filter_map(JsonValue::as_f64).filter(|p| *p > 0.0 && *p < 1.0).collect(),
            None => default.to_vec(),
        }
    };
    let stat = |l: &JsonValue, default: SummaryStat| {
        l.get("stat").and_then(JsonValue::as_str).and_then(SummaryStat::from_key).unwrap_or(default)
    };
    let d = GraphSpec::default();
    let points_default = PointsElement::default();
    let smoother_default = SmootherElement::default();
    let fit_default = FitLineElement::default();
    Ok(GraphSpec {
        encoding,
        options: FrameOptions {
            x_scale: scale("x_scale")?,
            y_scale: scale("y_scale")?,
            page: v.get("page").and_then(JsonValue::as_usize).unwrap_or(0),
            filter: filter_from_json(v.get("filter"))?,
            style: style_from_json(v.get("marker")),
        },
        label_positions: v
            .get("label_positions")
            .and_then(JsonValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(label_position)
            .collect(),
        switchers: switchers_from_json(v.get("switchers"))?,
        // Absent (older files): points on; `null`: off.
        points: if v.get("points").is_none() {
            d.points
        } else {
            layer("points").map(|l| PointsElement {
                jitter: l
                    .get("jitter")
                    .and_then(JsonValue::as_str)
                    .and_then(JitterKind::from_key)
                    .unwrap_or(points_default.jitter),
                jitter_width: f(l, "jitter_width", points_default.jitter_width).clamp(0.0, 1.0),
                fade_unselected: b(l, "fade_unselected", points_default.fade_unselected),
                label_selected_only: b(l, "label_selected_only", points_default.label_selected_only),
            })
        },
        smoother: layer("smoother").map(|l| SmootherElement {
            method: l
                .get("method")
                .and_then(JsonValue::as_str)
                .and_then(SmoothMethod::from_key)
                .unwrap_or(smoother_default.method),
            lambda: f(l, "lambda", smoother_default.lambda).max(0.0),
            span: f(l, "span", smoother_default.span).clamp(0.05, 4.0),
            local_degree: u(l, "local_degree", smoother_default.local_degree).clamp(1, 2),
            band: b(l, "band", smoother_default.band),
            level: f(l, "level", smoother_default.level).clamp(0.5, 0.999),
        }),
        fit: layer("fit").map(|l| FitLineElement {
            degree: u(l, "degree", 1).clamp(1, 5),
            band: b(l, "band", true),
            prediction: b(l, "prediction", fit_default.prediction),
            level: f(l, "level", 0.95).clamp(0.5, 0.999),
            labels: FitLabels {
                equation: b(l, "show_equation", fit_default.labels.equation),
                r_squared: b(l, "show_r_squared", fit_default.labels.r_squared),
                p_value: b(l, "show_p_value", fit_default.labels.p_value),
            },
        }),
        box_plot: layer("box_plot")
            .map(|l| BoxPlotElement { outliers: b(l, "outliers", true), notches: b(l, "notches", false) }),
        violin: layer("violin").map(|l| ViolinElement {
            bandwidth_scale: f(l, "bandwidth_scale", 1.0).clamp(0.05, 20.0),
            quartiles: b(l, "quartiles", true),
        }),
        summary: layer("summary").map(|l| SummaryElement {
            stat: l.get("stat").and_then(JsonValue::as_str).and_then(SummaryStat::from_key).unwrap_or_default(),
            error_bars: l
                .get("error_bars")
                .and_then(JsonValue::as_str)
                .and_then(ErrorBars::from_key)
                .unwrap_or_default(),
            level: f(l, "level", 0.95).clamp(0.5, 0.999),
            error_look: error_look_from_json(l),
        }),
        ellipse: layer("ellipse").map(|l| EllipseElement {
            levels: probabilities(l, "levels", &EllipseElement::default().levels),
            correlation: b(l, "correlation", true),
        }),
        contour: layer("contour").map(|l| ContourElement {
            fractions: probabilities(l, "fractions", &ContourElement::default().fractions),
            bandwidth_scale: f(l, "bandwidth_scale", 1.0).clamp(0.05, 20.0),
            filled: b(l, "filled", false),
        }),
        bins: layer("bins")
            .map(|l| BinElement { hexagonal: b(l, "hexagonal", true), bins: u(l, "bins", 20).clamp(2, 200) }),
        function: layer("function").map(|l| FunctionElement { formulas: name_list(l, "formulas").unwrap_or_default() }),
        bars: layer("bars").map(|l| BarElement {
            stat: stat(l, SummaryStat::Mean),
            layout: l.get("layout").and_then(JsonValue::as_str).and_then(BarLayout::from_key).unwrap_or_default(),
            error_bars: l
                .get("error_bars")
                .and_then(JsonValue::as_str)
                .and_then(ErrorBars::from_key)
                .unwrap_or_default(),
            level: f(l, "level", 0.95).clamp(0.5, 0.999),
            labels: b(l, "labels", false),
            error_look: error_look_from_json(l),
        }),
        line: layer("line").map(|l| LineElement {
            connect: l.get("connect").and_then(JsonValue::as_str).and_then(LineConnect::from_key).unwrap_or_default(),
            stat: stat(l, SummaryStat::Mean),
        }),
        heatmap: layer("heatmap")
            .map(|l| HeatmapElement { stat: stat(l, SummaryStat::Mean), labels: b(l, "labels", true) }),
        histogram: layer("histogram").map(|l| HistogramElement {
            bins: match (l.get("bin_width").and_then(JsonValue::as_f64), l.get("bins").and_then(JsonValue::as_usize)) {
                (Some(w), _) if w > 0.0 && w.is_finite() => HistogramBins::Width(w),
                (_, Some(k)) => HistogramBins::Count(k.clamp(1, 1000)),
                _ => HistogramBins::Auto,
            },
            stacked: b(l, "stacked", false),
            density: b(l, "density", false),
        }),
        dot_plot: layer("dot_plot").map(|l| DotPlotElement {
            width: l.get("width").and_then(JsonValue::as_f64).filter(|w| *w > 0.0 && w.is_finite()),
        }),
        probability_plot: layer("probability_plot").map(|l| ProbabilityPlotElement {
            family: l
                .get("distribution")
                .and_then(JsonValue::as_str)
                .and_then(stat_core::distribution_fit::FitFamily::from_key)
                .unwrap_or(stat_core::distribution_fit::FitFamily::Normal),
            band: b(l, "band", true),
            level: f(l, "level", 0.95).clamp(0.5, 0.999),
        }),
        legend: v.get("legend").and_then(JsonValue::as_str).and_then(LegendPlacement::from_key).unwrap_or_default(),
        form: match layer("form").and_then(|l| l.get("kind")).and_then(JsonValue::as_str) {
            Some("treemap") => {
                let l = layer("form").expect("present");
                GraphForm::Treemap(TreemapSettings {
                    size_by_sum: b(l, "size_by_sum", false),
                    labels: b(l, "labels", true),
                })
            }
            Some("parallel") => GraphForm::Parallel,
            Some("multi-vari") => GraphForm::MultiVari,
            Some("scatter-matrix") => GraphForm::ScatterMatrix,
            Some("cluster-map") => {
                let l = layer("form").expect("present");
                let d = ClusterMapSettings::default();
                GraphForm::ClusterMap(ClusterMapSettings {
                    standardize: b(l, "standardize", d.standardize),
                    cluster_rows: b(l, "cluster_rows", d.cluster_rows),
                    cluster_columns: b(l, "cluster_columns", d.cluster_columns),
                    linkage: l
                        .get("linkage")
                        .and_then(JsonValue::as_str)
                        .and_then(stat_viz::cluster_map::linkage_from_key)
                        .unwrap_or(d.linkage),
                    values: b(l, "values", d.values),
                })
            }
            Some("estimation") => GraphForm::Estimation,
            Some(kind @ ("sankey" | "chord")) => {
                let l = layer("form").expect("present");
                let d = FlowSettings::default();
                let f =
                    FlowSettings { size_by_sum: b(l, "size_by_sum", d.size_by_sum), labels: b(l, "labels", d.labels) };
                if kind == "sankey" { GraphForm::Sankey(f) } else { GraphForm::Chord(f) }
            }
            Some("network") => {
                let l = layer("form").expect("present");
                let d = NetworkSettings::default();
                GraphForm::Network(NetworkSettings {
                    seed: l.get("seed").and_then(JsonValue::as_f64).filter(|v| *v >= 0.0).map_or(d.seed, |v| v as u64),
                    iterations: u(l, "iterations", d.iterations).clamp(1, 5000),
                    labels: b(l, "labels", d.labels),
                })
            }
            Some(kind) => form_from_json(kind, layer("form").expect("present")),
            None => GraphForm::Panels,
        },
        axes: axes_from_json(v.get("axes")),
        hull: layer("hull").map(|l| HullElement { fill: b(l, "fill", HullElement::default().fill) }),
        brackets: layer("brackets").map(|l| {
            let d = BracketElement::default();
            BracketElement {
                test: l.get("test").and_then(JsonValue::as_str).and_then(BracketTest::from_key).unwrap_or(d.test),
                label: l.get("label").and_then(JsonValue::as_str).and_then(BracketLabel::from_key).unwrap_or(d.label),
                only_significant: b(l, "only_significant", d.only_significant),
                alpha: f(l, "alpha", d.alpha).clamp(1e-6, 0.5),
                pairs: l
                    .get("pairs")
                    .and_then(JsonValue::as_array)
                    .map(|list| {
                        list.iter()
                            .filter_map(|p| {
                                let p = p.as_array()?;
                                Some((p.first()?.as_str()?.to_owned(), p.get(1)?.as_str()?.to_owned()))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                thresholds: l
                    .get("thresholds")
                    .and_then(JsonValue::as_array)
                    .and_then(|t| {
                        let v: Vec<f64> = t.iter().filter_map(JsonValue::as_f64).collect();
                        let t: [f64; 4] = v.try_into().ok()?;
                        // Each threshold in (0, 1) and below the one before.
                        (t.iter().all(|x| *x > 0.0 && *x < 1.0) && t.windows(2).all(|w| w[1] < w[0])).then_some(t)
                    })
                    .unwrap_or(d.thresholds),
                ns_label: b(l, "ns_label", d.ns_label),
                gap: f(l, "gap", d.gap).clamp(0.0, 1.0),
            }
        }),
        estimation: layer("estimation").map_or_else(EstimationSettings::default, |l| {
            let d = EstimationSettings::default();
            EstimationSettings {
                interval: l
                    .get("interval")
                    .and_then(JsonValue::as_str)
                    .and_then(EstimationInterval::from_key)
                    .unwrap_or(d.interval),
                resamples: u(l, "resamples", d.resamples)
                    .clamp(stat_viz::estimation::RESAMPLE_RANGE.0, stat_viz::estimation::RESAMPLE_RANGE.1),
                seed: l.get("seed").and_then(JsonValue::as_f64).filter(|v| *v >= 0.0).map_or(d.seed, |v| v as u64),
                level: f(l, "level", d.level).clamp(0.5, 0.999),
                reference: l.get("reference").and_then(JsonValue::as_str).map(str::to_owned),
            }
        }),
        layer_order: v
            .get("layer_order")
            .and_then(JsonValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|k| k.as_str().and_then(stat_viz::ElementKind::from_key))
            .collect(),
    })
}

/// The chart forms saved with their own settings (pie, radar, polar, wind
/// rose, ternary, waterfall, ridgeline, vector field) from their `form`
/// object; unknown kinds give panels, missing settings their defaults, and
/// numbers out of range are clamped.
fn form_from_json(kind: &str, l: &JsonValue) -> GraphForm {
    let b = |key: &str, default: bool| l.get(key).and_then(JsonValue::as_bool).unwrap_or(default);
    let number = |key: &str, default: u16, lo: u16, hi: u16| -> u16 {
        l.get(key)
            .and_then(JsonValue::as_usize)
            .map_or(default, |v| u16::try_from(v.clamp(usize::from(lo), usize::from(hi))).unwrap_or(default))
    };
    match kind {
        "pie" => {
            let d = PieSettings::default();
            GraphForm::Pie(PieSettings {
                donut: b("donut", d.donut),
                size_by_sum: b("size_by_sum", d.size_by_sum),
                labels: b("labels", d.labels),
            })
        }
        "radar" => GraphForm::Radar(RadarSettings { group_means: b("group_means", false), fill: b("fill", false) }),
        "polar" => GraphForm::Polar(PolarSettings {
            angle: l.get("angle").and_then(JsonValue::as_str).and_then(AngleUnit::from_key).unwrap_or_default(),
            connect: b("connect", false),
        }),
        "wind-rose" => {
            let d = WindRoseSettings::default();
            let sectors = u8::try_from(number("sectors", u16::from(d.sectors), 4, 72)).unwrap_or(d.sectors);
            GraphForm::WindRose(WindRoseSettings { sectors, percent: b("percent", d.percent) })
        }
        "ternary" => GraphForm::Ternary,
        "waterfall" => {
            let d = WaterfallSettings::default();
            GraphForm::Waterfall(WaterfallSettings { total: b("total", d.total), labels: b("labels", d.labels) })
        }
        "ridgeline" => {
            let d = RidgelineSettings::default();
            GraphForm::Ridgeline(RidgelineSettings {
                height: number("height", d.height, 20, 400),
                rug: b("rug", d.rug),
            })
        }
        "vector-field" => {
            GraphForm::VectorField(VectorSettings { scale: number("scale", VectorSettings::default().scale, 10, 1000) })
        }
        _ => GraphForm::Panels,
    }
}

/// The `format` tag of a graph pipeline file.
pub const PIPELINE_FORMAT: &str = "anovacraft-graph";

/// File extension of graph pipeline files.
pub const PIPELINE_EXTENSION: &str = "graph.json";

/// A graph as a standalone pipeline file: a JSON object with a `format` tag
/// and the graph (columns by name), so the same graph can be rebuilt on any
/// dataset with columns of those names — by opening it in the application or
/// by any program that reads JSON:
///
/// ```json
/// { "format": "anovacraft-graph", "version": 1, "graph": { "analysis": "graph", … } }
/// ```
#[must_use]
pub fn pipeline_text(spec: &GraphSpec) -> String {
    obj([("format", s(PIPELINE_FORMAT)), ("version", JsonValue::Number(1.0)), ("graph", graph_to_json(spec))])
        .to_pretty()
}

/// Read a graph pipeline file written by [`pipeline_text`].
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] for text that is not JSON, not a
/// graph pipeline, or from a newer version.
pub fn pipeline_from_text(text: &str) -> Result<GraphSpec> {
    let doc = stat_data::parse_json(text)?;
    if doc.get("format").and_then(JsonValue::as_str) != Some(PIPELINE_FORMAT) {
        return Err(invalid("this is not a graph pipeline file"));
    }
    if doc.get("version").and_then(JsonValue::as_f64).is_some_and(|v| v > 1.0) {
        return Err(invalid("this graph pipeline was written by a newer version"));
    }
    graph_from_json(member(&doc, "graph")?)
}

/// Columns a graph uses that `table` lacks (empty when it fits).
#[must_use]
pub fn missing_columns<'a>(spec: &'a GraphSpec, table: &stat_data::DataTable) -> Vec<&'a str> {
    spec.columns().into_iter().filter(|c| table.find(c).is_none()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipelines_round_trip() {
        let spec = GraphSpec {
            encoding: EncodingPlan { x: vec!["Dose".into()], y: vec!["Response".into()], ..Default::default() },
            fit: Some(FitLineElement::default()),
            ..GraphSpec::default()
        };
        let text = pipeline_text(&spec);
        assert!(text.contains("anovacraft-graph"));
        assert_eq!(pipeline_from_text(&text).unwrap(), spec);
        assert!(pipeline_from_text("{}").is_err());
        assert!(pipeline_from_text("not json").is_err());
        assert!(pipeline_from_text(r#"{"format":"anovacraft-graph","version":2,"graph":{}}"#).is_err());
        let t = stat_data::DataTable::new("t", vec![stat_data::Column::numeric("Dose", vec![1.0])]).unwrap();
        assert_eq!(missing_columns(&spec, &t), ["Response"]);
    }

    #[test]
    fn marker_style_and_moved_labels_round_trip() {
        let mut spec = GraphSpec {
            encoding: EncodingPlan { x: vec!["a".into()], y: vec!["b".into()], ..Default::default() },
            ..GraphSpec::default()
        };
        spec.options.style = MarkerStyle {
            size: Some(6.5),
            color: Some(Rgba::rgb(10, 20, 30)),
            shape: Some(MarkShape::Diamond),
            ..MarkerStyle::default()
        };
        spec.options.style.level_colors.insert(MarkerStyle::key("g", "x y"), Rgba([1, 2, 3, 128]));
        spec.options.style.level_shapes.insert(MarkerStyle::key("g", "x y"), MarkShape::Cross);
        spec.options.style.level_shapes.insert(MarkerStyle::key("h", ""), MarkShape::Up);
        spec.options.style.level_sizes.insert(MarkerStyle::key("g", "z"), 9.5);
        let mut own = MarkerStyle { size: Some(3.0), ..MarkerStyle::default() };
        own.level_colors.insert(MarkerStyle::key("g", "x y"), Rgba::rgb(9, 9, 9));
        own.level_sizes.insert(MarkerStyle::key("g", "x y"), 12.0);
        spec.options.style.panels.insert(2, own);
        spec.place_label(1, "fit/0", [0.25, 0.5]);
        spec.place_label(0, "ellipse/2", [1.0, 0.0]);
        assert_eq!(pipeline_from_text(&pipeline_text(&spec)).unwrap(), spec);
        // Bad entries are dropped rather than failing the file.
        let odd = stat_data::parse_json(
            r##"{"encoding":{},"marker":{"size":-1,"color":"red","shape":"star",
                "levels":[{"column":"g"},{"column":"g","level":"a","color":"#ff0000"}]},
                "label_positions":[{"panel":0,"id":"fit/0","at":[2,-1]},{"panel":0,"at":[0,0]},{"id":"x"}]}"##,
        )
        .unwrap();
        let g = graph_from_json(&odd).unwrap();
        assert_eq!((g.options.style.size, g.options.style.color, g.options.style.shape), (None, None, None));
        assert_eq!(g.options.style.level_colors.len(), 1);
        assert_eq!(g.label_positions, [LabelPosition { panel: 0, id: "fit/0".into(), at: [1.0, 0.0] }]);
    }

    /// The right axis, typed titles, breaks and markup survive saving; a
    /// graph without them is written exactly as before (no `y2`, no `axes`).
    #[test]
    fn axes_and_right_axis_round_trip() {
        let plain = GraphSpec {
            encoding: EncodingPlan { x: vec!["a".into()], y: vec!["b".into()], ..Default::default() },
            ..GraphSpec::default()
        };
        let text = pipeline_text(&plain);
        assert!(!text.contains("y2") && !text.contains("\"axes\""), "{text}");
        let mut spec = plain.clone();
        spec.encoding.y2 = vec!["c".into(), "d".into()];
        spec.axes = GraphAxes {
            title: Some("Growth of $dN/dt$".into()),
            x_title: Some("t_{0}".into()),
            y_title: None,
            y2_title: Some("\\mu g".into()),
            x_breaks: stat_viz::AxisBreaks::single(5.0, 6.0),
            y_breaks: stat_viz::AxisBreaks {
                breaks: vec![stat_viz::AxisBreak { from: 40.0, to: 950.0 }, stat_viz::AxisBreak { from: 1200.0, to: 5000.0 }],
                segments: vec![stat_viz::AxisSegment { scale: Some(AxisScale::Log10), share: Some(60.0) }],
                mark: stat_viz::BreakMark::Zigzag,
            },
            markup_labels: true,
            style: Some({
                let mut style = crate::plot_style::PlotStyle::default();
                style.axes.y.minor = stat_viz::axis_settings::MinorTicks::Subdivisions(5);
                style.axes.y2.grid = stat_viz::axis_settings::GridLines::None;
                style.axes.show_key = false;
                style.annotations.items.push(stat_viz::Annotation {
                    shape: stat_viz::AnnotationShape::Text,
                    anchor: stat_viz::AnnotationAnchor::Frame,
                    panel: 1,
                    at: [0.25, 0.75],
                    to: [0.25, 0.75],
                    text: "Batch 2\nrecalibrated".into(),
                });
                style.key.hidden.push("smoother/Smoother".into());
                style
            }),
        };
        spec.layer_order = vec![stat_viz::ElementKind::Points, stat_viz::ElementKind::BoxPlot];
        assert_eq!(pipeline_from_text(&pipeline_text(&spec)).unwrap(), spec);
        // Unknown layer names in a drawing order are skipped.
        let order = stat_data::parse_json(r#"{"encoding":{},"layer_order":["fit","sparkles","points"]}"#).unwrap();
        assert_eq!(
            graph_from_json(&order).unwrap().layer_order,
            [stat_viz::ElementKind::FitLine, stat_viz::ElementKind::Points]
        );
        // Earlier versions read the first break of each axis.
        let text = pipeline_text(&spec);
        assert!(text.contains("\"y_break\": [40, 950]") || text.contains("\"y_break\":[40,950]"), "{text}");
        // Reversed or empty breaks and blank titles are tidied when read.
        let odd = stat_data::parse_json(
            r#"{"encoding":{"y2":["c"]},"axes":{"title":" ","x_break":[5,1],"y_break":[2,2],"markup_labels":1}}"#,
        )
        .unwrap();
        let g = graph_from_json(&odd).unwrap();
        assert_eq!(g.encoding.y2, ["c"]);
        assert_eq!(g.axes.x_breaks, stat_viz::AxisBreaks::single(1.0, 5.0));
        assert_eq!((g.axes.title.as_deref(), g.axes.y_breaks.is_empty(), g.axes.markup_labels), (None, true, false));
    }

    #[test]
    fn forms_round_trip() {
        for form in [
            GraphForm::Panels,
            GraphForm::Parallel,
            GraphForm::MultiVari,
            GraphForm::ScatterMatrix,
            GraphForm::Treemap(TreemapSettings { size_by_sum: true, labels: false }),
            GraphForm::ClusterMap(ClusterMapSettings {
                standardize: false,
                cluster_rows: true,
                cluster_columns: false,
                linkage: stat_viz::cluster_map::LINKAGES[3],
                values: true,
            }),
            GraphForm::Estimation,
            GraphForm::Sankey(FlowSettings { size_by_sum: true, labels: false }),
            GraphForm::Chord(FlowSettings { size_by_sum: false, labels: true }),
            GraphForm::Network(NetworkSettings { seed: 42, iterations: 250, labels: false }),
            GraphForm::Pie(PieSettings { donut: true, size_by_sum: true, labels: false }),
            GraphForm::Pie(PieSettings::default()),
            GraphForm::Radar(RadarSettings { group_means: true, fill: true }),
            GraphForm::Polar(PolarSettings { angle: AngleUnit::Radians, connect: true }),
            GraphForm::Polar(PolarSettings::default()),
            GraphForm::WindRose(WindRoseSettings { sectors: 36, percent: false }),
            GraphForm::Ternary,
            GraphForm::Waterfall(WaterfallSettings { total: false, labels: false }),
            GraphForm::Ridgeline(RidgelineSettings { height: 220, rug: false }),
            GraphForm::VectorField(VectorSettings { scale: 250 }),
        ] {
            let spec = GraphSpec {
                encoding: EncodingPlan { x: vec!["a".into(), "b".into()], ..Default::default() },
                form,
                ..GraphSpec::default()
            };
            let back = graph_from_json(&graph_to_json(&spec)).unwrap();
            assert_eq!(back, spec);
            assert_eq!(pipeline_from_text(&pipeline_text(&spec)).unwrap(), spec, "{form:?}");
        }
        // Lollipop bars keep their layout.
        let spec = GraphSpec {
            bars: Some(BarElement { layout: BarLayout::Lollipop, ..BarElement::default() }),
            ..GraphSpec::default()
        };
        assert_eq!(graph_from_json(&graph_to_json(&spec)).unwrap(), spec);
        // Settings out of range are clamped; missing ones take their defaults.
        let odd = stat_data::parse_json(r#"{"encoding":{},"form":{"kind":"wind-rose","sectors":500}}"#).unwrap();
        let rose = GraphForm::WindRose(WindRoseSettings { sectors: 72, percent: true });
        assert_eq!(graph_from_json(&odd).unwrap().form, rose);
        let odd = stat_data::parse_json(r#"{"encoding":{},"form":{"kind":"pie"}}"#).unwrap();
        assert_eq!(graph_from_json(&odd).unwrap().form, GraphForm::Pie(PieSettings::default()));
        let odd = stat_data::parse_json(r#"{"encoding":{},"form":{"kind":"a later kind"}}"#).unwrap();
        assert_eq!(graph_from_json(&odd).unwrap().form, GraphForm::Panels);
    }

    /// The beeswarm layout, hulls, brackets and estimation settings survive
    /// a save and reopen; files without them open with the defaults.
    #[test]
    fn annotation_layers_round_trip() {
        let spec = GraphSpec {
            encoding: EncodingPlan { x: vec!["g".into()], y: vec!["y".into()], ..Default::default() },
            points: Some(PointsElement { jitter: JitterKind::Swarm, ..PointsElement::default() }),
            hull: Some(HullElement { fill: false }),
            brackets: Some(BracketElement {
                test: BracketTest::Dunnett,
                label: BracketLabel::Stars,
                only_significant: true,
                alpha: 0.01,
                pairs: vec![("A".into(), "C".into())],
                thresholds: [0.1, 0.05, 0.01, 0.001],
                ns_label: true,
                gap: 0.12,
            }),
            estimation: EstimationSettings {
                interval: EstimationInterval::Welch,
                resamples: 2000,
                seed: 99,
                level: 0.9,
                reference: Some("Control".into()),
            },
            form: GraphForm::Estimation,
            ..GraphSpec::default()
        };
        assert_eq!(pipeline_from_text(&pipeline_text(&spec)).unwrap(), spec);
        let old = stat_data::parse_json(r#"{"encoding":{"x":["g"]}}"#).unwrap();
        let g = graph_from_json(&old).unwrap();
        assert!(g.hull.is_none() && g.brackets.is_none());
        assert_eq!(g.estimation, EstimationSettings::default());
    }

    #[test]
    fn older_graphs_still_open() {
        // A graph written before pages, filters and switchers existed.
        let old = stat_data::parse_json(
            r#"{"analysis":"graph","encoding":{"x":["a"],"y":["b"],"group_x":"g","wrap":null,"color":null},
                "x_scale":"log10","y_scale":"linear","points":null,"smoother":null,"fit":{"degree":2},"box_plot":null}"#,
        )
        .unwrap();
        let g = graph_from_json(&old).unwrap();
        assert_eq!((g.encoding.x.as_slice(), g.encoding.group_x.as_deref()), (&["a".to_owned()][..], Some("g")));
        assert_eq!((g.options.x_scale, g.options.page), (AxisScale::Log10, 0));
        assert!(!g.options.filter.is_active() && g.switchers.is_empty() && g.points.is_none());
        assert_eq!(g.fit.unwrap().degree, 2);
        // Both a grid and wrap saved (hand-edited): kept as written, so
        // validation reports it rather than silently dropping one.
        let both = stat_data::parse_json(r#"{"encoding":{"x":["a"],"group_x":"g","wrap":"h"}}"#).unwrap();
        let g = graph_from_json(&both).unwrap();
        assert!(g.encoding.group_x.is_some() && g.encoding.wrap.is_some());
        let bad = stat_data::parse_json(r#"{"encoding":{},"switchers":[{"role":"nope"}]}"#).unwrap();
        assert!(graph_from_json(&bad).is_err());
    }
}
