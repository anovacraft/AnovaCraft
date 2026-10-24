//! How category labels under a horizontal axis are set: level, slanted at
//! 45° or upright, chosen automatically from the room between categories.
//!
//! Rotating crowded tick labels rather than thinning them follows the usual
//! advice for categorical axes (Cleveland, *The Elements of Graphing Data*,
//! 1994, on keeping every label legible; Wilke, *Fundamentals of Data
//! Visualization*, 2019, ch. 24, which prefers level text and turns labels
//! only when they would otherwise overlap). The screen and every exported
//! figure use the same rule, each with its own text measurements.

/// The Plot settings choice for category labels along the horizontal axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum CategoryLabels {
    /// Level when every label fits, else slanted, else upright.
    #[default]
    Automatic,
    /// Always level (every k-th label when they would overlap).
    Horizontal,
    /// Always slanted 45°, reading upwards to the right.
    Slanted,
    /// Always upright, reading upwards.
    Vertical,
}

impl CategoryLabels {
    /// Every choice, in menu order.
    pub const ALL: [Self; 4] = [Self::Automatic, Self::Horizontal, Self::Slanted, Self::Vertical];

    /// The name shown in Plot settings.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Horizontal => "Horizontal",
            Self::Slanted => "Slanted (45°)",
            Self::Vertical => "Vertical",
        }
    }
}

/// How the labels are actually drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelTurn {
    /// Level text centred under its tick.
    Horizontal,
    /// Text slanted 45° anticlockwise, ending at its tick.
    Slanted,
    /// Text turned a quarter turn anticlockwise, ending at its tick.
    Vertical,
}

/// The labels' turn and which of them are drawn: every `step`-th, starting
/// with the first (1 draws every label).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CategoryLayout {
    /// How the labels are turned.
    pub turn: LabelTurn,
    /// Draw every `step`-th label.
    pub step: usize,
}

/// Room needed beside a level label (in the same units as the widths).
pub const LEVEL_GAP: f64 = 10.0;
/// Room needed between turned labels.
pub const TURNED_GAP: f64 = 2.0;

/// The layout for labels at most `widest` wide and `line` high whose
/// categories lie `spacing` apart (all in the same units, such as screen
/// points). Automatic keeps them level when they fit, slants them when
/// slanted lines fit, and otherwise stands them upright; only upright labels
/// that still overlap (or a fixed choice that does not fit) are thinned to
/// every k-th one.
#[must_use]
pub fn category_layout(choice: CategoryLabels, spacing: f64, widest: f64, line: f64) -> CategoryLayout {
    // Slanted lines `spacing` apart along the axis are `spacing / √2` apart across.
    let needs = |turn: LabelTurn| match turn {
        LabelTurn::Horizontal => widest + LEVEL_GAP,
        LabelTurn::Slanted => (line + TURNED_GAP) * std::f64::consts::SQRT_2,
        LabelTurn::Vertical => line + TURNED_GAP,
    };
    let step = |turn: LabelTurn| {
        if spacing.is_finite() && spacing > 0.0 { ((needs(turn) / spacing).ceil() as usize).max(1) } else { 1 }
    };
    let turn = match choice {
        CategoryLabels::Horizontal => LabelTurn::Horizontal,
        CategoryLabels::Slanted => LabelTurn::Slanted,
        CategoryLabels::Vertical => LabelTurn::Vertical,
        CategoryLabels::Automatic => [LabelTurn::Horizontal, LabelTurn::Slanted]
            .into_iter()
            .find(|&t| step(t) == 1)
            .unwrap_or(LabelTurn::Vertical),
    };
    CategoryLayout { turn, step: step(turn) }
}

/// How far below the axis labels at most `widest` wide and `line` high
/// reach when turned `turn`.
#[must_use]
pub fn label_depth(turn: LabelTurn, widest: f64, line: f64) -> f64 {
    match turn {
        LabelTurn::Horizontal => line,
        LabelTurn::Slanted => (widest + line) * std::f64::consts::FRAC_1_SQRT_2,
        LabelTurn::Vertical => widest,
    }
}

/// The smallest distance between neighbouring positions (`f64::INFINITY`
/// for fewer than two distinct ones).
#[must_use]
pub fn least_spacing(positions: &[f64]) -> f64 {
    let mut sorted: Vec<f64> = positions.iter().copied().filter(|v| v.is_finite()).collect();
    sorted.sort_by(f64::total_cmp);
    sorted.windows(2).map(|w| w[1] - w[0]).filter(|g| *g > 0.0).fold(f64::INFINITY, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_turns_labels_only_when_they_would_overlap() {
        // Labels 40 wide, 12 high.
        let auto = |spacing| category_layout(CategoryLabels::Automatic, spacing, 40.0, 12.0);
        assert_eq!(auto(60.0), CategoryLayout { turn: LabelTurn::Horizontal, step: 1 });
        // Too narrow for level text; slanted lines need (12 + 2) √2 ≈ 19.8.
        assert_eq!(auto(25.0), CategoryLayout { turn: LabelTurn::Slanted, step: 1 });
        // Upright lines need 14.
        assert_eq!(auto(15.0), CategoryLayout { turn: LabelTurn::Vertical, step: 1 });
        // Even upright labels overlap at 5 apart: every third is drawn.
        assert_eq!(auto(5.0), CategoryLayout { turn: LabelTurn::Vertical, step: 3 });
        // One category: level.
        assert_eq!(auto(f64::INFINITY), CategoryLayout { turn: LabelTurn::Horizontal, step: 1 });
    }

    #[test]
    fn fixed_choices_keep_their_turn_and_thin_when_crowded() {
        let at = |choice| category_layout(choice, 25.0, 40.0, 12.0);
        assert_eq!(at(CategoryLabels::Horizontal), CategoryLayout { turn: LabelTurn::Horizontal, step: 2 });
        assert_eq!(at(CategoryLabels::Vertical), CategoryLayout { turn: LabelTurn::Vertical, step: 1 });
        assert_eq!(at(CategoryLabels::Slanted).turn, LabelTurn::Slanted);
    }

    #[test]
    fn depth_and_spacing() {
        assert_eq!(label_depth(LabelTurn::Horizontal, 40.0, 12.0), 12.0);
        assert_eq!(label_depth(LabelTurn::Vertical, 40.0, 12.0), 40.0);
        assert!((label_depth(LabelTurn::Slanted, 40.0, 12.0) - 52.0 / 2f64.sqrt()).abs() < 1e-12);
        assert_eq!(least_spacing(&[3.0, 1.0, 2.0, 2.0]), 1.0);
        assert_eq!(least_spacing(&[1.0]), f64::INFINITY);
    }
}
