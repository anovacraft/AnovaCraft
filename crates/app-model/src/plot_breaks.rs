//! Axis breaks on report plots.
//!
//! Report scatter plots ([`PlotSpec`]) take the same breaks as graph
//! canvases ([`stat_viz::axis_breaks`]): several stretches left out of an
//! axis, each piece between them on its own scale and with its own share of
//! the axis, and slashes or a zigzag where the axis is cut. A plot's axes
//! are linear in data units, so the breaks are laid out on the linear scale,
//! with shares over the extent of everything the plot draws on that axis
//! ([`data_extent`]).
//!
//! An axis takes breaks when it is continuous: not labelled by categories,
//! a probability scale or dates, and not divided into panes; contour maps
//! and ternary plots take none ([`breakable`]). The view draws
//! [`broken_plot`] (every coordinate mapped, points inside a removed
//! stretch left out) and labels its ticks from the same maps, so box and
//! click selection, the screen and exported figures agree.

use stat_viz::axis_settings::PlotAxes;
use stat_viz::{AxisScale, BrokenAxis};

use crate::report::{PlotSpec, Polyline};

/// Whether each axis of `spec` (`[x, y]`) can be broken.
#[must_use]
pub fn breakable(spec: &PlotSpec) -> [bool; 2] {
    if spec.simplex || spec.contour.is_some() {
        return [false, false];
    }
    [
        spec.x_ticks.is_none() && spec.x_time.is_none() && spec.panes.is_empty(),
        spec.y_ticks.is_none() && spec.y_time.is_none(),
    ]
}

/// The extent of everything `spec` draws on each axis (`[x, y]`): points,
/// curves, bands, line work and annotations.
#[must_use]
pub fn data_extent(spec: &PlotSpec) -> [Option<(f64, f64)>; 2] {
    let mut ext = [(f64::INFINITY, f64::NEG_INFINITY); 2];
    let mut add = |p: [f64; 2]| {
        for (e, v) in ext.iter_mut().zip(p) {
            if v.is_finite() {
                *e = (e.0.min(v), e.1.max(v));
            }
        }
    };
    spec.points.iter().for_each(|p| add([p.x, p.y]));
    let curves = spec.line.iter().chain(spec.band.iter().flat_map(|(a, b)| [a, b]));
    let curves = curves.chain(&spec.segments).chain(&spec.point_runs);
    curves.chain(spec.groups.iter().flat_map(|g| g.lines.iter())).flatten().for_each(|&p| add(p));
    if let Some(notes) = &spec.annotations {
        notes.items.iter().for_each(|it| add([it.0, it.1]));
        notes.labels.iter().for_each(|l| add([l.0, l.1]));
    }
    ext.map(|(lo, hi)| (hi > lo).then_some((lo, hi)))
}

/// The maps of `spec`'s broken axes under `axes` (`[x, y]`; `None` for an
/// axis without breaks or one that cannot take them).
#[must_use]
pub fn plot_breaks(spec: &PlotSpec, axes: &PlotAxes) -> [Option<BrokenAxis>; 2] {
    let ok = breakable(spec);
    if !(ok[0] && !axes.x.breaks.is_empty() || ok[1] && !axes.y.breaks.is_empty()) {
        return [None, None];
    }
    let extent = data_extent(spec);
    let map = |i: usize, b: &stat_viz::AxisBreaks| {
        (ok[i] && !b.is_empty()).then(|| b.map(AxisScale::Linear, extent[i])).flatten()
    };
    [map(0, &axes.x.breaks), map(1, &axes.y.breaks)]
}

/// `spec` with every coordinate drawn through the broken axes `[x, y]`:
/// points inside a removed stretch are left out (with their group
/// memberships), curve vertices inside one are dropped and annotations in
/// one are not drawn. Returns the plot and how many points were left out.
#[must_use]
pub fn broken_plot(spec: &PlotSpec, breaks: &[Option<BrokenAxis>; 2]) -> (PlotSpec, usize) {
    let [bx, by] = breaks;
    if bx.is_none() && by.is_none() {
        return (spec.clone(), 0);
    }
    let at = |p: [f64; 2]| [bx.as_ref().map_or(p[0], |b| b.map(p[0])), by.as_ref().map_or(p[1], |b| b.map(p[1]))];
    let finite = |p: [f64; 2]| p[0].is_finite() && p[1].is_finite();
    let curve = |c: &Polyline| -> Polyline { c.iter().map(|&p| at(p)).filter(|&p| finite(p)).collect() };
    let mut out = spec.clone();
    let mut kept = Vec::new();
    let mut membership = Vec::new();
    for (i, p) in spec.points.iter().enumerate() {
        let q = at([p.x, p.y]);
        if finite(q) {
            kept.push(crate::report::RowPoint { row: p.row, x: q[0], y: q[1] });
            if let Some(g) = spec.groups.as_ref().and_then(|g| g.membership.get(i)) {
                membership.push(*g);
            }
        }
    }
    let hidden = spec.points.len() - kept.len();
    out.points = kept;
    if let Some(g) = out.groups.as_mut() {
        g.membership = membership;
        g.lines = g.lines.iter().map(curve).collect();
    }
    out.line = spec.line.as_ref().map(curve);
    out.band = spec.band.as_ref().map(|(a, b)| (curve(a), curve(b)));
    out.segments = spec.segments.iter().map(curve).collect();
    out.point_runs = spec.point_runs.iter().map(curve).collect();
    if let Some(notes) = out.annotations.as_mut() {
        notes.items = notes
            .items
            .iter()
            .filter_map(|it| {
                let q = at([it.0, it.1]);
                finite(q).then(|| (q[0], q[1], it.2.clone(), it.3))
            })
            .collect();
        notes.labels = notes
            .labels
            .iter()
            .filter_map(|l| {
                let q = at([l.0, l.1]);
                finite(q).then(|| (q[0], q[1], l.2.clone()))
            })
            .collect();
    }
    (out, hidden)
}

/// The note under a plot with points left out by a break, or whose broken
/// axes mix scales.
#[must_use]
pub fn breaks_notes(axes: &PlotAxes, breaks: &[Option<BrokenAxis>; 2], hidden: usize) -> Vec<String> {
    let mut notes = Vec::new();
    if hidden > 0 {
        notes.push(format!("{hidden} point(s) inside an axis break are not drawn."));
    }
    if breaks[0].is_some() {
        notes.extend(axes.x.breaks.scales_note("horizontal", AxisScale::Linear));
    }
    if breaks[1].is_some() {
        notes.extend(axes.y.breaks.scales_note("vertical", AxisScale::Linear));
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{Annotations, PointGroups, RowPoint};

    fn spec() -> PlotSpec {
        PlotSpec {
            points: [(0, 1.0, 2.0), (1, 2.0, 50.0), (2, 3.0, 900.0), (3, 4.0, 1000.0)]
                .into_iter()
                .map(|(row, x, y)| RowPoint { row, x, y })
                .collect(),
            line: Some(vec![[1.0, 0.0], [2.0, 500.0], [4.0, 1000.0]]),
            groups: Some(PointGroups {
                title: "g".into(),
                labels: vec!["a".into(), "b".into()],
                membership: vec![0, 1, 0, 1],
                lines: Vec::new(),
            }),
            annotations: Some(Annotations {
                groups: vec!["m".into()],
                items: vec![(2.5, 40.0, "in".into(), 0), (2.5, 950.0, "up".into(), 0)],
                labels: Vec::new(),
            }),
            ..PlotSpec::default()
        }
    }

    /// A break on the vertical axis moves the upper values down, drops the
    /// point and annotation inside it (keeping group memberships in step)
    /// and leaves the horizontal axis alone.
    #[test]
    fn breaks_map_every_coordinate() {
        let mut axes = PlotAxes::default();
        axes.y.breaks = stat_viz::AxisBreaks::single(10.0, 800.0);
        let maps = plot_breaks(&spec(), &axes);
        assert!(maps[0].is_none() && maps[1].is_some());
        let (p, hidden) = broken_plot(&spec(), &maps);
        assert_eq!(hidden, 1);
        let ys: Vec<f64> = p.points.iter().map(|q| q.y).collect();
        assert_eq!(ys, vec![2.0, 110.0, 210.0]);
        assert_eq!(p.points.iter().map(|q| q.row).collect::<Vec<_>>(), vec![0, 2, 3]);
        assert_eq!(p.groups.as_ref().unwrap().membership, vec![0, 0, 1]);
        assert_eq!(p.line.as_ref().unwrap().len(), 2, "the vertex inside the break is dropped");
        let notes = p.annotations.as_ref().unwrap();
        assert_eq!(notes.items.len(), 1);
        assert!((notes.items[0].1 - 160.0).abs() < 1e-12);
        assert_eq!(breaks_notes(&axes, &maps, hidden), vec!["1 point(s) inside an axis break are not drawn."]);
        // No breaks: unchanged.
        assert_eq!(broken_plot(&spec(), &[None, None]), (spec(), 0));
    }

    /// Category, probability and date axes, panes, contour maps and ternary
    /// plots take no breaks.
    #[test]
    fn only_continuous_axes_break() {
        let mut axes = PlotAxes::default();
        axes.x.breaks = stat_viz::AxisBreaks::single(1.5, 2.5);
        axes.y.breaks = stat_viz::AxisBreaks::single(10.0, 800.0);
        let s = PlotSpec { x_ticks: Some(vec![(1.0, "A".into())]), ..spec() };
        assert_eq!(breakable(&s), [false, true]);
        assert!(plot_breaks(&s, &axes)[0].is_none());
        let s = PlotSpec { y_ticks: Some(vec![(1.0, "50%".into())]), ..spec() };
        assert_eq!(breakable(&s), [true, false]);
        let s = PlotSpec { simplex: true, ..spec() };
        assert_eq!(plot_breaks(&s, &axes), [None, None]);
        assert_eq!(breakable(&spec()), [true, true]);
    }

    /// An exported figure draws the plot through its breaks, with ticks from
    /// both pieces and marks on both spines, in every file format.
    #[test]
    fn figures_draw_the_breaks() {
        use crate::report::{ReportSection, SectionContent, SectionKind};
        use crate::report_figure::{FigureLayout, section_figure_with};
        let section = ReportSection {
            kind: SectionKind::PruningPlot,
            title: "Plot".into(),
            content: SectionContent::Plot(Box::new(spec())),
        };
        let mut axes = PlotAxes::default();
        axes.y.breaks = stat_viz::AxisBreaks::single(60.0, 800.0);
        let layout = FigureLayout::default().with_axes(axes);
        let text = |format| String::from_utf8(section_figure_with(&section, None, format, layout.clone()).unwrap());
        let svg = text(stat_viz::export::VectorFormat::Svg).unwrap();
        assert!(svg.contains(">1000</text>") && svg.contains(">50</text>"), "{svg}");
        assert_eq!(svg.matches("fill=\"#ffffff\" fill-opacity=\"1\"").count(), 2);
        for format in stat_viz::export::VectorFormat::ALL {
            assert!(section_figure_with(&section, None, format, layout.clone()).is_some_and(|b| !b.is_empty()));
        }
    }

    /// Shares are laid out over everything the plot draws.
    #[test]
    fn shares_use_the_plot_extent() {
        assert_eq!(data_extent(&spec()), [Some((1.0, 4.0)), Some((0.0, 1000.0))]);
        let mut axes = PlotAxes::default();
        axes.y.breaks = stat_viz::AxisBreaks {
            segments: vec![stat_viz::AxisSegment { share: Some(50.0), scale: None }],
            ..stat_viz::AxisBreaks::single(10.0, 800.0)
        };
        let [_, y] = plot_breaks(&spec(), &axes);
        let y = y.unwrap();
        assert!((y.map(10.0) / y.map(1000.0) - 0.5).abs() < 1e-12);
    }
}
