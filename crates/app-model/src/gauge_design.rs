//! The layout of a gauge R&R study: crossed, nested (destructive tests) or
//! expanded (extra factors, fitted by REML), and the column that gives the
//! run order for the gauge run chart.
//!
//! * **Crossed** — every operator measures every part (balanced; ANOVA method,
//!   [`stat_quality::gauge_rr_anova`]).
//! * **Nested** — each operator measures their own parts, as in a destructive
//!   test (balanced; ANOVA method, [`stat_quality::gauge_rr_nested`]). A part
//!   is identified by its label together with its operator, so parts may be
//!   numbered 1, 2, 3… within each operator.
//! * **Expanded** — parts and operators plus extra factor columns (gauge,
//!   fixture, location, day…), each fixed or random and crossed or nested
//!   in parts or operators; parts may be nested in operators; unbalanced data
//!   and missing cells are allowed ([`stat_quality::gauge_rr_expanded`]).

use crate::project::{invalid, member, s};
use stat_core::Result;
use stat_data::{DataTable, JsonValue};

/// How the study is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GaugeStudyKind {
    /// Every operator measures every part.
    #[default]
    Crossed,
    /// Each operator measures their own parts (destructive tests).
    Nested,
    /// Extra factors, fixed or random, crossed or nested; fitted by REML.
    Expanded,
}

impl GaugeStudyKind {
    /// All kinds, in menu order.
    pub const ALL: [Self; 3] = [Self::Crossed, Self::Nested, Self::Expanded];

    /// Project-file key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Crossed => "crossed",
            Self::Nested => "nested",
            Self::Expanded => "expanded",
        }
    }

    /// Display label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Crossed => "Crossed (every operator measures every part)",
            Self::Nested => "Nested (destructive: parts within operators)",
            Self::Expanded => "Expanded (extra factors, unbalanced data)",
        }
    }

    /// The kind with project-file key `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// Where an extra factor sits relative to parts and operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GaugeFactorNesting {
    /// Crossed with parts and operators.
    #[default]
    Crossed,
    /// Nested within parts (e.g. a location on the part).
    WithinPart,
    /// Nested within operators (e.g. each operator's own gauge).
    WithinOperator,
}

impl GaugeFactorNesting {
    /// All choices, in menu order.
    pub const ALL: [Self; 3] = [Self::Crossed, Self::WithinPart, Self::WithinOperator];

    /// Project-file key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Crossed => "crossed",
            Self::WithinPart => "within-part",
            Self::WithinOperator => "within-operator",
        }
    }

    /// Display label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Crossed => "Crossed",
            Self::WithinPart => "Within part",
            Self::WithinOperator => "Within operator",
        }
    }

    /// The choice with project-file key `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// An extra factor of an expanded study.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GaugeExtraFactor {
    /// The factor's column (text or numeric codes).
    pub column: usize,
    /// Random (a variance component) or fixed (tested for mean shifts).
    pub random: bool,
    /// Crossed with, or nested within, parts or operators.
    pub nesting: GaugeFactorNesting,
}

/// Study layout options of a gauge R&R request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GaugeStudyDesign {
    /// Crossed, nested or expanded.
    pub kind: GaugeStudyKind,
    /// Expanded studies: parts are nested within operators (destructive).
    pub parts_within_operators: bool,
    /// Expanded studies: the extra factors.
    pub factors: Vec<GaugeExtraFactor>,
    /// Expanded studies: include two-factor interactions of factors not nested
    /// in one another (part × operator, part × gauge, …).
    pub interactions: bool,
    /// Optional column ordering the runs on the gauge run chart (time stamps
    /// or run numbers); row order when absent.
    pub run_order: Option<usize>,
}

impl Default for GaugeStudyDesign {
    fn default() -> Self {
        Self {
            kind: GaugeStudyKind::Crossed,
            parts_within_operators: false,
            factors: Vec::new(),
            interactions: true,
            run_order: None,
        }
    }
}

impl GaugeStudyDesign {
    /// A nested (destructive) study with default options.
    #[must_use]
    pub fn nested() -> Self {
        Self { kind: GaugeStudyKind::Nested, ..Self::default() }
    }

    /// Columns the design refers to (extra factors, run order).
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let extra = if self.kind == GaugeStudyKind::Expanded { self.factors.as_slice() } else { &[] };
        extra.iter().map(|f| f.column).chain(self.run_order).collect()
    }

    /// Renumber columns by `f`.
    pub fn map_columns(&mut self, f: impl Fn(usize) -> usize) {
        self.factors.iter_mut().for_each(|x| x.column = f(x.column));
        self.run_order = self.run_order.map(&f);
    }

    /// Project-file form of the extra factors: `[{ column, random, nesting }]`.
    #[must_use]
    pub fn factors_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        JsonValue::Array(
            self.factors
                .iter()
                .map(|f| {
                    JsonValue::Object(vec![
                        ("column".into(), col(f.column)),
                        ("random".into(), JsonValue::Bool(f.random)),
                        ("nesting".into(), s(f.nesting.key())),
                    ])
                })
                .collect(),
        )
    }

    /// Project-file members of the gauge study's object: `design`,
    /// `parts_within_operators`, `interactions`, `factors` (see
    /// [`factors_json`](Self::factors_json)) and `run_order` (a column or
    /// null).
    #[must_use]
    pub fn json_members(&self, table: &DataTable) -> Vec<(String, JsonValue)> {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        vec![
            ("design".into(), s(self.kind.key())),
            ("parts_within_operators".into(), JsonValue::Bool(self.parts_within_operators)),
            ("interactions".into(), JsonValue::Bool(self.interactions)),
            ("factors".into(), self.factors_json(table)),
            ("run_order".into(), self.run_order.map_or(JsonValue::Null, col)),
        ]
    }

    /// Read the members written by [`json_members`](Self::json_members);
    /// absent members keep their defaults, so older projects open as crossed
    /// studies.
    ///
    /// # Errors
    /// An unknown design or nesting key, or a column the table does not have.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let d = Self::default();
        let find = |name: Option<&str>| -> Result<usize> {
            let name = name.ok_or_else(|| invalid("gauge study: expected a column name"))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let kind = match v.get("design").and_then(JsonValue::as_str) {
            None => d.kind,
            Some(k) => {
                GaugeStudyKind::from_key(k).ok_or_else(|| invalid(format!("gauge study: unknown design {k:?}")))?
            }
        };
        let mut factors = Vec::new();
        if let Some(list) = v.get("factors").and_then(JsonValue::as_array) {
            for f in list {
                let nesting = match f.get("nesting").and_then(JsonValue::as_str) {
                    None => GaugeFactorNesting::Crossed,
                    Some(k) => GaugeFactorNesting::from_key(k)
                        .ok_or_else(|| invalid(format!("gauge study: unknown nesting {k:?}")))?,
                };
                factors.push(GaugeExtraFactor {
                    column: find(member(f, "column")?.as_str())?,
                    random: f.get("random").and_then(JsonValue::as_bool).unwrap_or(true),
                    nesting,
                });
            }
        }
        let run_order = match v.get("run_order") {
            None | Some(JsonValue::Null) => None,
            Some(c) => Some(find(c.as_str())?),
        };
        Ok(Self {
            kind,
            parts_within_operators: v
                .get("parts_within_operators")
                .and_then(JsonValue::as_bool)
                .unwrap_or(d.parts_within_operators),
            factors,
            interactions: v.get("interactions").and_then(JsonValue::as_bool).unwrap_or(d.interactions),
            run_order,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, SavedReport, load_project, save_project};
    use crate::sample;
    use stat_data::RowStateManager;

    #[test]
    fn expanded_and_nested_requests_round_trip_through_projects() {
        for prefix in ["Bore", "Weld"] {
            let s = (sample::catalog().into_iter().find(|s| s.name.starts_with(prefix)).unwrap().build)();
            let project = Project {
                external: None,
                origin: None,
                scripts: Vec::new(),
                rows: RowStateManager::new(s.table.n_rows()),
                table: s.table,
                reports: s.reports,
            };
            let back = load_project(&save_project(&project)).unwrap();
            assert_eq!(back.reports, project.reports);
        }
    }

    #[test]
    fn missing_members_read_as_a_crossed_study_and_columns_map() {
        let t = sample::bore_diameter();
        let d = GaugeStudyDesign::from_json(&JsonValue::Object(Vec::new()), &t).unwrap();
        assert_eq!(d, GaugeStudyDesign::default());
        let mut e = GaugeStudyDesign {
            kind: GaugeStudyKind::Expanded,
            factors: vec![GaugeExtraFactor { column: 3, random: false, nesting: GaugeFactorNesting::WithinPart }],
            run_order: Some(0),
            ..GaugeStudyDesign::default()
        };
        let members = JsonValue::Object(e.json_members(&t));
        assert_eq!(GaugeStudyDesign::from_json(&members, &t).unwrap(), e);
        assert_eq!(e.columns(), [3, 0]);
        e.map_columns(|c| c + 1);
        assert_eq!(e.columns(), [4, 1]);
        // Extra factors are not used unless the study is expanded.
        e.kind = GaugeStudyKind::Nested;
        assert_eq!(e.columns(), [1]);
        let report = SavedReport::Gauge {
            request: crate::GaugeStudyRequest { design: e, ..crate::GaugeStudyRequest::new(1, 2, 4) },
            options: crate::GaugeReportOptions::default(),
        };
        assert_eq!(report.columns(), [1, 2, 4, 1]);
    }
}
