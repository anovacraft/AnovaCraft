//! Shared per-row state: selection, exclusion, hiding and display color.
//!
//! Every view of a dataset (data grid, plots, reports) reads row state from one
//! [`RowStateManager`], and every interaction that changes it writes back to the
//! same manager. Views redraw from the manager each frame, so a selection made in
//! one place is immediately visible everywhere — the linked-brushing model of
//! Becker & Cleveland (1987), "Brushing Scatterplots", *Technometrics* 29(2).
//!
//! The three boolean states are independent:
//!
//! * **Selected** — highlighted in every view; the target of row commands.
//! * **Excluded** — omitted from analyses (models are refit without the row), but
//!   still shown in the data grid and drawn in plots with a distinct marker.
//! * **Hidden** — not drawn in plots, but still shown in the data grid and still
//!   used by analyses unless also excluded.
//!
//! Rows can also belong to **named subsets** (up to [`MAX_SUBSETS`]), saved
//! groups of rows to switch between. While a subset is active, the rows
//! outside it count as excluded and hidden ([`RowStateManager::is_excluded`],
//! [`RowStateManager::is_hidden`]), so every analysis and plot follows the
//! subset; the rows' own exclusion and hiding are kept underneath
//! ([`RowState`], [`RowStateManager::is_excluded_flag`]) and come back when
//! the subset is switched off.

/// An sRGB display color assigned to a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowColor {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

/// A marker shape assigned to a row, so rows coloured by a column's levels
/// are told apart without colour too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowMarker {
    /// Filled circle.
    Circle,
    /// Square.
    Square,
    /// Diamond.
    Diamond,
    /// Upward triangle.
    Up,
    /// Downward triangle.
    Down,
    /// Plus sign.
    Plus,
    /// Diagonal cross.
    Cross,
    /// Asterisk.
    Asterisk,
}

impl RowMarker {
    /// Every marker, in the order they are assigned to levels.
    pub const ALL: [RowMarker; 8] = [
        RowMarker::Circle,
        RowMarker::Square,
        RowMarker::Up,
        RowMarker::Diamond,
        RowMarker::Down,
        RowMarker::Plus,
        RowMarker::Asterisk,
        RowMarker::Cross,
    ];

    /// The name scripts and project files use (`"circle"`, `"square"` …).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Diamond => "diamond",
            Self::Up => "triangle",
            Self::Down => "down triangle",
            Self::Plus => "plus",
            Self::Cross => "cross",
            Self::Asterisk => "asterisk",
        }
    }

    /// The marker named `key` (as [`RowMarker::key`] writes it).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// How a new selection combines with the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SelectionMode {
    /// The given rows become the whole selection.
    #[default]
    Replace,
    /// Add the given rows to the selection.
    Extend,
    /// Flip the selection state of each given row.
    Toggle,
    /// Remove the given rows from the selection.
    Subtract,
}

/// Snapshot of one row's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowState {
    /// Highlighted in all views.
    pub selected: bool,
    /// Omitted from analyses.
    pub excluded: bool,
    /// Not drawn in plots.
    pub hidden: bool,
    /// Optional display color.
    pub color: Option<RowColor>,
    /// Optional marker shape.
    pub marker: Option<RowMarker>,
    /// Named subsets the row belongs to: bit `i` for subset `i`.
    pub subsets: u64,
}

/// Row totals for status displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowCounts {
    /// Number of rows.
    pub total: usize,
    /// Number of selected rows.
    pub selected: usize,
    /// Number of excluded rows.
    pub excluded: usize,
    /// Number of hidden rows.
    pub hidden: usize,
    /// Number of rows outside the active subset (0 when none is active);
    /// they are counted in `excluded` and `hidden` too.
    pub outside_subset: usize,
}

const SELECTED: u8 = 1;
const EXCLUDED: u8 = 1 << 1;
const HIDDEN: u8 = 1 << 2;

/// Most named subsets a table can have.
pub const MAX_SUBSETS: usize = 64;

/// Owner of the selection, exclusion, hidden and color state of every row.
///
/// Two revision counters let views avoid redundant work:
/// [`revision`](Self::revision) changes on any state change (redraw), and
/// [`exclusion_revision`](Self::exclusion_revision) changes only when exclusion
/// changes (analyses must be recomputed). A third,
/// [`saved_revision`](Self::saved_revision), changes on every change except
/// to the selection, and is unique across the process like
/// [`DataTable::revision`](crate::DataTable::revision): it tells whether
/// what a project file keeps of the row states changed (the selection is
/// saved too, but is too transient to count as an unsaved change).
///
/// Row indices are positions in the dataset, `0..len()`. Methods that take row
/// indices panic on an out-of-range index, since that indicates a logic error in
/// the caller rather than a recoverable condition.
#[derive(Debug, Clone, Default)]
pub struct RowStateManager {
    flags: Vec<u8>,
    colors: Vec<Option<RowColor>>,
    markers: Vec<Option<RowMarker>>,
    /// The column the colours and markers were last set from (for plot keys).
    style_column: Option<String>,
    revision: u64,
    exclusion_revision: u64,
    saved_revision: u64,
    /// Subset membership of each row (bit `i`: subset `i`).
    members: Vec<u64>,
    /// Names of the subsets, by bit.
    subset_names: Vec<String>,
    /// The subset analyses and plots follow.
    active_subset: Option<usize>,
}

impl RowStateManager {
    /// State for `n_rows` rows, none selected, excluded or hidden.
    #[must_use]
    pub fn new(n_rows: usize) -> Self {
        Self {
            flags: vec![0; n_rows],
            colors: vec![None; n_rows],
            markers: vec![None; n_rows],
            style_column: None,
            revision: 0,
            exclusion_revision: 0,
            saved_revision: crate::next_revision(),
            members: vec![0; n_rows],
            subset_names: Vec::new(),
            active_subset: None,
        }
    }

    /// Number of rows tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.flags.len()
    }

    /// `true` if no rows are tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }

    /// Incremented on every change to any row's state.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Incremented only when some row's exclusion changes.
    #[must_use]
    pub fn exclusion_revision(&self) -> u64 {
        self.exclusion_revision
    }

    /// Advanced by every change but selection; unique across the process,
    /// so an unchanged value means unchanged exclusion, hiding, colours,
    /// markers and subsets.
    #[must_use]
    pub fn saved_revision(&self) -> u64 {
        self.saved_revision
    }

    /// Put back `revision`, a saved revision these row states had with
    /// exactly the exclusion, hiding, colours, markers and subsets they have
    /// now (an undo that restored them). Passing any other revision breaks
    /// the promise that equal saved revisions mean equal states.
    pub fn restore_saved_revision(&mut self, revision: u64) {
        self.saved_revision = revision;
    }

    /// Advance the redraw and saved revisions after a change other than
    /// to the selection.
    fn changed(&mut self) {
        self.revision += 1;
        self.saved_revision = crate::next_revision();
    }

    /// Full state of `row`.
    ///
    /// # Panics
    /// If `row >= len()`.
    #[must_use]
    pub fn state(&self, row: usize) -> RowState {
        let f = self.flags[row];
        RowState {
            selected: f & SELECTED != 0,
            excluded: f & EXCLUDED != 0,
            hidden: f & HIDDEN != 0,
            color: self.colors[row],
            marker: self.markers[row],
            subsets: self.members[row],
        }
    }

    /// Whether `row` is selected. Panics if out of range.
    #[must_use]
    pub fn is_selected(&self, row: usize) -> bool {
        self.flags[row] & SELECTED != 0
    }

    /// Whether `row` is left out of analyses: excluded itself, or outside
    /// the active subset. Panics if out of range.
    #[must_use]
    pub fn is_excluded(&self, row: usize) -> bool {
        self.flags[row] & EXCLUDED != 0 || self.is_outside_subset(row)
    }

    /// Whether `row` is left out of plots: hidden itself, or outside the
    /// active subset. Panics if out of range.
    #[must_use]
    pub fn is_hidden(&self, row: usize) -> bool {
        self.flags[row] & HIDDEN != 0 || self.is_outside_subset(row)
    }

    /// Whether `row` itself is excluded, whatever the active subset (what
    /// is saved, and what Exclude/Include change). Panics if out of range.
    #[must_use]
    pub fn is_excluded_flag(&self, row: usize) -> bool {
        self.flags[row] & EXCLUDED != 0
    }

    /// Whether `row` itself is hidden, whatever the active subset. Panics if
    /// out of range.
    #[must_use]
    pub fn is_hidden_flag(&self, row: usize) -> bool {
        self.flags[row] & HIDDEN != 0
    }

    /// Whether a subset is active and `row` is not in it. Panics if out of
    /// range.
    #[must_use]
    pub fn is_outside_subset(&self, row: usize) -> bool {
        self.active_subset.is_some_and(|s| self.members[row] & (1 << s) == 0)
    }

    /// Display color of `row`, if one is assigned. Panics if out of range.
    #[must_use]
    pub fn color(&self, row: usize) -> Option<RowColor> {
        self.colors[row]
    }

    /// Marker shape of `row`, if one is assigned. Panics if out of range.
    #[must_use]
    pub fn marker(&self, row: usize) -> Option<RowMarker> {
        self.markers[row]
    }

    /// Whether any row has a colour or marker of its own.
    #[must_use]
    pub fn has_row_styles(&self) -> bool {
        self.colors.iter().any(Option::is_some) || self.markers.iter().any(Option::is_some)
    }

    /// The column the row colours and markers were set from, if they were
    /// set by column (plots name its levels in their key).
    #[must_use]
    pub fn style_column(&self) -> Option<&str> {
        self.style_column.as_deref()
    }

    /// Record (or with `None`, forget) the column the row colours and
    /// markers come from.
    pub fn set_style_column(&mut self, column: Option<String>) -> bool {
        if self.style_column == column {
            return false;
        }
        self.style_column = column;
        self.changed();
        true
    }

    /// Totals of each state.
    #[must_use]
    pub fn counts(&self) -> RowCounts {
        // One pass over the flags (the app asks several times a frame, and
        // tables can have millions of rows).
        let bit = self.active_subset.map(|s| 1_u64 << s);
        let (mut selected, mut excluded, mut hidden, mut outside) = (0, 0, 0, 0);
        for (&f, &m) in self.flags.iter().zip(&self.members) {
            let out = bit.is_some_and(|b| m & b == 0);
            selected += usize::from(f & SELECTED != 0);
            excluded += usize::from(f & EXCLUDED != 0 || out);
            hidden += usize::from(f & HIDDEN != 0 || out);
            outside += usize::from(out);
        }
        RowCounts { total: self.len(), selected, excluded, hidden, outside_subset: outside }
    }

    /// How many rows are excluded and hidden themselves, whatever the
    /// active subset (what Exclude/Include and Hide/Show changed), in one
    /// pass.
    #[must_use]
    pub fn own_flag_counts(&self) -> (usize, usize) {
        self.flags.iter().fold((0, 0), |(e, h), &f| {
            (e + usize::from(f & EXCLUDED != 0), h + usize::from(f & HIDDEN != 0))
        })
    }

    /// Indices of selected rows, ascending.
    #[must_use]
    pub fn selected_rows(&self) -> Vec<usize> {
        self.rows_with(SELECTED, true)
    }

    /// Indices of rows that analyses should use (not excluded, and in the
    /// active subset), ascending.
    #[must_use]
    pub fn included_rows(&self) -> Vec<usize> {
        (0..self.len()).filter(|&r| !self.is_excluded(r)).collect()
    }

    /// Indices of rows that plots should draw (not hidden, and in the
    /// active subset), ascending.
    #[must_use]
    pub fn visible_rows(&self) -> Vec<usize> {
        (0..self.len()).filter(|&r| !self.is_hidden(r)).collect()
    }

    fn rows_with(&self, bit: u8, set: bool) -> Vec<usize> {
        self.flags.iter().enumerate().filter(|(_, f)| (*f & bit != 0) == set).map(|(i, _)| i).collect()
    }

    /// Apply a selection gesture: combine `rows` with the current selection
    /// according to `mode`. Returns `true` if anything changed.
    ///
    /// # Panics
    /// If any index is `>= len()`.
    pub fn select<I: IntoIterator<Item = usize>>(&mut self, rows: I, mode: SelectionMode) -> bool {
        let before_rev = self.revision;
        let mut changed = false;
        if mode == SelectionMode::Replace {
            let mut target = vec![false; self.len()];
            for r in rows {
                assert!(r < self.len(), "row index {r} out of range ({} rows)", self.len());
                target[r] = true;
            }
            for (f, t) in self.flags.iter_mut().zip(target) {
                let new = if t { *f | SELECTED } else { *f & !SELECTED };
                changed |= new != *f;
                *f = new;
            }
        } else {
            for r in rows {
                assert!(r < self.len(), "row index {r} out of range ({} rows)", self.len());
                let f = &mut self.flags[r];
                let new = match mode {
                    SelectionMode::Extend => *f | SELECTED,
                    SelectionMode::Subtract => *f & !SELECTED,
                    SelectionMode::Toggle => *f ^ SELECTED,
                    SelectionMode::Replace => unreachable!("handled above"),
                };
                changed |= new != *f;
                *f = new;
            }
        }
        if changed {
            self.revision = before_rev + 1;
        }
        changed
    }

    /// Deselect every row. Returns `true` if anything changed.
    pub fn clear_selection(&mut self) -> bool {
        self.select(std::iter::empty(), SelectionMode::Replace)
    }

    /// Select every row that is currently unselected, and deselect the rest.
    pub fn invert_selection(&mut self) {
        let rows: Vec<usize> = (0..self.len()).collect();
        self.select(rows, SelectionMode::Toggle);
    }

    /// Set or clear exclusion for `rows`. Returns `true` if anything changed.
    ///
    /// # Panics
    /// If any index is `>= len()`.
    pub fn set_excluded<I: IntoIterator<Item = usize>>(&mut self, rows: I, excluded: bool) -> bool {
        let changed = self.set_bit(rows, EXCLUDED, excluded);
        if changed {
            self.exclusion_revision += 1;
        }
        changed
    }

    /// Set or clear hiding for `rows`. Returns `true` if anything changed.
    ///
    /// # Panics
    /// If any index is `>= len()`.
    pub fn set_hidden<I: IntoIterator<Item = usize>>(&mut self, rows: I, hidden: bool) -> bool {
        self.set_bit(rows, HIDDEN, hidden)
    }

    /// Assign (or with `None`, clear) the display color of `rows`.
    ///
    /// # Panics
    /// If any index is `>= len()`.
    pub fn set_color<I: IntoIterator<Item = usize>>(&mut self, rows: I, color: Option<RowColor>) -> bool {
        let mut changed = false;
        for r in rows {
            changed |= self.colors[r] != color;
            self.colors[r] = color;
        }
        if changed {
            self.changed();
        }
        changed
    }

    /// Assign (or with `None`, clear) the marker shape of `rows`.
    ///
    /// # Panics
    /// If any index is `>= len()`.
    pub fn set_marker<I: IntoIterator<Item = usize>>(&mut self, rows: I, marker: Option<RowMarker>) -> bool {
        let mut changed = false;
        for r in rows {
            changed |= self.markers[r] != marker;
            self.markers[r] = marker;
        }
        if changed {
            self.changed();
        }
        changed
    }

    /// Clear every row's colour and marker, and the column they came from.
    pub fn clear_row_styles(&mut self) -> bool {
        let n = self.len();
        let a = self.set_color(0..n, None);
        let b = self.set_marker(0..n, None);
        let c = self.set_style_column(None);
        a || b || c
    }

    /// Exclude the selected rows from analyses.
    pub fn exclude_selected(&mut self) -> bool {
        self.set_excluded(self.selected_rows(), true)
    }

    /// Hide the selected rows in plots.
    pub fn hide_selected(&mut self) -> bool {
        self.set_hidden(self.selected_rows(), true)
    }

    /// Clear exclusion on every row.
    pub fn include_all(&mut self) -> bool {
        self.set_excluded(0..self.len(), false)
    }

    /// Clear hiding on every row.
    pub fn unhide_all(&mut self) -> bool {
        self.set_hidden(0..self.len(), false)
    }

    /// Insert rows with the given states so that, in ascending order of the
    /// target indices, `rows[k].0` is the index each row ends up at (indices of
    /// the final arrangement). Used when rows are added to the dataset, and to
    /// restore removed rows. Both revisions advance, since row positions shift.
    ///
    /// # Panics
    /// If the indices are not strictly ascending or one is past the end.
    pub fn insert_rows(&mut self, rows: &[(usize, RowState)]) {
        let mut last = None;
        for (len, &(at, _)) in (self.len()..).zip(rows) {
            assert!(last.is_none_or(|l| at > l), "insert positions must be strictly ascending");
            assert!(at <= len, "insert position {at} out of range ({len} rows)");
            last = Some(at);
        }
        // One merge per vector (linear in the length), not one insert per row.
        crate::table::insert_sorted(
            &mut self.flags,
            rows.iter().map(|&(at, state)| {
                let flags = (u8::from(state.selected) * SELECTED)
                    | (u8::from(state.excluded) * EXCLUDED)
                    | (u8::from(state.hidden) * HIDDEN);
                (at, flags)
            }),
        );
        crate::table::insert_sorted(&mut self.colors, rows.iter().map(|&(at, state)| (at, state.color)));
        crate::table::insert_sorted(&mut self.markers, rows.iter().map(|&(at, state)| (at, state.marker)));
        let mask = self.subset_mask();
        crate::table::insert_sorted(&mut self.members, rows.iter().map(|&(at, state)| (at, state.subsets & mask)));
        self.changed();
        self.exclusion_revision += 1;
    }

    /// Remove `rows` (strictly ascending indices) and return their states, in the
    /// same order. Both revisions advance, since row positions shift.
    ///
    /// # Panics
    /// If the indices are not strictly ascending or one is out of range.
    pub fn remove_rows(&mut self, rows: &[usize]) -> Vec<RowState> {
        assert!(rows.windows(2).all(|w| w[0] < w[1]), "rows to remove must be strictly ascending");
        let states: Vec<RowState> = rows.iter().map(|&r| self.state(r)).collect();
        crate::table::remove_sorted(&mut self.flags, rows);
        crate::table::remove_sorted(&mut self.colors, rows);
        crate::table::remove_sorted(&mut self.markers, rows);
        crate::table::remove_sorted(&mut self.members, rows);
        self.changed();
        self.exclusion_revision += 1;
        states
    }

    /// Put the rows in a new order, as [`crate::DataTable::reorder_rows`]
    /// does: row `i` takes the state of the row that was at `order[i]`. Both
    /// revisions advance, since row positions change.
    ///
    /// # Panics
    /// If `order` is not a permutation of `0..len()`.
    pub fn reorder(&mut self, order: &[usize]) {
        assert!(crate::table::is_permutation(order, self.len()), "invalid row order");
        self.flags = order.iter().map(|&r| self.flags[r]).collect();
        self.colors = order.iter().map(|&r| self.colors[r]).collect();
        self.markers = order.iter().map(|&r| self.markers[r]).collect();
        self.members = order.iter().map(|&r| self.members[r]).collect();
        self.changed();
        self.exclusion_revision += 1;
    }

    /// State for rows with the given states (for a table built from other
    /// tables' rows).
    #[must_use]
    pub fn from_states(states: &[RowState]) -> Self {
        let mut m = Self::new(0);
        m.insert_rows(&states.iter().enumerate().map(|(i, s)| (i, *s)).collect::<Vec<_>>());
        m.revision = 0;
        m.exclusion_revision = 0;
        m.saved_revision = crate::next_revision();
        m
    }

    /// Bits of the subsets that exist.
    fn subset_mask(&self) -> u64 {
        match self.subset_names.len() {
            0 => 0,
            MAX_SUBSETS => u64::MAX,
            n => (1u64 << n) - 1,
        }
    }

    /// Names of the saved subsets, in the order they were made.
    #[must_use]
    pub fn subset_names(&self) -> &[String] {
        &self.subset_names
    }

    /// Rows of subset `index`, ascending (empty for an unknown index).
    #[must_use]
    pub fn subset_rows(&self, index: usize) -> Vec<usize> {
        if index >= self.subset_names.len() {
            return Vec::new();
        }
        (0..self.len()).filter(|&r| self.members[r] & (1 << index) != 0).collect()
    }

    /// Save `rows` as subset `name`, replacing a subset of that name.
    /// Returns its index, or `None` when there are already [`MAX_SUBSETS`]
    /// subsets. If it replaces the active subset, the analyses follow the
    /// new rows.
    ///
    /// # Panics
    /// If a row index is `>= len()`.
    pub fn save_subset<I: IntoIterator<Item = usize>>(&mut self, name: &str, rows: I) -> Option<usize> {
        let index = match self.subset_names.iter().position(|n| n == name) {
            Some(i) => i,
            None if self.subset_names.len() >= MAX_SUBSETS => return None,
            None => {
                self.subset_names.push(name.to_owned());
                self.subset_names.len() - 1
            }
        };
        let bit = 1u64 << index;
        for m in &mut self.members {
            *m &= !bit;
        }
        for r in rows {
            assert!(r < self.len(), "row index {r} out of range ({} rows)", self.len());
            self.members[r] |= bit;
        }
        self.subsets_changed(self.active_subset == Some(index));
        Some(index)
    }

    /// Rename subset `index`. Returns `false` for an unknown index or a name
    /// another subset has.
    pub fn rename_subset(&mut self, index: usize, name: &str) -> bool {
        if index >= self.subset_names.len()
            || self.subset_names.iter().enumerate().any(|(i, n)| i != index && n == name)
        {
            return false;
        }
        self.subset_names[index] = name.to_owned();
        self.changed();
        true
    }

    /// Delete subset `index` (switching it off first if active); later
    /// subsets move down one place.
    pub fn remove_subset(&mut self, index: usize) {
        if index >= self.subset_names.len() {
            return;
        }
        let was_active = self.active_subset == Some(index);
        self.subset_names.remove(index);
        let low = (1u64 << index) - 1;
        for m in &mut self.members {
            *m = (*m & low) | ((*m >> 1) & !low);
        }
        self.active_subset = match self.active_subset {
            Some(a) if a == index => None,
            Some(a) if a > index => Some(a - 1),
            other => other,
        };
        self.subsets_changed(was_active);
    }

    /// The subset analyses and plots follow, if any.
    #[must_use]
    pub fn active_subset(&self) -> Option<usize> {
        self.active_subset
    }

    /// Make subset `index` the one analyses and plots follow (`None`: every
    /// row). Returns `true` if that changed anything; an unknown index is
    /// ignored.
    pub fn set_active_subset(&mut self, index: Option<usize>) -> bool {
        if index.is_some_and(|i| i >= self.subset_names.len()) || index == self.active_subset {
            return false;
        }
        self.active_subset = index;
        self.subsets_changed(true);
        true
    }

    /// Advance the revisions after a subset change; the exclusion revision
    /// too when what analyses use changed.
    fn subsets_changed(&mut self, analyses: bool) {
        self.changed();
        if analyses {
            self.exclusion_revision += 1;
        }
    }

    fn set_bit<I: IntoIterator<Item = usize>>(&mut self, rows: I, bit: u8, on: bool) -> bool {
        let mut changed = false;
        for r in rows {
            let f = &mut self.flags[r];
            let new = if on { *f | bit } else { *f & !bit };
            changed |= new != *f;
            *f = new;
        }
        if changed {
            self.changed();
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_active_subset_leaves_the_other_rows_out_of_analyses_and_plots() {
        let mut m = RowStateManager::new(6);
        m.set_excluded([1], true);
        assert_eq!(m.save_subset("first half", [0, 1, 2]), Some(0));
        assert_eq!(m.save_subset("evens", [0, 2, 4]), Some(1));
        assert_eq!(m.subset_rows(1), [0, 2, 4]);
        assert_eq!(m.included_rows(), [0, 2, 3, 4, 5], "no subset active yet");
        let excl = m.exclusion_revision();
        assert!(m.set_active_subset(Some(0)));
        assert!(m.exclusion_revision() > excl, "analyses re-run");
        assert_eq!(m.included_rows(), [0, 2]);
        assert_eq!(m.visible_rows(), [0, 1, 2]);
        assert!(m.is_outside_subset(4) && m.is_excluded(4) && !m.is_excluded_flag(4));
        let c = m.counts();
        assert_eq!((c.excluded, c.hidden, c.outside_subset), (4, 3, 3));
        // The row's own states are kept underneath, and saved as they are.
        assert!(!m.state(4).excluded && m.state(1).excluded);
        assert!(!m.set_active_subset(Some(0)), "no change");
        assert!(!m.set_active_subset(Some(7)), "unknown subset");
        assert!(m.set_active_subset(Some(1)));
        assert_eq!(m.included_rows(), [0, 2, 4]);
        // Membership moves with the rows.
        let removed = m.remove_rows(&[0]);
        assert_eq!(m.subset_rows(1), [1, 3]);
        m.insert_rows(&[(0, removed[0])]);
        assert_eq!(m.subset_rows(1), [0, 2, 4]);
        m.reorder(&[5, 4, 3, 2, 1, 0]);
        assert_eq!(m.subset_rows(1), [1, 3, 5]);
        // Replacing the active subset's rows re-runs analyses too.
        let excl = m.exclusion_revision();
        assert_eq!(m.save_subset("evens", [0]), Some(1));
        assert!(m.exclusion_revision() > excl);
        assert_eq!(m.included_rows(), [0]);
        // Deleting a subset moves later ones down and switches it off.
        assert!(m.rename_subset(0, "top"));
        assert!(!m.rename_subset(0, "evens"), "names stay unique");
        m.remove_subset(0);
        assert_eq!(m.subset_names(), ["evens"]);
        assert_eq!(m.active_subset(), Some(0));
        assert_eq!(m.subset_rows(0), [0]);
        m.remove_subset(0);
        assert!(m.active_subset().is_none() && m.subset_names().is_empty());
        assert_eq!(m.included_rows().len(), 5);
    }

    #[test]
    fn subsets_are_limited_in_number() {
        let mut m = RowStateManager::new(2);
        for i in 0..MAX_SUBSETS {
            assert_eq!(m.save_subset(&format!("s{i}"), [i % 2]), Some(i));
        }
        assert_eq!(m.save_subset("one more", [0]), None);
        assert_eq!(m.save_subset("s63", [0, 1]), Some(63), "replacing one is allowed");
        assert_eq!(m.subset_rows(63), [0, 1]);
        m.remove_subset(0);
        assert_eq!(m.subset_rows(62), [0, 1]);
        assert_eq!(m.subset_rows(0), [1], "the old second subset moves down");
    }

    #[test]
    fn insert_and_remove_rows_keep_states_with_their_rows() {
        let mut m = RowStateManager::new(5);
        m.select([1, 4], SelectionMode::Replace);
        m.set_excluded([2], true);
        m.set_color([3], Some(RowColor { r: 1, g: 2, b: 3 }));
        let (rev, excl) = (m.revision(), m.exclusion_revision());
        let removed = m.remove_rows(&[1, 2]);
        assert!(m.revision() > rev && m.exclusion_revision() > excl);
        assert_eq!(m.len(), 3);
        assert!(removed[0].selected && removed[1].excluded);
        // Former rows 3 and 4 are now 1 and 2.
        assert_eq!(m.color(1), Some(RowColor { r: 1, g: 2, b: 3 }));
        assert_eq!(m.selected_rows(), vec![2]);
        m.insert_rows(&[(1, removed[0]), (2, removed[1])]);
        assert_eq!(m.selected_rows(), vec![1, 4]);
        assert_eq!(m.rows_with(EXCLUDED, true), vec![2]);
        assert_eq!(m.color(3), Some(RowColor { r: 1, g: 2, b: 3 }));
        m.insert_rows(&[(5, RowState::default())]);
        assert_eq!(m.len(), 6);
    }

    #[test]
    fn selection_modes() {
        let mut m = RowStateManager::new(6);
        assert!(m.select([1, 3], SelectionMode::Replace));
        assert_eq!(m.selected_rows(), vec![1, 3]);
        m.select([4], SelectionMode::Extend);
        assert_eq!(m.selected_rows(), vec![1, 3, 4]);
        m.select([3, 5], SelectionMode::Toggle);
        assert_eq!(m.selected_rows(), vec![1, 4, 5]);
        m.select([1, 2], SelectionMode::Subtract);
        assert_eq!(m.selected_rows(), vec![4, 5]);
        m.select([0], SelectionMode::Replace);
        assert_eq!(m.selected_rows(), vec![0]);
        m.invert_selection();
        assert_eq!(m.selected_rows(), vec![1, 2, 3, 4, 5]);
        assert!(m.clear_selection());
        assert!(!m.clear_selection(), "clearing an empty selection changes nothing");
    }

    #[test]
    fn counts_match_row_by_row_tests() {
        let mut m = RowStateManager::new(10);
        m.select([0, 1, 2, 7], SelectionMode::Replace);
        m.exclude_selected();
        m.select([2, 3, 8], SelectionMode::Replace);
        m.hide_selected();
        let s = m.save_subset("first half", 0..5).unwrap();
        for active in [None, Some(s)] {
            m.set_active_subset(active);
            let count = |test: &dyn Fn(usize) -> bool| (0..m.len()).filter(|&r| test(r)).count();
            let expected = RowCounts {
                total: 10,
                selected: count(&|r| m.is_selected(r)),
                excluded: count(&|r| m.is_excluded(r)),
                hidden: count(&|r| m.is_hidden(r)),
                outside_subset: count(&|r| m.is_outside_subset(r)),
            };
            assert_eq!(m.counts(), expected);
            assert_eq!(m.own_flag_counts(), (count(&|r| m.is_excluded_flag(r)), count(&|r| m.is_hidden_flag(r))));
        }
    }

    #[test]
    fn states_are_independent() {
        let mut m = RowStateManager::new(4);
        m.select([0, 1], SelectionMode::Replace);
        m.exclude_selected();
        m.select([1, 2], SelectionMode::Replace);
        m.hide_selected();
        assert_eq!(m.state(0), RowState { selected: false, excluded: true, hidden: false, color: None, marker: None, subsets: 0 });
        assert_eq!(m.state(1), RowState { selected: true, excluded: true, hidden: true, color: None, marker: None, subsets: 0 });
        assert_eq!(m.state(2), RowState { selected: true, excluded: false, hidden: true, color: None, marker: None, subsets: 0 });
        assert_eq!(m.included_rows(), vec![2, 3]);
        assert_eq!(m.visible_rows(), vec![0, 3]);
        assert_eq!(m.counts(), RowCounts { total: 4, selected: 2, excluded: 2, hidden: 2, outside_subset: 0 });
        m.include_all();
        m.unhide_all();
        assert_eq!(m.counts(), RowCounts { total: 4, selected: 2, excluded: 0, hidden: 0, outside_subset: 0 });
        assert_eq!(m.own_flag_counts(), (0, 0));
    }

    #[test]
    fn revisions_track_the_right_changes() {
        let mut m = RowStateManager::new(3);
        let (r0, e0) = (m.revision(), m.exclusion_revision());
        m.select([1], SelectionMode::Replace);
        assert!(m.revision() > r0);
        assert_eq!(m.exclusion_revision(), e0, "selection must not invalidate analyses");
        let r1 = m.revision();
        m.set_excluded([2], true);
        assert!(m.revision() > r1 && m.exclusion_revision() > e0);
        let (r2, e2) = (m.revision(), m.exclusion_revision());
        m.set_excluded([2], true);
        assert_eq!((m.revision(), m.exclusion_revision()), (r2, e2), "no-op changes keep revisions");
        m.set_hidden([0], true);
        assert_eq!(m.exclusion_revision(), e2);
    }

    /// The saved revision ignores selection, follows every other change,
    /// and is never shared by two managers that differ.
    #[test]
    fn saved_revision_ignores_selection_only() {
        let mut m = RowStateManager::new(3);
        let s0 = m.saved_revision();
        m.select([1], SelectionMode::Replace);
        m.clear_selection();
        assert_eq!(m.saved_revision(), s0, "selection is not a saved change");
        m.set_hidden([0], true);
        let s1 = m.saved_revision();
        assert_ne!(s1, s0);
        m.set_color([2], Some(RowColor { r: 1, g: 2, b: 3 }));
        assert_ne!(m.saved_revision(), s1);
        assert_ne!(RowStateManager::new(3).saved_revision(), RowStateManager::new(3).saved_revision());
        let copy = m.clone();
        assert_eq!(copy.saved_revision(), m.saved_revision(), "a clone has the same contents");
        let mut other = m.clone();
        other.set_excluded([1], true);
        m.set_hidden([1], true);
        assert_ne!(other.saved_revision(), m.saved_revision(), "two different edits of one state differ");
    }

    #[test]
    fn colors() {
        let mut m = RowStateManager::new(2);
        let red = RowColor { r: 200, g: 30, b: 30 };
        assert!(m.set_color([1], Some(red)));
        assert_eq!(m.color(1), Some(red));
        assert_eq!(m.state(0).color, None);
        assert!(!m.set_color([1], Some(red)));
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn out_of_range_selection_panics() {
        RowStateManager::new(2).select([5], SelectionMode::Extend);
    }
}
