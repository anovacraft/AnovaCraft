//! Decision trees bound to a dataset: a classification tree for a
//! categorical response, a regression tree for a continuous one
//! ([`stat_predictive::tree`]), sized by cost-complexity pruning on held-out
//! data, with the shared predictive-model sections ([`crate::prediction`]).
//!
//! **Rows.** Rows that are excluded or miss the response are left out.
//! Missing predictor values are kept: the tree routes them with surrogate
//! splits. Training rows grow the tree; validation rows (or, without them,
//! k-fold cross-validation of the training rows: the method's folds, or 10)
//! choose the pruned size; test rows are used for neither.
//!
//! **Size.** Either the subtree with the lowest held-out error, the
//! smallest subtree within one standard error of it (the default; Breiman
//! et al. 1984, §3.4.3), or the full grown tree.
//!
//! **Predictor types.** Numeric columns with a continuous measurement level
//! split at thresholds; any other column splits on sets of its levels (as
//! the data view shows them). A level a tree did not see while growing is
//! treated like a missing value when predicting.

use stat_core::{Result, StatError};
use stat_data::{ColumnKind, DataTable, JsonValue, RowStateManager};
use stat_linear::TermValue;
use stat_predictive::tree::{Node, Rule, Split};
use stat_predictive::{
    Criterion, CrossValidation, Feature, Role, Target, Tree, TreeOptions, Value, choose_step, cross_validate_pruning,
    fit_tree,
};

use crate::format::format_number;
use crate::formula::column_reference;
use crate::prediction::{
    FeatureSet, PredictionFormulas, Predictions, Predictor, PredictorInfo, Response, RoleAssignment, ValidationMethod,
    confusion_section, fit_measures_section, prediction_columns, setting_value,
};
use crate::report::{
    PlotSpec, PointGroups, ReportSection, SectionContent, SectionKind, TextTable, TreeDiagram, TreeDiagramNode,
};
use crate::script_text::{number, string};
use crate::surface_profiler::{Observation, ProfileFactor, ProfileFactorKind, SurfaceModel};

const DIGITS: usize = 6;
/// Folds of the cross-validation that sizes a tree without validation rows.
const DEFAULT_FOLDS: usize = 10;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// How the pruned size is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pruning {
    /// Smallest subtree within one standard error of the lowest held-out error.
    OneStandardError,
    /// Subtree with the lowest held-out error.
    MinimumError,
    /// The full grown tree.
    Unpruned,
}

impl Pruning {
    /// Every choice, in dialog order.
    pub const ALL: [Self; 3] = [Self::OneStandardError, Self::MinimumError, Self::Unpruned];

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::OneStandardError => "one-se",
            Self::MinimumError => "minimum",
            Self::Unpruned => "none",
        }
    }

    /// The choice with identifier `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

    /// Label in dialogs.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::OneStandardError => "Smallest within 1 SE of the best",
            Self::MinimumError => "Lowest held-out error",
            Self::Unpruned => "Don't prune",
        }
    }
}

/// Columns and settings of a decision tree.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeRequest {
    /// Response column.
    pub response: usize,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Training / validation / test roles.
    pub validation: ValidationMethod,
    /// Impurity of a classification tree.
    pub criterion: Criterion,
    /// Smallest node that may be split.
    pub min_split: usize,
    /// Fewest rows on each side of a split.
    pub min_leaf: usize,
    /// Greatest depth.
    pub max_depth: usize,
    /// How the pruned size is chosen.
    pub pruning: Pruning,
}

impl TreeRequest {
    /// Default settings: no held-out rows (10-fold cross-validation sizes
    /// the tree), Gini, nodes of 20 or more split, leaves of at least 7,
    /// depth up to 10, one-SE pruning.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>) -> Self {
        let d = TreeOptions::default();
        Self {
            response,
            predictors,
            validation: ValidationMethod::None,
            criterion: Criterion::Gini,
            min_split: d.min_split,
            min_leaf: d.min_leaf,
            max_depth: d.max_depth,
            pruning: Pruning::OneStandardError,
        }
    }

    /// Columns read.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.response];
        c.extend(&self.predictors);
        c.extend(self.validation.columns());
        c
    }

    /// The same request with column indices passed through `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.response = f(self.response);
        self.predictors.iter_mut().for_each(|c| *c = f(*c));
        self.validation = self.validation.map_columns(&f);
        self
    }

    fn options(&self) -> TreeOptions {
        TreeOptions {
            criterion: self.criterion,
            min_split: self.min_split,
            min_leaf: self.min_leaf,
            max_depth: self.max_depth,
            ..TreeOptions::default()
        }
    }
}

/// Held-out error along the pruning sequence.
#[derive(Debug, Clone)]
struct HeldOut {
    /// "validation rows" or "10-fold cross-validation".
    source: String,
    /// Mean error and standard error per step.
    errors: Vec<(f64, f64)>,
}

/// A fitted, pruned decision tree.
#[derive(Debug, Clone)]
pub struct TreeAnalysis {
    request: TreeRequest,
    response_name: String,
    profile_name: String,
    response: Response,
    roles: RoleAssignment,
    predictors: Vec<PredictorInfo>,
    features: Vec<Feature>,
    tree: Tree,
    step: usize,
    alpha: f64,
    held_out: Option<HeldOut>,
    predictions: Predictions,
    cross_validated: Option<Predictions>,
    /// Class whose probability the profilers show.
    profile_class: usize,
}

impl TreeAnalysis {
    /// Grow, prune and evaluate the tree.
    ///
    /// # Errors
    /// No predictors, the response among them, too few classes, invalid
    /// validation settings or tree limits, or no training rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &TreeRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        if request.predictors.contains(&request.response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        if request.min_leaf == 0 || request.max_depth == 0 {
            return Err(invalid("leaves need at least one row and the tree at least one level"));
        }
        let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let response = Response::read(table, request.response, &included)?;
        let response_name = table.column(request.response).map_or(String::new(), |c| c.name.clone());
        let roles = RoleAssignment::assign(table, states, &response, &request.validation)?;
        let train = roles.rows(Role::Training);
        let validation_rows = roles.rows(Role::Validation);

        let FeatureSet { predictors, features } = FeatureSet::read(table, &request.predictors, &train)?;
        let target = response.target();
        let options = request.options();
        let tree = fit_tree(&features, &target, &train, &options)?;

        // Held-out error of each pruning step.
        let mut cv: Option<CrossValidation> = None;
        let held_out = if !validation_rows.is_empty() {
            let errors = (0..tree.steps.len())
                .map(|k| {
                    let e: Vec<f64> = validation_rows
                        .iter()
                        .map(|&r| row_error(&tree, &features, &target, r, tree.steps[k].alpha))
                        .collect();
                    mean_and_se(&e)
                })
                .collect();
            Some(HeldOut { source: "validation rows".to_owned(), errors })
        } else {
            let (folds, seed) = roles.folds.unwrap_or((DEFAULT_FOLDS, 1));
            let folds = folds.min(train.len());
            if folds >= 2 {
                let result = cross_validate_pruning(&features, &target, &train, &options, &tree, folds, seed)?;
                let errors = result.errors.clone();
                cv = Some(result);
                Some(HeldOut { source: format!("{folds}-fold cross-validation (seed {seed})"), errors })
            } else {
                None
            }
        };
        let step = match (&held_out, request.pruning) {
            (_, Pruning::Unpruned) | (None, _) => 0,
            (Some(h), Pruning::OneStandardError) => choose_step(&h.errors, true),
            (Some(h), Pruning::MinimumError) => choose_step(&h.errors, false),
        };
        let alpha = tree.steps[step].alpha;

        let predict = |tree: &Tree, r: usize, alpha: f64| -> Vec<f64> {
            let node = &tree.nodes[tree.leaf_for(&|j| features[j].value(r), alpha)];
            match &response {
                Response::Continuous(_) => vec![node.mean],
                Response::Categorical { .. } => node.probabilities(),
            }
        };
        let wrap = |values: Vec<Vec<f64>>| match &response {
            Response::Continuous(_) => Predictions::Values(values.into_iter().map(|v| v[0]).collect()),
            Response::Categorical { .. } => Predictions::Probabilities(values),
        };
        let predictions = wrap((0..table.n_rows()).map(|r| predict(&tree, r, alpha)).collect());
        // Held-out predictions when the roles call for cross-validation.
        let cross_validated = match (&cv, roles.folds) {
            (Some(cv), Some(_)) => {
                let beta = tree.representative_alpha(step);
                let width = response.classes().map_or(1, <[String]>::len);
                let mut values = vec![vec![f64::NAN; width]; table.n_rows()];
                for (&r, &f) in train.iter().zip(&cv.fold_of) {
                    values[r] = predict(&cv.trees[f], r, beta);
                }
                Some(wrap(values))
            }
            _ => None,
        };

        let profile_class = match &response {
            Response::Continuous(_) => 0,
            Response::Categorical { classes, .. } if classes.len() == 2 => 1,
            Response::Categorical { .. } => tree.nodes[0].majority_class(),
        };
        let profile_name = match response.classes() {
            None => response_name.clone(),
            Some(c) => format!("P({response_name} = {})", c[profile_class]),
        };
        Ok(Self {
            request: request.clone(),
            response_name,
            profile_name,
            response,
            roles,
            predictors,
            features,
            tree,
            step,
            alpha,
            held_out,
            predictions,
            cross_validated,
            profile_class,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &TreeRequest {
        &self.request
    }

    /// The grown tree (with its whole pruning sequence).
    #[must_use]
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// The step of the pruning sequence kept.
    #[must_use]
    pub fn chosen_step(&self) -> usize {
        self.step
    }

    /// Leaves of the kept tree, left to right.
    #[must_use]
    pub fn leaves(&self) -> Vec<usize> {
        self.tree.leaves(self.alpha)
    }

    fn is_classification(&self) -> bool {
        self.response.classes().is_some()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let kind = if self.is_classification() { "Classification Tree" } else { "Regression Tree" };
        let names: Vec<&str> = self.predictors.iter().map(|p| p.name.as_str()).collect();
        format!("{kind}: {} ~ {}", self.response_name, names.join(" + "))
    }

    /// Text of the rule leading to a child of `split` (`left` side or not).
    /// The rule of `split` (of node `parent`) for the left or right side.
    /// A categorical rule lists only the levels present in the parent's
    /// training rows, so a second split on the same predictor doesn't
    /// repeat levels an earlier split sent elsewhere.
    fn rule_text(&self, parent: usize, split: &Split, left: bool) -> String {
        let p = &self.predictors[split.feature];
        let t = |x: f64| format_number(x, DIGITS);
        match (&split.rule, left) {
            (Rule::Below(x), true) | (Rule::AtLeast(x), false) => format!("{} < {}", p.name, t(*x)),
            (Rule::Below(x), false) | (Rule::AtLeast(x), true) => format!("{} ≥ {}", p.name, t(*x)),
            (Rule::Levels(set), side) => {
                let levels = p.levels.as_deref().unwrap_or_default();
                let feature = &self.features[split.feature];
                let rows = &self.tree.nodes[parent].rows;
                let present = |k: usize| rows.iter().any(|&r| feature.value(r) == Value::Level(k));
                let on_side = |k: usize| set.get(k).copied() == Some(side);
                let mut chosen: Vec<&str> =
                    (0..levels.len()).filter(|&k| on_side(k) && present(k)).map(|k| levels[k].as_str()).collect();
                if chosen.is_empty() {
                    chosen = (0..levels.len()).filter(|&k| on_side(k)).map(|k| levels[k].as_str()).collect();
                }
                format!("{} is {}", p.name, chosen.join(", "))
            }
        }
    }

    fn node_summary(&self, node: &Node) -> String {
        match self.response.classes() {
            None => format!("n = {} · mean {}", node.rows.len(), format_number(node.mean, DIGITS)),
            Some(classes) => {
                let p = node.probabilities();
                let mut order: Vec<usize> = (0..classes.len()).collect();
                order.sort_by(|&a, &b| p[b].total_cmp(&p[a]));
                let shown: Vec<String> = order
                    .iter()
                    .take(3)
                    .map(|&k| format!("{} {}%", classes[k], format_number(100.0 * p[k], 3)))
                    .collect();
                format!("n = {} · {}", node.rows.len(), shown.join(", "))
            }
        }
    }

    /// The kept tree for display.
    #[must_use]
    pub fn diagram(&self) -> TreeDiagram {
        let leaves = self.leaves();
        let mut nodes = Vec::new();
        // Depth-first, recording each node's position.
        fn visit(a: &TreeAnalysis, t: usize, rule: String, leaves: &[usize], out: &mut Vec<TreeDiagramNode>) -> usize {
            let node = &a.tree.nodes[t];
            let index = out.len();
            out.push(TreeDiagramNode {
                rule,
                summary: a.node_summary(node),
                rows: node.rows.clone(),
                children: Vec::new(),
                shares: if a.is_classification() { node.probabilities() } else { Vec::new() },
                leaf: leaves.iter().position(|&l| l == t).map(|k| k + 1),
            });
            if let (Some((l, r)), false, Some(split)) = (node.children, a.tree.is_leaf_at(t, a.alpha), &node.split) {
                let li = visit(a, l, a.rule_text(t, split, true), leaves, out);
                let ri = visit(a, r, a.rule_text(t, split, false), leaves, out);
                out[index].children = vec![li, ri];
            }
            index
        }
        visit(self, 0, "All rows".to_owned(), &leaves, &mut nodes);
        TreeDiagram { nodes, classes: self.response.classes().map(<[String]>::to_vec).unwrap_or_default() }
    }

    /// Path of rules from the root to node `target` of the kept tree.
    fn path_to(&self, target: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut t = 0;
        while t != target {
            let node = &self.tree.nodes[t];
            let (Some((l, r)), Some(split)) = (node.children, &node.split) else { break };
            // Children index above their parent; the target lies under the
            // child whose subtree range contains it.
            let go_left = target < r;
            out.push(self.rule_text(t, split, go_left));
            t = if go_left { l } else { r };
        }
        out
    }

    /// Error measure name and per-row divisor of the training risk.
    fn error_name(&self) -> &'static str {
        if self.is_classification() { "Misclassification rate" } else { "Mean squared error" }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary_section(), fit_measures_section(self)];
        out.extend(confusion_section(self));
        out.push(ReportSection {
            kind: SectionKind::TreeDiagram,
            title: "Tree Diagram".to_owned(),
            content: SectionContent::Tree(self.diagram()),
        });
        out.push(self.importance_section());
        out.push(self.leaves_section());
        if let Some(plot) = self.pruning_plot() {
            out.push(plot);
        }
        out.push(self.pruning_table());
        out.push(crate::report::prediction_formula_section(
            self.prediction_formulas().and_then(|f| f.prediction.ok_or_else(|| invalid("no formula"))),
        ));
        out
    }

    fn summary_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Response", self.response_name.clone());
        row(
            "Tree",
            match self.response.classes() {
                Some(c) => format!(
                    "classification, {} classes, {} impurity",
                    c.len(),
                    if self.request.criterion == Criterion::Gini { "Gini" } else { "entropy" }
                ),
                None => "regression (squared error)".to_owned(),
            },
        );
        for role in Role::ALL {
            let n = self.roles.rows(role).len();
            if n > 0 || role == Role::Training {
                row(&format!("{} rows", role.label()), n.to_string());
            }
        }
        let grown = &self.tree.steps[0];
        let kept = &self.tree.steps[self.step];
        row("Leaves (grown → kept)", format!("{} → {}", grown.leaves, kept.leaves));
        let depth = self.leaves().iter().map(|&l| self.tree.nodes[l].depth).max().unwrap_or(0);
        row("Depth of the kept tree", depth.to_string());
        row(
            "Size chosen by",
            match (&self.held_out, self.request.pruning) {
                (_, Pruning::Unpruned) => "not pruned".to_owned(),
                (None, _) => "not pruned (too few rows to hold any back)".to_owned(),
                (Some(h), p) => format!("{} — {}", h.source, p.label().to_lowercase()),
            },
        );
        row("Complexity α of the kept tree", format_number(self.alpha, 4));
        let missing = self
            .roles
            .rows(Role::Training)
            .iter()
            .filter(|&&r| self.features.iter().any(|f| matches!(f.value(r), Value::Missing)))
            .count();
        if missing > 0 {
            row("Training rows with a missing predictor (placed by surrogate splits)", missing.to_string());
        }
        row(
            "Growing limits",
            format!(
                "split nodes of {} or more rows; at least {} rows per side; depth up to {}",
                self.request.min_split, self.request.min_leaf, self.request.max_depth
            ),
        );
        ReportSection {
            kind: SectionKind::TreeSummary,
            title: "Tree Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn importance_section(&self) -> ReportSection {
        let raw = self.tree.importance(self.predictors.len(), self.alpha);
        let total: f64 = raw.iter().sum();
        let mut order: Vec<usize> = (0..raw.len()).collect();
        order.sort_by(|&a, &b| raw[b].total_cmp(&raw[a]));
        let mut t = TextTable::new(["Predictor", "Impurity reduction", "Share"]);
        for j in order {
            let share = if total > 0.0 { 100.0 * raw[j] / total } else { 0.0 };
            t.push([
                self.predictors[j].name.clone(),
                format_number(raw[j], DIGITS),
                format!("{}%", format_number(share, 3)),
            ]);
        }
        ReportSection {
            kind: SectionKind::VariableImportance,
            title: "Variable Importance (impurity reduction by the kept tree's splits)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn leaves_section(&self) -> ReportSection {
        let classes = self.response.classes();
        let mut headers = vec!["Leaf".to_owned(), "Rule".to_owned(), "Training rows".to_owned()];
        match classes {
            None => headers.push("Mean".to_owned()),
            Some(c) => {
                headers.push("Most likely".to_owned());
                headers.extend(c.iter().map(|k| format!("P({k})")));
            }
        }
        let mut t = TextTable::new(headers);
        for (k, &l) in self.leaves().iter().enumerate() {
            let node = &self.tree.nodes[l];
            let path = self.path_to(l);
            let mut row = vec![
                (k + 1).to_string(),
                if path.is_empty() { "All rows".to_owned() } else { path.join(" and ") },
                node.rows.len().to_string(),
            ];
            match classes {
                None => row.push(format_number(node.mean, DIGITS)),
                Some(c) => {
                    row.push(c[node.majority_class()].clone());
                    row.extend(node.probabilities().iter().map(|p| format_number(*p, 4)));
                }
            }
            t.push(row);
        }
        ReportSection { kind: SectionKind::TreeLeaves, title: "Leaves".to_owned(), content: SectionContent::Table(t) }
    }

    fn training_errors(&self) -> Vec<f64> {
        let n = self.roles.rows(Role::Training).len().max(1) as f64;
        self.tree.steps.iter().map(|s| s.risk / n).collect()
    }

    fn pruning_plot(&self) -> Option<ReportSection> {
        let h = self.held_out.as_ref()?;
        let leaves: Vec<f64> = self.tree.steps.iter().map(|s| s.leaves as f64).collect();
        let training: Vec<[f64; 2]> = leaves.iter().zip(self.training_errors()).map(|(&x, y)| [x, y]).collect();
        let held: Vec<[f64; 2]> = leaves.iter().zip(&h.errors).map(|(&x, e)| [x, e.0]).collect();
        let lower = leaves.iter().zip(&h.errors).map(|(&x, e)| [x, e.0 - e.1]).collect();
        let upper = leaves.iter().zip(&h.errors).map(|(&x, e)| [x, e.0 + e.1]).collect();
        let top = h.errors.iter().map(|e| e.0 + e.1).chain(self.training_errors()).fold(0.0_f64, f64::max);
        let chosen = leaves[self.step];
        let held_label = if h.source.starts_with("validation") { "Validation" } else { "Cross-validation" };
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: "Leaves".to_owned(),
            y_label: self.error_name().to_owned(),
            points: Vec::new(),
            line: None,
            band: Some((lower, upper)),
            band_name: Some(format!("{held_label} ± 1 standard error")),
            groups: Some(PointGroups {
                title: "Rows".to_owned(),
                labels: vec!["Training".to_owned(), held_label.to_owned()],
                membership: Vec::new(),
                lines: vec![training, held],
            }),
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            segments: vec![vec![[chosen, 0.0], [chosen, top]]],
            ..PlotSpec::default()
        };
        Some(ReportSection {
            kind: SectionKind::PruningPlot,
            title: format!("Error by Tree Size ({}; band ± 1 SE; vertical line at the kept size)", h.source),
            content: SectionContent::Plot(Box::new(spec)),
        })
    }

    fn pruning_table(&self) -> ReportSection {
        let name = self.error_name();
        let mut headers = vec!["Leaves".to_owned(), "α".to_owned(), format!("Training {}", name.to_lowercase())];
        if self.held_out.is_some() {
            headers.push(format!("Held-out {}", name.to_lowercase()));
            headers.push("Standard error".to_owned());
        }
        headers.push(String::new());
        let mut t = TextTable::new(headers);
        let training = self.training_errors();
        for (k, s) in self.tree.steps.iter().enumerate() {
            let mut row = vec![s.leaves.to_string(), format_number(s.alpha, 4), format_number(training[k], DIGITS)];
            if let Some(h) = &self.held_out {
                row.push(format_number(h.errors[k].0, DIGITS));
                row.push(format_number(h.errors[k].1, 4));
            }
            row.push(if k == self.step { "◀ kept".to_owned() } else { String::new() });
            t.push(row);
        }
        ReportSection {
            kind: SectionKind::PruningSequence,
            title: "Pruning Sequence (nested subtrees from cost-complexity pruning)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    // ---- Formulas ----------------------------------------------------------

    fn known_expression(&self, j: usize) -> String {
        let p = &self.predictors[j];
        let x = column_reference(&p.name);
        match &p.levels {
            None => format!("!is_missing({x})"),
            Some(levels) => {
                let any: Vec<String> = levels.iter().map(|l| format!("text({x}) == {}", string(l))).collect();
                if any.is_empty() { "false".to_owned() } else { format!("({})", any.join(" || ")) }
            }
        }
    }

    fn left_expression(&self, j: usize, rule: &Rule) -> String {
        let p = &self.predictors[j];
        let x = column_reference(&p.name);
        match rule {
            Rule::Below(t) => format!("{x} < {}", number(*t)),
            Rule::AtLeast(t) => format!("{x} >= {}", number(*t)),
            Rule::Levels(set) => {
                let levels = p.levels.as_deref().unwrap_or_default();
                let any: Vec<String> = levels
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| set.get(*k).copied().unwrap_or(false))
                    .map(|(_, l)| format!("text({x}) == {}", string(l)))
                    .collect();
                if any.is_empty() { "false".to_owned() } else { any.join(" || ") }
            }
        }
    }

    /// `true` when a row goes left at `split`, as a formula expression
    /// (surrogates for missing values, then the majority side).
    fn direction(&self, split: &Split) -> String {
        let mut s = format!(
            "if {} {{ {} }}",
            self.known_expression(split.feature),
            self.left_expression(split.feature, &split.rule)
        );
        for sur in &split.surrogates {
            s.push_str(&format!(
                " else if {} {{ {} }}",
                self.known_expression(sur.feature),
                self.left_expression(sur.feature, &sur.rule)
            ));
        }
        s.push_str(&format!(" else {{ {} }}", split.majority_left));
        s
    }

    fn node_formula(&self, t: usize, depth: usize, value: &dyn Fn(&Node) -> String) -> String {
        let node = &self.tree.nodes[t];
        match (node.children, &node.split) {
            (Some((l, r)), Some(split)) if !self.tree.is_leaf_at(t, self.alpha) => {
                let pad = "    ".repeat(depth + 1);
                let close = "    ".repeat(depth);
                format!(
                    "if ({}) {{\n{pad}{}\n{close}}} else {{\n{pad}{}\n{close}}}",
                    self.direction(split),
                    self.node_formula(l, depth + 1, value),
                    self.node_formula(r, depth + 1, value)
                )
            }
            _ => value(node),
        }
    }

    /// The kept tree as formulas: the prediction (mean or most likely class)
    /// and, for a classification tree, each class's probability.
    ///
    /// # Errors
    /// A date-time column used as a category (its labels have no formula
    /// equivalent).
    pub fn prediction_formulas(&self) -> Result<PredictionFormulas> {
        if let Some(p) =
            self.predictors.iter().find(|p| p.levels.is_some() && matches!(p.kind, ColumnKind::DateTime(_)))
        {
            return Err(invalid(format!(
                "the date-time column {:?} is used as a category; a formula cannot match its labels",
                p.name
            )));
        }
        Ok(match self.response.classes() {
            None => PredictionFormulas {
                prediction: Some(self.node_formula(0, 0, &|n| number(n.mean))),
                probabilities: Vec::new(),
            },
            Some(classes) => PredictionFormulas {
                prediction: Some(self.node_formula(0, 0, &|n| string(&classes[n.majority_class()]))),
                probabilities: (0..classes.len())
                    .map(|k| self.node_formula(0, 0, &|n| number(n.probabilities()[k])))
                    .collect(),
            },
        })
    }

    /// Columns the report can save (see [`prediction_columns`]).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<crate::ReportColumn> {
        prediction_columns(self, &self.prediction_formulas().unwrap_or_default())
    }

    fn value_of(&self, j: usize, v: &TermValue) -> Value {
        setting_value(&self.features[j], v)
    }
}

/// Squared error, or 1 for a misclassified row, of row `r` at `alpha`.
fn row_error(tree: &Tree, features: &[Feature], target: &Target, r: usize, alpha: f64) -> f64 {
    let leaf = &tree.nodes[tree.leaf_for(&|j| features[j].value(r), alpha)];
    match target {
        Target::Regression(y) => (y[r] - leaf.mean).powi(2),
        Target::Classification { classes, .. } => f64::from(u8::from(leaf.majority_class() != classes[r])),
    }
}

fn mean_and_se(e: &[f64]) -> (f64, f64) {
    let n = e.len() as f64;
    let mean = e.iter().sum::<f64>() / n;
    let var = if n > 1.0 { e.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0) } else { 0.0 };
    (mean, (var / n).sqrt())
}

impl Predictor for TreeAnalysis {
    fn response_name(&self) -> &str {
        &self.response_name
    }

    fn response(&self) -> &Response {
        &self.response
    }

    fn roles(&self) -> &RoleAssignment {
        &self.roles
    }

    fn predictions(&self) -> &Predictions {
        &self.predictions
    }

    fn cross_validated(&self) -> Option<&Predictions> {
        self.cross_validated.as_ref()
    }
}

impl SurfaceModel for TreeAnalysis {
    // The profilers show the class probability, e.g. "P(Churn = Yes)".
    #[allow(clippy::misnamed_getters)]
    fn response_name(&self) -> &str {
        &self.profile_name
    }

    fn profile_factors(&self) -> Vec<ProfileFactor> {
        self.predictors
            .iter()
            .map(|p| ProfileFactor {
                name: p.name.clone(),
                kind: match &p.levels {
                    Some(levels) => ProfileFactorKind::Categorical { levels: levels.clone() },
                    None => ProfileFactorKind::Continuous { range: p.range },
                },
            })
            .collect()
    }

    fn default_profile_settings(&self) -> Vec<TermValue> {
        self.predictors.iter().map(|p| p.default).collect()
    }

    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        points
            .iter()
            .map(|s| {
                if s.len() != self.predictors.len() {
                    return Err(invalid("the settings do not match the predictors"));
                }
                let node = &self.tree.nodes[self.tree.leaf_for(&|j| self.value_of(j, &s[j]), self.alpha)];
                Ok(match self.response {
                    Response::Continuous(_) => node.mean,
                    Response::Categorical { .. } => node.probabilities()[self.profile_class],
                })
            })
            .collect()
    }

    fn observations(&self) -> Vec<Observation> {
        self.roles
            .rows(Role::Training)
            .into_iter()
            .filter_map(|r| {
                let settings: Option<Vec<TermValue>> = self
                    .features
                    .iter()
                    .map(|f| match f.value(r) {
                        Value::Number(x) => Some(TermValue::Number(x)),
                        Value::Level(l) => Some(TermValue::Level(l)),
                        Value::Missing => None,
                    })
                    .collect();
                let response = match &self.response {
                    Response::Continuous(y) => y[r],
                    Response::Categorical { codes, .. } => f64::from(u8::from(codes[r] == Some(self.profile_class))),
                };
                settings.map(|settings| Observation { row: r, settings, response })
            })
            .collect()
    }

    fn profile_note(&self) -> Option<String> {
        Some(format!(
            "Predictions of the kept tree, which change in steps at its split points; no confidence bands.{}",
            if self.is_classification() {
                format!(" Shown: the probability of {}.", self.profile_name)
            } else {
                String::new()
            }
        ))
    }
}

// ---- Project files and scripts ------------------------------------------------

/// The request's members after `response` and `predictors`, for project files.
#[must_use]
pub fn request_members_to_json(
    request: &TreeRequest,
    col: &dyn Fn(usize) -> JsonValue,
) -> Vec<(&'static str, JsonValue)> {
    vec![
        ("validation", request.validation.to_json(col)),
        (
            "criterion",
            JsonValue::String(if request.criterion == Criterion::Gini { "gini" } else { "entropy" }.to_owned()),
        ),
        ("min_split", JsonValue::Number(request.min_split as f64)),
        ("min_leaf", JsonValue::Number(request.min_leaf as f64)),
        ("max_depth", JsonValue::Number(request.max_depth as f64)),
        ("pruning", JsonValue::String(request.pruning.key().to_owned())),
    ]
}

/// Read a request from a project file's report entry (missing members take
/// their defaults).
///
/// # Errors
/// An unknown criterion, pruning or validation method, or a missing column.
pub fn request_from_json(
    v: &JsonValue,
    response: usize,
    predictors: Vec<usize>,
    find: &dyn Fn(&JsonValue) -> Result<usize>,
) -> Result<TreeRequest> {
    let mut r = TreeRequest::new(response, predictors);
    r.validation = ValidationMethod::from_json(v.get("validation"), find)?;
    if let Some(c) = v.get("criterion").and_then(JsonValue::as_str) {
        r.criterion = match c {
            "gini" => Criterion::Gini,
            "entropy" => Criterion::Entropy,
            other => return Err(invalid(format!("decision tree: unknown criterion {other:?}"))),
        };
    }
    let size = |k: &str| v.get(k).and_then(JsonValue::as_usize);
    r.min_split = size("min_split").unwrap_or(r.min_split);
    r.min_leaf = size("min_leaf").unwrap_or(r.min_leaf);
    r.max_depth = size("max_depth").unwrap_or(r.max_depth);
    if let Some(p) = v.get("pruning").and_then(JsonValue::as_str) {
        r.pruning = Pruning::from_key(p).ok_or_else(|| invalid(format!("decision tree: unknown pruning {p:?}")))?;
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formula::{FormulaValue, evaluate};
    use stat_data::Column;

    /// 200 rows: churn depends on contract and tenure; charges are noise;
    /// some tenure values missing.
    fn churn() -> DataTable {
        let n = 200;
        let mut rng = stat_core::rng::Rng::new(11);
        let contract: Vec<String> = (0..n).map(|i| ["Monthly", "One year", "Two year"][i % 3].to_owned()).collect();
        let tenure: Vec<f64> =
            (0..n).map(|i| if i % 17 == 5 { f64::NAN } else { (rng.next_f64() * 60.0).round() }).collect();
        let charges: Vec<f64> = (0..n).map(|_| 20.0 + rng.next_f64() * 80.0).collect();
        let churn: Vec<String> = (0..n)
            .map(|i| {
                let p = if contract[i] == "Monthly" { if tenure[i] < 12.0 { 0.95 } else { 0.7 } } else { 0.08 };
                if rng.next_f64() < p { "Yes" } else { "No" }.to_owned()
            })
            .collect();
        let spend: Vec<f64> = (0..n)
            .map(|i| if contract[i] == "Two year" { 80.0 } else { 40.0 } + if tenure[i] > 30.0 { 15.0 } else { 0.0 } + rng.normal())
            .collect();
        DataTable::new(
            "churn",
            vec![
                Column::text("Contract", contract),
                Column::numeric("Tenure", tenure),
                Column::numeric("Charges", charges),
                Column::text("Churn", churn),
                Column::numeric("Spend", spend),
            ],
        )
        .unwrap()
    }

    fn formula_values(table: &DataTable, formula: &str) -> Vec<FormulaValue> {
        evaluate(table, None, formula, 0..table.n_rows()).unwrap_or_else(|e| panic!("{e}\n{formula}")).values
    }

    #[test]
    fn classification_tree_report_and_formulas() {
        let t = churn();
        let mut req = TreeRequest::new(3, vec![0, 1, 2]);
        req.validation = ValidationMethod::Holdout { validation: 0.3, test: 0.2, seed: 4, stratify: true, group: None };
        req.min_split = 10;
        req.min_leaf = 4;
        let a = TreeAnalysis::run(&t, &RowStateManager::new(200), &req).unwrap();
        assert!(a.title().starts_with("Classification Tree: Churn ~ Contract"));
        // The first split separates monthly contracts.
        let root = a.tree().nodes[0].split.as_ref().unwrap();
        assert_eq!(root.feature, 0, "contract splits first");
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::TreeSummary,
                SectionKind::FitByRole,
                SectionKind::ConfusionMatrix,
                SectionKind::TreeDiagram,
                SectionKind::VariableImportance,
                SectionKind::TreeLeaves,
                SectionKind::PruningPlot,
                SectionKind::PruningSequence,
                SectionKind::PredictionFormula,
            ]
        );
        // Fit by role lists the three roles.
        let s = a.sections();
        let SectionContent::Table(fit) = &s[1].content else { panic!() };
        assert_eq!(fit.rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(), ["Training", "Validation", "Test"]);
        // The formulas reproduce every row's prediction, missing tenure included.
        let f = a.prediction_formulas().unwrap();
        let Predictions::Probabilities(p) = a.predictions() else { panic!() };
        let most = formula_values(&t, f.prediction.as_ref().unwrap());
        let yes = formula_values(&t, &f.probabilities[1]);
        for r in 0..200 {
            let FormulaValue::Number(v) = yes[r] else { panic!("row {r}: {:?}", yes[r]) };
            assert!((v - p[r][1]).abs() < 1e-12, "row {r}");
            let label = if p[r][1] > p[r][0] { "Yes" } else { "No" };
            assert_eq!(most[r], FormulaValue::Text(label.to_owned()), "row {r}");
        }
        // Diagram: leaves numbered, the root holds every training row.
        let d = a.diagram();
        assert_eq!(d.nodes[0].rows.len(), a.roles.rows(Role::Training).len());
        assert_eq!(d.nodes.iter().filter(|n| n.leaf.is_some()).count(), a.leaves().len());
        // Layout: one slot per leaf; hiding the root's branches leaves one box.
        let (places, slots, deepest) = d.layout(&|_| false);
        assert_eq!(slots, a.leaves().len());
        assert!(places.iter().all(Option::is_some));
        assert!(deepest >= 1);
        let (places, slots, deepest) = d.layout(&|i| i == 0);
        assert_eq!((slots, deepest), (1, 0));
        assert_eq!(places.iter().filter(|p| p.is_some()).count(), 1);
        let svg = crate::report_figure::tree_figure(&d, &|_| false, "Tree", stat_viz::export::VectorFormat::Svg);
        let svg = String::from_utf8(svg).unwrap();
        assert!(svg.contains("Contract is Monthly") && svg.contains("Yes"), "rules and class key are drawn");
        // Saved columns: two probabilities, the class and the roles.
        let keys: Vec<String> = a.saveable_columns().into_iter().map(|c| c.key).collect();
        assert_eq!(keys, ["probability_1", "probability_2", "most_likely", "validation"]);
        // Profiler: probability of "Yes" is higher for monthly contracts.
        let monthly =
            a.predict_settings(&[vec![TermValue::Level(0), TermValue::Number(5.0), TermValue::Number(50.0)]]).unwrap();
        let two_year =
            a.predict_settings(&[vec![TermValue::Level(2), TermValue::Number(5.0), TermValue::Number(50.0)]]).unwrap();
        let levels: Vec<String> = match &a.profile_factors()[0].kind {
            ProfileFactorKind::Categorical { levels } => levels.clone(),
            _ => panic!(),
        };
        assert!(monthly[0] > two_year[0] + 0.3, "{monthly:?} {two_year:?} {levels:?} {}", a.profile_name);
    }

    #[test]
    fn regression_tree_with_cross_validation_and_json() {
        let t = churn();
        let mut req = TreeRequest::new(4, vec![0, 1, 2]);
        req.validation = ValidationMethod::KFold { folds: 5, seed: 2 };
        let a = TreeAnalysis::run(&t, &RowStateManager::new(200), &req).unwrap();
        assert!(a.title().starts_with("Regression Tree: Spend"));
        // Contract and tenure carry the signal; charges carry none.
        let imp = a.tree().importance(3, a.tree().steps[a.chosen_step()].alpha);
        assert!(imp[0] > imp[2] && imp[1] > imp[2]);
        let s = a.sections();
        let SectionContent::Table(fit) = &s[1].content else { panic!() };
        assert_eq!(fit.rows.last().unwrap()[0], "Cross-validation");
        let r2: f64 = fit.rows[0][4].parse().unwrap();
        assert!(r2 > 0.9, "training R² {r2}");
        let f = a.prediction_formulas().unwrap();
        let Predictions::Values(p) = a.predictions() else { panic!() };
        for (r, v) in formula_values(&t, f.prediction.as_ref().unwrap()).iter().enumerate() {
            let FormulaValue::Number(v) = v else { panic!() };
            assert!((v - p[r]).abs() < 1e-9);
        }
        // JSON round trip.
        let col = |i: usize| JsonValue::String(t.column(i).unwrap().name.clone());
        let find = |v: &JsonValue| t.find(v.as_str().unwrap_or_default()).ok_or_else(|| invalid("no column"));
        let members = request_members_to_json(&req, &col);
        let json = JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
        assert_eq!(request_from_json(&json, 4, vec![0, 1, 2], &find).unwrap(), req);
        // Unpruned keeps every grown leaf.
        let full =
            TreeAnalysis::run(&t, &RowStateManager::new(200), &TreeRequest { pruning: Pruning::Unpruned, ..req })
                .unwrap();
        assert_eq!(full.leaves().len(), full.tree().steps[0].leaves);
    }
}
