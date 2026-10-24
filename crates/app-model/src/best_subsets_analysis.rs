//! Best-subsets regression ([`stat_linear::best_subsets`]) bound to the
//! dataset's row state.
//!
//! The candidate model (predictors and effects, as in a linear model) is coded
//! once on the rows used, and every subset of the candidate effects is
//! searched by branch and bound for the best models of each size. The report
//! lists them with R², adjusted R², Mallows' Cp, AICc and BIC, marks the best
//! overall by the chosen criterion, and plots that criterion against size.
//! Any listed model opens as a full linear-model report.

use crate::format::format_number;
use crate::linear_analysis::{Coded, LinearModelSpec};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_linear::{
    BestSubsetsOptions, BestSubsetsResult, Heredity, SubsetCriterion, SubsetModel, best_subsets, hierarchical_closure,
    parent_indices,
};

/// What to search.
#[derive(Debug, Clone, PartialEq)]
pub struct BestSubsetsRequest {
    /// Response, predictors and candidate effects (empty effects: main effects).
    pub candidates: LinearModelSpec,
    /// Effects in every model, written like the candidates' effects
    /// (predictor positions and powers); each must be a candidate.
    pub forced: Vec<Vec<(usize, u8)>>,
    /// Parent effects an interaction or power needs in a model (strong: all;
    /// weak: one). Missing parents are added to the candidates.
    pub heredity: Heredity,
    /// Models kept for each size.
    pub models_per_size: usize,
    /// Largest number of effects in a model (`None`: every candidate).
    pub max_size: Option<usize>,
    /// Criterion that picks the best model overall.
    pub criterion: SubsetCriterion,
}

impl BestSubsetsRequest {
    /// Default settings for `candidates`: three models per size, strong
    /// heredity, best overall by BIC.
    #[must_use]
    pub fn new(candidates: LinearModelSpec) -> Self {
        Self {
            candidates,
            forced: Vec::new(),
            heredity: Heredity::Strong,
            models_per_size: 3,
            max_size: None,
            criterion: SubsetCriterion::Bic,
        }
    }
}

/// A completed search.
#[derive(Debug, Clone)]
pub struct BestSubsetsAnalysis {
    request: BestSubsetsRequest,
    /// Every candidate effect (with parents added for heredity).
    effects: Vec<Vec<(usize, u8)>>,
    effect_names: Vec<String>,
    result: BestSubsetsResult,
    response_name: String,
    parents_added: Vec<String>,
    exclusion_revision: u64,
}

/// A model of `effects` using only the predictors those effects use, with
/// predictor positions renumbered to match.
fn model_spec(candidates: &LinearModelSpec, effects: &[Vec<(usize, u8)>]) -> LinearModelSpec {
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

fn criterion_value(c: SubsetCriterion, m: &SubsetModel) -> f64 {
    match c {
        SubsetCriterion::RSquared => m.r_squared,
        SubsetCriterion::AdjustedRSquared => m.adj_r_squared,
        SubsetCriterion::Cp => m.cp,
        SubsetCriterion::Aicc => m.aicc,
        SubsetCriterion::Bic => m.bic,
    }
}

/// Every criterion, in report order.
pub const SUBSET_CRITERIA: [SubsetCriterion; 5] = [
    SubsetCriterion::RSquared,
    SubsetCriterion::AdjustedRSquared,
    SubsetCriterion::Cp,
    SubsetCriterion::Aicc,
    SubsetCriterion::Bic,
];

impl BestSubsetsAnalysis {
    /// Run the search.
    ///
    /// # Errors
    /// Errors from coding the candidate model (invalid columns, a
    /// single-level category), a forced effect that is not a candidate, and
    /// the search options.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &BestSubsetsRequest) -> Result<Self> {
        let given = request.candidates.effective_effects();
        let all = if request.heredity == Heredity::None { given.clone() } else { hierarchical_closure(&given) };
        let candidates = LinearModelSpec { effects: all.clone(), ..request.candidates.clone() };
        let coded = Coded::new(table, states, &candidates)?;
        let ranges: Vec<_> = coded.design.effects().iter().map(|e| e.columns.clone()).collect();
        let effect_names: Vec<String> = coded.design.effects().iter().map(|e| e.name.clone()).collect();
        let parents_added: Vec<String> =
            all.iter().zip(&effect_names).filter(|(e, _)| !given.contains(e)).map(|(_, n)| n.clone()).collect();
        let forced = request
            .forced
            .iter()
            .map(|f| {
                all.iter().position(|e| e == f).ok_or_else(|| StatError::InvalidArgument {
                    message: "a forced effect must be one of the candidate effects".to_owned(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let options = BestSubsetsOptions {
            models_per_size: request.models_per_size,
            max_size: request.max_size,
            forced,
            heredity: request.heredity,
        };
        let result = best_subsets(&coded.x, &coded.y, &ranges, &parent_indices(&all), &options)?;
        Ok(Self {
            request: request.clone(),
            effects: all,
            effect_names,
            result,
            response_name: coded.response_name,
            parents_added,
            exclusion_revision: states.exclusion_revision(),
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &BestSubsetsRequest {
        &self.request
    }

    /// `true` if row exclusion has changed since the search.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Best Subsets: {}", self.response_name)
    }

    /// The models found, by size and then residual sum of squares.
    #[must_use]
    pub fn models(&self) -> &[SubsetModel] {
        &self.result.models
    }

    /// Position in [`Self::models`] of the best model by the request's criterion.
    #[must_use]
    pub fn best_index(&self) -> Option<usize> {
        let best = self.result.best_by(self.request.criterion)?;
        self.result.models.iter().position(|m| std::ptr::eq(m, best))
    }

    /// Effect names of model `i`, joined with " + ".
    #[must_use]
    pub fn model_label(&self, i: usize) -> String {
        self.result.models.get(i).map_or_else(String::new, |m| {
            m.effects.iter().map(|&e| self.effect_names[e].as_str()).collect::<Vec<_>>().join(" + ")
        })
    }

    /// Model `i` as a linear model.
    #[must_use]
    pub fn model_spec(&self, i: usize) -> Option<LinearModelSpec> {
        let m = self.result.models.get(i)?;
        let effects: Vec<Vec<(usize, u8)>> = m.effects.iter().map(|&e| self.effects[e].clone()).collect();
        Some(model_spec(&self.request.candidates, &effects))
    }

    /// Report sections: settings, the best models by size, and the chosen
    /// criterion against size.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![self.settings(), self.models_table(), self.criterion_plot()]
    }

    fn settings(&self) -> ReportSection {
        let r = &self.request;
        let mut t = TextTable::new(["Setting", "Value"]);
        t.push(["Candidate effects".to_owned(), self.effect_names.join(", ")]);
        if !self.parents_added.is_empty() {
            t.push(["Parent effects added to the candidates".to_owned(), self.parents_added.join(", ")]);
        }
        let forced: Vec<&str> = r
            .forced
            .iter()
            .filter_map(|f| self.effects.iter().position(|e| e == f))
            .map(|i| self.effect_names[i].as_str())
            .collect();
        t.push([
            "Effects in every model".to_owned(),
            if forced.is_empty() { "none".to_owned() } else { forced.join(", ") },
        ]);
        t.push(["Heredity".to_owned(), r.heredity.label().to_owned()]);
        t.push(["Models kept per size".to_owned(), r.models_per_size.to_string()]);
        t.push([
            "Largest model size".to_owned(),
            r.max_size.map_or_else(|| format!("{} (every candidate)", self.effects.len()), |m| m.to_string()),
        ]);
        t.push(["Best overall chosen by".to_owned(), r.criterion.label().to_owned()]);
        t.push(["Rows used".to_owned(), self.result.n.to_string()]);
        t.push(["Models fitted by the search".to_owned(), self.result.fits_evaluated.to_string()]);
        if !self.result.sigma_squared.is_finite() {
            t.push([
                "Mallows' Cp".to_owned(),
                "not available: the model with every candidate leaves no error degrees of freedom".to_owned(),
            ]);
        }
        ReportSection {
            kind: SectionKind::BestSubsetsSettings,
            title: "Best Subsets Settings (criteria do not account for the search)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn models_table(&self) -> ReportSection {
        let best = self.best_index();
        let mut t = TextTable::new(["Size", "Effects", "R²", "Adjusted R²", "Cp", "AICc", "BIC", "Best overall"]);
        let num = |x: f64, d: usize| if x.is_finite() { format_number(x, d) } else { "—".to_owned() };
        for (i, m) in self.result.models.iter().enumerate() {
            t.push([
                m.size().to_string(),
                self.model_label(i),
                num(m.r_squared, 5),
                num(m.adj_r_squared, 5),
                num(m.cp, 5),
                num(m.aicc, 6),
                num(m.bic, 6),
                if best == Some(i) { format!("best by {}", self.request.criterion.label()) } else { String::new() },
            ]);
        }
        let content = if t.rows.is_empty() {
            SectionContent::Note("No candidate model could be fitted (aliased effects or too few rows).".to_owned())
        } else {
            SectionContent::Table(t)
        };
        ReportSection { kind: SectionKind::BestSubsetsModels, title: "Best Models by Size".to_owned(), content }
    }

    fn criterion_plot(&self) -> ReportSection {
        let c = self.request.criterion;
        let models = &self.result.models;
        let best = self.best_index();
        let mut items = Vec::new();
        let mut line = Vec::new();
        for (i, m) in models.iter().enumerate() {
            let v = criterion_value(c, m);
            if !v.is_finite() {
                continue;
            }
            let group = usize::from(best == Some(i));
            let label = if group == 1 { "best".to_owned() } else { String::new() };
            items.push((m.size() as f64, v, label, group));
            // The first model of each size has the smallest RSS, so the best
            // value of every criterion at that size.
            if i == 0 || models[i - 1].size() != m.size() {
                line.push([m.size() as f64, v]);
            }
        }
        let mut segments = Vec::new();
        if c == SubsetCriterion::Cp {
            // Cp ≈ p for a model without bias (Mallows 1973).
            if let (Some(lo), Some(hi)) = (models.first(), models.last()) {
                let p = |m: &SubsetModel| m.parameters as f64 - m.size() as f64;
                segments.push(vec![
                    [lo.size() as f64, lo.size() as f64 + p(lo)],
                    [hi.size() as f64, hi.size() as f64 + p(hi)],
                ]);
            }
        }
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: "Number of effects".to_owned(),
            y_label: c.label().to_owned(),
            points: Vec::new(),
            line: Some(line),
            line_name: Some("Best model of each size".to_owned()),
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: Some(Annotations {
                groups: vec!["Model".to_owned(), format!("Best by {}", c.label())],
                items,
                labels: Vec::new(),
            }),
            segments,
            ..PlotSpec::default()
        };
        let note = if c == SubsetCriterion::Cp { "; thin line: Cp = number of coefficients" } else { "" };
        ReportSection {
            kind: SectionKind::BestSubsetsPlot,
            title: format!("{} by Model Size (line: best model of each size{note})", c.label()),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hald_best_subsets() {
        let t = crate::sample::hald();
        let rows = RowStateManager::new(t.n_rows());
        let request = BestSubsetsRequest::new(LinearModelSpec {
            response: 4,
            predictors: vec![0, 1, 2, 3],
            effects: Vec::new(),
            splines: Vec::new(),
        });
        let a = BestSubsetsAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.title(), "Best Subsets: Heat (cal/g)");
        // Best two-effect model: x1 + x2, R² = 0.97868 (Draper & Smith).
        let two = a.models().iter().position(|m| m.size() == 2).unwrap();
        assert_eq!(a.model_label(two), "Tricalcium aluminate (%) + Tricalcium silicate (%)");
        assert!((a.models()[two].r_squared - 0.978_678).abs() < 1e-5);
        let spec = a.model_spec(two).unwrap();
        assert_eq!(spec.predictors, [0, 1]);
        let s = a.sections();
        let SectionContent::Table(models) = &s[1].content else { panic!() };
        assert_eq!(models.rows.iter().filter(|r| r[7].starts_with("best by")).count(), 1);
        assert!(matches!(s[2].content, SectionContent::Plot(_)));
        // Forcing x4 keeps it in every model.
        let forced = BestSubsetsRequest { forced: vec![vec![(3, 1)]], criterion: SubsetCriterion::Cp, ..request };
        let b = BestSubsetsAnalysis::run(&t, &rows, &forced).unwrap();
        assert!(b.models().iter().all(|m| m.effects.contains(&3)));
        assert!(b.sections()[2].title.contains("Cp = number of coefficients"));
    }

    #[test]
    fn forced_effect_must_be_a_candidate() {
        let t = crate::sample::hald();
        let rows = RowStateManager::new(t.n_rows());
        let request = BestSubsetsRequest {
            forced: vec![vec![(0, 2)]],
            ..BestSubsetsRequest::new(LinearModelSpec {
                response: 4,
                predictors: vec![0, 1],
                effects: Vec::new(),
                splines: Vec::new(),
            })
        };
        assert!(BestSubsetsAnalysis::run(&t, &rows, &request).is_err());
    }
}
