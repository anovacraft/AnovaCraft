//! Everything a user sets on one plot, kept with it in projects, dashboards,
//! scripts and graph files.
//!
//! A [`PlotStyle`] holds the plot's settings ([`PlotAxes`]: ticks, grid,
//! number format, titles, key, equal scale, right axis) and one part per
//! group of formatting, each with its own member in the saved form so that
//! the parts can grow independently:
//!
//! * [`ExportLook`] — size, font and background of saved and copied figures;
//! * [`ReferenceLook`] — reference lines and shaded bands added by the user;
//! * [`AnnotationLook`] — text, arrows and shapes placed by the user;
//! * [`KeyLook`] — changes to the generated key (names, order, hidden entries).
//!
//! Members of [`PlotAxes`] that the view sets on every frame (date-time and
//! whole-number axes, whether breaks are offered, the right axis's default
//! title) are not part of the style: [`PlotStyle::of`] clears them and
//! [`PlotStyle::apply_to`] leaves them as the view set them.

use crate::axis_settings::{AxisSettings, PlotAxes};
use crate::figure_page::LengthUnit;
pub use crate::element::reference::ReferenceLook;

/// The typeface of a plot's text in figures (and of its titles on screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PlotFont {
    /// A neo-grotesque sans serif: Helvetica in PDF and EPS, Liberation Sans
    /// (metric-compatible with it) in PNG, "Helvetica, Arial, Liberation
    /// Sans" in SVG.
    #[default]
    Sans,
    /// A transitional serif: Times in PDF and EPS, Liberation Serif in PNG,
    /// "Times New Roman, Times, Liberation Serif" in SVG.
    Serif,
}

impl PlotFont {
    /// Both fonts.
    pub const ALL: [PlotFont; 2] = [PlotFont::Sans, PlotFont::Serif];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sans => "Sans serif",
            Self::Serif => "Serif",
        }
    }

    /// Name in saved files.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Sans => "sans",
            Self::Serif => "serif",
        }
    }

    /// The font named `key` ([`PlotFont::key`]).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == key)
    }
}

/// A figure's size on paper, in `unit`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FigureSize {
    /// Width in `unit`.
    pub width: f64,
    /// Height in `unit`.
    pub height: f64,
    /// Millimetres or inches.
    pub unit: LengthUnit,
}

impl FigureSize {
    /// Smallest side, in points (1 inch).
    pub const MIN_POINTS: f64 = 72.0;
    /// Largest side, in points (about 1.2 m, as figure pages).
    pub const MAX_POINTS: f64 = 3400.0;

    /// `(width, height)` in points (1/72 inch), each kept within
    /// [`FigureSize::MIN_POINTS`]–[`FigureSize::MAX_POINTS`].
    #[must_use]
    pub fn points(&self) -> (f64, f64) {
        let side = |v: f64| {
            let p = self.unit.to_points(v);
            if p.is_finite() { p.clamp(Self::MIN_POINTS, Self::MAX_POINTS) } else { Self::MIN_POINTS }
        };
        (side(self.width), side(self.height))
    }
}

/// Size, font and background of saved and copied figures.
///
/// A figure with a [`size`](Self::size) is drawn at that size on paper with
/// its text at its usual point size (a larger figure gives the data more
/// room, not larger text); without one it keeps the plot's on-screen
/// proportions. [`text_size`](Self::text_size) then enlarges (or shrinks)
/// the text, marks and lines together: the figure is laid out on a page
/// `1 / text_size` as large and scaled up to the size asked for, as a
/// figure drawn small and enlarged would be. On screen the font and text
/// size apply to the plot's titles and tick labels.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportLook {
    /// Typeface of the figure's text.
    pub font: PlotFont,
    /// Scale of text, marks and lines (1 = as drawn; 0.5–3).
    pub text_size: f64,
    /// Leave the background clear instead of white (PNG, SVG and PDF; EPS
    /// pages are left unpainted but translucent marks are still blended
    /// with white).
    pub transparent: bool,
    /// Size on paper (`None`: the plot's on-screen proportions).
    pub size: Option<FigureSize>,
    /// Whether the width and height keep their ratio when one is changed
    /// (a setting of the Save plot dialog).
    pub lock_ratio: bool,
    /// Pixels per inch of PNG figures (36–1200).
    pub ppi: f64,
}

impl Default for ExportLook {
    fn default() -> Self {
        Self {
            font: PlotFont::Sans,
            text_size: 1.0,
            transparent: false,
            size: None,
            lock_ratio: true,
            ppi: crate::raster::PNG_DPI,
        }
    }
}

impl ExportLook {
    /// Range of [`ExportLook::text_size`].
    pub const TEXT_SIZES: std::ops::RangeInclusive<f64> = 0.5..=3.0;
    /// Range of [`ExportLook::ppi`].
    pub const PPI: std::ops::RangeInclusive<f64> = 36.0..=1200.0;

    /// `true` when nothing differs from the defaults.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// `true` when figures are drawn exactly as without a look: no size,
    /// font, text size, background or resolution of their own.
    #[must_use]
    pub fn draws_as_plain(&self) -> bool {
        let d = Self::default();
        self.font == d.font
            && self.text_size == d.text_size
            && !self.transparent
            && self.size.is_none()
            && self.ppi == d.ppi
    }

    /// [`ExportLook::text_size`] within [`ExportLook::TEXT_SIZES`].
    #[must_use]
    pub fn text_scale(&self) -> f64 {
        if self.text_size.is_finite() { self.text_size.clamp(0.5, 3.0) } else { 1.0 }
    }

    /// [`ExportLook::ppi`] within [`ExportLook::PPI`].
    #[must_use]
    pub fn pixels_per_inch(&self) -> f64 {
        if self.ppi.is_finite() { self.ppi.clamp(36.0, 1200.0) } else { crate::raster::PNG_DPI }
    }
}

/// Text, arrows, lines and rectangles placed on the plot by the user, in
/// drawing order (later ones on top).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotationLook {
    /// The annotations.
    pub items: Vec<crate::element::Annotation>,
}

impl AnnotationLook {
    /// `true` when nothing differs from the defaults.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Changes to the generated key: renamed, hidden or reordered entries.
///
/// Entries are named by an identifier that stays the same while the data
/// change (what the entry stands for: a layer and its label, or a column
/// and one of its levels, see [`crate::element::GraphSpec::key_items`]), so
/// a level that appears later still gets its own entry and edits stay with
/// the entries they were made to.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KeyLook {
    /// `(identifier, text)`: entries shown with the user's text.
    pub renamed: Vec<(String, String)>,
    /// Identifiers of entries left out of the key.
    pub hidden: Vec<String>,
    /// Identifiers in the order the user put them; entries not listed keep
    /// their generated places (see [`reorder_slots`]).
    pub order: Vec<String>,
}

impl KeyLook {
    /// `true` when nothing differs from the defaults.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The user's text for entry `id`, if renamed.
    #[must_use]
    pub fn renamed(&self, id: &str) -> Option<&str> {
        self.renamed.iter().find(|(i, _)| i == id).map(|(_, t)| t.as_str())
    }

    /// `true` when entry `id` is left out.
    #[must_use]
    pub fn is_hidden(&self, id: &str) -> bool {
        self.hidden.iter().any(|h| h == id)
    }

    /// Show entry `id` with `text` (its generated text `generated` clears
    /// the change).
    pub fn rename(&mut self, id: &str, text: &str, generated: &str) {
        self.renamed.retain(|(i, _)| i != id);
        if text != generated {
            self.renamed.push((id.to_owned(), text.to_owned()));
        }
    }

    /// Show or hide entry `id`.
    pub fn set_hidden(&mut self, id: &str, hidden: bool) {
        self.hidden.retain(|h| h != id);
        if hidden {
            self.hidden.push(id.to_owned());
        }
    }

    /// Move entry `id` one place earlier (`earlier`) or later among `ids`
    /// (the key's entries in their current order), recording the new order.
    pub fn move_entry(&mut self, ids: &[String], id: &str, earlier: bool) {
        let mut ids = ids.to_vec();
        let Some(i) = ids.iter().position(|x| x == id) else { return };
        let j = if earlier { i.checked_sub(1) } else { Some(i + 1).filter(|j| *j < ids.len()) };
        if let Some(j) = j {
            ids.swap(i, j);
            self.order = ids;
        }
    }

    /// `entries` (identifier and generated text, plus anything else) as the
    /// key shows them: hidden ones left out, renamed ones with the user's
    /// text, in the user's order.
    #[must_use]
    pub fn apply<T>(&self, entries: Vec<(String, String, T)>) -> Vec<(String, String, T)> {
        let entries = reorder_slots(entries, |e| self.order.iter().position(|o| *o == e.0));
        entries
            .into_iter()
            .filter(|e| !self.is_hidden(&e.0))
            .map(|(id, text, x)| {
                let text = self.renamed(&id).map_or(text, str::to_owned);
                (id, text, x)
            })
            .collect()
    }
}

/// `items` with those `rank` places (`Some(position)` in a user's order)
/// put in that order, in the places such items take among `items`; the
/// other items keep their places. So an order recorded once still applies
/// when items are added or removed.
#[must_use]
pub fn reorder_slots<T>(items: Vec<T>, rank: impl Fn(&T) -> Option<usize>) -> Vec<T> {
    let slots: Vec<usize> = items.iter().enumerate().filter(|(_, it)| rank(it).is_some()).map(|(i, _)| i).collect();
    if slots.len() < 2 {
        return items;
    }
    let mut cells: Vec<Option<T>> = items.into_iter().map(Some).collect();
    let mut listed: Vec<(usize, T)> =
        slots.iter().map(|&i| cells[i].take().expect("a listed item")).map(|it| (rank(&it).unwrap_or(0), it)).collect();
    listed.sort_by_key(|(r, _)| *r);
    for (slot, (_, it)) in slots.into_iter().zip(listed) {
        cells[slot] = Some(it);
    }
    cells.into_iter().map(|c| c.expect("every slot filled")).collect()
}

/// Everything a user sets on one plot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlotStyle {
    /// Axis, tick, grid, title and key settings, without the members the
    /// view sets (see the module documentation).
    pub axes: PlotAxes,
    /// Saved and copied figures.
    pub export: ExportLook,
    /// Reference lines and bands.
    pub reference: ReferenceLook,
    /// Annotations.
    pub annotations: AnnotationLook,
    /// Key changes.
    pub key: KeyLook,
}

/// `settings` without the members the view sets on every frame.
fn user_axis(settings: &AxisSettings) -> AxisSettings {
    AxisSettings { time: None, whole_numbers: false, ..settings.clone() }
}

/// `axes` without the members the view sets on every frame.
#[must_use]
pub fn user_axes(axes: &PlotAxes) -> PlotAxes {
    PlotAxes {
        x: user_axis(&axes.x),
        y: user_axis(&axes.y),
        y2: user_axis(&axes.y2),
        breaks: [false; 2],
        y2_label: None,
        ..axes.clone()
    }
}

impl PlotStyle {
    /// The style of a plot drawn with `axes` (the other parts at their
    /// defaults).
    #[must_use]
    pub fn of(axes: &PlotAxes) -> Self {
        Self { axes: user_axes(axes), ..Self::default() }
    }

    /// Put this style's settings into `axes`, keeping the members the view
    /// sets.
    pub fn apply_to(&self, axes: &mut PlotAxes) {
        let keep_axis = |user: &AxisSettings, view: &AxisSettings| AxisSettings {
            time: view.time,
            whole_numbers: view.whole_numbers,
            ..user.clone()
        };
        *axes = PlotAxes {
            x: keep_axis(&self.axes.x, &axes.x),
            y: keep_axis(&self.axes.y, &axes.y),
            y2: keep_axis(&self.axes.y2, &axes.y2),
            breaks: axes.breaks,
            y2_label: axes.y2_label.clone(),
            ..self.axes.clone()
        };
    }

    /// `true` when `axes` carry the same user settings as this style's.
    #[must_use]
    pub fn same_axes(&self, axes: &PlotAxes) -> bool {
        self.axes == user_axes(axes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axis_settings::{GridLines, MajorTicks};

    #[test]
    fn styles_leave_out_what_the_view_sets() {
        let mut axes = PlotAxes::default();
        axes.x.whole_numbers = true;
        axes.breaks = [true; 2];
        axes.y2_label = Some("Rate".into());
        axes.y.major = MajorTicks::Every(5.0);
        let style = PlotStyle::of(&axes);
        assert!(!style.axes.x.whole_numbers && style.axes.breaks == [false; 2] && style.axes.y2_label.is_none());
        assert_eq!(style.axes.y.major, MajorTicks::Every(5.0));
        assert!(style.same_axes(&axes));

        // Applying keeps the view's members and takes the user's.
        let mut shown = PlotAxes::default();
        shown.x.whole_numbers = true;
        shown.breaks = [true; 2];
        let mut saved = style.clone();
        saved.axes.y.grid = GridLines::None;
        saved.apply_to(&mut shown);
        assert!(shown.x.whole_numbers && shown.breaks == [true; 2]);
        assert_eq!(shown.y.grid, GridLines::None);
        assert_eq!(shown.y.major, MajorTicks::Every(5.0));
    }

    #[test]
    fn orders_fill_the_places_of_the_items_they_name() {
        let items = vec!["a", "b", "c", "d", "e"];
        let by = |order: &'static [&'static str]| move |s: &&str| order.iter().position(|o| o == s);
        // Only b and d are listed: they swap places, the rest stay.
        assert_eq!(reorder_slots(items.clone(), by(&["d", "b"])), ["a", "d", "c", "b", "e"]);
        // Names of items that are gone are skipped; new items keep their place.
        assert_eq!(reorder_slots(items.clone(), by(&["x", "e", "a"])), ["e", "b", "c", "d", "a"]);
        assert_eq!(reorder_slots(items.clone(), by(&[])), items);
    }

    #[test]
    fn key_changes_rename_hide_and_reorder() {
        let entries = || {
            vec![
                ("color/A".to_owned(), "Group A".to_owned(), 1),
                ("color/B".to_owned(), "Group B".to_owned(), 2),
                ("fit/Fit".to_owned(), "Fit".to_owned(), 3),
            ]
        };
        let mut key = KeyLook::default();
        assert_eq!(key.apply(entries()), entries());
        key.rename("fit/Fit", "Least squares", "Fit");
        key.set_hidden("color/A", true);
        let ids: Vec<String> = entries().into_iter().map(|e| e.0).collect();
        key.move_entry(&ids, "fit/Fit", true);
        let shown = key.apply(entries());
        assert_eq!(shown.iter().map(|e| e.1.as_str()).collect::<Vec<_>>(), ["Least squares", "Group B"]);
        // A level added later still shows; renaming back to the generated text clears the change.
        let mut more = entries();
        more.insert(2, ("color/C".to_owned(), "Group C".to_owned(), 4));
        assert_eq!(key.apply(more).len(), 3);
        key.rename("fit/Fit", "Fit", "Fit");
        assert!(key.renamed.is_empty());
        key.set_hidden("color/A", false);
        key.order.clear();
        assert!(key.is_default());
    }
}
