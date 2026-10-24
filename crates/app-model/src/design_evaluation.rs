//! Evaluating a design held in a table before its responses are collected
//! ([`stat_doe::evaluation`]): the power of the test of each model term for
//! an anticipated coefficient, the relative prediction variance (profile
//! along each factor, maximum and average over the region, and the
//! fraction-of-design-space curve), the alias matrix for terms left out of the
//! model, the absolute correlations among model and alias terms, and the D,
//! A, G and I criteria. A comparison column whose two levels split the rows
//! into two candidate designs puts the same criteria side by side, with the
//! relative efficiencies of the first design to the second.
//!
//! The region is the cube of coded settings unless the request gives
//! constraints (linear limits and disallowed combinations in actual units,
//! as for optimal designs; [`stat_doe::Region::parse`]): then the maximum and
//! average prediction variance and the fraction of design space are over
//! the constrained region ([`stat_doe::evaluate_design_in`]). Optimal designs
//! created with constraints come with this report already set up
//! ([`crate::doe::design_project`]).
//!
//! Factors are coded as in [`crate::declared_design`]: numeric factors from
//! their low and high settings (by default the smallest and largest values
//! among all rows, so both compared designs share one region), two-level
//! categorical factors −1 and +1 in sorted order.

use crate::declared_design::{DeclaredDesign, DeclaredFactor, DesignDeclarationRequest};
use crate::format::format_number;
use crate::heat_map::HeatMapSpec;
use crate::project::{invalid, member, obj, s};
use crate::report::{
    Annotations, PlotPane, PlotSpec, PointGroups, ReportSection, SectionContent, SectionKind, TextTable,
};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_doe::{
    DesignComparison, DesignEvaluation, EvaluationOptions, Factor, ModelFormula, Region, compare_designs,
    evaluate_design_in,
};

/// Significant digits in report tables.
const DIGITS: usize = 4;

/// Fractions of the design space reported as quantiles.
const FRACTIONS: [f64; 7] = [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0];

/// What to evaluate: the factor columns, the model and the test settings.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignEvaluationRequest {
    /// Factor columns, with optional settings coded −1 and +1.
    pub factors: Vec<DeclaredFactor>,
    /// Model formula over the factor column names (e.g. `A + B + A*B + A^2`),
    /// or one of `main effects`, `interactions`, `quadratic`.
    pub model: String,
    /// Alias terms as a formula (empty: the two-factor interactions not in
    /// the model, or the three-factor ones when the model has them all).
    pub alias_terms: String,
    /// Anticipated coefficients in units of the error standard deviation:
    /// one value for every term, or one per non-intercept term in model order.
    pub coefficients: Vec<f64>,
    /// Significance level of the tests.
    pub alpha: f64,
    /// Column whose two levels split the rows into two designs to compare.
    pub compare_by: Option<usize>,
    /// Constraints on the factors in actual units, one per line (see
    /// [`stat_doe::Region::parse`]): the region is the cube cut by them.
    /// Empty for the whole cube.
    pub constraints: String,
}

impl DesignEvaluationRequest {
    /// The interaction model in `factors`, coefficient 1, α = 0.05, no comparison.
    #[must_use]
    pub fn new(factors: &[usize]) -> Self {
        Self {
            factors: factors.iter().map(|&column| DeclaredFactor { column, low: None, high: None }).collect(),
            model: "interactions".to_owned(),
            alias_terms: String::new(),
            coefficients: vec![1.0],
            alpha: 0.05,
            compare_by: None,
            constraints: String::new(),
        }
    }

    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.factors.iter().map(|f| f.column).chain(self.compare_by).collect()
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.factors.iter_mut().for_each(|d| d.column = f(d.column));
        self.compare_by = self.compare_by.map(&f);
        self
    }

    /// The project-file form (`"analysis": "design-evaluation"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let name = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let num = |v: Option<f64>| v.map_or(JsonValue::Null, JsonValue::number);
        let coefficient = match self.coefficients.as_slice() {
            [one] => JsonValue::number(*one),
            many => JsonValue::Array(many.iter().map(|&v| JsonValue::number(v)).collect()),
        };
        obj([
            ("analysis", s("design-evaluation")),
            (
                "factors",
                JsonValue::Array(
                    self.factors
                        .iter()
                        .map(|f| obj([("column", name(f.column)), ("low", num(f.low)), ("high", num(f.high))]))
                        .collect(),
                ),
            ),
            ("model", s(&self.model)),
            ("alias", s(&self.alias_terms)),
            ("coefficient", coefficient),
            ("alpha", JsonValue::number(self.alpha)),
            ("compare", self.compare_by.map_or(JsonValue::Null, name)),
            ("constraints", s(&self.constraints)),
        ])
    }

    /// Read the project-file form. Factors may be plain column names; every
    /// other member is optional.
    ///
    /// # Errors
    /// Missing or unknown columns, or malformed values.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let declared = DesignDeclarationRequest::from_json(
            &obj([("factors", member(v, "factors")?.clone()), ("responses", JsonValue::Array(Vec::new()))]),
            table,
        )?;
        let text = |key: &str, default: &str| -> Result<String> {
            match v.get(key) {
                None | Some(JsonValue::Null) => Ok(default.to_owned()),
                Some(x) => x.as_str().map(str::to_owned).ok_or_else(|| invalid(format!("{key}: expected text"))),
            }
        };
        let coefficients = match v.get("coefficient") {
            None | Some(JsonValue::Null) => vec![1.0],
            Some(JsonValue::Array(list)) => list
                .iter()
                .map(|x| x.as_f64().ok_or_else(|| invalid("coefficient: expected numbers")))
                .collect::<Result<_>>()?,
            Some(x) => vec![x.as_f64().ok_or_else(|| invalid("coefficient: expected a number or a list"))?],
        };
        let alpha = match v.get("alpha") {
            None | Some(JsonValue::Null) => 0.05,
            Some(x) => x.as_f64().ok_or_else(|| invalid("alpha: expected a number"))?,
        };
        let compare_by = match v.get("compare") {
            None | Some(JsonValue::Null) => None,
            Some(x) => {
                let name = x.as_str().ok_or_else(|| invalid("compare: expected a column name"))?;
                Some(table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))?)
            }
        };
        Ok(Self {
            factors: declared.factors,
            model: text("model", "interactions")?,
            alias_terms: text("alias", "")?,
            coefficients,
            alpha,
            compare_by,
            constraints: text("constraints", "")?,
        })
    }
}

/// One evaluated design.
#[derive(Debug, Clone)]
struct Evaluated {
    /// Comparison level (empty when there is one design).
    label: String,
    evaluation: DesignEvaluation,
}

/// A computed design evaluation.
#[derive(Debug, Clone)]
pub struct DesignEvaluationAnalysis {
    request: DesignEvaluationRequest,
    factor_names: Vec<String>,
    /// Labels of the low, middle and high settings of each factor.
    settings: Vec<[String; 3]>,
    compare_name: Option<String>,
    designs: Vec<Evaluated>,
    comparison: Option<DesignComparison>,
    /// The constraints read from the request (`None`: the whole cube).
    region: Option<Region>,
}

impl DesignEvaluationAnalysis {
    /// Evaluate the design (or the two designs) in `table`.
    ///
    /// # Errors
    /// Invalid factors (as [`DeclaredDesign::run`]), a model or alias formula
    /// naming unknown columns, a comparison column without exactly two
    /// levels or used as a factor, or a design that cannot estimate the model
    /// ([`stat_doe::evaluate_design`]).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &DesignEvaluationRequest) -> Result<Self> {
        let declare = |factors: Vec<DeclaredFactor>, states: &RowStateManager| {
            DeclaredDesign::run(
                table,
                states,
                &DesignDeclarationRequest { factors, ..DesignDeclarationRequest::new(&[], &[]) },
            )
        };
        let all = declare(request.factors.clone(), states)?;
        let names = all.factor_names().to_vec();
        let formula = ModelFormula::parse(&request.model, &names)
            .map_err(|e| invalid(format!("model: {e} (factors: {})", names.join(", "))))?;
        let alias_terms = if request.alias_terms.trim().is_empty() {
            None
        } else {
            let f =
                ModelFormula::parse(&request.alias_terms, &names).map_err(|e| invalid(format!("alias terms: {e}")))?;
            Some(f.terms()[1..].to_vec())
        };
        let options = EvaluationOptions {
            alpha: request.alpha,
            coefficients: request.coefficients.clone(),
            alias_terms,
            ..EvaluationOptions::default()
        };
        let settings = all
            .ranges()
            .iter()
            .zip(all.level_labels())
            .map(|(&(low, high), labels)| match labels {
                Some((lo, hi)) => [lo.clone(), String::new(), hi.clone()],
                None => [low, (low + high) / 2.0, high].map(|v| format_number(v, 5)),
            })
            .collect();
        // Constraints are in actual units: read them against the coding of
        // this evaluation (categorical factors stay at their two levels).
        let region = if request.constraints.trim().is_empty() {
            None
        } else {
            let factors: Vec<Factor> = names
                .iter()
                .zip(all.ranges())
                .zip(all.level_labels())
                .map(|((name, &(low, high)), labels)| match labels {
                    Some(_) => Factor::coded(name.clone()),
                    None => Factor::new(name.clone(), low, high),
                })
                .collect();
            let mut region =
                Region::parse(&request.constraints, &factors).map_err(|e| invalid(format!("constraints: {e}")))?;
            for (j, labels) in all.level_labels().iter().enumerate() {
                if labels.is_some() {
                    region.restrict_to_two_levels(j);
                }
            }
            Some(region)
        };
        let cube = Region::cube(names.len());
        let over = region.as_ref().unwrap_or(&cube);
        let evaluate = |runs: &[Vec<f64>], label: &str| -> Result<DesignEvaluation> {
            evaluate_design_in(&formula, runs, over, &options).map_err(|e| match e {
                stat_core::StatError::RankDeficient { rank, columns } => invalid(format!(
                    "{}the runs cannot estimate every term of the model (rank {rank} of {columns} terms); \
                     use a smaller model or add runs",
                    if label.is_empty() { String::new() } else { format!("design {label}: ") }
                )),
                other => other,
            })
        };

        let Some(by) = request.compare_by else {
            let evaluation = evaluate(all.coded_runs(), "")?;
            return Ok(Self {
                request: request.clone(),
                factor_names: names,
                settings,
                compare_name: None,
                designs: vec![Evaluated { label: String::new(), evaluation }],
                comparison: None,
                region,
            });
        };
        if request.factors.iter().any(|f| f.column == by) {
            return Err(invalid("the comparison column cannot also be a factor"));
        }
        let column = table.column(by).ok_or_else(|| invalid(format!("no column {by}")))?;
        let text: Vec<String> = (0..table.n_rows()).map(|r| column.level_text(r)).collect();
        let mut levels: Vec<&String> =
            text.iter().enumerate().filter(|&(r, t)| !t.is_empty() && !states.is_excluded(r)).map(|(_, t)| t).collect();
        levels.sort_by(|a, b| stat_data::table_ops::natural_order(a, b));
        levels.dedup();
        if levels.len() != 2 {
            return Err(invalid(format!(
                "{} has {} levels: comparing needs a column with exactly two, one per design",
                column.name,
                levels.len()
            )));
        }
        // Both designs are coded over the region of all rows.
        let fixed: Vec<DeclaredFactor> = request
            .factors
            .iter()
            .zip(all.ranges())
            .map(|(f, &(low, high))| DeclaredFactor { column: f.column, low: Some(low), high: Some(high) })
            .collect();
        let mut designs = Vec::new();
        for level in levels {
            let mut only = states.clone();
            only.set_excluded((0..table.n_rows()).filter(|&r| text[r] != *level), true);
            let d = declare(fixed.clone(), &only).map_err(|e| invalid(format!("design {level}: {e}")))?;
            designs.push(Evaluated { label: level.clone(), evaluation: evaluate(d.coded_runs(), level)? });
        }
        let comparison = Some(compare_designs(&designs[0].evaluation, &designs[1].evaluation)?);
        Ok(Self {
            request: request.clone(),
            factor_names: names,
            settings,
            compare_name: Some(column.name.clone()),
            designs,
            comparison,
            region,
        })
    }

    /// The evaluation of each design (one, or two when comparing).
    pub fn evaluations(&self) -> impl Iterator<Item = &DesignEvaluation> {
        self.designs.iter().map(|d| &d.evaluation)
    }

    /// The relative efficiencies of the first design to the second, when comparing.
    #[must_use]
    pub fn comparison(&self) -> Option<&DesignComparison> {
        self.comparison.as_ref()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.compare_name {
            Some(c) => format!("Design Evaluation: {} vs {} ({c})", self.designs[0].label, self.designs[1].label),
            None => format!("Design Evaluation: {} runs", self.designs[0].evaluation.runs),
        }
    }

    fn design_name(&self, i: usize) -> String {
        match &self.compare_name {
            Some(c) => {
                let label = &self.designs[i].label;
                if label.trim().parse::<f64>().is_ok() { format!("{c} {label}") } else { label.clone() }
            }
            None => "Design".to_owned(),
        }
    }

    fn term_label(&self, t: &stat_doe::Term) -> String {
        t.label(&self.factor_names)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![
            section(SectionKind::EvaluationSummary, "Design Summary", SectionContent::Table(self.summary())),
            section(SectionKind::EvaluationPower, "Power by Effect", SectionContent::Table(self.power())),
            section(SectionKind::EvaluationProfile, "Prediction Variance Profile", self.profile_plot()),
            section(SectionKind::EvaluationFds, "Fraction of Design Space", self.fds_plot()),
            section(
                SectionKind::EvaluationFdsQuantiles,
                if self.region.is_some() {
                    "Prediction Variance over the Constrained Region"
                } else {
                    "Prediction Variance over the Region"
                },
                SectionContent::Table(self.fds_table()),
            ),
            section(SectionKind::EvaluationAlias, "Alias Matrix", self.alias_table()),
        ];
        for (i, kind) in [SectionKind::EvaluationCorrelations, SectionKind::EvaluationCorrelationsSecond]
            .into_iter()
            .enumerate()
            .take(self.designs.len())
        {
            let title = if self.designs.len() > 1 {
                format!("Absolute Correlations of Terms: {}", self.design_name(i))
            } else {
                "Absolute Correlations of Terms".to_owned()
            };
            out.push(ReportSection { kind, title, content: self.heat_map(i) });
        }
        if let Some(c) = &self.comparison {
            out.push(section(SectionKind::EvaluationComparison, "Design Comparison", self.comparison_table(c)));
        }
        out.push(section(SectionKind::EvaluationNote, "About These Measures", SectionContent::Note(self.note())));
        out
    }

    fn header(&self, first: &str) -> TextTable {
        TextTable::new(std::iter::once(first.to_owned()).chain((0..self.designs.len()).map(|i| self.design_name(i))))
    }

    fn summary(&self) -> TextTable {
        let mut t = self.header("Measure");
        let mut row = |label: &str, f: &dyn Fn(&DesignEvaluation) -> String| {
            t.push(std::iter::once(label.to_owned()).chain(self.evaluations().map(f)));
        };
        row("Runs", &|e| e.runs.to_string());
        row("Model terms (with intercept)", &|e| e.formula.n_terms().to_string());
        row("Error degrees of freedom", &|e| e.error_df.to_string());
        row("D-efficiency (%)", &|e| format_number(e.d_efficiency, DIGITS));
        row("A-efficiency (%)", &|e| format_number(e.a_efficiency, DIGITS));
        row("G-efficiency (%)", &|e| format_number(e.g_efficiency, DIGITS));
        row("Average relative prediction variance (I)", &|e| format_number(e.average_variance, DIGITS));
        row("Largest relative prediction variance", &|e| format_number(e.max_variance, DIGITS));
        row("Where it is largest (coded)", &|e| {
            e.max_at.iter().map(|v| format_number(*v, 3)).collect::<Vec<_>>().join(", ")
        });
        t
    }

    fn power(&self) -> TextTable {
        let several = self.designs.len() > 1;
        let mut header = vec!["Term".to_owned(), "Anticipated coefficient (σ)".to_owned()];
        for i in 0..self.designs.len() {
            let suffix = if several { format!(" ({})", self.design_name(i)) } else { String::new() };
            header.push(format!("Relative SE{suffix}"));
            header.push(format!("VIF{suffix}"));
            header.push(format!("Power{suffix}"));
        }
        let mut t = TextTable::new(header);
        let first = &self.designs[0].evaluation;
        for (j, term) in first.terms.iter().enumerate() {
            let mut line =
                vec![self.term_label(&first.formula.terms()[term.term]), format_number(term.coefficient, DIGITS)];
            for e in self.evaluations() {
                let tp = &e.terms[j];
                line.push(format_number(tp.relative_se, DIGITS));
                line.push(format_number(tp.vif, DIGITS));
                line.push(tp.power.map_or_else(|| "— (no error df)".to_owned(), |p| format!("{p:.4}")));
            }
            t.push(line);
        }
        t
    }

    /// Group labels and styling shared by the two plots.
    fn design_labels(&self) -> Vec<String> {
        (0..self.designs.len()).map(|i| self.design_name(i)).collect()
    }

    /// One pane per factor: `d(x)` along the factor from its low to its high
    /// setting with the other factors at their midpoints. One design: a
    /// coloured curve per factor; two designs: each design's curves with its
    /// own marker shape and colour.
    fn profile_plot(&self) -> SectionContent {
        const STEPS: usize = 41;
        let k = self.factor_names.len();
        let offset = |j: usize| 3.0 * j as f64;
        let panes = (0..k)
            .map(|j| PlotPane { label: self.factor_names[j].clone(), x_min: offset(j) - 1.25, x_max: offset(j) + 1.25 })
            .collect();
        let x_ticks = (0..k)
            .flat_map(|j| {
                self.settings[j].iter().enumerate().map(move |(i, l)| (offset(j) + i as f64 - 1.0, l.clone()))
            })
            .collect();
        let curve = |e: &DesignEvaluation, j: usize| -> Vec<[f64; 2]> {
            e.profile(j, STEPS).into_iter().map(|[x, d]| [offset(j) + x, d]).collect()
        };
        let mut spec = PlotSpec {
            x_label: "Factor setting (others at their midpoints)".to_owned(),
            y_label: "Relative prediction variance".to_owned(),
            x_ticks: Some(x_ticks),
            panes,
            ..PlotSpec::default()
        };
        if self.designs.len() == 1 {
            let e = &self.designs[0].evaluation;
            spec.groups = Some(PointGroups {
                title: "Factor".to_owned(),
                labels: self.factor_names.clone(),
                membership: Vec::new(),
                lines: (0..k).map(|j| curve(e, j)).collect(),
            });
        } else {
            let mut items = Vec::new();
            for (g, e) in self.evaluations().enumerate() {
                for j in 0..k {
                    let c = curve(e, j);
                    items.extend(c.iter().step_by(5).map(|p| (p[0], p[1], String::new(), g)));
                    spec.segments.push(c);
                }
            }
            spec.annotations = Some(Annotations { groups: self.design_labels(), items, labels: Vec::new() });
        }
        SectionContent::Plot(Box::new(spec))
    }

    /// Sorted `d(x)` against the fraction of the region, one curve per
    /// design, marked every tenth of the region.
    fn fds_plot(&self) -> SectionContent {
        let lines: Vec<Vec<[f64; 2]>> = self.evaluations().map(|e| e.fds.curve(201)).collect();
        let items = self
            .evaluations()
            .enumerate()
            .flat_map(|(g, e)| {
                (0..=10).map(move |i| (i as f64 / 10.0, e.fds.quantile(i as f64 / 10.0), String::new(), g))
            })
            .collect();
        SectionContent::Plot(Box::new(PlotSpec {
            x_label: "Fraction of design space".to_owned(),
            y_label: "Relative prediction variance".to_owned(),
            groups: Some(PointGroups {
                title: self.compare_name.clone().unwrap_or_default(),
                labels: self.design_labels(),
                membership: Vec::new(),
                lines,
            }),
            annotations: Some(Annotations { groups: self.design_labels(), items, labels: Vec::new() }),
            ..PlotSpec::default()
        }))
    }

    fn fds_table(&self) -> TextTable {
        let mut t = self.header("Fraction of design space");
        for f in FRACTIONS {
            let label = match f {
                0.0 => "0 (smallest)".to_owned(),
                1.0 => "1 (largest)".to_owned(),
                f => format_number(f, 2),
            };
            t.push(std::iter::once(label).chain(self.evaluations().map(|e| format_number(e.fds.quantile(f), DIGITS))));
        }
        t
    }

    fn alias_table(&self) -> SectionContent {
        let first = &self.designs[0].evaluation;
        if first.alias_terms.is_empty() {
            return SectionContent::Note(
                "No alias terms: the model already holds every two- and three-factor interaction (enter alias \
                 terms in the report's settings to check others)."
                    .to_owned(),
            );
        }
        let several = self.designs.len() > 1;
        let mut header: Vec<String> = Vec::new();
        if several {
            header.push("Design".to_owned());
        }
        header.push("Term".to_owned());
        header.extend(first.alias_terms.iter().map(|t| self.term_label(t)));
        let mut t = TextTable::new(header);
        for (i, e) in self.evaluations().enumerate() {
            for (term, row) in e.formula.terms().iter().zip(&e.alias_matrix) {
                let mut line = Vec::new();
                if several {
                    line.push(self.design_name(i));
                }
                line.push(self.term_label(term));
                line.extend(row.iter().map(|&v| format_number(v, 3)));
                t.push(line);
            }
        }
        SectionContent::Table(t)
    }

    fn heat_map(&self, i: usize) -> SectionContent {
        let e = &self.designs[i].evaluation;
        let labels: Vec<String> = e.correlation_terms().iter().map(|t| self.term_label(t)).collect();
        let text = e.correlations.iter().map(|r| r.iter().map(|&v| format_number(v, 2)).collect()).collect();
        SectionContent::HeatMap(HeatMapSpec {
            rows: labels.clone(),
            columns: labels,
            values: e.correlations.clone(),
            text,
            measure: "|r|".to_owned(),
            limit: 1.0,
        })
    }

    fn comparison_table(&self, c: &DesignComparison) -> SectionContent {
        let (a, b) = (&self.designs[0].evaluation, &self.designs[1].evaluation);
        let mut t = TextTable::new([
            "Criterion".to_owned(),
            self.design_name(0),
            self.design_name(1),
            format!("Relative efficiency ({} to {})", self.design_name(0), self.design_name(1)),
            "Per run".to_owned(),
        ]);
        let f = |v: f64| format_number(v, DIGITS);
        t.push(["Runs".to_owned(), a.runs.to_string(), b.runs.to_string(), String::new(), String::new()]);
        t.push([
            "D: |XᵀX|^(1/p)".to_owned(),
            f((a.log_det / a.formula.n_terms() as f64).exp()),
            f((b.log_det / b.formula.n_terms() as f64).exp()),
            f(c.relative_d),
            f(c.relative_d_per_run),
        ]);
        t.push([
            "A: trace (XᵀX)⁻¹".to_owned(),
            f(a.trace_inverse),
            f(b.trace_inverse),
            f(c.relative_a),
            f(c.relative_a_per_run),
        ]);
        t.push([
            "G: largest relative prediction variance".to_owned(),
            f(a.max_variance),
            f(b.max_variance),
            f(c.relative_g),
            f(c.relative_g_per_run),
        ]);
        t.push([
            "I: average relative prediction variance".to_owned(),
            f(a.average_variance),
            f(b.average_variance),
            f(c.relative_i),
            f(c.relative_i_per_run),
        ]);
        SectionContent::Table(t)
    }

    fn note(&self) -> String {
        let e = &self.designs[0].evaluation;
        let mut text = format!(
            "Model: {}. Factors are coded −1 (low) to +1 (high); variances are relative to the error variance σ², \
             and anticipated coefficients are in units of σ (the change in the mean response from a factor's \
             midpoint to its high setting, for a main effect). Power is for the two-sided t test of each term at \
             α = {}. ",
            e.formula.describe(&self.factor_names),
            format_number(self.request.alpha, 3),
        );
        match &self.region {
            None => text.push_str(&format!(
                "The region is the cube of coded settings; the fraction-of-design-space curve and the largest \
                 variance use {} random points plus a grid with the vertices.",
                e.fds.sorted.len()
            )),
            Some(region) => text.push_str(&format!(
                "The evaluation is over the constrained region: the cube of coded settings cut by {}. The largest \
                 variance, the average variance and the fraction-of-design-space curve use only points inside it \
                 ({} quasi-random points of the region, plus the grid points inside it; the average variance's \
                 moments are estimated from {} points). The variance profiles still run across the whole cube.",
                region.describe().join("; "),
                e.fds.sorted.len(),
                e.region_points.unwrap_or(0)
            )),
        }
        if self.designs.iter().any(|d| d.evaluation.error_df == 0) {
            text.push_str(" A design with as many runs as model terms leaves no error degrees of freedom, so no test (and no power) is available for it.");
        }
        if self.comparison.is_some() {
            text.push_str(" Relative efficiencies above 1 favour the first design; per-run figures divide out the numbers of runs.");
        }
        text
    }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// A 2³ factorial in actual units (temperature 150–190, time 10–30,
    /// catalyst A/B) and a replicate of it with a third level of time.
    fn table() -> DataTable {
        let mut temp = Vec::new();
        let mut time = Vec::new();
        let mut cat = Vec::new();
        let mut design = Vec::new();
        for (d, times) in [("One", [10.0, 30.0]), ("Two", [10.0, 30.0])] {
            for &a in &[150.0, 190.0] {
                for &b in &times {
                    for c in ["A", "B"] {
                        temp.push(a);
                        time.push(b);
                        cat.push(c.to_owned());
                        design.push(d.to_owned());
                    }
                }
            }
        }
        // The second design gets two extra centre runs.
        for _ in 0..2 {
            temp.push(170.0);
            time.push(20.0);
            cat.push("A".to_owned());
            design.push("Two".to_owned());
        }
        DataTable::new(
            "eval",
            vec![
                Column::numeric("Temp", temp),
                Column::numeric("Time", time),
                Column::text("Catalyst", cat),
                Column::text("Design", design),
            ],
        )
        .unwrap()
    }

    /// The 2³ in actual units gives the hand results of the engine test: D, A,
    /// G 100%, I = 7/24, d = 1/8 at the centre.
    #[test]
    fn evaluates_a_factorial_in_actual_units() {
        let t = table();
        let mut states = RowStateManager::new(t.n_rows());
        states.set_excluded(8..t.n_rows(), true);
        let r = DesignEvaluationRequest::new(&[0, 1, 2]);
        let a = DesignEvaluationAnalysis::run(&t, &states, &r).unwrap();
        let e = a.evaluations().next().unwrap();
        assert_eq!(e.runs, 8);
        assert!((e.d_efficiency - 100.0).abs() < 1e-9 && (e.g_efficiency - 100.0).abs() < 1e-9);
        assert!((e.average_variance - 7.0 / 24.0).abs() < 1e-12);
        assert!((e.variance(&[0.0, 0.0, 0.0]) - 0.125).abs() < 1e-14);
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::EvaluationSummary,
                SectionKind::EvaluationPower,
                SectionKind::EvaluationProfile,
                SectionKind::EvaluationFds,
                SectionKind::EvaluationFdsQuantiles,
                SectionKind::EvaluationAlias,
                SectionKind::EvaluationCorrelations,
                SectionKind::EvaluationNote,
            ]
        );
        let SectionContent::Table(power) = &s[1].content else { panic!() };
        assert_eq!(power.rows.len(), 6);
        assert_eq!(power.rows[3][0], "Temp × Time");
        assert_eq!(power.rows[0][4], "0.1757", "SciPy ncf power with 1 error df");
        let SectionContent::Plot(profile) = &s[2].content else { panic!() };
        assert_eq!(profile.panes.len(), 3);
        let ticks = profile.x_ticks.as_ref().unwrap();
        assert_eq!((ticks[0].1.as_str(), ticks[2].1.as_str()), ("150", "190"));
        assert_eq!((ticks[6].1.as_str(), ticks[8].1.as_str()), ("A", "B"));
        let g = profile.groups.as_ref().unwrap();
        assert_eq!(g.lines.len(), 3);
        assert!((g.lines[0][20][1] - 0.125).abs() < 1e-14, "midpoint of Temp");
        let SectionContent::Table(alias) = &s[5].content else { panic!() };
        assert_eq!(alias.columns[1], "Temp × Time × Catalyst");
        let SectionContent::HeatMap(h) = &s[6].content else { panic!() };
        assert_eq!(h.rows.len(), 7);
        assert_eq!(a.title(), "Design Evaluation: 8 runs");
    }

    /// Two designs split by a column: the second is the first plus two
    /// centre runs (time at its midpoint is not coded 0 in design One's
    /// region, but both designs share the region of all rows).
    #[test]
    fn compares_two_designs() {
        let t = table();
        let states = RowStateManager::new(t.n_rows());
        let mut r = DesignEvaluationRequest::new(&[0, 1, 2]);
        r.model = "Temp + Time + Catalyst".to_owned();
        r.compare_by = Some(3);
        let a = DesignEvaluationAnalysis::run(&t, &states, &r).unwrap();
        let runs: Vec<usize> = a.evaluations().map(|e| e.runs).collect();
        assert_eq!(runs, [8, 10]);
        let c = a.comparison().unwrap();
        // Main effects: XᵀX = diag(8, 8, 8, 8) vs diag(10, 8, 8, 8) with the
        // centre runs at 0 except the categorical factor (A, coded −1):
        // compare with the engine directly.
        let e: Vec<&DesignEvaluation> = a.evaluations().collect();
        assert!((c.relative_d - ((e[0].log_det - e[1].log_det) / 4.0).exp()).abs() < 1e-12);
        assert!(c.relative_d < 1.0, "the larger design carries more information");
        let s = a.sections();
        assert!(s.iter().any(|s| s.kind == SectionKind::EvaluationComparison));
        assert!(s.iter().any(|s| s.kind == SectionKind::EvaluationCorrelationsSecond));
        let SectionContent::Plot(fds) = &s[3].content else { panic!() };
        assert_eq!(fds.groups.as_ref().unwrap().lines.len(), 2);
        assert_eq!(fds.annotations.as_ref().unwrap().groups, ["One", "Two"]);
        let SectionContent::Plot(profile) = &s[2].content else { panic!() };
        assert_eq!(profile.segments.len(), 6);
        assert!(a.title().contains("One vs Two"));
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        assert_eq!(summary.columns, ["Measure", "One", "Two"]);
        assert_eq!(summary.rows[0], ["Runs", "8", "10"]);

        // JSON round trip.
        let back = DesignEvaluationRequest::from_json(&r.to_json(&t), &t).unwrap();
        assert_eq!(back, r);
        let json = stat_data::parse_json(
            r#"{"analysis": "design-evaluation", "factors": ["Temp", "Time"], "coefficient": [2, 1, 0.5]}"#,
        )
        .unwrap();
        let back = DesignEvaluationRequest::from_json(&json, &t).unwrap();
        assert_eq!((back.coefficients.len(), back.model.as_str(), back.compare_by), (3, "interactions", None));
    }

    #[test]
    fn errors_are_explained() {
        let t = table();
        let states = RowStateManager::new(t.n_rows());
        let mut r = DesignEvaluationRequest::new(&[0, 1]);
        r.model = "quadratic".to_owned();
        let e = DesignEvaluationAnalysis::run(&t, &states, &r).unwrap_err().to_string();
        assert!(e.contains("cannot estimate every term"), "{e}");
        r.model = "Temp + Pressure".to_owned();
        let e = DesignEvaluationAnalysis::run(&t, &states, &r).unwrap_err().to_string();
        assert!(e.contains("Pressure"), "{e}");
        r.model = "main effects".to_owned();
        r.compare_by = Some(2);
        let mut states = RowStateManager::new(t.n_rows());
        assert!(DesignEvaluationAnalysis::run(&t, &states, &r).is_ok(), "Catalyst has two levels");
        r.compare_by = Some(0);
        assert!(DesignEvaluationAnalysis::run(&t, &states, &r).is_err(), "a factor");
        states.set_excluded(0..8, true);
        r.compare_by = Some(3);
        let e = DesignEvaluationAnalysis::run(&t, &states, &r).unwrap_err().to_string();
        assert!(e.contains("1 levels") || e.contains("exactly two"), "{e}");
        r.compare_by = None;
        r.coefficients = vec![1.0, 2.0, 3.0];
        assert!(DesignEvaluationAnalysis::run(&t, &states, &r).is_err());
    }

    /// Constraints in actual units are read against the evaluation's coding:
    /// the result equals the engine's evaluation over the same region given
    /// in coded units, and the report says it is over the constrained region.
    #[test]
    fn constraints_restrict_the_region() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let mut req = DesignEvaluationRequest::new(&[0, 1]);
        // The two centre runs give one curvature term.
        req.model = "Temp + Time + Temp*Time + Temp^2".into();
        let cube = DesignEvaluationAnalysis::run(&t, &rows, &req).unwrap();
        // Temp 150–190 (centre 170, half-range 20), Time 10–30 (20, 10):
        // Temp + 4·Time ≤ 230 is 20a + 40b ≤ −20 in coded units.
        req.constraints = "Temp + 4*Time <= 230".into();
        let cut = DesignEvaluationAnalysis::run(&t, &rows, &req).unwrap();
        let mut region = Region::cube(2);
        region.add_linear(vec![20.0, 40.0], -20.0, "by hand").unwrap();
        let formula = ModelFormula::parse(&req.model, &["Temp".to_owned(), "Time".to_owned()]).unwrap();
        let runs = DeclaredDesign::run(
            &t,
            &rows,
            &DesignDeclarationRequest { factors: req.factors.clone(), ..DesignDeclarationRequest::new(&[], &[]) },
        )
        .unwrap();
        let direct =
            stat_doe::evaluate_design_in(&formula, runs.coded_runs(), &region, &EvaluationOptions::default()).unwrap();
        let e = cut.evaluations().next().unwrap();
        assert!((e.max_variance - direct.max_variance).abs() < 1e-12);
        assert!((e.average_variance - direct.average_variance).abs() < 1e-12);
        assert_eq!(e.fds.sorted, direct.fds.sorted);
        let c = cube.evaluations().next().unwrap();
        assert!(e.max_variance <= c.max_variance + 1e-12 && (e.average_variance - c.average_variance).abs() > 1e-3);
        assert!(e.region_points.is_some() && c.region_points.is_none());
        let sections = cut.sections();
        assert!(sections.iter().any(|s| s.title == "Prediction Variance over the Constrained Region"));
        let SectionContent::Note(note) = &sections.last().unwrap().content else { panic!("note") };
        assert!(note.contains("over the constrained region") && note.contains("Temp + 4*Time <= 230"), "{note}");
        // Saved and read back with the project.
        let back = DesignEvaluationRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
        req.constraints = "Pressure < 3".into();
        let err = DesignEvaluationAnalysis::run(&t, &rows, &req).unwrap_err().to_string();
        assert!(err.contains("constraints: constraint line 1"), "{err}");
    }
}
