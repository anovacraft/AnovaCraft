//! Acceptance sampling reports ([`stat_quality::acceptance`]). Like power
//! calculations, these do not read the data table: they design a lot-by-lot
//! inspection plan from a producer's and a consumer's risk point, and
//! describe it — and any plans the analyst enters for comparison — by their
//! operating-characteristic (OC), average-outgoing-quality (AOQ, with its
//! limit AOQL), average-total-inspection (ATI) and, for double plans,
//! average-sample-number (ASN) curves.

use crate::format::format_number;
use crate::project::{invalid, s};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::JsonValue;
use stat_quality::{
    OcModel, PlanContext, PlanPoint, RiskPoints, SamplingPlan, SigmaKnowledge, double_plan, single_plan, variables_plan,
};

/// Significant digits in report tables.
const DIGITS: usize = 5;
/// Points on each curve.
const CURVE_POINTS: usize = 121;

/// Which plan to design from the risk points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanDesign {
    /// Single attribute plan `(n, c)`.
    #[default]
    Single,
    /// Double attribute plan with `n₂ = second_ratio · n₁`.
    Double {
        /// `n₂ / n₁` (1 to 5).
        second_ratio: u64,
    },
    /// k-method variables plan.
    Variables {
        /// Known σ or sample SD.
        sigma: SigmaKnowledge,
    },
    /// Design nothing: only evaluate and compare the entered plans.
    None,
}

impl PlanDesign {
    /// Key in project files.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Double { .. } => "double",
            Self::Variables { .. } => "variables",
            Self::None => "none",
        }
    }
}

/// An acceptance sampling question.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptanceRequest {
    /// The plan to design.
    pub design: PlanDesign,
    /// Distribution of the number nonconforming (attribute plans).
    pub model: OcModel,
    /// Producer's and consumer's risk points.
    pub risks: RiskPoints,
    /// Lot size `N`.
    pub lot_size: u64,
    /// Plans entered for comparison.
    pub plans: Vec<SamplingPlan>,
}

impl Default for AcceptanceRequest {
    fn default() -> Self {
        Self {
            design: PlanDesign::Single,
            model: OcModel::Binomial,
            risks: RiskPoints::default(),
            lot_size: 2000,
            plans: Vec::new(),
        }
    }
}

fn plan_to_json(p: &SamplingPlan) -> JsonValue {
    let n = |v: u64| JsonValue::Number(v as f64);
    let m: Vec<(&str, JsonValue)> = match *p {
        SamplingPlan::Single { n: size, c } => vec![("type", s("single")), ("n", n(size)), ("c", n(c))],
        SamplingPlan::Double { n1, c1, n2, c2 } => {
            vec![("type", s("double")), ("n1", n(n1)), ("c1", n(c1)), ("n2", n(n2)), ("c2", n(c2))]
        }
        SamplingPlan::Variables { n: size, k, sigma } => {
            vec![("type", s("variables")), ("n", n(size)), ("k", JsonValue::number(k)), ("sigma", s(sigma_key(sigma)))]
        }
    };
    JsonValue::Object(m.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn sigma_key(s: SigmaKnowledge) -> &'static str {
    match s {
        SigmaKnowledge::Known => "known",
        SigmaKnowledge::Unknown => "unknown",
    }
}

fn parse_sigma(v: &JsonValue) -> Result<SigmaKnowledge> {
    match v.get("sigma").and_then(JsonValue::as_str) {
        None | Some("known") => Ok(SigmaKnowledge::Known),
        Some("unknown") => Ok(SigmaKnowledge::Unknown),
        Some(other) => Err(invalid(format!("acceptance sampling: unknown σ setting {other:?}"))),
    }
}

fn plan_from_json(v: &JsonValue) -> Result<SamplingPlan> {
    let whole = |key: &str| -> Result<u64> {
        v.get(key)
            .and_then(JsonValue::as_usize)
            .map(|x| x as u64)
            .ok_or_else(|| invalid(format!("acceptance sampling: plan needs a whole number {key:?}")))
    };
    Ok(match v.get("type").and_then(JsonValue::as_str) {
        Some("single") => SamplingPlan::Single { n: whole("n")?, c: whole("c")? },
        Some("double") => {
            SamplingPlan::Double { n1: whole("n1")?, c1: whole("c1")?, n2: whole("n2")?, c2: whole("c2")? }
        }
        Some("variables") => SamplingPlan::Variables {
            n: whole("n")?,
            k: v.get("k").and_then(JsonValue::as_f64).ok_or_else(|| invalid("acceptance sampling: plan needs k"))?,
            sigma: parse_sigma(v)?,
        },
        other => return Err(invalid(format!("acceptance sampling: unknown plan type {other:?}"))),
    })
}

impl AcceptanceRequest {
    /// The project-file form (`"analysis": "acceptance-sampling"`).
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let num = JsonValue::number;
        let mut m: Vec<(String, JsonValue)> =
            vec![("analysis".into(), s("acceptance-sampling")), ("design".into(), s(self.design.key()))];
        match self.design {
            PlanDesign::Double { second_ratio } => m.push(("second_ratio".into(), num(second_ratio as f64))),
            PlanDesign::Variables { sigma } => m.push(("sigma".into(), s(sigma_key(sigma)))),
            PlanDesign::Single | PlanDesign::None => {}
        }
        let r = &self.risks;
        m.extend([
            ("model".into(), s(self.model.key())),
            ("aql".into(), num(r.aql)),
            ("alpha".into(), num(r.alpha)),
            ("ltpd".into(), num(r.ltpd)),
            ("beta".into(), num(r.beta)),
            ("lot_size".into(), num(self.lot_size as f64)),
            ("plans".into(), JsonValue::Array(self.plans.iter().map(plan_to_json).collect())),
        ]);
        JsonValue::Object(m)
    }

    /// Read the project-file form; missing settings keep their defaults.
    ///
    /// # Errors
    /// Unknown design, model or plan type; a plan with missing numbers.
    pub fn from_json(v: &JsonValue) -> Result<Self> {
        let d = Self::default();
        let num = |key: &str, default: f64| v.get(key).and_then(JsonValue::as_f64).unwrap_or(default);
        let design = match v.get("design").and_then(JsonValue::as_str) {
            None | Some("single") => PlanDesign::Single,
            Some("double") => PlanDesign::Double {
                second_ratio: v.get("second_ratio").and_then(JsonValue::as_usize).unwrap_or(2) as u64,
            },
            Some("variables") => PlanDesign::Variables { sigma: parse_sigma(v)? },
            Some("none") => PlanDesign::None,
            Some(other) => return Err(invalid(format!("acceptance sampling: unknown design {other:?}"))),
        };
        let model = match v.get("model").and_then(JsonValue::as_str) {
            None => d.model,
            Some(k) => {
                OcModel::from_key(k).ok_or_else(|| invalid(format!("acceptance sampling: unknown model {k:?}")))?
            }
        };
        let plans = match v.get("plans").and_then(JsonValue::as_array) {
            None => Vec::new(),
            Some(list) => list.iter().map(plan_from_json).collect::<Result<_>>()?,
        };
        Ok(Self {
            design,
            model,
            risks: RiskPoints {
                aql: num("aql", d.risks.aql),
                alpha: num("alpha", d.risks.alpha),
                ltpd: num("ltpd", d.risks.ltpd),
                beta: num("beta", d.risks.beta),
            },
            lot_size: v.get("lot_size").and_then(JsonValue::as_usize).map_or(d.lot_size, |n| n as u64),
            plans,
        })
    }
}

/// Extra reference lines and their labels on a curve plot.
type Extras = (Vec<Vec<[f64; 2]>>, Vec<(f64, f64, String)>);

/// One plan in the report, with its measures.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedPlan {
    /// Legend name, e.g. `"Designed"` or `"Plan 2"`.
    pub name: String,
    /// Short marker label on the plots.
    pub mark: String,
    /// The plan.
    pub plan: SamplingPlan,
    /// Measures at the producer's quality level.
    pub at_aql: PlanPoint,
    /// Measures at the consumer's quality level.
    pub at_ltpd: PlanPoint,
    /// `(AOQL, p at which it occurs)`.
    pub aoql: (f64, f64),
    /// Measures along the curve's grid of `p`.
    pub curve: Vec<PlanPoint>,
}

/// A computed acceptance sampling report.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptanceAnalysis {
    request: AcceptanceRequest,
    /// The designed plan (first) and the entered plans.
    pub plans: Vec<EvaluatedPlan>,
    designed: bool,
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

impl AcceptanceAnalysis {
    /// Design the plan (if asked) and evaluate it with the entered plans.
    ///
    /// # Errors
    /// Invalid risk points or lot size, no plan meeting the risk points,
    /// nothing to show (no design and no plans), and invalid entered plans.
    pub fn run(request: &AcceptanceRequest) -> Result<Self> {
        let r = request;
        r.risks.validate()?;
        let ctx = PlanContext { model: r.model, lot_size: r.lot_size };
        let designed = match r.design {
            PlanDesign::Single => Some(single_plan(&r.risks, &ctx)?),
            PlanDesign::Double { second_ratio } => Some(double_plan(&r.risks, &ctx, second_ratio)?),
            PlanDesign::Variables { sigma } => {
                let plan = variables_plan(&r.risks, sigma)?;
                plan.validate(&ctx)?;
                Some(plan)
            }
            PlanDesign::None => None,
        };
        if designed.is_none() && r.plans.is_empty() {
            return Err(invalid("enter at least one plan to evaluate, or choose a plan to design"));
        }
        let named: Vec<(String, String, SamplingPlan)> = designed
            .iter()
            .map(|p| ("Designed".to_owned(), "D".to_owned(), *p))
            .chain(r.plans.iter().enumerate().map(|(i, p)| (format!("Plan {}", i + 1), (i + 1).to_string(), *p)))
            .collect();
        for (_, _, p) in &named {
            p.validate(&ctx)?;
        }
        // Grid: from 0 to where every plan accepts less than 1% of lots
        // (at least 1.5 × the consumer's level).
        let mut p_max = (1.5 * r.risks.ltpd).min(1.0);
        while p_max < 1.0 {
            let mut done = true;
            for (_, _, p) in &named {
                if p.acceptance_probability(p_max, &ctx)? > 0.01 {
                    done = false;
                }
            }
            if done {
                break;
            }
            p_max = (p_max * 1.25).min(1.0);
        }
        let plans = named
            .into_iter()
            .map(|(name, mark, plan)| {
                let curve = (0..CURVE_POINTS)
                    .map(|i| plan.evaluate(p_max * i as f64 / (CURVE_POINTS - 1) as f64, &ctx))
                    .collect::<Result<Vec<_>>>()?;
                Ok(EvaluatedPlan {
                    name,
                    mark,
                    at_aql: plan.evaluate(r.risks.aql, &ctx)?,
                    at_ltpd: plan.evaluate(r.risks.ltpd, &ctx)?,
                    aoql: plan.aoql(&ctx)?,
                    plan,
                    curve,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { request: r.clone(), plans, designed: designed.is_some() })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &AcceptanceRequest {
        &self.request
    }

    /// The designed plan, if one was asked for.
    #[must_use]
    pub fn designed(&self) -> Option<&SamplingPlan> {
        self.designed.then(|| &self.plans[0].plan)
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match self.request.design {
            PlanDesign::Single => "Acceptance Sampling: Single Attribute Plan".to_owned(),
            PlanDesign::Double { .. } => "Acceptance Sampling: Double Attribute Plan".to_owned(),
            PlanDesign::Variables { .. } => "Acceptance Sampling: Variables Plan".to_owned(),
            PlanDesign::None => "Acceptance Sampling: Plan Comparison".to_owned(),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.requirements()];
        if self.designed {
            out.push(self.plan_table());
        }
        if self.plans.len() > 1 || !self.designed {
            out.push(self.comparison());
        }
        out.push(self.oc_curve());
        out.push(self.aoq_curve());
        out.push(self.ati_curve());
        if self.plans.iter().any(|p| matches!(p.plan, SamplingPlan::Double { .. })) {
            out.push(self.asn_curve());
        }
        out
    }

    fn requirements(&self) -> ReportSection {
        let r = &self.request;
        let f = |v: f64| format_number(v, DIGITS);
        let mut t = TextTable::new(["Setting", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Producer's quality level p₁ (AQL)", f(r.risks.aql));
        row("Producer's risk α", f(r.risks.alpha));
        row("Consumer's quality level p₂ (LTPD)", f(r.risks.ltpd));
        row("Consumer's risk β", f(r.risks.beta));
        row("Lot size N", r.lot_size.to_string());
        let any_attribute = self.plans.iter().any(|p| !matches!(p.plan, SamplingPlan::Variables { .. }))
            || matches!(r.design, PlanDesign::Single | PlanDesign::Double { .. });
        if any_attribute {
            row("Model for attribute plans", r.model.label().to_owned());
        }
        if self.plans.iter().any(|p| matches!(p.plan, SamplingPlan::Variables { .. })) {
            row("Model for variables plans", "normal measurements, one specification limit".to_owned());
        }
        section(SectionKind::AcceptanceRequirements, "Risk Points", SectionContent::Table(t))
    }

    fn plan_table(&self) -> ReportSection {
        let e = &self.plans[0];
        let r = &self.request.risks;
        let f = |v: f64| format_number(v, 4);
        let mut t = TextTable::new(["Quantity", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Plan", e.plan.kind_label().to_owned());
        match e.plan {
            SamplingPlan::Single { n, c } => {
                row("Sample size n", n.to_string());
                row("Acceptance number c", c.to_string());
                row("Rule", format!("inspect {n}; accept the lot with at most {c} nonconforming"));
            }
            SamplingPlan::Double { n1, c1, n2, c2 } => {
                row("First sample n₁", n1.to_string());
                row("First acceptance number c₁", c1.to_string());
                row("Second sample n₂", n2.to_string());
                row("Second acceptance number c₂", c2.to_string());
                row(
                    "Rule",
                    format!(
                        "inspect {n1}: accept with at most {c1} nonconforming, reject with more than {c2}; \
                         otherwise inspect {n2} more and accept with at most {c2} in all"
                    ),
                );
            }
            SamplingPlan::Variables { n, k, sigma } => {
                row("Sample size n", n.to_string());
                row("Acceptability constant k", format_number(k, DIGITS));
                let sd = if sigma == SigmaKnowledge::Known { "σ" } else { "s" };
                row("Rule", format!("measure {n}; accept when (USL − x̄)/{sd} ≥ k (lower limit: (x̄ − LSL)/{sd} ≥ k)"));
            }
        }
        row("Probability of acceptance at p₁", f(e.at_aql.pa));
        row("Probability of acceptance at p₂", f(e.at_ltpd.pa));
        row("Producer's risk achieved", f(1.0 - e.at_aql.pa));
        row("Consumer's risk achieved", f(e.at_ltpd.pa));
        row("AOQL (rejected lots screened)", format!("{} at p = {}", f(e.aoql.0), f(e.aoql.1)));
        row("Average total inspection at p₁", format_number(e.at_aql.ati, DIGITS));
        if matches!(e.plan, SamplingPlan::Double { .. }) {
            row("Average sample number at p₁", format_number(e.at_aql.asn, DIGITS));
            row("Average sample number at p₂", format_number(e.at_ltpd.asn, DIGITS));
        }
        let meets = e.at_aql.pa >= 1.0 - r.alpha && e.at_ltpd.pa <= r.beta;
        let title = if meets { "Sampling Plan (meets both risk points)" } else { "Sampling Plan" };
        section(SectionKind::AcceptancePlan, title, SectionContent::Table(t))
    }

    fn comparison(&self) -> ReportSection {
        let f = |v: f64| format_number(v, 4);
        let mut t = TextTable::new([
            "Plan",
            "Mark",
            "Type",
            "Settings",
            "Pa at p₁",
            "Pa at p₂",
            "AOQL",
            "ASN at p₁",
            "ATI at p₁",
            "Meets both points",
        ]);
        let r = &self.request.risks;
        for e in &self.plans {
            let meets = e.at_aql.pa >= 1.0 - r.alpha && e.at_ltpd.pa <= r.beta;
            t.push([
                e.name.clone(),
                e.mark.clone(),
                e.plan.kind_label().to_owned(),
                e.plan.describe(),
                f(e.at_aql.pa),
                f(e.at_ltpd.pa),
                f(e.aoql.0),
                format_number(e.at_aql.asn, DIGITS),
                format_number(e.at_aql.ati, DIGITS),
                if meets { "yes" } else { "no" }.to_owned(),
            ]);
        }
        section(SectionKind::AcceptanceComparison, "Plan Comparison", SectionContent::Table(t))
    }

    /// One plot of `measure` against `p` for every plan: the first plan is
    /// the main line, the others thin lines; each plan carries a labelled
    /// marker (its own colour and shape) at `marker(plan)`'s index.
    fn curves(
        &self,
        kind: SectionKind,
        title: String,
        (y_label, line_name): (&str, &str),
        measure: &dyn Fn(&PlanPoint) -> f64,
        marker: &dyn Fn(&EvaluatedPlan) -> usize,
        extra: Extras,
    ) -> ReportSection {
        let line = |e: &EvaluatedPlan| -> Vec<[f64; 2]> { e.curve.iter().map(|q| [q.p, measure(q)]).collect() };
        let (mut segments, labels) = extra;
        segments.extend(self.plans.iter().skip(1).map(line));
        let mut annotations = Annotations { groups: Vec::new(), items: Vec::new(), labels };
        for (g, e) in self.plans.iter().enumerate() {
            annotations.groups.push(format!("{} ({}): {}", e.name, e.mark, e.plan.describe()));
            let q = &e.curve[marker(e).min(e.curve.len() - 1)];
            annotations.items.push((q.p, measure(q), e.mark.clone(), g));
        }
        section(
            kind,
            title,
            SectionContent::Plot(Box::new(PlotSpec {
                x_label: "Lot fraction nonconforming p".to_owned(),
                y_label: y_label.to_owned(),
                points: Vec::new(),
                line: Some(line(&self.plans[0])),
                line_name: Some(line_name.to_owned()),
                band: None,
                groups: None,
                x_ticks: None,
                panes: Vec::new(),
                annotations: Some(annotations),
                x_time: None,
                y_time: None,
                segments,
                ..PlotSpec::default()
            })),
        )
    }

    /// Index of the grid point where `measure` is nearest `target`.
    fn nearest(e: &EvaluatedPlan, measure: impl Fn(&PlanPoint) -> f64, target: f64) -> usize {
        e.curve
            .iter()
            .enumerate()
            .min_by(|a, b| (measure(a.1) - target).abs().total_cmp(&(measure(b.1) - target).abs()))
            .map_or(0, |(i, _)| i)
    }

    fn oc_curve(&self) -> ReportSection {
        let r = &self.request.risks;
        let end = self.plans[0].curve.last().map_or(1.0, |q| q.p);
        // Risk points: dotted-looking cross hairs as thin lines, with labels.
        let extra = (
            vec![
                vec![[r.aql, 0.0], [r.aql, 1.0 - r.alpha], [0.0, 1.0 - r.alpha]],
                vec![[r.ltpd, 0.0], [r.ltpd, r.beta], [0.0, r.beta]],
            ],
            vec![
                (
                    r.aql,
                    1.0 - r.alpha + 0.04,
                    format!("p₁ = {}, 1 − α = {}", format_number(r.aql, 4), format_number(1.0 - r.alpha, 4)),
                ),
                (
                    r.ltpd + 0.02 * end,
                    r.beta + 0.04,
                    format!("p₂ = {}, β = {}", format_number(r.ltpd, 4), format_number(r.beta, 4)),
                ),
            ],
        );
        self.curves(
            SectionKind::AcceptanceOc,
            "Operating Characteristic Curve (thin lines: risk points)".to_owned(),
            ("Probability of acceptance", "OC curve"),
            &|q| q.pa,
            &|e| Self::nearest(e, |q| q.pa, 0.5),
            extra,
        )
    }

    fn aoq_curve(&self) -> ReportSection {
        let main = &self.plans[0];
        let end = main.curve.last().map_or(1.0, |q| q.p);
        let extra = (
            vec![vec![[0.0, main.aoql.0], [end, main.aoql.0]]],
            vec![(0.75 * end, main.aoql.0, format!("AOQL {}", format_number(main.aoql.0, 4)))],
        );
        self.curves(
            SectionKind::AcceptanceAoq,
            format!("Average Outgoing Quality ({} AOQL = {})", main.name.to_lowercase(), format_number(main.aoql.0, 4)),
            ("Average outgoing quality", "AOQ curve"),
            &|q| q.aoq,
            &|e| Self::nearest(e, |q| q.p, e.aoql.1),
            extra,
        )
    }

    fn ati_curve(&self) -> ReportSection {
        let lot = self.request.lot_size as f64;
        let end = self.plans[0].curve.last().map_or(1.0, |q| q.p);
        let extra = (vec![vec![[0.0, lot], [end, lot]]], vec![(0.15 * end, lot, format!("Lot size {lot}"))]);
        self.curves(
            SectionKind::AcceptanceAti,
            "Average Total Inspection (rejected lots screened)".to_owned(),
            ("Average total inspection", "ATI curve"),
            &|q| q.ati,
            &|e| Self::nearest(e, |q| q.ati, 0.5 * (e.curve[0].ati + lot)),
            extra,
        )
    }

    fn asn_curve(&self) -> ReportSection {
        self.curves(
            SectionKind::AcceptanceAsn,
            "Average Sample Number".to_owned(),
            ("Average sample number", "ASN curve"),
            &|q| q.asn,
            &|e| Self::nearest(e, |q| q.p, self.request.risks.ltpd),
            (Vec::new(), Vec::new()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_plan_report() {
        let a = AcceptanceAnalysis::run(&AcceptanceRequest::default()).unwrap();
        assert_eq!(a.designed(), Some(&SamplingPlan::Single { n: 110, c: 3 }));
        assert_eq!(a.title(), "Acceptance Sampling: Single Attribute Plan");
        let s = a.sections();
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::AcceptanceRequirements,
                SectionKind::AcceptancePlan,
                SectionKind::AcceptanceOc,
                SectionKind::AcceptanceAoq,
                SectionKind::AcceptanceAti,
            ]
        );
        assert!(s[1].title.contains("meets both"));
        let SectionContent::Plot(oc) = &s[2].content else { panic!() };
        let line = oc.line.as_ref().unwrap();
        assert_eq!(line.len(), CURVE_POINTS);
        assert!((line[0][1] - 1.0).abs() < 1e-12 && line.last().unwrap()[1] < 0.01);
        assert!(line.windows(2).all(|w| w[1][1] <= w[0][1] + 1e-12), "OC curve falls");
        assert_eq!(oc.line_label(), "OC curve");
        let SectionContent::Plot(aoq) = &s[3].content else { panic!() };
        let peak = aoq.line.as_ref().unwrap().iter().map(|q| q[1]).fold(0.0, f64::max);
        assert!(peak <= a.plans[0].aoql.0 + 1e-12, "the AOQL is the curve's maximum");
        assert_eq!(aoq.line_label(), "AOQ curve");
        let SectionContent::Plot(ati) = &s[4].content else { panic!() };
        assert_eq!(ati.line_label(), "ATI curve");
    }

    #[test]
    fn comparison_and_double_plan() {
        let request = AcceptanceRequest {
            design: PlanDesign::Double { second_ratio: 2 },
            plans: vec![
                SamplingPlan::Single { n: 89, c: 2 },
                SamplingPlan::Variables { n: 15, k: 1.89, sigma: SigmaKnowledge::Known },
            ],
            lot_size: 10_000,
            ..AcceptanceRequest::default()
        };
        let a = AcceptanceAnalysis::run(&request).unwrap();
        assert_eq!(a.plans.len(), 3);
        let s = a.sections();
        assert_eq!(s.last().unwrap().kind, SectionKind::AcceptanceAsn);
        let SectionContent::Table(c) = &s[2].content else { panic!() };
        assert_eq!(c.rows.len(), 3);
        assert_eq!(c.rows[1][9], "no", "n = 89, c = 2 misses the producer's point");
        let SectionContent::Plot(oc) = &s[3].content else { panic!() };
        assert_eq!(oc.segments.len(), 2 + 2, "risk-point lines and the two other plans");
        assert_eq!(oc.annotations.as_ref().unwrap().groups.len(), 3);
        // Only comparing: nothing designed, an error without plans.
        let none = AcceptanceRequest { design: PlanDesign::None, ..request.clone() };
        let b = AcceptanceAnalysis::run(&none).unwrap();
        assert!(b.designed().is_none() && b.title().ends_with("Plan Comparison"));
        assert!(AcceptanceAnalysis::run(&AcceptanceRequest { plans: Vec::new(), ..none }).is_err());
    }

    #[test]
    fn json_round_trip() {
        let request = AcceptanceRequest {
            design: PlanDesign::Variables { sigma: SigmaKnowledge::Unknown },
            model: OcModel::Hypergeometric,
            risks: RiskPoints { aql: 0.02, alpha: 0.05, ltpd: 0.08, beta: 0.1 },
            lot_size: 800,
            plans: vec![
                SamplingPlan::Double { n1: 30, c1: 0, n2: 60, c2: 3 },
                SamplingPlan::Variables { n: 20, k: 1.7, sigma: SigmaKnowledge::Unknown },
                SamplingPlan::Single { n: 50, c: 1 },
            ],
        };
        assert_eq!(AcceptanceRequest::from_json(&request.to_json()).unwrap(), request);
        let double = AcceptanceRequest { design: PlanDesign::Double { second_ratio: 1 }, ..request };
        assert_eq!(AcceptanceRequest::from_json(&double.to_json()).unwrap(), double);
    }
}
