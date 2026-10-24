//! Flow and network charts drawn from categorical columns: Sankey
//! (alluvial) diagrams, chord diagrams and node-link networks. Each is a
//! whole-panel chart form with its own [`Frame`] (one panel, geometry in
//! [`PanelExtra`]); clicking a flow, ribbon, arc, node or edge selects its
//! rows, and parts holding selected rows are outlined.
//!
//! * **Sankey diagram** — two or more columns on the horizontal shelf are
//!   stages, left to right; each level of a stage is a node whose height is
//!   its share of the rows (frequency-weighted) or of the sum of the first
//!   vertical column, and each pair of consecutive stages is joined by flows
//!   of the rows passing between their levels (Riehmann, Hanfler & Froehlich,
//!   "Interactive Sankey diagrams", *IEEE InfoVis* 2005). Flows keep the
//!   color of their level in the first stage (as in alluvial diagrams,
//!   Rosvall & Bergstrom, *PLoS ONE* 5(1), 2010), so a group can be followed
//!   across. Nodes are ordered within each stage to reduce crossings by the
//!   barycentre heuristic (Sugiyama, Tagawa & Toda, *IEEE Trans. SMC* 11(2),
//!   1981): alternate sweeps left to right and right to left sort each
//!   stage's nodes by the flow-weighted mean position of their neighbors in
//!   the stage just swept; the ordering with the fewest weighted crossings
//!   (`Σ wᵢ wⱼ` over pairs of flows that cross) is kept.
//! * **Chord diagram** — the first two horizontal columns are "from" and "to"
//!   (one set of nodes when they share levels, as in an origin–destination
//!   table). Each node is an arc of a circle whose angle is proportional to
//!   its flow out plus in; each pair is a ribbon from the "from" node's arc
//!   to the "to" node's arc, its ends as wide as the flow (Krzywinski et al.,
//!   "Circos", *Genome Research* 19, 2009). Ribbons take the "from" color.
//! * **Network** — the first two horizontal columns are the two ends of each
//!   edge, an optional numeric vertical column its weight (else each row
//!   counts once); edges between the same pair of nodes are combined. Nodes
//!   are placed by the force-directed algorithm of Fruchterman & Reingold
//!   ("Graph drawing by force-directed placement", *Software: Practice and
//!   Experience* 21(11), 1991): in a unit frame with `k = √(1/n)`, every pair
//!   of nodes repels with force `k²/d`, the two ends of every edge attract
//!   with `d²/k`, and each node moves along its net force by at most the
//!   temperature, which cools linearly from 0.1 to 0 over the iterations.
//!   Starting positions are seeded, so the layout is the same every time.
//!   Node size grows with degree; edge width with weight.

use std::collections::BTreeMap;

use stat_core::Result;
use stat_core::rng::Rng;
use stat_data::{DataTable, RowStateManager};
use stat_linear::natural_order;

use crate::element::{Element, ElementKind};
use crate::encoding::{EncodingPlan, invalid, is_continuous};
use crate::frame::{
    AxisKind, AxisSpec, ColorKey, Frame, FrameOptions, GroupStyle, Legend, Panel, PanelExtra, filter_note, select_rows,
};
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, MarkShape, Region, Rgba, TextAnchor};
use crate::palette;
use crate::scale::AxisScale;
use crate::view::format_value;

/// Settings of a Sankey or chord diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowSettings {
    /// Size flows by the sum of the first vertical column (else by rows).
    pub size_by_sum: bool,
    /// Write the nodes' names and sizes.
    pub labels: bool,
}

impl Default for FlowSettings {
    fn default() -> Self {
        Self { size_by_sum: false, labels: true }
    }
}

/// Settings of a network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkSettings {
    /// Seed of the starting positions.
    pub seed: u64,
    /// Layout iterations.
    pub iterations: usize,
    /// Write the nodes' names.
    pub labels: bool,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self { seed: 1, iterations: 500, labels: true }
    }
}

/// Most nodes a network lays out.
pub const MAX_NODES: usize = 500;

/// A node of a flow chart.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowNode {
    /// Level label.
    pub label: String,
    /// Stage (Sankey) or 0.
    pub stage: usize,
    /// Total flow through the node.
    pub value: f64,
    /// Rows through the node, ascending.
    pub rows: Vec<usize>,
    /// Outline (closed) in panel coordinates.
    pub outline: Vec<[f64; 2]>,
    /// Fill.
    pub color: Rgba,
    /// Where its label goes, and how.
    pub label_at: ([f64; 2], TextAnchor),
}

/// A flow, ribbon or edge.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowLink {
    /// Its size.
    pub value: f64,
    /// Rows in it, ascending.
    pub rows: Vec<usize>,
    /// Outline (closed) in panel coordinates; for an edge, its two ends.
    pub outline: Vec<[f64; 2]>,
    /// Fill (or line) color.
    pub color: Rgba,
    /// Name for hover.
    pub name: String,
    /// Line width of an edge (screen points; 0 for areas).
    pub width: f32,
}

/// What a flow chart panel draws.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowGeometry {
    /// Nodes (drawn over the links).
    pub nodes: Vec<FlowNode>,
    /// Links (flows, ribbons or edges).
    pub links: Vec<FlowLink>,
    /// Headings: text and where (stage names of a Sankey diagram).
    pub headings: Vec<([f64; 2], String)>,
    /// Write node labels.
    pub labels: bool,
    /// Links are lines (a network) rather than areas.
    pub edges: bool,
    /// Weighted crossings of a Sankey diagram's flows (after ordering).
    pub crossings: f64,
}

/// The rows a flow chart uses: shown, on the page, not excluded, inside
/// the filter; their weights (frequency, times the first vertical column's
/// value when `sum` is set); and notes.
fn flow_rows(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    sum: bool,
) -> (Vec<usize>, Vec<f64>, Vec<String>, crate::frame::RowSet) {
    let rs = select_rows(table, state, plan, options);
    let n = table.n_rows();
    let mut notes = Vec::new();
    let column = plan.y.first().filter(|c| sum && is_continuous(table, c));
    if sum && column.is_none() {
        notes.push("Sizing by a sum needs a numeric column on the vertical axis; sizes are row counts.".to_owned());
    }
    let values = column.and_then(|c| table.find(c)).and_then(|i| table.column(i)).and_then(|c| c.as_numeric());
    let mut dropped = 0;
    let weight: Vec<f64> = (0..n)
        .map(|r| {
            let f = rs.freq[r];
            match values {
                Some(v) => match v.get(r).copied() {
                    Some(x) if x.is_finite() && x >= 0.0 && (f * x).is_finite() => f * x,
                    _ => {
                        dropped += 1;
                        0.0
                    }
                },
                None => f,
            }
        })
        .collect();
    if dropped > 0 {
        notes.push(format!("{dropped} row(s) with a missing or negative size are left out."));
    }
    let used: Vec<usize> = rs
        .page_rows
        .iter()
        .copied()
        .filter(|&r| !rs.filtered[r] && (r >= state.len() || !state.is_excluded(r)) && weight[r] > 0.0)
        .collect();
    notes.extend(filter_note(state, options, &rs.passes));
    (used, weight, notes, rs)
}

/// Text of each row in `column` (`None` when blank or absent).
fn labels_of(table: &DataTable, column: &str) -> Vec<Option<String>> {
    let col = table.find(column).and_then(|i| table.column(i));
    (0..table.n_rows()).map(|r| col.map(|c| c.level_text(r).trim().to_owned()).filter(|l| !l.is_empty())).collect()
}

/// Sorted distinct values (natural order).
fn sorted_levels<'a>(labels: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut l: Vec<String> = labels.cloned().collect();
    l.sort_by(|a, b| natural_order(a, b));
    l.dedup();
    l
}

/// Flows from one stage to the next, keyed by (from node, to node, first
/// stage's level): the summed size and the rows.
type StageFlows = BTreeMap<(usize, usize, usize), (f64, Vec<usize>)>;

/// A frame of one panel on unnamed linear axes.
fn flow_frame(extra: PanelExtra, legend: Legend, notes: Vec<String>, rs: crate::frame::RowSet) -> Frame {
    let plain = AxisSpec { name: String::new(), kind: AxisKind::Continuous, scale: AxisScale::Linear, time: None };
    let panel = Panel {
        grid_row: 0,
        grid_col: 0,
        title: None,
        x: plain.clone(),
        y: plain,
        points: Vec::new(),
        groups: vec![GroupStyle { label: String::new(), color: palette::DEFAULT_POINT, shape: MarkShape::Circle }],
        slot_half_width: 0.0,
        extra,
        labels: BTreeMap::new(),
        data_extent: [None; 2],
    };
    Frame {
        panels: vec![panel],
        n_rows: 1,
        n_cols: 1,
        column_headers: vec![String::new()],
        row_headers: vec![String::new()],
        legend,
        notes,
        pages: rs.pages,
        page: rs.page,
    }
}

/// Neutral fill of nodes after a Sankey diagram's first stage.
const NODE_GRAY: Rgba = Rgba::rgb(120, 128, 144);

// ---------------------------------------------------------------------------
// Sankey
// ---------------------------------------------------------------------------

/// Weighted crossings between consecutive stages: `links[s]` holds
/// `(source position, target position, weight)` between stage `s` and
/// `s + 1`; two links cross when their sources and targets are in opposite
/// orders, adding the product of their weights.
#[must_use]
pub fn weighted_crossings(links: &[Vec<(usize, usize, f64)>]) -> f64 {
    let mut total = 0.0;
    for stage in links {
        for (i, a) in stage.iter().enumerate() {
            for b in &stage[i + 1..] {
                if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                    total += a.2 * b.2;
                }
            }
        }
    }
    total
}

/// Order the nodes of each stage to reduce crossings (barycentre sweeps).
/// `counts[s]` is the number of nodes of stage `s` and `flows[s]` the
/// `(source node, target node, weight)` triples between stages `s` and
/// `s + 1`, with node indices into each stage. Returns `order[s]`, the node
/// indices of stage `s` from top to bottom, and the weighted crossings of
/// that ordering. The starting order (as given) is kept unless a sweep
/// improves on it.
#[must_use]
pub fn barycentre_order(counts: &[usize], flows: &[Vec<(usize, usize, f64)>]) -> (Vec<Vec<usize>>, f64) {
    let positions = |order: &[Vec<usize>]| -> Vec<Vec<usize>> {
        order
            .iter()
            .map(|o| {
                let mut p = vec![0; o.len()];
                for (rank, &node) in o.iter().enumerate() {
                    p[node] = rank;
                }
                p
            })
            .collect()
    };
    let cost = |order: &[Vec<usize>]| {
        let p = positions(order);
        let mapped: Vec<Vec<(usize, usize, f64)>> = flows
            .iter()
            .enumerate()
            .map(|(s, f)| f.iter().map(|&(a, b, w)| (p[s][a], p[s + 1][b], w)).collect())
            .collect();
        weighted_crossings(&mapped)
    };
    let mut order: Vec<Vec<usize>> = counts.iter().map(|&c| (0..c).collect()).collect();
    let mut best = (order.clone(), cost(&order));
    let stages = counts.len();
    // Sort stage `s` by the weighted mean position of its neighbors in `other`.
    let sweep = |order: &mut Vec<Vec<usize>>, s: usize, other: usize| {
        let p = positions(order);
        let mut sums = vec![(0.0, 0.0); counts[s]];
        let (f, forward) = if other < s { (&flows[other], true) } else { (&flows[s], false) };
        for &(a, b, w) in f {
            let (mine, theirs) = if forward { (b, a) } else { (a, b) };
            sums[mine].0 += w * p[other][theirs] as f64;
            sums[mine].1 += w;
        }
        let current = p[s].clone();
        let key = |node: usize| if sums[node].1 > 0.0 { sums[node].0 / sums[node].1 } else { current[node] as f64 };
        order[s].sort_by(|&a, &b| key(a).total_cmp(&key(b)).then(current[a].cmp(&current[b])));
    };
    for _ in 0..6 {
        for s in 1..stages {
            sweep(&mut order, s, s - 1);
        }
        for s in (0..stages.saturating_sub(1)).rev() {
            sweep(&mut order, s, s + 1);
        }
        let c = cost(&order);
        if c < best.1 {
            best = (order.clone(), c);
        }
    }
    best
}

/// The frame of a Sankey diagram.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] with fewer than two columns on
/// the horizontal shelf, no rows with every stage, or a plan that does not
/// fit the table.
pub fn build_sankey_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: FlowSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    if plan.x.len() < 2 {
        return Err(invalid("a Sankey diagram needs two or more columns on the horizontal axis (the stages)"));
    }
    let (used, weight, mut notes, rs) = flow_rows(table, state, plan, options, settings.size_by_sum);
    let stage_labels: Vec<Vec<Option<String>>> = plan.x.iter().map(|c| labels_of(table, c)).collect();
    let complete: Vec<usize> = used.iter().copied().filter(|&r| stage_labels.iter().all(|s| s[r].is_some())).collect();
    if complete.len() < used.len() {
        notes.push(format!("{} row(s) missing a stage are left out.", used.len() - complete.len()));
    }
    if complete.is_empty() {
        return Err(invalid("a Sankey diagram needs rows with a value in every stage"));
    }
    let label = |s: usize, r: usize| stage_labels[s][r].as_ref().expect("complete row");
    let levels: Vec<Vec<String>> =
        (0..plan.x.len()).map(|s| sorted_levels(complete.iter().map(|&r| label(s, r)))).collect();
    let code = |s: usize, r: usize| levels[s].binary_search_by(|l| natural_order(l, label(s, r))).expect("a level");
    let stages = levels.len();
    // Flows between consecutive stages, split by the first stage's level.
    let mut flows: Vec<StageFlows> = vec![BTreeMap::new(); stages - 1];
    let mut node_rows: Vec<Vec<(f64, Vec<usize>)>> = levels.iter().map(|l| vec![(0.0, Vec::new()); l.len()]).collect();
    for &r in &complete {
        for s in 0..stages {
            let e = &mut node_rows[s][code(s, r)];
            e.0 += weight[r];
            e.1.push(r);
        }
        let origin = code(0, r);
        for s in 0..stages - 1 {
            let e = flows[s].entry((code(s, r), code(s + 1, r), origin)).or_insert((0.0, Vec::new()));
            e.0 += weight[r];
            e.1.push(r);
        }
    }
    let pair_flows: Vec<Vec<(usize, usize, f64)>> = flows
        .iter()
        .map(|f| {
            let mut m: BTreeMap<(usize, usize), f64> = BTreeMap::new();
            for (&(a, b, _), (w, _)) in f {
                *m.entry((a, b)).or_default() += w;
            }
            m.into_iter().map(|((a, b), w)| (a, b, w)).collect()
        })
        .collect();
    let counts: Vec<usize> = levels.iter().map(Vec::len).collect();
    let (order, crossings) = barycentre_order(&counts, &pair_flows);
    let mut rank: Vec<Vec<usize>> = counts.iter().map(|&c| vec![0; c]).collect();
    for (s, o) in order.iter().enumerate() {
        for (k, &node) in o.iter().enumerate() {
            rank[s][node] = k;
        }
    }
    // Vertical layout: every stage carries the same total.
    let total: f64 = node_rows[0].iter().map(|n| n.0).sum();
    let most = counts.iter().copied().max().unwrap_or(1);
    let gap = if most > 1 { (0.3 / (most - 1) as f64).min(0.05) } else { 0.0 };
    let scale = (1.0 - gap * (most.saturating_sub(1)) as f64) / total;
    let span = (stages - 1) as f64;
    let node_w = 0.035 * span;
    let style = options.style.for_panel(0);
    // Top and bottom of every node.
    let mut extent: Vec<Vec<(f64, f64)>> = counts.iter().map(|&c| vec![(0.0, 0.0); c]).collect();
    let mut nodes = Vec::new();
    for s in 0..stages {
        let height = scale * total + gap * (counts[s].saturating_sub(1)) as f64;
        let mut top = 1.0 - (1.0 - height) / 2.0;
        for &k in &order[s] {
            let h = node_rows[s][k].0 * scale;
            extent[s][k] = (top - h, top);
            let x = s as f64;
            let color = if s == 0 { style.level(&plan.x[0], &levels[0][k], k).0 } else { NODE_GRAY };
            let mut rows = node_rows[s][k].1.clone();
            rows.sort_unstable();
            let (label_at, anchor) = if s + 1 == stages {
                ([x - node_w / 2.0 - 0.01 * span, top - h / 2.0], TextAnchor::RightOfText)
            } else {
                ([x + node_w / 2.0 + 0.01 * span, top - h / 2.0], TextAnchor::LeftOfText)
            };
            nodes.push(FlowNode {
                label: format!("{} ({})", levels[s][k], format_value(node_rows[s][k].0)),
                stage: s,
                value: node_rows[s][k].0,
                rows,
                outline: vec![
                    [x - node_w / 2.0, top - h],
                    [x + node_w / 2.0, top - h],
                    [x + node_w / 2.0, top],
                    [x - node_w / 2.0, top],
                ],
                color,
                label_at: (label_at, anchor),
            });
            top -= h + gap;
        }
    }
    // Flows: stacked within each node in the order of the other end (then origin).
    let mut links = Vec::new();
    for s in 0..stages - 1 {
        let mut out_cursor: Vec<f64> = extent[s].iter().map(|e| e.1).collect();
        let mut in_cursor: Vec<f64> = extent[s + 1].iter().map(|e| e.1).collect();
        let mut keys: Vec<&(usize, usize, usize)> = flows[s].keys().collect();
        keys.sort_by_key(|&&(a, b, o)| (rank[s][a], rank[s + 1][b], rank[0][o]));
        // Source ends in target order, target ends in source order.
        let mut source_end: BTreeMap<(usize, usize, usize), (f64, f64)> = BTreeMap::new();
        for &&key in &keys {
            let h = flows[s][&key].0 * scale;
            let c = &mut out_cursor[key.0];
            source_end.insert(key, (*c - h, *c));
            *c -= h;
        }
        keys.sort_by_key(|&&(a, b, o)| (rank[s + 1][b], rank[s][a], rank[0][o]));
        for &&key in &keys {
            let (w, rows) = &flows[s][&key];
            let h = w * scale;
            let c = &mut in_cursor[key.1];
            let (t0, t1) = (*c - h, *c);
            *c -= h;
            let (s0, s1) = source_end[&key];
            let (x0, x1) = (s as f64 + node_w / 2.0, (s + 1) as f64 - node_w / 2.0);
            let curve = |a: f64, b: f64| -> Vec<[f64; 2]> {
                (0..=24)
                    .map(|i| {
                        let t = f64::from(i) / 24.0;
                        let e = t * t * (3.0 - 2.0 * t);
                        [x0 + (x1 - x0) * t, a + (b - a) * e]
                    })
                    .collect()
            };
            let (lower, upper) = (curve(s0, t0), curve(s1, t1));
            let mut outline = lower.clone();
            outline.extend(upper.iter().rev());
            let mut rows = rows.clone();
            rows.sort_unstable();
            let origin = &levels[0][key.2];
            links.push(FlowLink {
                value: *w,
                rows,
                outline,
                color: style.level(&plan.x[0], origin, key.2).0,
                name: format!("{} → {}: {}", levels[s][key.0], levels[s + 1][key.1], format_value(*w)),
                width: 0.0,
            });
        }
    }
    let headings = plan.x.iter().enumerate().map(|(s, c)| ([s as f64, 1.03], c.clone())).collect();
    let legend = Legend {
        color: Some(ColorKey::Levels {
            title: plan.x[0].clone(),
            entries: levels[0]
                .iter()
                .enumerate()
                .map(|(k, l)| {
                    let (c, _) = style.level(&plan.x[0], l, k);
                    (l.clone(), c, MarkShape::Square)
                })
                .collect(),
        }),
        ..Legend::default()
    };
    notes.push(format!("Flows keep the color of their {} level; node order chosen to reduce crossings.", plan.x[0]));
    let geometry = FlowGeometry { nodes, links, headings, labels: settings.labels, edges: false, crossings };
    Ok(flow_frame(PanelExtra::Flows(geometry), legend, notes, rs))
}

// ---------------------------------------------------------------------------
// Chord
// ---------------------------------------------------------------------------

/// A point on the circle of radius `r` at angle `a` (radians, counter-clockwise from +x).
fn polar(r: f64, a: f64) -> [f64; 2] {
    [r * a.cos(), r * a.sin()]
}

/// Points along the arc of radius `r` from angle `a0` to `a1`.
fn arc(r: f64, a0: f64, a1: f64) -> Vec<[f64; 2]> {
    let steps = (((a1 - a0).abs() / 0.05).ceil() as usize).clamp(2, 200);
    (0..=steps).map(|i| polar(r, a0 + (a1 - a0) * i as f64 / steps as f64)).collect()
}

/// The quadratic Bézier from `p` to `q` through the circle's centre.
fn through_centre(p: [f64; 2], q: [f64; 2]) -> Vec<[f64; 2]> {
    (1..24)
        .map(|i| {
            let t = f64::from(i) / 24.0;
            let (u, v) = ((1.0 - t) * (1.0 - t), t * t);
            [u * p[0] + v * q[0], u * p[1] + v * q[1]]
        })
        .collect()
}

/// Inner radius of the node arcs (ribbons end here).
const CHORD_RADIUS: f64 = 1.0;

/// The frame of a chord diagram.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] with fewer than two columns on
/// the horizontal shelf, no complete rows, or a plan that does not fit the
/// table.
pub fn build_chord_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: FlowSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    if plan.x.len() < 2 {
        return Err(invalid("a chord diagram needs a \"from\" and a \"to\" column on the horizontal axis"));
    }
    let (used, weight, mut notes, rs) = flow_rows(table, state, plan, options, settings.size_by_sum);
    let (from, to) = (labels_of(table, &plan.x[0]), labels_of(table, &plan.x[1]));
    let complete: Vec<usize> = used.iter().copied().filter(|&r| from[r].is_some() && to[r].is_some()).collect();
    if complete.len() < used.len() {
        notes.push(format!("{} row(s) missing an end are left out.", used.len() - complete.len()));
    }
    if complete.is_empty() {
        return Err(invalid("a chord diagram needs rows with both ends"));
    }
    if plan.x.len() > 2 {
        notes.push("A chord diagram uses the first two horizontal columns only.".to_owned());
    }
    let end = |v: &[Option<String>], r: usize| v[r].clone().expect("complete row");
    let levels = sorted_levels(
        complete
            .iter()
            .map(|&r| from[r].as_ref().expect("complete"))
            .chain(complete.iter().map(|&r| to[r].as_ref().expect("complete"))),
    );
    let idx = |l: &str| levels.binary_search_by(|x| natural_order(x, l)).expect("a level");
    let n = levels.len();
    let mut flow: BTreeMap<(usize, usize), (f64, Vec<usize>)> = BTreeMap::new();
    let mut node_rows: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut totals = vec![0.0; n];
    for &r in &complete {
        let (a, b) = (idx(&end(&from, r)), idx(&end(&to, r)));
        let e = flow.entry((a, b)).or_insert((0.0, Vec::new()));
        e.0 += weight[r];
        e.1.push(r);
        totals[a] += weight[r];
        totals[b] += weight[r];
        node_rows[a].push(r);
        node_rows[b].push(r);
    }
    let total: f64 = totals.iter().sum();
    let gap = (std::f64::consts::TAU * 0.25 / n as f64).min(0.06);
    let scale = (std::f64::consts::TAU - gap * n as f64) / total;
    let style = options.style.for_panel(0);
    // Angles run clockwise from the top.
    let mut start = vec![0.0; n];
    let mut angle = std::f64::consts::FRAC_PI_2;
    for k in 0..n {
        start[k] = angle;
        angle -= totals[k] * scale + gap;
    }
    let mut nodes = Vec::new();
    for k in 0..n {
        let (a0, a1) = (start[k], start[k] - totals[k] * scale);
        let mut outline = arc(CHORD_RADIUS + 0.02, a0, a1);
        outline.extend(arc(CHORD_RADIUS + 0.09, a1, a0));
        let mid = 0.5 * (a0 + a1);
        let mut rows = node_rows[k].clone();
        rows.sort_unstable();
        rows.dedup();
        let anchor = if mid.cos() >= 0.0 { TextAnchor::LeftOfText } else { TextAnchor::RightOfText };
        nodes.push(FlowNode {
            label: format!("{} ({})", levels[k], format_value(totals[k])),
            stage: 0,
            value: totals[k],
            rows,
            outline,
            color: style.level(&plan.x[0], &levels[k], k).0,
            label_at: (polar(CHORD_RADIUS + 0.14, mid), anchor),
        });
    }
    // Each node's span: outgoing ribbons first (in node order), then incoming.
    let mut cursor = start.clone();
    let mut source_end: BTreeMap<(usize, usize), (f64, f64)> = BTreeMap::new();
    let mut target_end: BTreeMap<(usize, usize), (f64, f64)> = BTreeMap::new();
    for k in 0..n {
        for (&(a, b), (w, _)) in flow.iter().filter(|((a, _), _)| *a == k) {
            source_end.insert((a, b), (cursor[k], cursor[k] - w * scale));
            cursor[k] -= w * scale;
        }
        for (&(a, b), (w, _)) in flow.iter().filter(|((_, b), _)| *b == k) {
            target_end.insert((a, b), (cursor[k], cursor[k] - w * scale));
            cursor[k] -= w * scale;
        }
    }
    let mut links = Vec::new();
    for (&(a, b), (w, rows)) in &flow {
        let (s0, s1) = source_end[&(a, b)];
        let (t0, t1) = target_end[&(a, b)];
        let mut outline = arc(CHORD_RADIUS, s0, s1);
        outline.extend(through_centre(polar(CHORD_RADIUS, s1), polar(CHORD_RADIUS, t0)));
        outline.extend(arc(CHORD_RADIUS, t0, t1));
        outline.extend(through_centre(polar(CHORD_RADIUS, t1), polar(CHORD_RADIUS, s0)));
        let mut rows = rows.clone();
        rows.sort_unstable();
        links.push(FlowLink {
            value: *w,
            rows,
            outline,
            color: style.level(&plan.x[0], &levels[a], a).0,
            name: format!("{} → {}: {}", levels[a], levels[b], format_value(*w)),
            width: 0.0,
        });
    }
    let legend = Legend {
        color: Some(ColorKey::Levels {
            title: format!("{} → {}", plan.x[0], plan.x[1]),
            entries: levels
                .iter()
                .enumerate()
                .map(|(k, l)| (l.clone(), style.level(&plan.x[0], l, k).0, MarkShape::Square))
                .collect(),
        }),
        ..Legend::default()
    };
    notes.push(format!(
        "Ribbons run from {} to {} and take the color of their {} level.",
        plan.x[0], plan.x[1], plan.x[0]
    ));
    let geometry =
        FlowGeometry { nodes, links, headings: Vec::new(), labels: settings.labels, edges: false, crossings: 0.0 };
    Ok(flow_frame(PanelExtra::Flows(geometry), legend, notes, rs))
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

/// Fruchterman–Reingold positions of `n` nodes joined by `edges` (pairs of
/// node indices; self-loops ignored), in the frame `[−½, ½]²`, from seeded
/// uniform starting positions, over `iterations` steps (see the module
/// notes). Deterministic for a given seed.
#[must_use]
pub fn fruchterman_reingold(n: usize, edges: &[(usize, usize)], iterations: usize, seed: u64) -> Vec<[f64; 2]> {
    let mut rng = Rng::new(seed);
    let mut pos: Vec<[f64; 2]> = (0..n).map(|_| [rng.next_f64() - 0.5, rng.next_f64() - 0.5]).collect();
    if n < 2 {
        return vec![[0.0, 0.0]; n];
    }
    let k = (1.0 / n as f64).sqrt();
    let t0 = 0.1;
    for it in 0..iterations {
        let t = t0 * (1.0 - it as f64 / iterations as f64);
        let mut disp = vec![[0.0_f64; 2]; n];
        for i in 0..n {
            for j in i + 1..n {
                let d = [pos[i][0] - pos[j][0], pos[i][1] - pos[j][1]];
                let dist = d[0].hypot(d[1]).max(1e-9);
                let f = k * k / dist;
                let u = [d[0] / dist * f, d[1] / dist * f];
                disp[i][0] += u[0];
                disp[i][1] += u[1];
                disp[j][0] -= u[0];
                disp[j][1] -= u[1];
            }
        }
        for &(a, b) in edges.iter().filter(|(a, b)| a != b && *a < n && *b < n) {
            let d = [pos[a][0] - pos[b][0], pos[a][1] - pos[b][1]];
            let dist = d[0].hypot(d[1]).max(1e-9);
            let f = dist * dist / k;
            let u = [d[0] / dist * f, d[1] / dist * f];
            disp[a][0] -= u[0];
            disp[a][1] -= u[1];
            disp[b][0] += u[0];
            disp[b][1] += u[1];
        }
        for i in 0..n {
            let len = disp[i][0].hypot(disp[i][1]);
            if len > 0.0 {
                let step = len.min(t) / len;
                pos[i][0] = (pos[i][0] + disp[i][0] * step).clamp(-0.5, 0.5);
                pos[i][1] = (pos[i][1] + disp[i][1] * step).clamp(-0.5, 0.5);
            }
        }
    }
    pos
}

/// A circle of radius `r` around `c` as a polygon.
fn circle(c: [f64; 2], r: f64) -> Vec<[f64; 2]> {
    (0..24)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 24.0;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// The frame of a node-link network.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] with fewer than two columns on
/// the horizontal shelf, no complete rows, more than [`MAX_NODES`] nodes, or
/// a plan that does not fit the table.
pub fn build_network_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: NetworkSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    if plan.x.len() < 2 {
        return Err(invalid("a network needs the two ends of each edge as columns on the horizontal axis"));
    }
    let weighted = plan.y.first().is_some_and(|c| is_continuous(table, c));
    let (used, weight, mut notes, rs) = flow_rows(table, state, plan, options, weighted);
    let (from, to) = (labels_of(table, &plan.x[0]), labels_of(table, &plan.x[1]));
    let complete: Vec<usize> = used.iter().copied().filter(|&r| from[r].is_some() && to[r].is_some()).collect();
    if complete.len() < used.len() {
        notes.push(format!("{} row(s) missing an end are left out.", used.len() - complete.len()));
    }
    if complete.is_empty() {
        return Err(invalid("a network needs rows with both ends"));
    }
    let levels = sorted_levels(
        complete
            .iter()
            .map(|&r| from[r].as_ref().expect("complete"))
            .chain(complete.iter().map(|&r| to[r].as_ref().expect("complete"))),
    );
    let n = levels.len();
    if n > MAX_NODES {
        return Err(invalid(format!("a network draws at most {MAX_NODES} nodes (these columns have {n})")));
    }
    let idx = |l: &str| levels.binary_search_by(|x| natural_order(x, l)).expect("a level");
    // Edges between unordered pairs.
    let mut edges: BTreeMap<(usize, usize), (f64, Vec<usize>)> = BTreeMap::new();
    let mut node_rows: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut loops = 0;
    for &r in &complete {
        let (a, b) = (idx(from[r].as_deref().expect("complete")), idx(to[r].as_deref().expect("complete")));
        node_rows[a].push(r);
        node_rows[b].push(r);
        if a == b {
            loops += 1;
            continue;
        }
        let e = edges.entry((a.min(b), a.max(b))).or_insert((0.0, Vec::new()));
        e.0 += weight[r];
        e.1.push(r);
    }
    if loops > 0 {
        notes.push(format!("{loops} row(s) join a node to itself; they are counted at the node but not drawn."));
    }
    let pairs: Vec<(usize, usize)> = edges.keys().copied().collect();
    let raw = fruchterman_reingold(n, &pairs, settings.iterations.clamp(1, 5000), settings.seed);
    // Fit into the unit square, keeping the aspect.
    let (lo, hi) = raw.iter().fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(lo, hi), p| {
        ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])
    });
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-9);
    let centre = [0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1])];
    let pos: Vec<[f64; 2]> =
        raw.iter().map(|p| [0.5 + (p[0] - centre[0]) / size, 0.5 + (p[1] - centre[1]) / size]).collect();
    let mut degree = vec![0usize; n];
    for &(a, b) in &pairs {
        degree[a] += 1;
        degree[b] += 1;
    }
    let max_degree = degree.iter().copied().max().unwrap_or(1).max(1) as f64;
    let max_w = edges.values().map(|e| e.0).fold(0.0_f64, f64::max);
    let mut links = Vec::new();
    for (&(a, b), (w, rows)) in &edges {
        let mut rows = rows.clone();
        rows.sort_unstable();
        links.push(FlowLink {
            value: *w,
            rows,
            outline: vec![pos[a], pos[b]],
            color: palette::LINE.with_alpha(0.55),
            name: format!("{} – {}: {}", levels[a], levels[b], format_value(*w)),
            width: if max_w > 0.0 { (0.8 + 3.7 * w / max_w) as f32 } else { 1.0 },
        });
    }
    let nodes = (0..n)
        .map(|k| {
            let r = 0.012 + 0.022 * (degree[k] as f64 / max_degree).sqrt();
            let mut rows = node_rows[k].clone();
            rows.sort_unstable();
            rows.dedup();
            FlowNode {
                label: levels[k].clone(),
                stage: 0,
                value: degree[k] as f64,
                rows,
                outline: circle(pos[k], r),
                color: palette::DEFAULT_POINT,
                label_at: ([pos[k][0] + r + 0.006, pos[k][1]], TextAnchor::LeftOfText),
            }
        })
        .collect();
    let what = if weighted { format!("the sum of {}", plan.y[0]) } else { "the number of rows".to_owned() };
    notes.push(format!(
        "Force-directed layout (Fruchterman–Reingold, {} iterations, seed {}); edge width shows {what}, node size the number of neighbors.",
        settings.iterations.clamp(1, 5000),
        settings.seed
    ));
    let geometry =
        FlowGeometry { nodes, links, headings: Vec::new(), labels: settings.labels, edges: true, crossings: 0.0 };
    Ok(flow_frame(PanelExtra::Flows(geometry), Legend::default(), notes, rs))
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// A thin quadrilateral around the segment `a`–`b` (for clicking an edge).
fn segment_outline(a: [f64; 2], b: [f64; 2], half: f64) -> Vec<[f64; 2]> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy).max(1e-12);
    let (nx, ny) = (-dy / len * half, dx / len * half);
    vec![[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]]
}

/// Draws a Sankey diagram, chord diagram or network panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlowElement;

impl Element for FlowElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Flows
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::Flows(g) = &panel.extra else { return layer };
        let selected = |rows: &[usize]| rows.iter().any(|&r| r < state.len() && state.is_selected(r));
        let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();
        for l in &g.links {
            if g.edges {
                let hit = selected(&l.rows);
                layer.marks.push(Mark::Line {
                    points: l.outline.clone(),
                    color: if hit { palette::SELECTED } else { l.color },
                    width: l.width,
                    pattern: LinePattern::Solid,
                    name: l.name.clone(),
                });
                layer
                    .regions
                    .push(Region { outline: segment_outline(l.outline[0], l.outline[1], 0.008), rows: l.rows.clone() });
            } else {
                layer.marks.push(Mark::Polygon {
                    points: l.outline.clone(),
                    fill: l.color.with_alpha(0.45),
                    stroke: l.color.with_alpha(0.6),
                    name: l.name.clone(),
                });
                layer.regions.push(Region { outline: l.outline.clone(), rows: l.rows.clone() });
                if selected(&l.rows) {
                    rings.push(l.outline.clone());
                }
            }
        }
        for node in &g.nodes {
            layer.marks.push(Mark::Polygon {
                points: node.outline.clone(),
                fill: node.color,
                stroke: Rgba::rgb(255, 255, 255),
                name: node.label.clone(),
            });
            layer.regions.push(Region { outline: node.outline.clone(), rows: node.rows.clone() });
            if selected(&node.rows) {
                rings.push(node.outline.clone());
            }
        }
        for mut ring in rings {
            ring.push(ring[0]);
            layer.marks.push(Mark::Line {
                points: ring,
                color: palette::SELECTED,
                width: 2.2,
                pattern: LinePattern::Solid,
                name: "Holds selected rows".to_owned(),
            });
        }
        if g.labels {
            for node in &g.nodes {
                layer.marks.push(Mark::Text {
                    at: node.label_at.0,
                    text: node.label.clone(),
                    color: palette::LABEL,
                    anchor: node.label_at.1,
                    movable: None,
                });
            }
        }
        for (at, text) in &g.headings {
            layer.marks.push(Mark::Text {
                at: *at,
                text: text.clone(),
                color: palette::LABEL,
                anchor: TextAnchor::Above,
                movable: None,
            });
        }
        if layer.marks.iter().any(|m| matches!(m, Mark::Line { color, .. } if *color == palette::SELECTED)) {
            layer.key_entry("Holds selected rows", palette::SELECTED, KeyGlyph::Line(LinePattern::Solid));
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn text(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn table() -> DataTable {
        DataTable::new(
            "f",
            vec![
                Column::text("from", text(&["N", "N", "S", "S", "E", "E", "N", "W"])),
                Column::text("hub", text(&["H2", "H1", "H1", "H2", "H1", "H1", "H2", "H2"])),
                Column::text("to", text(&["S", "E", "N", "W", "N", "S", "W", "N"])),
                Column::numeric("parcels", vec![5.0, 3.0, 2.0, 4.0, 1.0, 6.0, 2.0, 3.0]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn barycentre_sweeps_remove_avoidable_crossings() {
        // Stage 0: a, b; stage 1: x, y with a→y and b→x: one crossing that
        // reordering removes.
        let flows = vec![vec![(0, 1, 2.0), (1, 0, 3.0)]];
        let (order, crossings) = barycentre_order(&[2, 2], &flows);
        assert_eq!(crossings, 0.0);
        // a and its target y now sit at the same rank, as do b and x.
        let rank = |o: &[usize], k: usize| o.iter().position(|&n| n == k).unwrap();
        assert_eq!(rank(&order[0], 0), rank(&order[1], 1));
        assert_eq!(rank(&order[0], 1), rank(&order[1], 0));
        assert_eq!(weighted_crossings(&[vec![(0, 1, 2.0), (1, 0, 3.0)]]), 6.0);
    }

    #[test]
    fn sankey_nodes_and_flows_conserve_the_rows() {
        let t = table();
        let mut state = RowStateManager::new(8);
        let plan = EncodingPlan {
            x: vec!["from".into(), "hub".into(), "to".into()],
            y: vec!["parcels".into()],
            ..Default::default()
        };
        let f = build_sankey_frame(&t, &state, &plan, &FrameOptions::default(), FlowSettings::default()).unwrap();
        let PanelExtra::Flows(g) = &f.panels[0].extra else { panic!() };
        for s in 0..3 {
            let total: f64 = g.nodes.iter().filter(|n| n.stage == s).map(|n| n.value).sum();
            assert_eq!(total, 8.0, "every stage carries every row");
        }
        let flow_total: f64 = g.links.iter().map(|l| l.value).sum();
        assert_eq!(flow_total, 16.0, "two gaps between three stages");
        // Sized by the sum of parcels.
        let sums = build_sankey_frame(
            &t,
            &state,
            &plan,
            &FrameOptions::default(),
            FlowSettings { size_by_sum: true, labels: true },
        )
        .unwrap();
        let PanelExtra::Flows(g) = &sums.panels[0].extra else { panic!() };
        let n = g.nodes.iter().find(|n| n.stage == 0 && n.label.starts_with("N ")).unwrap();
        assert_eq!(n.value, 10.0);
        assert_eq!(n.rows, [0, 1, 6]);
        // Nodes in a stage do not overlap.
        for s in 0..3 {
            let mut spans: Vec<(f64, f64)> =
                g.nodes.iter().filter(|n| n.stage == s).map(|n| (n.outline[0][1], n.outline[2][1])).collect();
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert!(spans.windows(2).all(|w| w[0].1 <= w[1].0 + 1e-12), "{spans:?}");
        }
        state.select([0], stat_data::SelectionMode::Replace);
        let layer = FlowElement.layer(&sums.panels[0], &state);
        assert!(layer.all_finite());
        assert!(layer.marks.iter().any(|m| matches!(m, Mark::Line { color, .. } if *color == palette::SELECTED)));
        // Clicking the middle of N's node selects its rows.
        let c = [0.0, 0.5 * (n.outline[0][1] + n.outline[2][1])];
        assert_eq!(crate::view::region_rows(&[layer], c).unwrap(), [0, 1, 6]);
        let one = EncodingPlan { x: vec!["from".into()], ..Default::default() };
        assert!(build_sankey_frame(&t, &state, &one, &FrameOptions::default(), FlowSettings::default()).is_err());
    }

    #[test]
    fn chord_arcs_share_the_circle_by_flow() {
        let t = table();
        let state = RowStateManager::new(8);
        let plan =
            EncodingPlan { x: vec!["from".into(), "to".into()], y: vec!["parcels".into()], ..Default::default() };
        let settings = FlowSettings { size_by_sum: true, labels: true };
        let f = build_chord_frame(&t, &state, &plan, &FrameOptions::default(), settings).unwrap();
        let PanelExtra::Flows(g) = &f.panels[0].extra else { panic!() };
        let labels: Vec<&str> = g.nodes.iter().map(|n| n.label.split(' ').next().unwrap()).collect();
        assert_eq!(labels, ["E", "N", "S", "W"], "one node per place, from or to");
        // Each node's value is its flow out plus in: N sends 5 + 3 + 2 and receives 2 + 1 + 3.
        assert_eq!(g.nodes[1].value, 16.0);
        let total: f64 = g.nodes.iter().map(|n| n.value).sum();
        assert_eq!(total, 2.0 * 26.0);
        assert_eq!(g.links.len(), 8);
        let layer = FlowElement.layer(&f.panels[0], &state);
        assert!(layer.all_finite() && layer.regions.len() == 12);
        // Every outline point lies within the circle of the arcs.
        assert!(g.links.iter().flat_map(|l| &l.outline).all(|p| p[0].hypot(p[1]) <= CHORD_RADIUS + 1e-9));
    }

    #[test]
    fn network_layout_is_seeded_and_pulls_neighbors_together() {
        // Two triangles joined by one edge.
        let edges = [(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5), (2, 3)];
        let a = fruchterman_reingold(6, &edges, 300, 7);
        assert_eq!(a, fruchterman_reingold(6, &edges, 300, 7), "deterministic");
        assert_ne!(a, fruchterman_reingold(6, &edges, 300, 8));
        let d = |i: usize, j: usize| (a[i][0] - a[j][0]).hypot(a[i][1] - a[j][1]);
        let within = (d(0, 1) + d(1, 2) + d(4, 5) + d(3, 5)) / 4.0;
        let across = (d(0, 4) + d(1, 5) + d(0, 5) + d(1, 4)) / 4.0;
        assert!(within < across, "{within} vs {across}");
        let t = table();
        let state = RowStateManager::new(8);
        let plan =
            EncodingPlan { x: vec!["from".into(), "to".into()], y: vec!["parcels".into()], ..Default::default() };
        let f = build_network_frame(&t, &state, &plan, &FrameOptions::default(), NetworkSettings::default()).unwrap();
        let PanelExtra::Flows(g) = &f.panels[0].extra else { panic!() };
        assert_eq!(g.nodes.len(), 4);
        // N–S joins rows 0 (N→S, 5) and 2 (S→N, 2).
        let ns = g.links.iter().find(|l| l.name.starts_with("N – S")).unwrap();
        assert_eq!((ns.value, ns.rows.as_slice()), (7.0, &[0, 2][..]));
        let layer = FlowElement.layer(&f.panels[0], &state);
        assert!(layer.all_finite());
        assert!(g.nodes.iter().flat_map(|n| &n.outline).all(|p| p[0] > -0.1 && p[0] < 1.1));
    }
}
