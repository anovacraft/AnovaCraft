//! Stepwise selection of linear-model effects ([`stat_linear::stepwise`]),
//! bound to the dataset's row state.
//!
//! The candidate model (predictors and effects, as in a linear model) is coded
//! once on the rows used, without fitting it, so the candidates may outnumber
//! the observations or be aliased with one another; the search only visits
//! estimable models. The selected model is then fitted and reported like any
//! linear model.

use crate::format::{format_number, format_p_value};
use crate::linear_analysis::{Coded, LinearModelAnalysis, LinearModelSpec};
use crate::report::{ReportOptions, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::{
    Heredity, StepAction, StepCriterion, StepDirection, StepwiseOptions, StepwiseResult, hierarchical_closure,
    parent_indices, stepwise_selection_with_parents,
};

/// What to search.
#[derive(Debug, Clone, PartialEq)]
pub struct StepwiseRequest {
    /// Response, predictors and candidate effects (empty effects: main effects).
    pub candidates: LinearModelSpec,
    /// Search direction.
    pub direction: StepDirection,
    /// Selection criterion.
    pub criterion: StepCriterion,
    /// Parent effects an interaction or power needs in the model (strong:
    /// all; weak: one). Missing parents are added to the candidates.
    pub heredity: Heredity,
}

/// A completed search and the selected model.
#[derive(Debug, Clone)]
pub struct StepwiseAnalysis {
    request: StepwiseRequest,
    effect_names: Vec<String>,
    result: StepwiseResult,
    /// The selected model (`None` when no effect was selected).
    selected: Option<LinearModelAnalysis>,
    response_name: String,
    /// Rows the selected model uses that the search could not (missing only
    /// in predictors that were not selected).
    rows_added: usize,
    /// Parent effects added to the candidates for heredity.
    parents_added: Vec<String>,
}

/// A search's direction, criterion (with its p-value thresholds) and
/// heredity as project-file members.
pub(crate) fn search_to_json(
    direction: StepDirection,
    criterion: StepCriterion,
    heredity: Heredity,
) -> Vec<(&'static str, JsonValue)> {
    let s = |v: &str| JsonValue::String(v.to_owned());
    let mut members = vec![(
        "direction",
        s(match direction {
            StepDirection::Forward => "forward",
            StepDirection::Backward => "backward",
            StepDirection::Mixed => "mixed",
        }),
    )];
    match criterion {
        StepCriterion::PValue { enter, remove } => {
            members.push(("criterion", s("p-value")));
            members.push(("enter", JsonValue::number(enter)));
            members.push(("remove", JsonValue::number(remove)));
        }
        StepCriterion::Aic => members.push(("criterion", s("aic"))),
        StepCriterion::Bic => members.push(("criterion", s("bic"))),
    }
    members.push(("heredity", s(heredity.key())));
    members
}

/// A search's direction, criterion and heredity from the members of `v`
/// written by [`search_to_json`]; missing members are mixed, p-values with
/// 0.15 thresholds, and `heredity` (searches saved before heredity existed
/// were unrestricted).
pub(crate) fn search_from_json(v: &JsonValue, heredity: Heredity) -> Result<(StepDirection, StepCriterion, Heredity)> {
    let invalid = |m: String| stat_core::StatError::InvalidArgument { message: m };
    let direction = match v.get("direction").and_then(JsonValue::as_str) {
        Some("forward") => StepDirection::Forward,
        Some("backward") => StepDirection::Backward,
        None | Some("mixed") => StepDirection::Mixed,
        Some(other) => return Err(invalid(format!("stepwise: unknown direction {other:?}"))),
    };
    let num = |key: &str, d: f64| v.get(key).and_then(JsonValue::as_f64).unwrap_or(d);
    let criterion = match v.get("criterion").and_then(JsonValue::as_str) {
        None | Some("p-value") => StepCriterion::PValue { enter: num("enter", 0.15), remove: num("remove", 0.15) },
        Some("aic") => StepCriterion::Aic,
        Some("bic") => StepCriterion::Bic,
        Some(other) => return Err(invalid(format!("stepwise: unknown criterion {other:?}"))),
    };
    let heredity = match v.get("heredity").and_then(JsonValue::as_str) {
        None => heredity,
        Some(k) => Heredity::from_key(k).ok_or_else(|| invalid(format!("stepwise: unknown heredity {k:?}")))?,
    };
    Ok((direction, criterion, heredity))
}

/// How a search is described in report text, e.g. "mixed stepwise by AIC,
/// strong heredity".
#[must_use]
pub fn search_description(direction: StepDirection, criterion: StepCriterion, heredity: Heredity) -> String {
    let by = match criterion {
        StepCriterion::PValue { enter, remove } => format!("p-values (enter < {enter}, remove > {remove})"),
        StepCriterion::Aic => "AIC".to_owned(),
        StepCriterion::Bic => "BIC".to_owned(),
    };
    let direction = match direction {
        StepDirection::Forward => "forward",
        StepDirection::Backward => "backward",
        StepDirection::Mixed => "mixed",
    };
    let heredity = match heredity {
        Heredity::Strong => "strong heredity",
        Heredity::Weak => "weak heredity",
        Heredity::None => "no heredity rule",
    };
    format!("{direction} stepwise by {by}, {heredity}")
}

/// The selected model: only the predictors its effects use, with the effects'
/// predictor positions renumbered to match.
fn selected_spec(candidates: &LinearModelSpec, effects: &[Vec<(usize, u8)>]) -> LinearModelSpec {
    let mut used: Vec<usize> = effects.iter().flatten().map(|&(p, _)| p).collect();
    used.sort_unstable();
    used.dedup();
    let position = |p: usize| used.binary_search(&p).expect("listed above");
    LinearModelSpec {
        response: candidates.response,
        predictors: used.iter().map(|&p| candidates.predictors[p]).collect(),
        effects: effects.iter().map(|e| e.iter().map(|&(p, k)| (position(p), k)).collect()).collect(),
        splines: Vec::new(),
    }
}

impl StepwiseAnalysis {
    /// Run the search and fit the selected model.
    ///
    /// # Errors
    /// Errors from coding the candidate model (invalid columns, a single-level
    /// category), from the search options, and from fitting the selected model.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &StepwiseRequest) -> Result<Self> {
        Self::run_with(table, states, request, false)
    }

    /// As [`StepwiseAnalysis::run`], but the selected model keeps every
    /// candidate predictor (those no selected effect uses included), so it
    /// uses the same rows as the search and its factors line up with other
    /// responses' models over the same predictors.
    ///
    /// # Errors
    /// As [`StepwiseAnalysis::run`].
    pub fn run_keeping_predictors(
        table: &DataTable,
        states: &RowStateManager,
        request: &StepwiseRequest,
    ) -> Result<Self> {
        Self::run_with(table, states, request, true)
    }

    fn run_with(table: &DataTable, states: &RowStateManager, request: &StepwiseRequest, keep: bool) -> Result<Self> {
        // Heredity needs every parent among the candidates.
        let given = request.candidates.effective_effects();
        let all = if request.heredity == Heredity::None { given.clone() } else { hierarchical_closure(&given) };
        let candidates = LinearModelSpec { effects: all.clone(), ..request.candidates.clone() };
        let coded = Coded::new(table, states, &candidates)?;
        let ranges: Vec<_> = coded.design.effects().iter().map(|e| e.columns.clone()).collect();
        let effect_names: Vec<String> = coded.design.effects().iter().map(|e| e.name.clone()).collect();
        let parents_added: Vec<String> =
            all.iter().zip(&effect_names).filter(|(e, _)| !given.contains(e)).map(|(_, n)| n.clone()).collect();
        let options = StepwiseOptions {
            direction: request.direction,
            criterion: request.criterion,
            max_steps: 200,
            heredity: request.heredity,
        };
        let result = stepwise_selection_with_parents(&coded.x, &coded.y, &ranges, &parent_indices(&all), &options)?;
        let selected = if result.selected.is_empty() {
            None
        } else {
            let chosen: Vec<Vec<(usize, u8)>> = result.selected.iter().map(|&i| all[i].clone()).collect();
            let spec = if keep {
                LinearModelSpec { effects: chosen, splines: Vec::new(), ..request.candidates.clone() }
            } else {
                selected_spec(&request.candidates, &chosen)
            };
            Some(LinearModelAnalysis::run(table, states, &spec)?)
        };
        let rows_added = selected.as_ref().map_or(0, |m| m.rows_used().len().saturating_sub(coded.y.len()));
        Ok(Self {
            request: request.clone(),
            effect_names,
            result,
            selected,
            response_name: coded.response_name,
            rows_added,
            parents_added,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &StepwiseRequest {
        &self.request
    }

    /// Names of the candidate effects (parents added for heredity included).
    #[must_use]
    pub fn effect_names(&self) -> &[String] {
        &self.effect_names
    }

    /// Names of the selected effects, in candidate order.
    #[must_use]
    pub fn selected_effect_names(&self) -> Vec<String> {
        let mut chosen = self.result.selected.clone();
        chosen.sort_unstable();
        chosen.into_iter().map(|i| self.effect_names[i].clone()).collect()
    }

    /// The search: steps taken, the selected effects and the criterion.
    #[must_use]
    pub fn result(&self) -> &StepwiseResult {
        &self.result
    }

    /// Name of the response.
    #[must_use]
    pub fn response_name(&self) -> &str {
        &self.response_name
    }

    /// Parent effects added to the candidates for heredity.
    #[must_use]
    pub fn parents_added(&self) -> &[String] {
        &self.parents_added
    }

    /// The selected model, if any effect was selected.
    #[must_use]
    pub fn selected(&self) -> Option<&LinearModelAnalysis> {
        self.selected.as_ref()
    }

    /// `true` if row exclusion has changed since the search.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        self.selected.as_ref().is_some_and(|s| s.is_stale(states))
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Stepwise Selection: {}", self.response_name)
    }

    /// Report sections: settings, step history, then the selected model's fit
    /// statistics, parameter estimates and effect tests.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.settings(), self.history()];
        match &self.selected {
            Some(model) => {
                let options = ReportOptions {
                    show_fit_plot: false,
                    show_residual_plot: false,
                    show_diagnostics: false,
                    show_anova: true,
                    ..ReportOptions::default()
                };
                for mut s in model.sections(&options) {
                    s.title = format!("Selected Model: {}", s.title);
                    out.push(s);
                }
            }
            None => out.push(ReportSection {
                kind: SectionKind::FitStatistics,
                title: "Selected Model".to_owned(),
                content: SectionContent::Note(
                    "No effect met the entry criterion: the selected model is the mean only.".to_owned(),
                ),
            }),
        }
        out
    }

    pub(crate) fn criterion_name(&self) -> &'static str {
        match self.request.criterion {
            StepCriterion::Bic => "BIC",
            _ => "AIC",
        }
    }

    fn settings(&self) -> ReportSection {
        let mut t = TextTable::new(["Setting", "Value"]);
        t.push(["Direction".to_owned(), self.request.direction.label().to_owned()]);
        let criterion = match self.request.criterion {
            StepCriterion::PValue { enter, remove } => format!("p-value (enter < {enter}, remove > {remove})"),
            StepCriterion::Aic => "AIC (lowest wins)".to_owned(),
            StepCriterion::Bic => "BIC (lowest wins)".to_owned(),
        };
        t.push(["Criterion".to_owned(), criterion]);
        t.push(["Heredity".to_owned(), self.request.heredity.label().to_owned()]);
        if !self.parents_added.is_empty() {
            t.push(["Parent effects added to the candidates".to_owned(), self.parents_added.join(", ")]);
        }
        t.push(["Candidate effects".to_owned(), self.effect_names.join(", ")]);
        let chosen: Vec<&str> = self.result.selected.iter().map(|&i| self.effect_names[i].as_str()).collect();
        t.push(["Selected effects".to_owned(), if chosen.is_empty() { "none".to_owned() } else { chosen.join(", ") }]);
        if self.rows_added > 0 {
            t.push([
                "Rows".to_owned(),
                format!(
                    "the selected model also uses {} row(s) the search could not (missing only in unselected predictors)",
                    self.rows_added
                ),
            ]);
        }
        if !self.result.set_aside.is_empty() {
            let names: Vec<&str> = self.result.set_aside.iter().map(|&i| self.effect_names[i].as_str()).collect();
            t.push([
                "Set aside (full model not estimable)".to_owned(),
                format!("{} — aliased with earlier effects, or too few rows", names.join(", ")),
            ]);
        }
        t.push([
            format!("{} of the starting model", self.criterion_name()),
            format_number(self.result.start_criterion, 6),
        ]);
        ReportSection {
            kind: SectionKind::StepwiseSettings,
            title: "Stepwise Settings (p-values of the selected model do not account for the search)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    /// The "Step History" section: each step's action, effect, p-value,
    /// criterion and R².
    pub(crate) fn history(&self) -> ReportSection {
        let mut t = TextTable::new([
            "Step".to_owned(),
            "Action".to_owned(),
            "Effect".to_owned(),
            "p-value".to_owned(),
            self.criterion_name().to_owned(),
            "R²".to_owned(),
        ]);
        for (i, s) in self.result.steps.iter().enumerate() {
            t.push([
                (i + 1).to_string(),
                match s.action {
                    StepAction::Enter => "Entered",
                    StepAction::Remove => "Removed",
                }
                .to_owned(),
                self.effect_names[s.effect].clone(),
                format_p_value(s.p_value),
                format_number(s.criterion, 6),
                format_number(s.r_squared, 5),
            ]);
        }
        let content = if t.rows.is_empty() {
            SectionContent::Note("No step improved the model.".to_owned())
        } else {
            SectionContent::Table(t)
        };
        ReportSection { kind: SectionKind::StepHistory, title: "Step History".to_owned(), content }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn hald() -> DataTable {
        let cols = [
            ("x1", vec![7.0, 1.0, 11.0, 11.0, 7.0, 11.0, 3.0, 1.0, 2.0, 21.0, 1.0, 11.0, 10.0]),
            ("x2", vec![26.0, 29.0, 56.0, 31.0, 52.0, 55.0, 71.0, 31.0, 54.0, 47.0, 40.0, 66.0, 68.0]),
            ("x3", vec![6.0, 15.0, 8.0, 8.0, 6.0, 9.0, 17.0, 22.0, 18.0, 4.0, 23.0, 9.0, 8.0]),
            ("x4", vec![60.0, 52.0, 20.0, 47.0, 33.0, 22.0, 6.0, 44.0, 22.0, 26.0, 34.0, 12.0, 12.0]),
            ("heat", vec![78.5, 74.3, 104.3, 87.6, 95.9, 109.2, 102.7, 72.5, 93.1, 115.9, 83.8, 113.3, 109.4]),
        ];
        DataTable::new("hald", cols.into_iter().map(|(n, v)| Column::numeric(n, v)).collect()).unwrap()
    }

    #[test]
    fn stepwise_report() {
        let t = hald();
        let rows = RowStateManager::new(13);
        let request = StepwiseRequest {
            candidates: LinearModelSpec {
                response: 4,
                predictors: vec![0, 1, 2, 3],
                effects: Vec::new(),
                splines: Vec::new(),
            },
            direction: StepDirection::Mixed,
            criterion: StepCriterion::PValue { enter: 0.15, remove: 0.15 },
            heredity: Heredity::Strong,
        };
        let a = StepwiseAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.title(), "Stepwise Selection: heat");
        assert_eq!(a.selected().unwrap().title(), "Linear Model: heat ~ x1 + x2");
        assert_eq!(a.selected().unwrap().spec().predictors, [0, 1], "unselected predictors are dropped");
        let s = a.sections();
        let SectionContent::Table(h) = &s[1].content else { panic!() };
        let actions: Vec<(&str, &str)> = h.rows.iter().map(|r| (r[1].as_str(), r[2].as_str())).collect();
        assert_eq!(actions, [("Entered", "x4"), ("Entered", "x1"), ("Entered", "x2"), ("Removed", "x4")]);
        assert!(s.iter().any(|s| s.title.starts_with("Selected Model: Parameter Estimates")));
        // AIC with interactions among the candidates: still runs; candidates listed.
        let with_interactions = StepwiseRequest {
            candidates: LinearModelSpec {
                effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(3, 1)], vec![(0, 1), (1, 1)]],
                ..request.candidates.clone()
            },
            criterion: StepCriterion::Aic,
            ..request.clone()
        };
        let b = StepwiseAnalysis::run(&t, &rows, &with_interactions).unwrap();
        let SectionContent::Table(settings) = &b.sections()[0].content else { panic!() };
        assert_eq!(settings.rows[3][1], "x1, x2, x4, x1 × x2");
    }

    /// The candidate set that prompted this: catalyst main effects, all
    /// interactions and both squares, where the two squares are aliased.
    #[test]
    fn aliased_candidates_do_not_stop_the_search() {
        let t = crate::sample::catalyst();
        let rows = RowStateManager::new(t.n_rows());
        let effects = vec![
            vec![(0, 1)],
            vec![(1, 1)],
            vec![(2, 1)],
            vec![(0, 1), (1, 1)],
            vec![(0, 1), (2, 1)],
            vec![(1, 1), (2, 1)],
            vec![(0, 2)],
            vec![(1, 2)],
        ];
        let candidates = LinearModelSpec { response: 3, predictors: vec![0, 1, 2], effects, splines: Vec::new() };
        assert!(LinearModelAnalysis::run(&t, &rows, &candidates).is_err(), "the full model is rank deficient");
        for direction in [StepDirection::Forward, StepDirection::Mixed, StepDirection::Backward] {
            let request = StepwiseRequest {
                candidates: candidates.clone(),
                direction,
                criterion: StepCriterion::Aic,
                heredity: Heredity::Strong,
            };
            let a = StepwiseAnalysis::run(&t, &rows, &request).unwrap();
            let model = a.selected().unwrap().title();
            assert!(model.contains("Temperature × Pressure"), "{direction:?}: {model}");
            assert!(!(model.contains("Temperature²") && model.contains("Pressure²")), "{model}");
            let SectionContent::Table(settings) = &a.sections()[0].content else { panic!() };
            let set_aside = settings.rows.iter().any(|r| r[0].starts_with("Set aside"));
            assert_eq!(set_aside, direction == StepDirection::Backward, "{direction:?}");
        }
    }

    #[test]
    fn selected_spec_keeps_only_used_predictors() {
        let candidates =
            LinearModelSpec { response: 9, predictors: vec![4, 5, 6, 7], effects: Vec::new(), splines: Vec::new() };
        let spec = selected_spec(&candidates, &[vec![(3, 1)], vec![(1, 1), (3, 1)], vec![(1, 2)]]);
        assert_eq!(spec.predictors, [5, 7]);
        assert_eq!(spec.effects, [vec![(1, 1)], vec![(0, 1), (1, 1)], vec![(0, 2)]]);
        assert_eq!(spec.response, 9);
    }

    #[test]
    fn nothing_selected() {
        // Pure noise against a response with p-to-enter tiny: the mean only.
        let t = hald();
        let rows = RowStateManager::new(13);
        let request = StepwiseRequest {
            candidates: LinearModelSpec { response: 4, predictors: vec![2], effects: Vec::new(), splines: Vec::new() },
            direction: StepDirection::Forward,
            criterion: StepCriterion::PValue { enter: 1e-9, remove: 1e-9 },
            heredity: Heredity::Strong,
        };
        let a = StepwiseAnalysis::run(&t, &rows, &request).unwrap();
        assert!(a.selected().is_none());
        assert!(matches!(a.sections()[2].content, SectionContent::Note(_)));
    }
}
