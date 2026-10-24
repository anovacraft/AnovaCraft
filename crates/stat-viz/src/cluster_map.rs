//! The clustered heat map: a data matrix (rows of the table down, numeric
//! columns across) shaded by value, with its rows and columns reordered by
//! agglomerative hierarchical clustering and the two dendrograms drawn
//! beside it (Eisen, Spellman, Brown & Botstein, "Cluster analysis and
//! display of genome-wide expression patterns", *PNAS* 95(25), 1998;
//! clustering by `stat_multivariate::hierarchical_clustering`, Lance &
//! Williams 1967).
//!
//! * Columns: the numeric columns on the horizontal-axis shelf, in order
//!   (categorical ones are left out with a note). Rows: the shown rows that
//!   are not excluded, inside the graph's filter and complete in those
//!   columns, up to [`MAX_ROWS`].
//! * Values are drawn as they are or standardized per column,
//!   `z = (x − x̄) / s` with `s` the sample standard deviation (divisor
//!   `n − 1`; a constant column gives 0), so columns on different scales
//!   compare.
//! * Rows are clustered on their (standardized) values by Euclidean
//!   distance, and columns on their values across the rows, with single,
//!   complete, average (UPGMA) or Ward linkage. The leaves are drawn in the
//!   dendrogram's order (no crossing branches): the leaf order of
//!   `Dendrogram::leaf_order`, left (lower-numbered) branch first.
//! * Cell `(i, j)` (display order) spans `x ∈ [j, j + 1]`, `y ∈ [R − i − 1,
//!   R − i]` for `R` rows, so the first row is at the top. The row
//!   dendrogram grows to the left of the cells and the column dendrogram
//!   above them, each a quarter of the matrix's size, scaled so the root's
//!   height fills it. Row labels (the point-label column, or row numbers)
//!   are written to the right, column names below.
//! * The color ramp is the sequential palette ramp from the smallest to the
//!   largest value (shown as the graph's key). Each cell is a region:
//!   clicking it selects its row; selected rows are outlined.

use stat_core::Result;
use stat_data::{DataTable, RowStateManager};
use stat_multivariate::{Dendrogram, Linkage, hierarchical_clustering};

use crate::element::{Element, ElementKind};
use crate::encoding::{EncodingPlan, invalid, is_continuous};
use crate::frame::{
    AxisKind, AxisSpec, Frame, FrameOptions, GroupStyle, Legend, Panel, PanelExtra, PanelPoint, filter_note,
    select_rows,
};
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, MarkShape, Ramp, Region, Rgba, TextAnchor};
use crate::palette;
use crate::scale::AxisScale;
use crate::view::format_value;

/// Most rows a clustered heat map draws.
pub const MAX_ROWS: usize = 1000;

/// Most rows that get a label beside the matrix.
pub const MAX_ROW_LABELS: usize = 80;

/// Settings of a clustered heat map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClusterMapSettings {
    /// Standardize each column to mean 0 and standard deviation 1.
    pub standardize: bool,
    /// Reorder the rows by clustering (with a dendrogram).
    pub cluster_rows: bool,
    /// Reorder the columns by clustering (with a dendrogram).
    pub cluster_columns: bool,
    /// Linkage criterion.
    pub linkage: Linkage,
    /// Write each cell's value in it.
    pub values: bool,
}

impl Default for ClusterMapSettings {
    fn default() -> Self {
        Self { standardize: true, cluster_rows: true, cluster_columns: true, linkage: Linkage::Average, values: false }
    }
}

/// Every linkage, in menu order.
pub const LINKAGES: [Linkage; 4] = [Linkage::Average, Linkage::Complete, Linkage::Single, Linkage::Ward];

/// Short name of a linkage for saving.
#[must_use]
pub fn linkage_key(linkage: Linkage) -> &'static str {
    match linkage {
        Linkage::Single => "single",
        Linkage::Complete => "complete",
        Linkage::Average => "average",
        Linkage::Ward => "ward",
    }
}

/// The linkage with this [`linkage_key`].
#[must_use]
pub fn linkage_from_key(key: &str) -> Option<Linkage> {
    LINKAGES.into_iter().find(|l| linkage_key(*l) == key)
}

/// What a clustered heat map panel draws.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterMapGeometry {
    /// Column names, in display order (left to right).
    pub columns: Vec<String>,
    /// Table rows, in display order (top to bottom).
    pub rows: Vec<usize>,
    /// Label of each displayed row.
    pub labels: Vec<String>,
    /// Drawn values, `[display row][display column]`.
    pub values: Vec<Vec<f64>>,
    /// Smallest and largest drawn value.
    pub range: (f64, f64),
    /// Row dendrogram polylines in the panel's coordinates.
    pub row_tree: Vec<Vec<[f64; 2]>>,
    /// Column dendrogram polylines.
    pub column_tree: Vec<Vec<[f64; 2]>>,
    /// Values are standardized.
    pub standardized: bool,
    /// Write values in the cells.
    pub show_values: bool,
}

/// The dendrogram of `points` with its leaf order, or `None` for fewer than
/// two points.
fn cluster(points: &[Vec<f64>], linkage: Linkage) -> Result<Option<(Dendrogram, Vec<usize>)>> {
    if points.len() < 2 {
        return Ok(None);
    }
    let tree = hierarchical_clustering(points, linkage)?;
    let order = tree.leaf_order();
    Ok(Some((tree, order)))
}

/// The dendrogram's polylines mapped into the panel: leaf `k` (1-based, in
/// leaf order) at `along(k)`, height `h` at `out(h / root height)`.
fn place_tree(
    tree: &Dendrogram,
    along: impl Fn(f64) -> f64,
    out: impl Fn(f64) -> f64,
    vertical: bool,
) -> Vec<Vec<[f64; 2]>> {
    let top = tree.merges.iter().map(|m| m.height).fold(0.0_f64, f64::max);
    let top = if top > 0.0 { top } else { 1.0 };
    tree.segments()
        .into_iter()
        .map(|line| {
            line.into_iter()
                .map(|[k, h]| {
                    let (a, o) = (along(k), out(h / top));
                    if vertical { [o, a] } else { [a, o] }
                })
                .collect()
        })
        .collect()
}

/// The frame of a clustered heat map: one panel holding a
/// [`ClusterMapGeometry`]; each row's points sit at its cells' centres
/// (for box selection).
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] without a numeric column on the
/// horizontal shelf, with fewer than two complete rows, or when the plan
/// does not fit the table.
pub fn build_cluster_map_frame(
    table: &DataTable,
    state: &RowStateManager,
    plan: &EncodingPlan,
    options: &FrameOptions,
    settings: ClusterMapSettings,
) -> Result<Frame> {
    plan.validate(table)?;
    let (columns, skipped): (Vec<&String>, Vec<&String>) = plan.x.iter().partition(|c| is_continuous(table, c));
    if columns.is_empty() {
        return Err(invalid("a clustered heat map needs numeric columns on the horizontal axis"));
    }
    let data: Vec<&[f64]> = columns
        .iter()
        .map(|c| table.find(c).and_then(|i| table.column(i)).and_then(|c| c.as_numeric()).unwrap_or(&[]))
        .collect();
    let rs = select_rows(table, state, plan, options);
    let mut notes = Vec::new();
    if !skipped.is_empty() {
        notes.push(format!(
            "Categorical columns are left out of the heat map: {}.",
            skipped.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    let usable = |r: usize| !rs.filtered[r] && (r >= state.len() || !state.is_excluded(r));
    let complete = |r: usize| data.iter().all(|v| v.get(r).is_some_and(|x| x.is_finite()));
    let candidates: Vec<usize> = rs.page_rows.iter().copied().filter(|&r| usable(r)).collect();
    let mut rows: Vec<usize> = candidates.iter().copied().filter(|&r| complete(r)).collect();
    let incomplete = candidates.len() - rows.len();
    if incomplete > 0 {
        notes.push(format!("{incomplete} row(s) with a missing value are left out."));
    }
    let left_out = rs.page_rows.len() - candidates.len();
    if left_out > 0 {
        notes.push(format!("{left_out} excluded or filtered row(s) are left out."));
    }
    if rows.len() > MAX_ROWS {
        notes.push(format!("Only the first {MAX_ROWS} of {} rows are drawn.", rows.len()));
        rows.truncate(MAX_ROWS);
    }
    if rows.len() < 2 {
        return Err(invalid("a clustered heat map needs two or more complete rows"));
    }
    // Values, standardized per column when asked.
    let mut matrix: Vec<Vec<f64>> = rows.iter().map(|&r| data.iter().map(|v| v[r]).collect()).collect();
    if settings.standardize {
        let n = matrix.len() as f64;
        for j in 0..columns.len() {
            let mean = matrix.iter().map(|row| row[j]).sum::<f64>() / n;
            let var = matrix.iter().map(|row| (row[j] - mean).powi(2)).sum::<f64>() / (n - 1.0);
            let sd = var.sqrt();
            for row in &mut matrix {
                row[j] = if sd > 0.0 && sd.is_finite() { (row[j] - mean) / sd } else { 0.0 };
            }
        }
    }
    let (n_r, n_c) = (matrix.len(), columns.len());
    let (rf, cf) = (n_r as f64, n_c as f64);
    let row_cluster = if settings.cluster_rows { cluster(&matrix, settings.linkage)? } else { None };
    let transposed: Vec<Vec<f64>> = (0..n_c).map(|j| matrix.iter().map(|row| row[j]).collect()).collect();
    let column_cluster = if settings.cluster_columns { cluster(&transposed, settings.linkage)? } else { None };
    let row_order: Vec<usize> = row_cluster.as_ref().map_or_else(|| (0..n_r).collect(), |(_, o)| o.clone());
    let column_order: Vec<usize> = column_cluster.as_ref().map_or_else(|| (0..n_c).collect(), |(_, o)| o.clone());
    // Dendrograms: rows to the left (leaf k at y = R + 0.5 − k), columns above.
    let (tree_w, tree_h) = (0.25 * cf.max(2.0), 0.25 * rf.max(4.0));
    let row_tree = row_cluster
        .as_ref()
        .map(|(t, _)| place_tree(t, |k| rf + 0.5 - k, |h| -0.04 * cf - h * tree_w, true))
        .unwrap_or_default();
    let column_tree = column_cluster
        .as_ref()
        .map(|(t, _)| place_tree(t, |k| k - 0.5, |h| rf + 0.02 * rf + h * tree_h, false))
        .unwrap_or_default();
    let label_col = plan.label.as_deref().and_then(|c| table.find(c)).and_then(|i| table.column(i));
    let display_rows: Vec<usize> = row_order.iter().map(|&i| rows[i]).collect();
    let labels: Vec<String> =
        display_rows.iter().map(|&r| label_col.map_or_else(|| format!("Row {}", r + 1), |c| c.level_text(r))).collect();
    let values: Vec<Vec<f64>> =
        row_order.iter().map(|&i| column_order.iter().map(|&j| matrix[i][j]).collect()).collect();
    let range = values.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let mut points = Vec::with_capacity(n_r * n_c);
    for (i, &r) in display_rows.iter().enumerate() {
        for j in 0..n_c {
            points.push(PanelPoint {
                row: r,
                x: j as f64 + 0.5,
                y: rf - i as f64 - 0.5,
                group: 0,
                color: palette::DEFAULT_POINT,
                shape: MarkShape::Square,
                radius: 0.0,
                excluded: false,
                filtered: false,
                freq: rs.freq[r],
                weight: rs.weight[r],
                color_value: f64::NAN,
            });
        }
    }
    notes.extend(filter_note(state, options, &rs.passes));
    if n_r > MAX_ROW_LABELS {
        notes.push(format!("Row labels are left out above {MAX_ROW_LABELS} rows; hover the cells to read them."));
    }
    let linkage = settings.linkage.label();
    let what = if settings.standardize { "standardized values" } else { "values" };
    notes.push(format!("Rows and columns clustered by {linkage} linkage on Euclidean distances of the {what}."));
    let geometry = ClusterMapGeometry {
        columns: column_order.iter().map(|&j| columns[j].clone()).collect(),
        rows: display_rows,
        labels,
        values,
        range,
        row_tree,
        column_tree,
        standardized: settings.standardize,
        show_values: settings.values,
    };
    let plain = AxisSpec { name: String::new(), kind: AxisKind::Continuous, scale: AxisScale::Linear, time: None };
    let panel = Panel {
        grid_row: 0,
        grid_col: 0,
        title: None,
        x: plain.clone(),
        y: plain,
        points,
        groups: vec![GroupStyle { label: String::new(), color: palette::DEFAULT_POINT, shape: MarkShape::Square }],
        slot_half_width: 0.0,
        extra: PanelExtra::ClusterMap(geometry),
        labels: std::collections::BTreeMap::new(),
        data_extent: [None; 2],
    };
    Ok(Frame {
        panels: vec![panel],
        n_rows: 1,
        n_cols: 1,
        column_headers: vec![String::new()],
        row_headers: vec![String::new()],
        legend: Legend::default(),
        notes,
        pages: rs.pages,
        page: rs.page,
    })
}

/// Draws a clustered heat map panel: cells, dendrograms, labels and the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClusterMapElement;

impl Element for ClusterMapElement {
    fn kind(&self) -> ElementKind {
        ElementKind::ClusterMap
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let PanelExtra::ClusterMap(g) = &panel.extra else { return layer };
        let (n_r, n_c) = (g.rows.len(), g.columns.len());
        let (rf, cf) = (n_r as f64, n_c as f64);
        let (lo, hi) = g.range;
        let t = |v: f64| if hi > lo { (v - lo) / (hi - lo) } else { 0.5 };
        for (i, &r) in g.rows.iter().enumerate() {
            let (y0, y1) = (rf - i as f64 - 1.0, rf - i as f64);
            for j in 0..n_c {
                let (x0, x1) = (j as f64, j as f64 + 1.0);
                let v = g.values[i][j];
                let outline = vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
                layer.marks.push(Mark::Polygon {
                    points: outline.clone(),
                    fill: palette::ramp(t(v)),
                    stroke: palette::ramp(t(v)),
                    name: format!("{} · {}: {}", g.labels[i], g.columns[j], format_value(v)),
                });
                if g.show_values {
                    let ink = if t(v) < 0.55 { Rgba::rgb(255, 255, 255) } else { palette::BAR_TEXT };
                    layer.marks.push(Mark::Text {
                        at: [x0 + 0.5, y0 + 0.5],
                        text: format_value(v),
                        color: ink,
                        anchor: TextAnchor::Centre,
                        movable: None,
                    });
                }
                layer.regions.push(Region { outline, rows: vec![r] });
            }
            if r < state.len() && state.is_selected(r) {
                layer.marks.push(Mark::Line {
                    points: vec![[0.0, y0], [cf, y0], [cf, y1], [0.0, y1], [0.0, y0]],
                    color: palette::SELECTED,
                    width: 2.0,
                    pattern: LinePattern::Solid,
                    name: "Selected rows".to_owned(),
                });
            }
        }
        for line in g.row_tree.iter().chain(&g.column_tree) {
            layer.marks.push(Mark::Line {
                points: line.clone(),
                color: palette::LINE,
                width: 1.0,
                pattern: LinePattern::Solid,
                name: "Dendrogram".to_owned(),
            });
        }
        if n_r <= MAX_ROW_LABELS {
            for (i, label) in g.labels.iter().enumerate() {
                layer.marks.push(Mark::Text {
                    at: [cf + 0.04 * cf.max(1.0), rf - i as f64 - 0.5],
                    text: label.clone(),
                    color: palette::LABEL,
                    anchor: TextAnchor::LeftOfText,
                    movable: None,
                });
            }
        }
        for (j, name) in g.columns.iter().enumerate() {
            layer.marks.push(Mark::Text {
                at: [j as f64 + 0.5, -0.04 * rf.max(4.0)],
                text: name.clone(),
                color: palette::LABEL,
                anchor: TextAnchor::Centre,
                movable: None,
            });
        }
        // Room for the row labels at the right and the column names below.
        let right = if n_r <= MAX_ROW_LABELS { cf * 1.45 + 0.5 } else { cf * 1.02 };
        layer.marks.push(Mark::Text {
            at: [right, -0.08 * rf.max(4.0)],
            text: String::new(),
            color: palette::LABEL,
            anchor: TextAnchor::Centre,
            movable: None,
        });
        let title = if g.standardized { "Standardized value" } else { "Value" };
        if lo.is_finite() {
            layer.ramp = Some(Ramp { title: title.to_owned(), min: lo, max: hi, reversed: false });
        }
        if g.rows.iter().any(|&r| r < state.len() && state.is_selected(r)) {
            layer.key_entry("Selected rows", palette::SELECTED, KeyGlyph::Line(LinePattern::Solid));
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        // Two clear row clusters (1–3 low, 4–6 high) and two column pairs.
        DataTable::new(
            "m",
            vec![
                Column::numeric("a", vec![1.0, 1.2, 0.9, 5.0, 5.2, 4.9]),
                Column::numeric("b", vec![1.1, 1.0, 1.2, 5.1, 4.8, 5.0]),
                Column::numeric("c", vec![9.0, 8.0, 9.5, 2.0, 1.5, 2.2]),
                Column::text("name", ["p", "q", "r", "s", "t", "u"].map(String::from).to_vec()),
            ],
        )
        .unwrap()
    }

    #[test]
    fn rows_and_columns_follow_the_dendrograms() {
        let t = table();
        let mut state = RowStateManager::new(6);
        let plan = EncodingPlan {
            x: vec!["a".into(), "c".into(), "b".into(), "name".into()],
            label: Some("name".into()),
            ..Default::default()
        };
        let f = build_cluster_map_frame(&t, &state, &plan, &FrameOptions::default(), ClusterMapSettings::default())
            .unwrap();
        let PanelExtra::ClusterMap(g) = &f.panels[0].extra else { panic!() };
        assert!(f.notes.iter().any(|n| n.contains("name")), "categorical column noted");
        // a and b cluster together, so they are neighbors across.
        let pos = |c: &str| g.columns.iter().position(|x| x == c).unwrap();
        assert_eq!(pos("a").abs_diff(pos("b")), 1);
        // The low rows are contiguous, as are the high rows.
        let low: Vec<usize> = g.rows.iter().enumerate().filter(|(_, r)| **r < 3).map(|(i, _)| i).collect();
        assert!(low == [0, 1, 2] || low == [3, 4, 5], "{:?}", g.rows);
        assert_eq!(g.labels.len(), 6);
        assert_eq!(g.row_tree.len(), 5);
        assert_eq!(g.column_tree.len(), 2);
        // Standardized columns: each has mean 0.
        for j in 0..3 {
            let m: f64 = g.values.iter().map(|r| r[j]).sum::<f64>() / 6.0;
            assert!(m.abs() < 1e-12);
        }
        // R 4.3: hclust(dist(scale(m)), "average")$height, and the same for
        // the transposed matrix.
        let r_rows = [
            0.083_773_496_174_421_34,
            0.145_935_729_353_377_8,
            0.229_802_685_983_337_9,
            0.352_569_565_306_108_1,
            3.153_204_376_995_923_5,
        ];
        let heights = |pts: Vec<Vec<f64>>| -> Vec<f64> {
            hierarchical_clustering(&pts, Linkage::Average).unwrap().merges.iter().map(|m| m.height).collect()
        };
        for (h, r) in heights(g.values.clone()).iter().zip(r_rows) {
            assert!((h - r).abs() < 1e-12, "{h} vs {r}");
        }
        let columns: Vec<Vec<f64>> = (0..3).map(|j| g.values.iter().map(|r| r[j]).collect()).collect();
        for (h, r) in heights(columns).iter().zip([0.250_652_650_694_168_1, 4.460_582_322_550_165]) {
            assert!((h - r).abs() < 1e-12, "{h} vs {r}");
        }
        state.select([g.rows[0]], stat_data::SelectionMode::Replace);
        let layer = ClusterMapElement.layer(&f.panels[0], &state);
        assert!(layer.all_finite());
        assert_eq!(layer.regions.len(), 18);
        assert!(layer.ramp.is_some());
        assert!(layer.marks.iter().any(|m| matches!(m, Mark::Line { color, .. } if *color == palette::SELECTED)));
        // The cell at the top-left is a region of the first displayed row.
        let hit = crate::view::region_rows(&[layer], [0.5, 5.5]).unwrap();
        assert_eq!(hit, [g.rows[0]]);
        // Without clustering the table order is kept.
        let plain = ClusterMapSettings { cluster_rows: false, cluster_columns: false, ..Default::default() };
        let f = build_cluster_map_frame(&t, &state, &plan, &FrameOptions::default(), plain).unwrap();
        let PanelExtra::ClusterMap(g) = &f.panels[0].extra else { panic!() };
        assert_eq!(g.rows, [0, 1, 2, 3, 4, 5]);
        assert_eq!(g.columns, ["a", "c", "b"]);
        assert!(g.row_tree.is_empty());
        let none = EncodingPlan { x: vec!["name".into()], ..Default::default() };
        assert!(build_cluster_map_frame(&t, &state, &none, &FrameOptions::default(), plain).is_err());
        for l in LINKAGES {
            assert_eq!(linkage_from_key(linkage_key(l)), Some(l));
        }
    }
}
