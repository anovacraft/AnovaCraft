//! Copying one plot's style onto other plots.
//!
//! "Copy plot style" keeps a [`CopiedStyle`]: the plot's [`PlotStyle`] and,
//! on a graph canvas, its marker colours, shapes and sizes and key
//! placement. "Paste plot style" puts the chosen [`StyleParts`] of it onto
//! another plot ([`paste_style`], [`paste_markers`]) and leaves the rest of
//! that plot's style as it is. Titles and annotations are never pasted:
//! they describe one plot's data.
//!
//! Report plots and canvases share [`PlotStyle`], so axes, fonts, reference
//! lines and the key cross between them; marker colours and the key's
//! placement belong to canvases and cross only between canvases.

use stat_viz::axis_settings::AxisSettings;
use stat_viz::{LegendPlacement, MarkerStyle};

use crate::plot_style::PlotStyle;

/// What "Paste plot style" carries across.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleParts {
    /// The typed axis ranges (off by default: ranges belong to one plot's
    /// data).
    pub ranges: bool,
    /// Ticks, tick labels and their format, grid lines, spines, axis sides,
    /// category label direction, equal scale and its ratio, print scale.
    pub axes: bool,
    /// The plot font, text size and figure size.
    pub text: bool,
    /// Marker and line colours, shapes and sizes (graph canvases only).
    pub markers: bool,
    /// Reference lines and bands.
    pub reference: bool,
    /// Whether the key shows, its placement and the edits to its entries.
    pub key: bool,
}

impl Default for StyleParts {
    fn default() -> Self {
        Self { ranges: false, axes: true, text: true, markers: true, reference: true, key: true }
    }
}

impl StyleParts {
    /// Every part, with its label and hover text, for the paste menu.
    pub const LABELS: [(&'static str, &'static str); 6] = [
        ("Axis ranges", "The typed minimum and maximum of each axis"),
        ("Ticks, grid and spines", "Tick spacing, marks, labels and their format, grid lines, axis sides and offsets"),
        ("Font and size", "The plot font, text size and saved figure size"),
        ("Colours and markers", "Marker and line colours, shapes and sizes (between graph canvases)"),
        ("Reference lines", "Reference lines and shaded bands"),
        ("Key", "Whether the key shows, where it sits, and renamed, hidden or reordered entries"),
    ];

    /// The parts as a list in the order of [`StyleParts::LABELS`].
    pub fn flags_mut(&mut self) -> [&mut bool; 6] {
        [&mut self.ranges, &mut self.axes, &mut self.text, &mut self.markers, &mut self.reference, &mut self.key]
    }

    /// `true` when no part is chosen.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !(self.ranges || self.axes || self.text || self.markers || self.reference || self.key)
    }
}

/// A copied plot style (see the [module documentation](self)).
#[derive(Debug, Clone, PartialEq)]
pub struct CopiedStyle {
    /// The plot it was copied from, for the paste menu.
    pub from: String,
    /// The plot's style. Titles are left in but never pasted.
    pub style: PlotStyle,
    /// Marker colours, shapes and sizes (copied from a graph canvas).
    pub markers: Option<MarkerStyle>,
    /// Key placement (copied from a graph canvas).
    pub legend: Option<LegendPlacement>,
}

/// One axis of `onto` given `from`'s settings for the chosen `parts`;
/// titles and breaks stay.
fn paste_axis(parts: StyleParts, from: &AxisSettings, onto: &mut AxisSettings) {
    if parts.axes {
        *onto = AxisSettings {
            title: onto.title.take(),
            breaks: std::mem::take(&mut onto.breaks),
            range: onto.range,
            time: onto.time,
            whole_numbers: onto.whole_numbers,
            ..from.clone()
        };
    }
    if parts.ranges {
        onto.range = from.range;
    }
}

/// Put the chosen `parts` of style `from` onto style `onto` (see the
/// [module documentation](self)). Plot and axis titles, axis breaks and
/// annotations stay as they are in `onto`.
pub fn paste_style(parts: StyleParts, from: &PlotStyle, onto: &mut PlotStyle) {
    let (f, o) = (&from.axes, &mut onto.axes);
    paste_axis(parts, &f.x, &mut o.x);
    paste_axis(parts, &f.y, &mut o.y);
    paste_axis(parts, &f.y2, &mut o.y2);
    if parts.axes {
        o.equal_scale = f.equal_scale;
        o.scale_ratio = f.scale_ratio;
        o.category_labels = f.category_labels;
        o.hover_coordinates = f.hover_coordinates;
        o.print_scale = f.print_scale;
    }
    if parts.text {
        onto.export = from.export.clone();
    }
    if parts.reference {
        onto.reference = from.reference.clone();
    }
    if parts.key {
        onto.axes.show_key = from.axes.show_key;
        onto.key = from.key.clone();
    }
}

/// Put the canvas-only parts of `from` onto a canvas's markers and key
/// placement: colours and markers when `parts.markers`, the placement when
/// `parts.key`. Nothing changes when `from` came from a report plot.
pub fn paste_markers(parts: StyleParts, from: &CopiedStyle, markers: &mut MarkerStyle, legend: &mut LegendPlacement) {
    if parts.markers
        && let Some(m) = &from.markers
    {
        *markers = m.clone();
    }
    if parts.key
        && let Some(l) = from.legend
    {
        *legend = l;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_viz::axis_settings::{GridLines, MajorTicks};
    use stat_viz::plot_style::PlotFont;

    fn styled() -> PlotStyle {
        let mut s = PlotStyle::default();
        s.axes.x.major = MajorTicks::Every(2.0);
        s.axes.y.grid = GridLines::MajorAndMinor;
        s.axes.x.range = Some([0.0, 10.0]);
        s.axes.x.title = Some("Load".into());
        s.axes.title = Some("Source".into());
        s.axes.equal_scale = true;
        s.axes.show_key = false;
        s.export.font = PlotFont::Serif;
        s.key.hidden.push("fit".into());
        s
    }

    #[test]
    fn default_parts_carry_everything_but_ranges_and_titles() {
        let from = styled();
        let mut onto = PlotStyle::default();
        onto.axes.x.title = Some("Strain".into());
        onto.axes.title = Some("Target".into());
        onto.axes.x.breaks = stat_viz::axis_breaks::AxisBreaks::single(3.0, 4.0);
        paste_style(StyleParts::default(), &from, &mut onto);
        assert_eq!(onto.axes.x.major, MajorTicks::Every(2.0));
        assert_eq!(onto.axes.y.grid, GridLines::MajorAndMinor);
        assert!(onto.axes.equal_scale && !onto.axes.show_key);
        assert_eq!(onto.export.font, PlotFont::Serif);
        assert_eq!(onto.key.hidden, vec!["fit".to_owned()]);
        // Titles, breaks and ranges stay the target's.
        assert_eq!(onto.axes.x.title.as_deref(), Some("Strain"));
        assert_eq!(onto.axes.title.as_deref(), Some("Target"));
        assert_eq!(onto.axes.x.breaks, stat_viz::axis_breaks::AxisBreaks::single(3.0, 4.0));
        assert_eq!(onto.axes.x.range, None);
    }

    #[test]
    fn each_part_goes_alone() {
        let from = styled();
        let only = |set: fn(&mut StyleParts)| {
            let mut p = StyleParts { ranges: false, axes: false, text: false, markers: false, reference: false, key: false };
            assert!(p.is_empty());
            set(&mut p);
            let mut onto = PlotStyle::default();
            paste_style(p, &from, &mut onto);
            onto
        };
        let ranges = only(|p| p.ranges = true);
        assert_eq!(ranges.axes.x.range, Some([0.0, 10.0]));
        assert_eq!(ranges.axes.x.major, MajorTicks::Auto);
        let text = only(|p| p.text = true);
        assert_eq!(text.export.font, PlotFont::Serif);
        assert!(!text.axes.equal_scale && text.axes.show_key);
        let key = only(|p| p.key = true);
        assert!(!key.axes.show_key);
        assert_eq!(key.export, PlotStyle::default().export);
        let none = only(|_| {});
        assert_eq!(none, PlotStyle::default());
    }

    #[test]
    fn markers_and_placement_cross_only_from_canvases() {
        let m = MarkerStyle { size: Some(6.0), ..MarkerStyle::default() };
        let copied = CopiedStyle {
            from: "Graph".into(),
            style: PlotStyle::default(),
            markers: Some(m.clone()),
            legend: Some(LegendPlacement::Right),
        };
        let (mut markers, mut legend) = (MarkerStyle::default(), LegendPlacement::default());
        paste_markers(StyleParts::default(), &copied, &mut markers, &mut legend);
        assert_eq!(markers, m);
        assert_eq!(legend, LegendPlacement::Right);
        // From a report plot nothing changes.
        let report = CopiedStyle { markers: None, legend: None, ..copied };
        let (mut markers, mut legend) = (MarkerStyle::default(), LegendPlacement::default());
        paste_markers(StyleParts::default(), &report, &mut markers, &mut legend);
        assert_eq!(markers, MarkerStyle::default());
        assert_eq!(legend, LegendPlacement::default());
    }
}
