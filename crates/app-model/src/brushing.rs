//! Turning pointer gestures into row-state changes (linked brushing).
//!
//! Views translate a gesture into data terms (a rectangle in plot coordinates,
//! or the screen positions of drawn points) and call into this module. The
//! result is applied to the shared [`RowStateManager`], so every other view picks
//! up the new selection the next time it draws.

use crate::report::{BarItem, BoxPlotSpec, ChartPoint, RowPoint};
use stat_data::{RowStateManager, SelectionMode};

/// An axis-aligned rectangle in plot (data) coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotRegion {
    /// Left edge.
    pub x_min: f64,
    /// Right edge.
    pub x_max: f64,
    /// Bottom edge.
    pub y_min: f64,
    /// Top edge.
    pub y_max: f64,
}

impl PlotRegion {
    /// The rectangle spanned by two opposite corners, in either order.
    #[must_use]
    pub fn from_corners(a: [f64; 2], b: [f64; 2]) -> Self {
        Self { x_min: a[0].min(b[0]), x_max: a[0].max(b[0]), y_min: a[1].min(b[1]), y_max: a[1].max(b[1]) }
    }

    /// Whether `(x, y)` lies inside or on the boundary.
    #[must_use]
    pub fn contains(&self, x: f64, y: f64) -> bool {
        (self.x_min..=self.x_max).contains(&x) && (self.y_min..=self.y_max).contains(&y)
    }

    /// Corners in drawing order, for outlining the region.
    #[must_use]
    pub fn corners(&self) -> [[f64; 2]; 4] {
        [[self.x_min, self.y_min], [self.x_max, self.y_min], [self.x_max, self.y_max], [self.x_min, self.y_max]]
    }
}

/// Combine modifier keys into a selection mode:
/// alt/option subtracts, command/ctrl toggles, shift extends, none replaces.
#[must_use]
pub fn selection_mode(shift: bool, command: bool, alt: bool) -> SelectionMode {
    if alt {
        SelectionMode::Subtract
    } else if command {
        SelectionMode::Toggle
    } else if shift {
        SelectionMode::Extend
    } else {
        SelectionMode::Replace
    }
}

/// The rows and selection mode for a click on a row number in the data grid.
/// Unlike a click in a plot, Shift selects the whole run of rows from
/// `anchor` (the row clicked last) to `row`, replacing the selection, or
/// adding to it with command/ctrl as well. Without Shift the modifiers mean
/// what they mean in plots ([`selection_mode`]). Returns the rows (ascending)
/// and the mode to apply them with.
#[must_use]
pub fn row_number_click(
    anchor: Option<usize>,
    row: usize,
    shift: bool,
    command: bool,
    alt: bool,
) -> (Vec<usize>, SelectionMode) {
    match anchor {
        Some(a) if shift && !alt => {
            let mode = if command { SelectionMode::Extend } else { SelectionMode::Replace };
            ((a.min(row)..=a.max(row)).collect(), mode)
        }
        _ => (vec![row], selection_mode(false, command, alt)),
    }
}

/// Rows whose points fall inside `region`, ascending and without duplicates
/// (a plot may draw a row more than once, e.g. once per model, and a toggle
/// must flip it once). Hidden rows are not drawn, so they cannot be brushed.
#[must_use]
pub fn rows_in_region(points: &[RowPoint], region: &PlotRegion, states: &RowStateManager) -> Vec<usize> {
    let mut rows: Vec<usize> =
        points.iter().filter(|p| !states.is_hidden(p.row) && region.contains(p.x, p.y)).map(|p| p.row).collect();
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// Rows behind the control-chart points inside `region` (sample number on the
/// horizontal axis), ascending and without duplicates. Points whose rows are all
/// hidden are not drawn and cannot be brushed.
#[must_use]
pub fn chart_rows_in_region(points: &[ChartPoint], region: &PlotRegion, states: &RowStateManager) -> Vec<usize> {
    let mut rows: Vec<usize> = points
        .iter()
        .filter(|p| chart_point_visible(p, states) && region.contains(p.sample as f64, p.value))
        .flat_map(|p| p.rows.iter().copied())
        .collect();
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// Whether a control-chart point is drawn: at least one of its rows is not hidden.
#[must_use]
pub fn chart_point_visible(point: &ChartPoint, states: &RowStateManager) -> bool {
    point.rows.iter().any(|&r| !states.is_hidden(r))
}

/// Whether a control-chart point shows as selected: any of its rows is selected.
#[must_use]
pub fn chart_point_selected(point: &ChartPoint, states: &RowStateManager) -> bool {
    point.rows.iter().any(|&r| states.is_selected(r))
}

/// Rows counted by the bars a box selection touches: a bar is touched when the
/// region overlaps the rectangle `[center ± width/2] × [0, count]`. Ascending,
/// without duplicates.
#[must_use]
pub fn bar_rows_in_region(bars: &[BarItem], region: &PlotRegion) -> Vec<usize> {
    let mut rows: Vec<usize> = bars
        .iter()
        .filter(|b| {
            let (lo, hi) = (b.center - b.width / 2.0, b.center + b.width / 2.0);
            region.x_max >= lo && region.x_min <= hi && region.y_max >= 0.0 && region.y_min <= b.count() as f64
        })
        .flat_map(|b| b.rows.iter().copied())
        .collect();
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// The bar under the plot-coordinate point `(x, y)`, if any. Clicks on very
/// short bars are accepted up to `min_height` above the baseline.
#[must_use]
pub fn bar_at(bars: &[BarItem], x: f64, y: f64, min_height: f64) -> Option<&BarItem> {
    bars.iter().find(|b| {
        (b.center - b.width / 2.0..=b.center + b.width / 2.0).contains(&x)
            && (0.0..=(b.count() as f64).max(min_height)).contains(&y)
    })
}

/// Number of a bar's rows that are selected (drawn as a highlighted portion).
#[must_use]
pub fn selected_in_bar(bar: &BarItem, states: &RowStateManager) -> usize {
    bar.rows.iter().filter(|&&r| states.is_selected(r)).count()
}

/// Rows of a box plot whose values fall within the region's horizontal range
/// (the vertical position is irrelevant: values lie on one line). Hidden rows
/// are skipped.
#[must_use]
pub fn box_rows_in_region(spec: &BoxPlotSpec, region: &PlotRegion, states: &RowStateManager) -> Vec<usize> {
    spec.points
        .iter()
        .filter(|p| !states.is_hidden(p.row) && (region.x_min..=region.x_max).contains(&p.x))
        .map(|p| p.row)
        .collect()
}

/// The row whose drawn point is nearest to `cursor` in screen space, if within
/// `max_distance` pixels. `screen_points` yields `(row, [x, y])` for drawn points.
#[must_use]
pub fn nearest_row<I>(screen_points: I, cursor: [f32; 2], max_distance: f32) -> Option<usize>
where
    I: IntoIterator<Item = (usize, [f32; 2])>,
{
    let mut best: Option<(usize, f32)> = None;
    for (row, p) in screen_points {
        let d2 = (p[0] - cursor[0]).powi(2) + (p[1] - cursor[1]).powi(2);
        if d2 <= max_distance * max_distance && best.is_none_or(|(_, b)| d2 < b) {
            best = Some((row, d2));
        }
    }
    best.map(|(row, _)| row)
}

/// Apply a box selection to the shared row state. Returns `true` if it changed.
pub fn brush_region(
    points: &[RowPoint],
    region: &PlotRegion,
    mode: SelectionMode,
    states: &mut RowStateManager,
) -> bool {
    let rows = rows_in_region(points, region, states);
    states.select(rows, mode)
}

/// Apply a click: select the clicked row, or with no row hit and
/// [`SelectionMode::Replace`], clear the selection (clicking empty space).
pub fn brush_click(hit: Option<usize>, mode: SelectionMode, states: &mut RowStateManager) -> bool {
    match hit {
        Some(row) => states.select([row], mode),
        None if mode == SelectionMode::Replace => states.clear_selection(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts() -> Vec<RowPoint> {
        vec![
            RowPoint { row: 0, x: 0.0, y: 0.0 },
            RowPoint { row: 1, x: 1.0, y: 1.0 },
            RowPoint { row: 2, x: 2.0, y: 2.0 },
            RowPoint { row: 3, x: 3.0, y: 0.5 },
        ]
    }

    /// A row drawn twice in the region (e.g. once per compared model) is
    /// toggled once, not twice.
    #[test]
    fn toggling_a_row_drawn_twice_selects_it() {
        let points = vec![
            RowPoint { row: 2, x: 0.5, y: 0.5 },
            RowPoint { row: 1, x: 0.5, y: 0.5 },
            RowPoint { row: 1, x: 0.6, y: 0.6 },
        ];
        let region = PlotRegion::from_corners([0.0, 0.0], [1.0, 1.0]);
        let mut s = RowStateManager::new(3);
        assert_eq!(rows_in_region(&points, &region, &s), vec![1, 2]);
        assert!(brush_region(&points, &region, SelectionMode::Toggle, &mut s));
        assert_eq!(s.selected_rows(), vec![1, 2]);
        assert!(brush_region(&points, &region, SelectionMode::Toggle, &mut s));
        assert!(s.selected_rows().is_empty());
    }

    #[test]
    fn shift_click_on_row_numbers_selects_the_run() {
        assert_eq!(row_number_click(Some(7), 3, true, false, false), (vec![3, 4, 5, 6, 7], SelectionMode::Replace));
        assert_eq!(row_number_click(Some(1), 2, true, true, false), (vec![1, 2], SelectionMode::Extend));
        assert_eq!(row_number_click(None, 4, true, false, false), (vec![4], SelectionMode::Replace));
        assert_eq!(row_number_click(Some(1), 4, false, true, false), (vec![4], SelectionMode::Toggle));
        assert_eq!(row_number_click(Some(1), 4, true, false, true), (vec![4], SelectionMode::Subtract));
    }

    #[test]
    fn region_from_any_corners() {
        let r = PlotRegion::from_corners([2.5, 0.0], [0.5, 2.5]);
        assert_eq!((r.x_min, r.x_max, r.y_min, r.y_max), (0.5, 2.5, 0.0, 2.5));
        assert!(r.contains(0.5, 2.5));
        assert!(!r.contains(3.0, 1.0));
    }

    #[test]
    fn box_select_skips_hidden_rows() {
        let mut s = RowStateManager::new(4);
        s.set_hidden([2], true);
        let r = PlotRegion::from_corners([0.5, 0.0], [3.5, 3.0]);
        assert_eq!(rows_in_region(&pts(), &r, &s), vec![1, 3]);
        assert!(brush_region(&pts(), &r, SelectionMode::Replace, &mut s));
        assert_eq!(s.selected_rows(), vec![1, 3]);
    }

    #[test]
    fn modifiers_map_to_modes() {
        assert_eq!(selection_mode(false, false, false), SelectionMode::Replace);
        assert_eq!(selection_mode(true, false, false), SelectionMode::Extend);
        assert_eq!(selection_mode(true, true, false), SelectionMode::Toggle);
        assert_eq!(selection_mode(true, true, true), SelectionMode::Subtract);
    }

    #[test]
    fn nearest_point_within_radius() {
        let screen = vec![(7, [10.0, 10.0]), (8, [14.0, 10.0])];
        assert_eq!(nearest_row(screen.clone(), [13.0, 10.0], 5.0), Some(8));
        assert_eq!(nearest_row(screen, [40.0, 40.0], 5.0), None);
    }

    #[test]
    fn clicks() {
        let mut s = RowStateManager::new(4);
        brush_click(Some(2), SelectionMode::Replace, &mut s);
        brush_click(Some(0), SelectionMode::Extend, &mut s);
        assert_eq!(s.selected_rows(), vec![0, 2]);
        assert!(!brush_click(None, SelectionMode::Extend, &mut s), "shift-click on empty space keeps selection");
        assert!(brush_click(None, SelectionMode::Replace, &mut s));
        assert!(s.selected_rows().is_empty());
    }

    #[test]
    fn bars_and_boxes() {
        let bars = vec![
            BarItem { center: 1.0, width: 1.0, rows: vec![0, 1, 2], unlinked: 0 },
            BarItem { center: 2.0, width: 1.0, rows: vec![3], unlinked: 0 },
            BarItem { center: 3.0, width: 1.0, rows: vec![], unlinked: 0 },
        ];
        // A region over the top of the first bar only.
        let r = PlotRegion::from_corners([0.6, 2.0], [1.4, 5.0]);
        assert_eq!(bar_rows_in_region(&bars, &r), vec![0, 1, 2]);
        // Above the second bar's height: not touched.
        let r = PlotRegion::from_corners([1.6, 2.0], [2.4, 5.0]);
        assert!(bar_rows_in_region(&bars, &r).is_empty());
        assert_eq!(bar_at(&bars, 2.2, 0.5, 0.3).map(|b| b.rows.clone()), Some(vec![3]));
        assert!(bar_at(&bars, 2.2, 1.5, 0.3).is_none());
        let mut s = RowStateManager::new(4);
        s.select([1, 2], SelectionMode::Replace);
        assert_eq!(selected_in_bar(&bars[0], &s), 2);

        let spec = BoxPlotSpec {
            x_time: None,
            x_label: "v".into(),
            q1: 2.0,
            median: 3.0,
            q3: 4.0,
            whisker_low: 1.0,
            whisker_high: 5.0,
            mean: 3.0,
            points: (0..4).map(|i| RowPoint { row: i, x: f64::from(i as u32) * 2.0, y: 0.0 }).collect(),
            outliers: vec![],
        };
        s.set_hidden([3], true);
        let r = PlotRegion::from_corners([1.0, -10.0], [6.5, 10.0]);
        assert_eq!(box_rows_in_region(&spec, &r, &s), vec![1, 2], "row 3 (x = 6) is hidden");
    }
}
