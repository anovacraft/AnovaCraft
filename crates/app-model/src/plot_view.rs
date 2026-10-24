//! Plot view limits and the axis-drag gestures that change them.
//!
//! Each axis behaves like a ruler the user can grab:
//!
//! * **Near either end** (the outer [`END_HANDLE_FRACTION`] of its length) the
//!   axis is a *handle* for that limit: dragging moves only that limit while the
//!   opposite limit stays fixed, and the value that was under the pointer when
//!   the drag began stays under the pointer — so the axis stretches or
//!   compresses to follow it.
//! * **Anywhere else** dragging *shifts* the whole axis: both limits move
//!   together and the visible span is unchanged, again keeping the grabbed value
//!   under the pointer.
//!
//! Positions are measured along the axis in screen units, from the end with the
//! smaller value: left-to-right for a horizontal axis and bottom-to-top for a
//! vertical one (callers convert from screen coordinates, whose y grows
//! downward).

/// Fraction of an axis's length, at each end, that acts as a limit handle.
pub const END_HANDLE_FRACTION: f64 = 0.18;
/// Closest the pointer may come to the fixed end while dragging a handle,
/// in screen units; prevents division by (near) zero.
pub const MIN_HANDLE_DISTANCE: f64 = 6.0;

/// Visible data ranges of a plot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewBounds {
    /// Left edge.
    pub x_min: f64,
    /// Right edge.
    pub x_max: f64,
    /// Bottom edge.
    pub y_min: f64,
    /// Top edge.
    pub y_max: f64,
}

/// Which axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Horizontal.
    X,
    /// Vertical.
    Y,
}

/// What dragging at a position on an axis does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisHandle {
    /// Move the smaller-value limit (left or bottom); the other limit is fixed.
    Lower,
    /// Move the larger-value limit (right or top); the other limit is fixed.
    Upper,
    /// Move both limits together.
    Shift,
}

/// The handle under a position `along` an axis of screen length `length`
/// (measured from the smaller-value end).
#[must_use]
pub fn handle_at(along: f64, length: f64) -> AxisHandle {
    if length <= 0.0 {
        return AxisHandle::Shift;
    }
    // Compare distances from each end, so both ends are treated symmetrically
    // (computing `1 − fraction` would round 0.82 up and miss the boundary).
    let reach = END_HANDLE_FRACTION * length;
    if along <= reach {
        AxisHandle::Lower
    } else if length - along <= reach {
        AxisHandle::Upper
    } else {
        AxisHandle::Shift
    }
}

/// An axis drag in progress: the state captured when it began.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisDrag {
    /// The axis being dragged.
    pub axis: Axis,
    /// What the drag does.
    pub handle: AxisHandle,
    /// Bounds when the drag began.
    pub start: ViewBounds,
    /// Pointer position along the axis when the drag began.
    pub start_along: f64,
    /// Screen length of the axis.
    pub length: f64,
}

impl AxisDrag {
    /// Bounds after moving the pointer to `along` (same units as `start_along`).
    #[must_use]
    pub fn bounds_at(&self, along: f64) -> ViewBounds {
        let (lo, hi) = match self.axis {
            Axis::X => (self.start.x_min, self.start.x_max),
            Axis::Y => (self.start.y_min, self.start.y_max),
        };
        let (lo, hi) = drag_range(lo, hi, self.length, self.handle, self.start_along, along);
        let mut b = self.start;
        match self.axis {
            Axis::X => (b.x_min, b.x_max) = (lo, hi),
            Axis::Y => (b.y_min, b.y_max) = (lo, hi),
        }
        b
    }

    /// Bounds after moving the pointer to `along` on a plot drawn at equal
    /// scale (one data unit the same length on both axes) in a frame
    /// `frame_width` × `frame_height` screen units.
    ///
    /// The dragged axis changes exactly as in [`AxisDrag::bounds_at`]; the
    /// other axis is then rescaled about its centre so that
    /// `x_span / frame_width == y_span / frame_height`. Without this, the
    /// plot's own aspect rule would restore the 1:1 scale by resetting the
    /// vertical span, turning a drag of one limit into a shift of the axis.
    /// Degenerate frames (zero or non-finite sizes) fall back to
    /// [`AxisDrag::bounds_at`].
    #[must_use]
    pub fn bounds_at_equal_scale(&self, along: f64, frame_width: f64, frame_height: f64) -> ViewBounds {
        self.bounds_at_aspect(along, frame_width, frame_height, 1.0)
    }

    /// [`Self::bounds_at_equal_scale`] for a fixed `aspect` other than 1:
    /// horizontal units per screen point over vertical units per screen
    /// point (2 shows twice as many x units per point as y units).
    #[must_use]
    pub fn bounds_at_aspect(&self, along: f64, frame_width: f64, frame_height: f64, aspect: f64) -> ViewBounds {
        let mut b = self.bounds_at(along);
        if !(frame_width > 0.0 && frame_height > 0.0 && frame_width.is_finite() && frame_height.is_finite()) {
            return b;
        }
        let aspect = if aspect > 0.0 && aspect.is_finite() { aspect } else { 1.0 };
        let rescale = |lo: f64, hi: f64, span: f64| {
            let centre = 0.5 * (lo + hi);
            (centre - 0.5 * span, centre + 0.5 * span)
        };
        match self.axis {
            Axis::X => {
                let span = (b.x_max - b.x_min) * frame_height / (frame_width * aspect);
                (b.y_min, b.y_max) = rescale(b.y_min, b.y_max, span);
            }
            Axis::Y => {
                let span = (b.y_max - b.y_min) * frame_width * aspect / frame_height;
                (b.x_min, b.x_max) = rescale(b.x_min, b.x_max, span);
            }
        }
        b
    }
}

/// New `(lower, upper)` limits of one axis after a drag from `start_along` to
/// `along`, for an axis of screen length `length` showing `[lower, upper]`.
///
/// If the inputs are degenerate (zero length or span, non-finite values) the
/// original limits are returned unchanged.
#[must_use]
pub fn drag_range(lower: f64, upper: f64, length: f64, handle: AxisHandle, start_along: f64, along: f64) -> (f64, f64) {
    let span = upper - lower;
    if !(length > 0.0 && span > 0.0 && span.is_finite() && start_along.is_finite() && along.is_finite()) {
        return (lower, upper);
    }
    let per_unit = span / length; // data units per screen unit
    match handle {
        AxisHandle::Shift => {
            let delta = (along - start_along) * per_unit;
            (lower - delta, upper - delta)
        }
        AxisHandle::Upper => {
            // Keep `lower` fixed; the grabbed value follows the pointer.
            let grabbed = lower + start_along * per_unit;
            let reach = along.max(MIN_HANDLE_DISTANCE);
            let new_per_unit = (grabbed - lower) / reach;
            if new_per_unit > 0.0 && new_per_unit.is_finite() {
                (lower, lower + new_per_unit * length)
            } else {
                (lower, upper)
            }
        }
        AxisHandle::Lower => {
            // Keep `upper` fixed; the grabbed value follows the pointer.
            let grabbed = upper - (length - start_along) * per_unit;
            let reach = (length - along).max(MIN_HANDLE_DISTANCE);
            let new_per_unit = (upper - grabbed) / reach;
            if new_per_unit > 0.0 && new_per_unit.is_finite() {
                (upper - new_per_unit * length, upper)
            } else {
                (lower, upper)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// Value displayed at position `along` for limits `(lo, hi)` and length `len`.
    fn value_at(lo: f64, hi: f64, len: f64, along: f64) -> f64 {
        lo + (hi - lo) * along / len
    }

    #[test]
    fn equal_scale_drag_moves_one_limit_and_rescales_the_other_axis() {
        // Frame 400 × 200 showing x [0, 4], y [0, 2]: 0.01 units per point on both.
        let start = ViewBounds { x_min: 0.0, x_max: 4.0, y_min: 0.0, y_max: 2.0 };
        // Grab y = 1.8 (at 180 of 200) and pull it to 120: y becomes [0, 3].
        let drag = AxisDrag { axis: Axis::Y, handle: AxisHandle::Upper, start, start_along: 180.0, length: 200.0 };
        let b = drag.bounds_at_equal_scale(120.0, 400.0, 200.0);
        assert!(close(b.y_min, 0.0) && close(b.y_max, 3.0), "{b:?}");
        // x widens about its centre (2) to keep one unit the same length.
        assert!(close(b.x_min, -1.0) && close(b.x_max, 5.0), "{b:?}");
        // Dragging the lower x limit keeps x_max and rescales y about its centre (1).
        let drag = AxisDrag { axis: Axis::X, handle: AxisHandle::Lower, start, start_along: 40.0, length: 400.0 };
        let b = drag.bounds_at_equal_scale(240.0, 400.0, 200.0);
        assert!(close(b.x_max, 4.0));
        assert!(close((b.y_max - b.y_min) / 200.0, (b.x_max - b.x_min) / 400.0));
        assert!(close(0.5 * (b.y_min + b.y_max), 1.0));
        // A shift keeps both spans.
        let drag = AxisDrag { axis: Axis::Y, handle: AxisHandle::Shift, start, start_along: 100.0, length: 200.0 };
        let b = drag.bounds_at_equal_scale(150.0, 400.0, 200.0);
        assert!(close(b.x_min, 0.0) && close(b.x_max, 4.0) && close(b.y_max - b.y_min, 2.0));
        // A degenerate frame changes only the dragged axis.
        let drag = AxisDrag { axis: Axis::Y, handle: AxisHandle::Upper, start, start_along: 180.0, length: 200.0 };
        assert_eq!(drag.bounds_at_equal_scale(120.0, 0.0, 200.0), drag.bounds_at(120.0));
    }

    #[test]
    fn a_fixed_ratio_drag_keeps_the_ratio() {
        // One x unit as long as two y units: x units per point are half the y units per point.
        let start = ViewBounds { x_min: 0.0, x_max: 4.0, y_min: 0.0, y_max: 4.0 };
        let drag = AxisDrag { axis: Axis::Y, handle: AxisHandle::Upper, start, start_along: 180.0, length: 200.0 };
        let b = drag.bounds_at_aspect(120.0, 400.0, 200.0, 0.5);
        let (x_per, y_per) = ((b.x_max - b.x_min) / 400.0, (b.y_max - b.y_min) / 200.0);
        assert!(close(x_per / y_per, 0.5), "{b:?}");
    }

    #[test]
    fn handles_by_position() {
        assert_eq!(handle_at(5.0, 100.0), AxisHandle::Lower);
        assert_eq!(handle_at(18.0, 100.0), AxisHandle::Lower);
        assert_eq!(handle_at(50.0, 100.0), AxisHandle::Shift);
        assert_eq!(handle_at(82.0, 100.0), AxisHandle::Upper);
        assert_eq!(handle_at(100.0, 100.0), AxisHandle::Upper);
        assert_eq!(handle_at(10.0, 0.0), AxisHandle::Shift);
    }

    #[test]
    fn shift_keeps_span_and_grabbed_value_under_pointer() {
        let (lo, hi) = drag_range(0.0, 10.0, 200.0, AxisHandle::Shift, 100.0, 140.0);
        assert!(close(hi - lo, 10.0));
        // The value grabbed at 100 px (5.0) is now displayed at 140 px.
        assert!(close(value_at(lo, hi, 200.0, 140.0), 5.0));
        assert!(close(lo, -2.0) && close(hi, 8.0));
    }

    #[test]
    fn upper_handle_rescales_with_lower_fixed() {
        // Grab the value 9.0 (at 180 px of 200) and pull it to 90 px: the axis
        // stretches so 9.0 now sits at 90 px, i.e. the range doubles.
        let (lo, hi) = drag_range(0.0, 10.0, 200.0, AxisHandle::Upper, 180.0, 90.0);
        assert_eq!(lo, 0.0);
        assert!(close(hi, 20.0));
        assert!(close(value_at(lo, hi, 200.0, 90.0), 9.0));
        // Pushing it outward compresses the range instead.
        let (_, hi2) = drag_range(0.0, 10.0, 200.0, AxisHandle::Upper, 180.0, 200.0);
        assert!(close(hi2, 9.0));
    }

    #[test]
    fn lower_handle_rescales_with_upper_fixed() {
        // Grab 1.0 (at 20 px) and drag to 110 px: 1.0 now sits at 110 px.
        let (lo, hi) = drag_range(0.0, 10.0, 200.0, AxisHandle::Lower, 20.0, 110.0);
        assert_eq!(hi, 10.0);
        assert!(close(value_at(lo, hi, 200.0, 110.0), 1.0));
        assert!(lo < 0.0, "range grew downward: {lo}");
    }

    #[test]
    fn handles_never_cross_or_blow_up() {
        // Dragging an upper handle past the fixed end clamps at MIN_HANDLE_DISTANCE.
        let (lo, hi) = drag_range(0.0, 10.0, 200.0, AxisHandle::Upper, 180.0, -50.0);
        assert!(hi > lo && hi.is_finite());
        let (lo, hi) = drag_range(0.0, 10.0, 200.0, AxisHandle::Lower, 20.0, 400.0);
        assert!(hi > lo && lo.is_finite());
        // Degenerate input is returned unchanged.
        assert_eq!(drag_range(3.0, 3.0, 200.0, AxisHandle::Shift, 0.0, 50.0), (3.0, 3.0));
        assert_eq!(drag_range(0.0, 1.0, 0.0, AxisHandle::Upper, 0.0, 50.0), (0.0, 1.0));
    }

    #[test]
    fn axis_drag_changes_only_its_axis() {
        let start = ViewBounds { x_min: 0.0, x_max: 10.0, y_min: -1.0, y_max: 1.0 };
        let d = AxisDrag { axis: Axis::Y, handle: AxisHandle::Shift, start, start_along: 50.0, length: 100.0 };
        let b = d.bounds_at(75.0);
        assert_eq!((b.x_min, b.x_max), (0.0, 10.0));
        assert!(close(b.y_min, -1.5) && close(b.y_max, 0.5));
    }
}
