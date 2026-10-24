//! A report's own row filter and column picker.
//!
//! Any report can carry a **row filter**: conditions on columns (ticked
//! levels of a categorical column, an interval of a continuous one) that a
//! row must all pass to enter the analysis. The filter never changes the
//! table's row states: the report is computed from a copy of them in which
//! rows outside the filter are also excluded and hidden
//! ([`ReportScope::apply`]), so the data grid and every other report still
//! use those rows. Conditions name their columns, so they survive columns
//! being inserted or removed, and follow renames.
//!
//! Reports with one response or process column (see
//! [`SavedReport::switch_slot`]) can also carry a **column picker**: a list
//! of columns to step the report through, one at a time, without opening its
//! dialog again. Switching changes the request's column and recomputes the
//! report; everything else in the request stays. A capability study's
//! specification limits belong to its column, so the scope keeps the limits
//! set for each column it has visited ([`ReportScope::spec_limits`]).
//!
//! On a stacked table of multiple imputations
//! ([`crate::multiple_imputation`]) a report can also be **pooled across
//! imputations** ([`ReportScope::pool`]): its usual sections are computed
//! from the original rows (imputation 0) only, and pooled sections
//! ([`crate::pooling`]) computed from the completed copies come first.
//!
//! All of these are kept with the request as [`SavedReport::Scoped`] and
//! stored in a project file as members of the report object: `"row_filter"`
//! (a list of conditions, each `{"column", "levels"}` or `{"column", "min",
//! "max"}`), `"column_switcher"` (column names), `"switcher_limits"`
//! (`[{"column", "lower", "upper", "target"}]`) and `"pool_imputations"`
//! (the name of the column numbering the imputations).

use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_quality::SpecLimits;
use stat_viz::FilterCondition;

use crate::project::SavedReport;

/// A report's row filter and column picker.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReportScope {
    /// Conditions a row must all pass to enter the analysis (at most one
    /// per column). Empty: every row the table includes.
    pub filter: Vec<FilterCondition>,
    /// Columns (by name) the switcher steps through, in order; `None` when
    /// the report has no switcher.
    pub switcher: Option<Vec<String>>,
    /// Capability studies: the specification limits set for each column the
    /// switcher has shown, by column name.
    pub spec_limits: Vec<(String, SpecLimits)>,
    /// Stacked imputation tables: the column (by name) numbering the
    /// imputations, when the report is pooled across them. The report's own
    /// sections then use imputation 0 only.
    pub pool: Option<String>,
    /// How the report's contour maps are drawn (set in their Plot
    /// settings); `None` for the default style.
    pub contour: Option<stat_viz::ContourStyle>,
    /// Plot styles changed by the user, by plot ([`plot_style_key`]); plots
    /// without an entry are drawn in their default style.
    pub plot_styles: Vec<(String, crate::plot_style::PlotStyle)>,
}

/// The key of the `nth` plot (counting from 0) of the sections of kind
/// `kind` in a report, under which [`ReportScope::plot_styles`] keeps its
/// style, e.g. `Residuals#0`. It is the section kind's name, so renaming a
/// kind loses saved styles of its plots (they are then drawn by default).
#[must_use]
pub fn plot_style_key(kind: crate::report::SectionKind, nth: usize) -> String {
    format!("{kind:?}#{nth}")
}

impl ReportScope {
    /// `true` when there is neither a filter nor a switcher.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.filter.is_empty()
            && self.switcher.is_none()
            && self.spec_limits.is_empty()
            && self.pool.is_none()
            && self.contour.is_none()
            && self.plot_styles.is_empty()
    }

    /// The saved style of the plot under `key`.
    #[must_use]
    pub fn plot_style(&self, key: &str) -> Option<&crate::plot_style::PlotStyle> {
        self.plot_styles.iter().find(|(k, _)| k == key).map(|(_, s)| s)
    }

    /// Keep `style` for the plot under `key`, or forget it (`None`: the plot
    /// is back to its default style). Returns `true` when this changed the
    /// scope.
    pub fn set_plot_style(&mut self, key: &str, style: Option<crate::plot_style::PlotStyle>) -> bool {
        let at = self.plot_styles.iter().position(|(k, _)| k == key);
        match (at, style) {
            (Some(i), Some(style)) if self.plot_styles[i].1 == style => false,
            (Some(i), Some(style)) => {
                self.plot_styles[i].1 = style;
                true
            }
            (None, Some(style)) => {
                self.plot_styles.push((key.to_owned(), style));
                true
            }
            (Some(i), None) => {
                self.plot_styles.remove(i);
                true
            }
            (None, None) => false,
        }
    }

    /// Draw the contour maps of `sections` in this scope's style (the
    /// default style when it has none).
    pub fn style_contours(&self, sections: &mut [crate::report::ReportSection]) {
        if let Some(style) = &self.contour {
            for s in sections {
                s.apply_contour_style(style);
            }
        }
    }

    /// `true` when the report's analysis uses fewer rows than the table
    /// includes: it has a filter, or it is pooled (its own sections use
    /// imputation 0 only).
    #[must_use]
    pub fn narrows(&self) -> bool {
        self.is_filtered() || self.pool.is_some()
    }

    /// `true` when the filter has at least one condition.
    #[must_use]
    pub fn is_filtered(&self) -> bool {
        !self.filter.is_empty()
    }

    /// Whether each row of `table` enters the report's own analysis: it
    /// passes every condition (all rows without conditions; a row with no
    /// value in a condition's column fails it) and, when pooled, belongs to
    /// imputation 0.
    #[must_use]
    pub fn matches(&self, table: &DataTable) -> Vec<bool> {
        let mut out = self.filter_matches(table);
        if let Some(v) = self.pool_column(table).and_then(|c| table.column(c)).and_then(|c| c.as_numeric()) {
            for (o, x) in out.iter_mut().zip(v) {
                *o &= *x == 0.0;
            }
        }
        out
    }

    /// Whether each row of `table` passes every filter condition.
    #[must_use]
    pub fn filter_matches(&self, table: &DataTable) -> Vec<bool> {
        let mut out = vec![true; table.n_rows()];
        for c in &self.filter {
            for (o, m) in out.iter_mut().zip(c.matches(table)) {
                *o &= m;
            }
        }
        out
    }

    /// The column of `table` numbering the imputations, when the report is
    /// pooled and `table` has it.
    #[must_use]
    pub fn pool_column(&self, table: &DataTable) -> Option<usize> {
        self.pool.as_deref().and_then(|name| table.find(name))
    }

    /// The row states the report is computed from: a copy of `rows` with
    /// the rows outside the filter (and, when pooled, outside imputation 0)
    /// also excluded and hidden. `None` when the scope narrows nothing (use
    /// `rows` itself).
    #[must_use]
    pub fn apply(&self, table: &DataTable, rows: &RowStateManager) -> Option<RowStateManager> {
        self.narrows().then(|| apply_mask(&self.matches(table), rows))
    }

    /// The pooled sections of `report` (its request without scope), when
    /// pooled: computed from `rows` narrowed by the filter only, so every
    /// imputation is there. Empty when not pooled.
    #[must_use]
    pub fn pooled_sections(
        &self,
        report: &SavedReport,
        table: &DataTable,
        rows: &RowStateManager,
    ) -> Vec<crate::report::ReportSection> {
        let Some(name) = &self.pool else { return Vec::new() };
        let Some(column) = self.pool_column(table) else {
            return vec![crate::report::ReportSection {
                kind: crate::report::SectionKind::PooledNote,
                title: "Pooled Across Imputations".to_owned(),
                content: crate::report::SectionContent::Note(format!(
                    "⚠ The table has no column {name:?} numbering the imputations, so nothing is pooled."
                )),
            }];
        };
        let local = self.is_filtered().then(|| apply_mask(&self.filter_matches(table), rows));
        crate::pooling::pooled_sections(report, table, local.as_ref().unwrap_or(rows), column)
    }

    /// The filter's condition on `column`.
    #[must_use]
    pub fn condition(&self, column: &str) -> Option<&FilterCondition> {
        self.filter.iter().find(|c| c.column() == column)
    }

    /// Columns the filter's conditions (and the pooling) name.
    #[must_use]
    pub fn filter_columns(&self) -> Vec<&str> {
        self.filter.iter().map(FilterCondition::column).chain(self.pool.as_deref()).collect()
    }

    /// Follow a column renamed from `old` to `new`.
    pub fn rename(&mut self, old: &str, new: &str) {
        for c in &mut self.filter {
            if c.column() == old {
                *c = match c.clone() {
                    FilterCondition::Levels { levels, .. } => {
                        FilterCondition::Levels { column: new.to_owned(), levels }
                    }
                    FilterCondition::Range { min, max, .. } => {
                        FilterCondition::Range { column: new.to_owned(), min, max }
                    }
                };
            }
        }
        for name in self
            .switcher
            .iter_mut()
            .flatten()
            .chain(self.spec_limits.iter_mut().map(|(n, _)| n))
            .chain(self.pool.as_mut())
        {
            if name == old {
                new.clone_into(name);
            }
        }
    }

    /// The conditions joined with "and", e.g. `Region ∈ {East} and 2 ≤ Age ≤ 9`.
    #[must_use]
    pub fn describe(&self) -> String {
        self.filter.iter().map(FilterCondition::describe).collect::<Vec<_>>().join(" and ")
    }

    /// The members this scope adds to a report's JSON object.
    pub(crate) fn json_members(&self) -> Vec<(String, JsonValue)> {
        let mut out = Vec::new();
        if !self.filter.is_empty() {
            let list = self.filter.iter().map(crate::table_ops::condition_to_json).collect();
            out.push(("row_filter".to_owned(), JsonValue::Array(list)));
        }
        if let Some(choices) = &self.switcher {
            let list = choices.iter().map(|c| JsonValue::String(c.clone())).collect();
            out.push(("column_switcher".to_owned(), JsonValue::Array(list)));
        }
        if !self.spec_limits.is_empty() {
            let list = self
                .spec_limits
                .iter()
                .map(|(column, l)| {
                    let mut m = vec![("column".to_owned(), JsonValue::String(column.clone()))];
                    for (key, v) in [("lower", l.lower), ("upper", l.upper), ("target", l.target)] {
                        if let Some(v) = v {
                            m.push((key.to_owned(), JsonValue::number(v)));
                        }
                    }
                    JsonValue::Object(m)
                })
                .collect();
            out.push(("switcher_limits".to_owned(), JsonValue::Array(list)));
        }
        if let Some(name) = &self.pool {
            out.push(("pool_imputations".to_owned(), JsonValue::String(name.clone())));
        }
        if let Some(style) = &self.contour {
            out.push(("contour_style".to_owned(), contour_style_to_json(style)));
        }
        if !self.plot_styles.is_empty() {
            let list = self
                .plot_styles
                .iter()
                .map(|(key, style)| {
                    let mut members = vec![("plot".to_owned(), JsonValue::String(key.clone()))];
                    if let JsonValue::Object(m) =
                        crate::plot_style::plot_style_to_json(style, crate::plot_style::StyleTitles::With)
                    {
                        members.extend(m);
                    }
                    JsonValue::Object(members)
                })
                .collect();
            out.push(("plot_styles".to_owned(), JsonValue::Array(list)));
        }
        out
    }

    /// Read the members [`ReportScope::json_members`] wrote from a report
    /// object. Filter conditions must name columns of `table`; switcher
    /// columns `table` lacks are dropped.
    pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> stat_core::Result<Self> {
        let invalid = |m: String| crate::project::invalid(m);
        let mut scope = Self::default();
        if let Some(list) = v.get("row_filter").filter(|f| !matches!(f, JsonValue::Null)) {
            let list = list.as_array().ok_or_else(|| invalid("row_filter must be a list of conditions".into()))?;
            for c in list {
                let c = crate::table_ops::condition_from_json(c, "row_filter")?;
                if table.find(c.column()).is_none() {
                    return Err(invalid(format!("report filter refers to a missing column {:?}", c.column())));
                }
                if scope.condition(c.column()).is_some() {
                    return Err(invalid(format!("row_filter: two conditions on {:?}", c.column())));
                }
                scope.filter.push(c);
            }
        }
        if let Some(list) = v.get("column_switcher").filter(|f| !matches!(f, JsonValue::Null)) {
            let list =
                list.as_array().ok_or_else(|| invalid("column_switcher must be a list of column names".into()))?;
            let mut names = Vec::new();
            for n in list {
                let n = n.as_str().ok_or_else(|| invalid("column_switcher must be a list of column names".into()))?;
                if table.find(n).is_some() && !names.iter().any(|m| m == n) {
                    names.push(n.to_owned());
                }
            }
            scope.switcher = Some(names);
        }
        if let Some(list) = v.get("switcher_limits").filter(|f| !matches!(f, JsonValue::Null)) {
            let list = list.as_array().ok_or_else(|| invalid("switcher_limits must be a list".into()))?;
            for l in list {
                let column = l
                    .get("column")
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| invalid("switcher_limits: missing column".into()))?;
                let num = |k: &str| l.get(k).and_then(JsonValue::as_f64);
                if table.find(column).is_some() {
                    let limits = SpecLimits { lower: num("lower"), upper: num("upper"), target: num("target") };
                    scope.spec_limits.push((column.to_owned(), limits));
                }
            }
        }
        if let Some(name) = v.get("pool_imputations").filter(|f| !matches!(f, JsonValue::Null)) {
            let name = name.as_str().ok_or_else(|| invalid("pool_imputations must be a column name".into()))?;
            if table.find(name).is_none() {
                return Err(invalid(format!("pool_imputations refers to a missing column {name:?}")));
            }
            scope.pool = Some(name.to_owned());
        }
        if let Some(style) = v.get("contour_style").filter(|f| !matches!(f, JsonValue::Null)) {
            scope.contour = Some(contour_style_from_json(style)?);
        }
        if let Some(list) = v.get("plot_styles").filter(|f| !matches!(f, JsonValue::Null)) {
            let list = list.as_array().ok_or_else(|| invalid("plot_styles must be a list".into()))?;
            for item in list {
                let key = item
                    .get("plot")
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| invalid("plot_styles: each style needs a \"plot\" name".into()))?;
                let mut style = crate::plot_style::PlotStyle::default();
                crate::plot_style::plot_style_from_json(item, &mut style, crate::plot_style::StyleTitles::With)?;
                scope.set_plot_style(key, Some(style));
            }
        }
        Ok(scope)
    }
}

/// The project-file form of a contour style: `fill`, `lines`, `labels`,
/// `scale` (`"sequential"`, `"heat"`, `"spectrum"`, `"diverging"`,
/// `"grey"`), `reverse`, `opacity`, and when set `levels`, `min` and `max`
/// (a fixed range) and `centre`.
#[must_use]
pub fn contour_style_to_json(style: &stat_viz::ContourStyle) -> JsonValue {
    let mut m = vec![
        ("fill".to_owned(), JsonValue::Bool(style.fill)),
        ("lines".to_owned(), JsonValue::Bool(style.lines)),
        ("labels".to_owned(), JsonValue::Bool(style.labels)),
        ("scale".to_owned(), JsonValue::String(style.scale.key().to_owned())),
        ("reverse".to_owned(), JsonValue::Bool(style.reverse)),
        ("opacity".to_owned(), JsonValue::number(style.opacity)),
    ];
    if let Some(n) = style.levels {
        m.push(("levels".to_owned(), JsonValue::number(n as f64)));
    }
    if let Some((lo, hi)) = style.range {
        m.push(("min".to_owned(), JsonValue::number(lo)));
        m.push(("max".to_owned(), JsonValue::number(hi)));
    }
    if let Some(c) = style.centre {
        m.push(("centre".to_owned(), JsonValue::number(c)));
    }
    JsonValue::Object(m)
}

/// Read [`contour_style_to_json`]; members left out keep their defaults.
///
/// # Errors
/// A member of the wrong type, an unknown scale, a number of levels
/// outside 1–50, only one end of a range or an empty range, or an opacity
/// outside 0–1.
pub fn contour_style_from_json(v: &JsonValue) -> stat_core::Result<stat_viz::ContourStyle> {
    let invalid = |m: &str| crate::project::invalid(format!("contour_style: {m}"));
    if !matches!(v, JsonValue::Object(_)) {
        return Err(invalid("must be an object"));
    }
    let d = stat_viz::ContourStyle::default();
    let flag = |k: &str, default: bool| -> stat_core::Result<bool> {
        match v.get(k) {
            None | Some(JsonValue::Null) => Ok(default),
            Some(JsonValue::Bool(b)) => Ok(*b),
            Some(_) => Err(invalid(&format!("{k} must be true or false"))),
        }
    };
    let number = |k: &str| -> stat_core::Result<Option<f64>> {
        match v.get(k) {
            None | Some(JsonValue::Null) => Ok(None),
            Some(x) => {
                x.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| invalid(&format!("{k} must be a number")))
            }
        }
    };
    let scale = match v.get("scale") {
        None | Some(JsonValue::Null) => d.scale,
        Some(x) => x
            .as_str()
            .and_then(stat_viz::palette::ColorScale::from_key)
            .ok_or_else(|| invalid("scale must be sequential, heat, spectrum, diverging or grey"))?,
    };
    let levels = match number("levels")? {
        None => None,
        Some(n) if (1.0..=50.0).contains(&n) && n.fract() == 0.0 => Some(n as usize),
        Some(_) => return Err(invalid("levels must be a whole number from 1 to 50")),
    };
    let range = match (number("min")?, number("max")?) {
        (None, None) => None,
        (Some(lo), Some(hi)) if hi > lo => Some((lo, hi)),
        (Some(_), Some(_)) => return Err(invalid("max must be above min")),
        _ => return Err(invalid("give both min and max, or neither")),
    };
    let opacity = number("opacity")?.unwrap_or(d.opacity);
    if !(0.0..=1.0).contains(&opacity) {
        return Err(invalid("opacity must be from 0 to 1"));
    }
    Ok(stat_viz::ContourStyle {
        fill: flag("fill", d.fill)?,
        lines: flag("lines", d.lines)?,
        labels: flag("labels", d.labels)?,
        scale,
        reverse: flag("reverse", d.reverse)?,
        levels,
        range,
        centre: number("centre")?,
        opacity,
    })
}

/// A copy of `rows` with the rows where `keep` is `false` also excluded and
/// hidden.
pub fn apply_mask(keep: &[bool], rows: &RowStateManager) -> RowStateManager {
    let mut out = rows.clone();
    let outside: Vec<usize> = keep.iter().enumerate().filter(|(_, k)| !**k).map(|(r, _)| r).collect();
    if !outside.is_empty() {
        out.set_excluded(outside.iter().copied(), true);
        out.set_hidden(outside, true);
    }
    out
}

/// The column a column picker changes in a report request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchSlot {
    /// What the column is to the analysis, e.g. `"Response"` or `"Column"`.
    pub role: &'static str,
    /// The column now in that role.
    pub column: usize,
}

impl SavedReport {
    /// `self` carrying `scope`: [`SavedReport::Scoped`] around the request
    /// (any scope it had is replaced), or the bare request when `scope` is
    /// empty.
    #[must_use]
    pub fn with_scope(self, scope: ReportScope) -> Self {
        let (inner, _) = self.split_scope();
        if scope.is_empty() { inner } else { Self::Scoped { scope, report: Box::new(inner) } }
    }

    /// The request without its scope, and the scope (empty when it had none).
    #[must_use]
    pub fn split_scope(self) -> (Self, ReportScope) {
        match self {
            Self::Scoped { scope, report } => {
                let (inner, _) = report.split_scope();
                (inner, scope)
            }
            other => (other, ReportScope::default()),
        }
    }

    /// The request without its scope.
    #[must_use]
    pub fn unscoped(&self) -> &Self {
        match self {
            Self::Scoped { report, .. } => report.unscoped(),
            other => other,
        }
    }

    /// The report's scope, when it has one.
    #[must_use]
    pub fn scope(&self) -> Option<&ReportScope> {
        match self {
            Self::Scoped { scope, .. } => Some(scope),
            _ => None,
        }
    }

    /// The single response or process column a column picker can change,
    /// for the reports built around one: linear, generalized linear,
    /// penalized and stepwise models (response), distributions, control and
    /// capability charts (column), bivariate fits (Y), multi-vari charts and
    /// analysis of means (response). `None` for other reports.
    #[must_use]
    pub fn switch_slot(&self) -> Option<SwitchSlot> {
        let (role, column) = match self.unscoped() {
            Self::Linear { spec, .. } => ("Response", spec.response),
            Self::Glm { request, .. } => ("Response", request.response),
            Self::Regularized { request, .. } => ("Response", request.response),
            Self::Stepwise { request, .. } => ("Response", request.candidates.response),
            Self::Distribution { request, .. } => ("Column", request.column),
            Self::ControlChart { request, .. } => ("Column", request.column),
            Self::Capability { request } => ("Column", request.column),
            Self::Bivariate { request } => ("Y", request.y),
            Self::MultiVari { request, .. } => ("Response", request.response),
            Self::Anom { request } => ("Response", request.response),
            _ => return None,
        };
        Some(SwitchSlot { role, column })
    }

    /// Columns of `table` the switcher can offer: those of the same kind
    /// (numeric, text or date-time) as the current column that the request
    /// does not already use in another role, in table order (the current
    /// column included). Empty when the report has no switch slot.
    #[must_use]
    pub fn switch_candidates(&self, table: &DataTable) -> Vec<usize> {
        let Some(slot) = self.switch_slot() else { return Vec::new() };
        let Some(kind) = table.column(slot.column).map(stat_data::Column::kind) else { return Vec::new() };
        let others: Vec<usize> = self.unscoped().columns().into_iter().filter(|&c| c != slot.column).collect();
        (0..table.n_cols())
            .filter(|&c| c == slot.column || !others.contains(&c))
            .filter(|&c| table.column(c).is_some_and(|col| col.kind() == kind))
            .collect()
    }

    /// The column that dropping `column` (by name) on a plot axis titled
    /// `axis_title` puts in the report's switch slot. An axis maps to the
    /// switch slot when its title is the name of the column now in the slot
    /// (the response of a model, the column of a distribution or chart);
    /// `column` must be one of [`Self::switch_candidates`] and not the
    /// column already there. `None` otherwise, so axes showing other
    /// quantities (counts, residuals, quantiles) take no drops.
    #[must_use]
    pub fn axis_drop(&self, table: &DataTable, axis_title: &str, column: &str) -> Option<usize> {
        let slot = self.switch_slot()?;
        let current = table.column(slot.column)?;
        if axis_title.trim() != current.name {
            return None;
        }
        let dropped = table.find(column).filter(|&c| c != slot.column)?;
        self.switch_candidates(table).contains(&dropped).then_some(dropped)
    }

    /// The same report with `column` in its switch slot (scope kept), or
    /// `None` when it has no switch slot or `column` is used in another role.
    ///
    /// A generalized linear model's event level goes back to its default
    /// (the new response may not have the old level). A capability study
    /// takes the limits its scope remembers for `column`, if any, and
    /// remembers the old column's limits.
    #[must_use]
    pub fn switched_to(&self, column: usize, table: &DataTable) -> Option<Self> {
        let slot = self.switch_slot()?;
        if column == slot.column {
            return Some(self.clone());
        }
        if self.unscoped().columns().contains(&column) {
            return None;
        }
        let (mut inner, mut scope) = self.clone().split_scope();
        match &mut inner {
            Self::Linear { spec, .. } => spec.response = column,
            Self::Glm { request, .. } => {
                request.response = column;
                request.event = None;
            }
            Self::Regularized { request, .. } => request.response = column,
            Self::Stepwise { request, .. } => request.candidates.response = column,
            Self::Distribution { request, .. } => request.column = column,
            Self::ControlChart { request, .. } => request.column = column,
            Self::Capability { request } => {
                let name = |c: usize| table.column(c).map(|c| c.name.clone()).unwrap_or_default();
                let (old, new) = (name(request.column), name(column));
                scope.spec_limits.retain(|(n, _)| *n != old);
                scope.spec_limits.push((old, request.limits));
                if let Some(i) = scope.spec_limits.iter().position(|(n, _)| *n == new) {
                    request.limits = scope.spec_limits.remove(i).1;
                }
                request.column = column;
            }
            Self::Bivariate { request } => request.y = column,
            Self::MultiVari { request, .. } => request.response = column,
            Self::Anom { request } => request.response = column,
            _ => return None,
        }
        Some(inner.with_scope(scope))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapabilityRequest, DistributionOptions, DistributionRequest, LinearModelSpec, ReportOptions};
    use stat_data::Column;

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("y", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
                Column::numeric("x", vec![2.0, 4.1, 5.9, 8.2, 9.9, 12.1]),
                Column::text("g", vec!["a".into(), "b".into(), "a".into(), "b".into(), "a".into(), "".into()]),
                Column::numeric("z", vec![6.0, 5.0, 4.0, 3.0, 2.0, 1.0]),
            ],
        )
        .unwrap()
    }

    fn linear() -> SavedReport {
        SavedReport::Linear {
            spec: LinearModelSpec { response: 0, predictors: vec![1], effects: Vec::new(), splines: Vec::new() },
            options: ReportOptions::default(),
        }
    }

    #[test]
    fn filter_excludes_and_hides_rows_outside_it_without_touching_the_table_rows() {
        let t = table();
        let mut rows = RowStateManager::new(6);
        rows.set_excluded([1], true);
        let scope = ReportScope {
            filter: vec![FilterCondition::Levels { column: "g".into(), levels: vec!["a".into()] }],
            ..ReportScope::default()
        };
        assert_eq!(scope.matches(&t), vec![true, false, true, false, true, false]);
        let local = scope.apply(&t, &rows).unwrap();
        for r in 0..6 {
            let inside = [0, 2, 4].contains(&r);
            assert_eq!(local.is_excluded(r), !inside, "row {r}");
            assert_eq!(local.is_hidden(r), !inside, "row {r}");
        }
        assert!(!rows.is_excluded(3) && !rows.is_hidden(3));
        assert!(ReportScope::default().apply(&t, &rows).is_none());
    }

    #[test]
    fn filtered_report_runs_on_matching_rows_only() {
        // The filtered fit equals the fit with the other rows excluded.
        let t = table();
        let scope = ReportScope {
            filter: vec![FilterCondition::Range { column: "y".into(), min: 2.0, max: 5.0 }],
            ..ReportScope::default()
        };
        let scoped = linear().with_scope(scope);
        let filtered = crate::run_report(&scoped, &t, &RowStateManager::new(6)).unwrap();
        let mut rows = RowStateManager::new(6);
        rows.set_excluded([0, 5], true);
        let excluded = crate::run_report(&linear(), &t, &rows).unwrap();
        assert_eq!(filtered.title, excluded.title);
        assert_eq!(filtered.sections, excluded.sections);
        assert_ne!(filtered.sections, crate::run_report(&linear(), &t, &RowStateManager::new(6)).unwrap().sections);
    }

    #[test]
    fn scope_round_trips_through_project_json() {
        let t = table();
        let scope = ReportScope {
            filter: vec![
                FilterCondition::Levels { column: "g".into(), levels: vec!["a".into()] },
                FilterCondition::Range { column: "x".into(), min: 1.5, max: 10.0 },
            ],
            switcher: Some(vec!["y".into(), "z".into()]),
            spec_limits: vec![("z".into(), SpecLimits { lower: Some(1.0), upper: None, target: Some(3.0) })],
            pool: None,
            contour: Some(stat_viz::ContourStyle {
                fill: false,
                scale: stat_viz::palette::ColorScale::Diverging,
                centre: Some(2.5),
                ..stat_viz::ContourStyle::default()
            }),
            plot_styles: vec![("Residuals#1".into(), {
                let mut style = crate::plot_style::PlotStyle::default();
                style.axes.y.grid = stat_viz::axis_settings::GridLines::None;
                style.axes.x.title = Some("Fitted \\hat{y}".into());
                style.axes.equal_scale = true;
                style
            })],
        };
        let scoped = linear().with_scope(scope);
        let json = crate::project::report_to_json(&scoped, &t);
        assert!(json.get("row_filter").is_some() && json.get("column_switcher").is_some());
        assert_eq!(crate::project::report_from_json(&json, &t).unwrap(), scoped);
        // A bare request writes no scope members and reads back bare.
        let bare = crate::project::report_to_json(&linear(), &t);
        assert!(bare.get("row_filter").is_none());
        assert_eq!(crate::project::report_from_json(&bare, &t).unwrap(), linear());
    }

    #[test]
    fn contour_styles_read_with_defaults_and_reject_bad_members() {
        let read = |text: &str| contour_style_from_json(&stat_data::parse_json(text).unwrap());
        assert_eq!(read("{}").unwrap(), stat_viz::ContourStyle::default());
        let s = read(r#"{"fill": false, "scale": "gray", "levels": 6, "min": 0, "max": 10}"#).unwrap();
        assert!(!s.fill && s.lines && s.labels);
        assert_eq!((s.scale, s.levels, s.range), (stat_viz::palette::ColorScale::Grey, Some(6), Some((0.0, 10.0))));
        for bad in [
            r#"{"scale": "rainbow"}"#,
            r#"{"levels": 0}"#,
            r#"{"levels": 2.5}"#,
            r#"{"min": 3}"#,
            r#"{"min": 3, "max": 1}"#,
            r#"{"opacity": 2}"#,
            r#"{"fill": "yes"}"#,
            "[]",
        ] {
            assert!(read(bad).is_err(), "{bad}");
        }
        let style = stat_viz::ContourStyle { range: Some((-1.0, 1.0)), centre: Some(0.25), ..Default::default() };
        assert_eq!(contour_style_from_json(&contour_style_to_json(&style)).unwrap(), style);
    }

    #[test]
    fn a_filter_on_a_missing_column_is_rejected() {
        let t = table();
        let scoped = linear().with_scope(ReportScope {
            filter: vec![FilterCondition::Levels { column: "g".into(), levels: vec!["a".into()] }],
            ..ReportScope::default()
        });
        let json = crate::project::report_to_json(&scoped, &t);
        let other =
            DataTable::new("t", vec![Column::numeric("y", vec![1.0, 2.0]), Column::numeric("x", vec![1.0, 2.0])])
                .unwrap();
        let err = crate::project::report_from_json(&json, &other).unwrap_err().to_string();
        assert!(err.contains("\"g\""), "{err}");
    }

    #[test]
    fn with_scope_never_nests_and_an_empty_scope_unwraps() {
        let s = ReportScope { switcher: Some(vec!["y".into()]), ..ReportScope::default() };
        let once = linear().with_scope(s.clone());
        let twice = once.clone().with_scope(s);
        assert_eq!(once, twice);
        assert_eq!(once.unscoped(), &linear());
        assert_eq!(twice.with_scope(ReportScope::default()), linear());
    }

    #[test]
    fn a_column_dropped_on_the_slot_columns_axis_switches_to_it() {
        let t = table();
        let fit = linear();
        // The response's axis takes another numeric column not in the model.
        assert_eq!(fit.axis_drop(&t, "y", "z"), Some(3));
        assert_eq!(fit.axis_drop(&t, " y ", "z"), Some(3));
        // Not the column already there, a predictor, another kind, or a
        // missing column; and other axes take nothing.
        assert_eq!(fit.axis_drop(&t, "y", "y"), None);
        assert_eq!(fit.axis_drop(&t, "y", "x"), None);
        assert_eq!(fit.axis_drop(&t, "y", "g"), None);
        assert_eq!(fit.axis_drop(&t, "y", "nope"), None);
        assert_eq!(fit.axis_drop(&t, "Residual", "z"), None);
        assert_eq!(fit.axis_drop(&t, "x", "z"), None);
        // Reports without a switch slot take no drops.
        let correlation = SavedReport::Correlations { request: crate::CorrelationRequest::new(vec![0, 1]) };
        assert_eq!(correlation.axis_drop(&t, "y", "z"), None);
    }

    #[test]
    fn switching_changes_only_the_slot_column() {
        let t = table();
        let report = linear();
        assert_eq!(report.switch_slot(), Some(SwitchSlot { role: "Response", column: 0 }));
        // Numeric columns not used as predictors.
        assert_eq!(report.switch_candidates(&t), vec![0, 3]);
        let SavedReport::Linear { spec, .. } = report.switched_to(3, &t).unwrap() else { panic!() };
        assert_eq!((spec.response, spec.predictors), (3, vec![1]));
        assert!(report.switched_to(1, &t).is_none(), "x is a predictor");
        let pca = SavedReport::Pca { request: crate::PcaRequest { columns: vec![0, 1], standardize: true } };
        assert!(pca.switch_slot().is_none() && pca.switch_candidates(&t).is_empty());
        let dist = SavedReport::Distribution {
            request: DistributionRequest {
                column: 2,
                quantile_method: stat_core::descriptive::QuantileMethod::default(), censoring: None },
            options: DistributionOptions::default(),
        };
        assert_eq!(dist.switch_candidates(&t), vec![2], "text columns switch among text columns");
    }

    #[test]
    fn capability_switching_remembers_limits_per_column() {
        let t = table();
        let a = SpecLimits { lower: Some(0.0), upper: Some(7.0), target: None };
        let report = SavedReport::Capability { request: CapabilityRequest::new(0, a) };
        let on_z = report.switched_to(3, &t).unwrap();
        let SavedReport::Capability { request } = on_z.unscoped() else { panic!() };
        assert_eq!((request.column, request.limits), (3, a), "a new column keeps the limits shown");
        // Set z's own limits, step back to y, then to z again.
        let b = SpecLimits { lower: Some(1.0), upper: Some(9.0), target: Some(5.0) };
        let (mut inner, scope) = on_z.split_scope();
        if let SavedReport::Capability { request } = &mut inner {
            request.limits = b;
        }
        let on_z = inner.with_scope(scope);
        let back = on_z.switched_to(0, &t).unwrap();
        let SavedReport::Capability { request } = back.unscoped() else { panic!() };
        assert_eq!(request.limits, a);
        let again = back.switched_to(3, &t).unwrap();
        let SavedReport::Capability { request } = again.unscoped() else { panic!() };
        assert_eq!(request.limits, b);
    }

    #[test]
    fn rename_follows_filter_and_switcher_columns() {
        let mut s = ReportScope {
            filter: vec![FilterCondition::Range { column: "x".into(), min: 0.0, max: 1.0 }],
            switcher: Some(vec!["x".into(), "y".into()]),
            spec_limits: vec![("x".into(), SpecLimits::default())],
            pool: Some("x".into()),
            contour: None,
            plot_styles: Vec::new(),
        };
        s.rename("x", "dose");
        assert_eq!(s.filter_columns(), vec!["dose", "dose"]);
        assert_eq!(s.pool.as_deref(), Some("dose"));
        assert_eq!(s.switcher.as_deref(), Some(&["dose".to_owned(), "y".to_owned()][..]));
        assert_eq!(s.spec_limits[0].0, "dose");
    }
}
