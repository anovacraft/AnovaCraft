//! Variable encodings: which data columns drive which visual channel of a
//! graph — its position axes, its panel grid, and the color, size and marker
//! of its points — following the grammar of graphics (Wilkinson, *The Grammar
//! of Graphics*, 2nd ed., 2005) and its shelf-based specification of views
//! (Stolte, Tang & Hanrahan, "Polaris", *IEEE TVCG* 8(1), 2002).

use stat_core::{Result, StatError};
use stat_data::{DataTable, MeasurementLevel};

/// The visual channel a column can be assigned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Horizontal position (several columns give several panel columns).
    X,
    /// Vertical position (several columns give several panel rows).
    Y,
    /// One panel column per level.
    GroupX,
    /// One panel row per level.
    GroupY,
    /// One panel per level, wrapped into a near-square grid.
    Wrap,
    /// Marker and line color.
    Color,
    /// Marker size (numeric columns).
    Size,
    /// Marker shape (one shape per level).
    Shape,
    /// Subgroups within each panel: separate fit lines, smoothers and boxes.
    Overlay,
    /// One page per level, shown one at a time.
    Page,
    /// How many observations each row stands for (numeric).
    Freq,
    /// Relative precision of each row in fits and smoothers (numeric).
    Weight,
    /// Text written beside each point.
    Label,
    /// Vertical position against a second scale on the right side of each
    /// panel (several columns share that scale).
    Y2,
}

impl Role {
    /// Every role, in a stable order.
    pub const ALL: [Role; 14] = [
        Role::X,
        Role::Y,
        Role::GroupX,
        Role::GroupY,
        Role::Wrap,
        Role::Page,
        Role::Color,
        Role::Size,
        Role::Shape,
        Role::Overlay,
        Role::Freq,
        Role::Weight,
        Role::Label,
        Role::Y2,
    ];

    /// `true` for the roles that hold several columns (the position axes).
    #[must_use]
    pub fn is_multi(self) -> bool {
        matches!(self, Role::X | Role::Y | Role::Y2)
    }

    /// `true` for the roles that need a numeric column.
    #[must_use]
    pub fn needs_numeric(self) -> bool {
        matches!(self, Role::Size | Role::Freq | Role::Weight | Role::Y2)
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Role::X => "x",
            Role::Y => "y",
            Role::GroupX => "group_x",
            Role::GroupY => "group_y",
            Role::Wrap => "wrap",
            Role::Page => "page",
            Role::Color => "color",
            Role::Size => "size",
            Role::Shape => "shape",
            Role::Overlay => "overlay",
            Role::Freq => "freq",
            Role::Weight => "weight",
            Role::Label => "label",
            Role::Y2 => "y2",
        }
    }

    /// The role with this [`Role::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == key)
    }
}

/// The column assigned to each visual channel of a graph.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct EncodingPlan {
    /// Horizontal position columns.
    pub x: Vec<String>,
    /// Vertical position columns.
    pub y: Vec<String>,
    /// Panel columns, one per level.
    pub group_x: Option<String>,
    /// Panel rows, one per level.
    pub group_y: Option<String>,
    /// Wrapped panels, one per level (instead of `group_x` / `group_y`).
    pub wrap: Option<String>,
    /// Color.
    pub color: Option<String>,
    /// Marker size (numeric).
    pub size: Option<String>,
    /// Marker shape.
    pub shape: Option<String>,
    /// Subgroups within each panel.
    pub overlay: Option<String>,
    /// One page per level.
    pub page: Option<String>,
    /// Frequency (count) of each row.
    pub freq: Option<String>,
    /// Weight of each row.
    pub weight: Option<String>,
    /// Point labels.
    pub label: Option<String>,
    /// Vertical position columns drawn against the right axis.
    pub y2: Vec<String>,
}

impl EncodingPlan {
    /// The columns assigned to `role`.
    #[must_use]
    pub fn get(&self, role: Role) -> Vec<&str> {
        fn one(o: &Option<String>) -> Vec<&str> {
            o.iter().map(String::as_str).collect()
        }
        match role {
            Role::X => self.x.iter().map(String::as_str).collect(),
            Role::Y => self.y.iter().map(String::as_str).collect(),
            Role::GroupX => one(&self.group_x),
            Role::GroupY => one(&self.group_y),
            Role::Wrap => one(&self.wrap),
            Role::Color => one(&self.color),
            Role::Size => one(&self.size),
            Role::Shape => one(&self.shape),
            Role::Overlay => one(&self.overlay),
            Role::Page => one(&self.page),
            Role::Freq => one(&self.freq),
            Role::Weight => one(&self.weight),
            Role::Label => one(&self.label),
            Role::Y2 => self.y2.iter().map(String::as_str).collect(),
        }
    }

    fn slot(&mut self, role: Role) -> Option<&mut Option<String>> {
        match role {
            Role::X | Role::Y | Role::Y2 => None,
            Role::GroupX => Some(&mut self.group_x),
            Role::GroupY => Some(&mut self.group_y),
            Role::Wrap => Some(&mut self.wrap),
            Role::Color => Some(&mut self.color),
            Role::Size => Some(&mut self.size),
            Role::Shape => Some(&mut self.shape),
            Role::Overlay => Some(&mut self.overlay),
            Role::Page => Some(&mut self.page),
            Role::Freq => Some(&mut self.freq),
            Role::Weight => Some(&mut self.weight),
            Role::Label => Some(&mut self.label),
        }
    }

    /// The column list of a position role (`None` for the others).
    fn list(&mut self, role: Role) -> Option<&mut Vec<String>> {
        match role {
            Role::X => Some(&mut self.x),
            Role::Y => Some(&mut self.y),
            Role::Y2 => Some(&mut self.y2),
            _ => None,
        }
    }

    /// Assign `column` to `role`: appended to a position axis (unless already
    /// there), or replacing a single-column role. Wrapping and the panel
    /// grid are alternatives, so assigning one clears the other.
    pub fn assign(&mut self, role: Role, column: &str) {
        match role {
            Role::X | Role::Y | Role::Y2 => {
                let list = self.list(role).expect("a position role");
                if !list.iter().any(|c| c == column) {
                    list.push(column.to_owned());
                }
            }
            Role::Wrap => {
                self.group_x = None;
                self.group_y = None;
                self.wrap = Some(column.to_owned());
            }
            Role::GroupX | Role::GroupY => {
                self.wrap = None;
                if let Some(slot) = self.slot(role) {
                    *slot = Some(column.to_owned());
                }
            }
            _ => {
                if let Some(slot) = self.slot(role) {
                    *slot = Some(column.to_owned());
                }
            }
        }
    }

    /// Put `column` in place of the column at position `slot` of `role` (the
    /// only column of a single-column role). Nothing happens if there is no
    /// such slot or `column` is already elsewhere in a position role.
    pub fn replace(&mut self, role: Role, slot: usize, column: &str) {
        match role {
            Role::X | Role::Y | Role::Y2 => {
                let list = self.list(role).expect("a position role");
                if slot < list.len() && !list.iter().enumerate().any(|(i, c)| i != slot && c == column) {
                    list[slot] = column.to_owned();
                }
            }
            _ => {
                if let Some(s) = self.slot(role) {
                    if slot == 0 && s.is_some() {
                        *s = Some(column.to_owned());
                    }
                }
            }
        }
    }

    /// Remove `column` from `role`.
    pub fn unassign(&mut self, role: Role, column: &str) {
        match role {
            Role::X => self.x.retain(|c| c != column),
            Role::Y => self.y.retain(|c| c != column),
            Role::Y2 => self.y2.retain(|c| c != column),
            _ => {
                if let Some(slot) = self.slot(role) {
                    if slot.as_deref() == Some(column) {
                        *slot = None;
                    }
                }
            }
        }
    }

    /// Every distinct column the plan uses, in role order.
    #[must_use]
    pub fn columns(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for role in Role::ALL {
            for c in self.get(role) {
                if !out.contains(&c) {
                    out.push(c);
                }
            }
        }
        out
    }

    /// `true` when no position column is assigned yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.x.is_empty() && self.y.is_empty()
    }

    /// Rename `old` to `new` wherever it appears.
    pub fn rename(&mut self, old: &str, new: &str) {
        for c in self.x.iter_mut().chain(self.y.iter_mut()).chain(self.y2.iter_mut()) {
            if c == old {
                *c = new.to_owned();
            }
        }
        for role in Role::ALL {
            if let Some(slot) = self.slot(role) {
                if slot.as_deref() == Some(old) {
                    *slot = Some(new.to_owned());
                }
            }
        }
    }

    /// Check the plan against `table`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if a column is missing, the size column
    /// is not numeric, or wrapping is combined with a panel grid.
    pub fn validate(&self, table: &DataTable) -> Result<()> {
        for c in self.columns() {
            if table.find(c).is_none() {
                return Err(invalid(format!("the column {c:?} is not in the data")));
            }
        }
        for role in [Role::Size, Role::Freq, Role::Weight] {
            for c in self.get(role) {
                let numeric = table.find(c).and_then(|i| table.column(i)).is_some_and(|col| col.as_numeric().is_some());
                if !numeric {
                    let what = match role {
                        Role::Size => "size",
                        Role::Freq => "frequency",
                        _ => "weight",
                    };
                    return Err(invalid(format!("{what} needs a numeric column; {c:?} is text")));
                }
            }
        }
        if self.wrap.is_some() && (self.group_x.is_some() || self.group_y.is_some()) {
            return Err(invalid("use either wrapped panels or a panel grid, not both"));
        }
        Ok(())
    }
}

/// A column picker: a list of columns the user can step through for one
/// slot of a role (e.g. the first vertical column), changing the graph
/// without rebuilding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSwitcher {
    /// The role switched.
    pub role: Role,
    /// Position within the role (0 for single-column roles).
    pub slot: usize,
    /// Columns to choose from, in order.
    pub choices: Vec<String>,
}

impl ColumnSwitcher {
    /// The column now in the switcher's slot of `plan`.
    #[must_use]
    pub fn current<'a>(&self, plan: &'a EncodingPlan) -> Option<&'a str> {
        plan.get(self.role).get(self.slot).copied()
    }

    /// Put the choice `steps` places after the current one (wrapping; negative
    /// goes back) into `plan`. With the current column not among the choices,
    /// the first choice is used.
    pub fn step(&self, plan: &mut EncodingPlan, steps: isize) {
        if self.choices.is_empty() {
            return;
        }
        let n = self.choices.len() as isize;
        let next = match self.current(plan).and_then(|c| self.choices.iter().position(|x| x == c)) {
            Some(i) => (i as isize + steps).rem_euclid(n) as usize,
            None => 0,
        };
        plan.replace(self.role, self.slot, &self.choices[next]);
    }

    /// Follow a column renamed from `old` to `new`.
    pub fn rename(&mut self, old: &str, new: &str) {
        for c in &mut self.choices {
            if c == old {
                *c = new.to_owned();
            }
        }
    }
}

/// `true` if `column` of `table` is treated as continuous (numeric or
/// date-time, with the continuous measurement level); everything else is
/// categorical.
#[must_use]
pub fn is_continuous(table: &DataTable, column: &str) -> bool {
    table
        .find(column)
        .and_then(|i| table.column(i))
        .is_some_and(|c| c.as_numeric().is_some() && c.level == MeasurementLevel::Continuous)
}

pub(crate) fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    #[test]
    fn assign_unassign_and_columns() {
        let mut p = EncodingPlan::default();
        assert!(p.is_empty());
        p.assign(Role::X, "a");
        p.assign(Role::X, "a");
        p.assign(Role::Y, "b");
        p.assign(Role::Y, "c");
        p.assign(Role::Color, "g");
        p.assign(Role::GroupX, "h");
        assert_eq!(p.x, ["a"]);
        assert_eq!(p.columns(), ["a", "b", "c", "h", "g"]);
        p.assign(Role::Wrap, "w");
        assert!(p.group_x.is_none() && p.wrap.as_deref() == Some("w"), "wrap replaces the grid");
        p.assign(Role::GroupY, "h");
        assert!(p.wrap.is_none(), "the grid replaces wrap");
        p.unassign(Role::Y, "b");
        p.unassign(Role::Color, "other");
        assert_eq!(p.y, ["c"]);
        assert_eq!(p.color.as_deref(), Some("g"));
        p.rename("c", "c2");
        p.rename("g", "g2");
        assert_eq!((p.y[0].as_str(), p.color.as_deref()), ("c2", Some("g2")));
        p.assign(Role::Page, "pg");
        p.assign(Role::Freq, "f");
        p.assign(Role::Weight, "w");
        assert_eq!((p.page.as_deref(), p.freq.as_deref(), p.weight.as_deref()), (Some("pg"), Some("f"), Some("w")));
        p.replace(Role::Y, 0, "c3");
        p.replace(Role::Color, 0, "g3");
        p.replace(Role::Size, 0, "s");
        assert_eq!((p.y[0].as_str(), p.color.as_deref(), p.size.as_deref()), ("c3", Some("g3"), None));
        p.x.push("b".into());
        p.replace(Role::X, 0, "b");
        assert_eq!(p.x, ["a", "b"], "no duplicate on an axis");
        for r in Role::ALL {
            assert_eq!(Role::from_key(r.key()), Some(r));
        }
        // Switching the first vertical column through a list.
        let sw = ColumnSwitcher { role: Role::Y, slot: 0, choices: vec!["p".into(), "q".into(), "c3".into()] };
        assert_eq!(sw.current(&p), Some("c3"));
        sw.step(&mut p, 1);
        assert_eq!(p.y[0], "p");
        sw.step(&mut p, -1);
        assert_eq!(p.y[0], "c3");
        let none = ColumnSwitcher { role: Role::Size, slot: 0, choices: vec!["z".into()] };
        none.step(&mut p, 1);
        assert!(p.size.is_none(), "an empty single-column slot is left empty");
    }

    #[test]
    fn validation() {
        let t = DataTable::new(
            "t",
            vec![Column::numeric("x", vec![1.0, 2.0]), Column::text("g", vec!["a".into(), "b".into()])],
        )
        .unwrap();
        let mut p = EncodingPlan { x: vec!["x".into()], ..EncodingPlan::default() };
        assert!(p.validate(&t).is_ok());
        p.size = Some("g".into());
        assert!(p.validate(&t).is_err(), "size must be numeric");
        p.size = None;
        p.weight = Some("g".into());
        assert!(p.validate(&t).is_err(), "weight must be numeric");
        p.weight = None;
        p.y = vec!["missing".into()];
        assert!(p.validate(&t).is_err());
        p.y.clear();
        p.wrap = Some("g".into());
        p.group_x = Some("g".into());
        assert!(p.validate(&t).is_err());
        assert!(is_continuous(&t, "x") && !is_continuous(&t, "g") && !is_continuous(&t, "nope"));
    }
}
