//! Report sections of nonlinear regressions made from an assay curve
//! template ([`crate::curve_templates`]): the parameters' meanings, derived
//! quantities, standard-curve acceptance and quantitation range, the
//! inhibition-mechanism comparison, Schild analysis and relative potency.

use super::{DIGITS, FitOutcome, NonlinearAnalysis, NonlinearRequest, Setup, Weighting, grid, percent, section, span};
use crate::curve_templates::{CurveTemplate, Extra, SecondKind, TemplateSettings};
use crate::format::{format_number, format_p_value};
use crate::formula::compile_with_parameters;
use crate::report::{
    Annotations, PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use stat_core::Result;
use stat_core::distributions::{ContinuousDistribution, StudentsT};
use stat_data::{Column, DataTable, RowStateManager};
use stat_linear::nonlinear::assay::{self, DerivedValue};
use stat_linear::nonlinear::compare::{FitScore, extra_sum_of_squares};

const PROFILE_POINTS: usize = 80;

fn num(v: f64) -> String {
    if v.is_finite() { format_number(v, DIGITS) } else { "—".to_owned() }
}

/// What a template's report computes while fitting.
#[derive(Debug, Clone, Default)]
pub(super) struct TemplateExtras {
    /// The four reversible inhibition mechanisms fitted to the same rows.
    mechanisms: Vec<(CurveTemplate, std::result::Result<FitScore, String>)>,
    /// Schild analysis from separate EC50s per antagonist concentration.
    schild: Option<std::result::Result<Schild, String>>,
}

/// Dose ratios of a Schild analysis.
#[derive(Debug, Clone)]
struct Schild {
    /// `(antagonist concentration, dose ratio)` of each concentration above
    /// the lowest (control) one.
    ratios: Vec<(f64, f64)>,
}

/// Template comparisons added to the model-comparison table: parallelism of
/// relative-potency curves and a Schild slope of 1.
pub(super) fn comparisons(
    t: &TemplateSettings,
    setup: &Setup<'_>,
    outcome: &FitOutcome,
    shared: &[bool],
    compare: &mut dyn FnMut(String, String, String, Result<FitOutcome>, bool),
) {
    match t.template.extra() {
        Extra::Potency if setup.groups.len() > 1 => {
            let separate = vec![false; shared.len()];
            compare(
                "Are the curves parallel? (common Bottom, Top and Hill)".to_owned(),
                "Parallel curves (this fit)".to_owned(),
                "Separate curves".to_owned(),
                setup.fit(&outcome.positions, &separate, Some(outcome), false, false),
                false,
            );
        }
        Extra::Schild => {
            let k = 5;
            let fixed = setup.settings.get(k).is_some_and(|p| p.lower.is_some() && p.lower == p.upper);
            if !fixed {
                let mut settings = setup.settings.clone();
                settings[k].start = Some(1.0);
                settings[k].lower = Some(1.0);
                settings[k].upper = Some(1.0);
                let one = Setup { settings, ..setup.clone() };
                compare(
                    "Is the Schild slope 1? (simple competitive antagonism)".to_owned(),
                    "Schild slope fixed at 1".to_owned(),
                    "This fit".to_owned(),
                    one.fit(&outcome.positions, shared, Some(outcome), false, false),
                    true,
                );
            }
        }
        _ => {}
    }
}

/// Extra fits a template's report needs.
pub(super) fn extras(
    t: &TemplateSettings,
    table: &DataTable,
    states: &RowStateManager,
    request: &NonlinearRequest,
    setup: &Setup<'_>,
    outcome: &FitOutcome,
) -> TemplateExtras {
    let mut out = TemplateExtras::default();
    match t.template.extra() {
        Extra::Mechanism => {
            for alt in [
                CurveTemplate::Competitive,
                CurveTemplate::Uncompetitive,
                CurveTemplate::Noncompetitive,
                CurveTemplate::Mixed,
            ] {
                let score = if alt == t.template {
                    Ok(FitScore { sse: outcome.fit.sse, n: outcome.fit.n, parameters: outcome.fit.n_estimated })
                } else {
                    let settings = TemplateSettings { template: alt, ..t.clone() };
                    settings
                        .request(table, request.response, request.group)
                        .and_then(|r| {
                            let super::NonlinearModel::Formula { formula, .. } = &r.model else {
                                return Err(crate::project::invalid("not a formula"));
                            };
                            let alt_setup = Setup {
                                kind: super::Kind::Formula { text: formula.clone() },
                                settings: r.parameters.clone(),
                                ..setup.clone()
                            };
                            let shared: Vec<bool> = r.parameters.iter().map(|p| p.shared).collect();
                            alt_setup.fit(&outcome.positions, &shared, None, false, false)
                        })
                        .map(|f| FitScore { sse: f.fit.sse, n: f.fit.n, parameters: f.fit.n_estimated })
                        .map_err(|e| e.to_string())
                };
                out.mechanisms.push((alt, score));
            }
        }
        Extra::Schild => {
            out.schild = Some(schild(t, table, states, request).map_err(|e| e.to_string()));
        }
        _ => {}
    }
    out
}

/// Separate EC50s of the agonist curve at each antagonist concentration
/// (Bottom, Top and Hill shared), and the dose ratios against the lowest.
fn schild(
    t: &TemplateSettings,
    table: &DataTable,
    states: &RowStateManager,
    request: &NonlinearRequest,
) -> Result<Schild> {
    let second = t.second.ok_or_else(|| crate::project::invalid("no antagonist column"))?;
    let agonist = TemplateSettings::new(CurveTemplate::Agonist, t.x);
    let mut r = agonist.request(table, request.response, Some(second))?;
    r.parameters[2].shared = false;
    r.template = None;
    r.profile_intervals = false;
    r.weighting = request.weighting;
    r.max_iterations = request.max_iterations;
    let a = NonlinearAnalysis::run(table, states, &r)?;
    let theta = a.fit().estimates();
    let levels: Vec<f64> = a.groups.iter().map(|g| g.trim().parse::<f64>().unwrap_or(f64::NAN)).collect();
    let base = (0..levels.len()).min_by(|&i, &j| levels[i].total_cmp(&levels[j])).unwrap_or(0);
    let log_ec50 = |g: usize| theta[a.outcome.map[g][2]];
    let ratios = (0..levels.len())
        .filter(|&g| g != base && levels[g] > 0.0)
        .map(|g| (levels[g], 10f64.powf(log_ec50(g) - log_ec50(base))))
        .collect();
    Ok(Schild { ratios })
}

impl NonlinearAnalysis {
    /// The template's report sections.
    pub(super) fn template_sections(&self) -> Vec<ReportSection> {
        let Some(t) = &self.request.template else { return Vec::new() };
        let mut out = vec![self.template_parameters(t)];
        if !t.template.derived().is_empty() {
            out.push(self.derived_section(t));
        }
        match t.template.extra() {
            Extra::StandardCurve => out.extend(self.standard_curve_sections(t)),
            Extra::Mechanism => out.push(self.mechanism_section()),
            Extra::Schild => out.push(self.schild_section(t)),
            Extra::Potency => out.push(self.potency_section(t)),
            Extra::None => {}
        }
        if t.template.second().is_some() {
            if let Some(s) = self.curves_by_second(t) {
                out.push(s);
            }
        }
        out
    }

    /// Model, parameters' meanings and the constants used.
    fn template_parameters(&self, t: &TemplateSettings) -> ReportSection {
        let mut table = TextTable::new(["Item", "Value"]);
        table.push(["Template".to_owned(), t.template.label().to_owned()]);
        table.push(["Model".to_owned(), t.template.equation().to_owned()]);
        for p in t.template.parameters() {
            table.push([format!("{} (parameter)", p.name), p.meaning.to_owned()]);
        }
        for (spec, v) in t.template.all_constants().iter().zip(&t.constants) {
            table.push([
                spec.label.to_owned(),
                if v.is_finite() { format_number(*v, 8) } else { "not given".to_owned() },
            ]);
        }
        section(SectionKind::TemplateModel, "Assay Template", SectionContent::Table(table))
    }

    /// Whether derived quantities are given per data set.
    fn per_data_set(&self) -> bool {
        self.groups.len() > 1 && self.request.parameters.iter().any(|p| !p.shared)
    }

    /// Derived quantities of data set `g`, with the labels of those that
    /// need a constant not given.
    fn derived_values(
        &self,
        t: &TemplateSettings,
        g: usize,
    ) -> (Vec<(&'static str, DerivedValue, &'static str)>, Vec<&'static str>) {
        let fit = self.fit();
        let theta = fit.estimates();
        let map = &self.outcome.map[g];
        let c = &t.constants;
        let mut values = Vec::new();
        let mut missing = Vec::new();
        for d in t.template.derived() {
            if d.needs.iter().any(|&i| !c.get(i).is_some_and(|v| v.is_finite())) {
                missing.push(d.label);
                continue;
            }
            let local = |th: &[f64]| -> Vec<f64> { map.iter().map(|&i| th[i]).collect() };
            let mut value =
                assay::delta_method(&|th: &[f64]| (d.value)(&local(th), c), &theta, &fit.covariance, fit.t_critical);
            let mut how = "delta method";
            if let Some(k) = d.single {
                let p = &fit.parameters[map[k]];
                let interval = match &p.profile_interval {
                    Some(pi) if pi.lower.is_some() && pi.upper.is_some() => {
                        how = "profile interval of the parameter";
                        (pi.lower.unwrap_or(f64::NAN), pi.upper.unwrap_or(f64::NAN))
                    }
                    _ => {
                        how = "Wald interval of the parameter";
                        p.wald_interval
                    }
                };
                let base = local(&theta);
                let at = |v: f64| {
                    let mut q = base.clone();
                    q[k] = v;
                    (d.value)(&q, c)
                };
                value.interval = assay::map_interval(&at, interval);
            }
            values.push((d.label, value, how));
        }
        (values, missing)
    }

    fn derived_section(&self, t: &TemplateSettings) -> ReportSection {
        let level = percent(self.fit().confidence_level);
        let per = self.per_data_set();
        let mut heads = vec!["Quantity".to_owned()];
        if per {
            heads.push(self.group_name.clone().unwrap_or_else(|| "Data set".to_owned()));
        }
        heads.extend([
            "Estimate".to_owned(),
            "Std Error".to_owned(),
            format!("Lower {level}%"),
            format!("Upper {level}%"),
            "Interval from".to_owned(),
        ]);
        let mut table = TextTable::new(heads);
        let mut missing_all: Vec<&str> = Vec::new();
        let sets: Vec<usize> = if per { (0..self.groups.len()).collect() } else { vec![0] };
        for g in sets {
            let (values, missing) = self.derived_values(t, g);
            for (label, v, how) in values {
                let mut row = vec![label.to_owned()];
                if per {
                    row.push(self.groups[g].clone());
                }
                row.extend([num(v.estimate), num(v.std_error), num(v.interval.0), num(v.interval.1), how.to_owned()]);
                table.push(row);
            }
            for m in missing {
                if !missing_all.contains(&m) {
                    missing_all.push(m);
                }
            }
        }
        let title = if missing_all.is_empty() {
            "Derived Quantities".to_owned()
        } else {
            format!("Derived Quantities (enter the template's constants for: {})", missing_all.join("; "))
        };
        section(SectionKind::TemplateDerived, title, SectionContent::Table(table))
    }

    /// The model of data set `g` at `xs`, with the second column at `second`
    /// (a number, or a label for an indicator), evaluated from the formula.
    fn curve_at(&self, t: &TemplateSettings, g: usize, xs: &[f64], second: Option<&str>) -> Option<Vec<f64>> {
        let super::Kind::Formula { text } = &self.predictor.kind else { return None };
        let n = xs.len();
        let x_name = self.x_name_of(t)?;
        let second_name = t.second.and_then(|j| self.source_name(j));
        let mini = &self.predictor.mini;
        let columns: Vec<Column> = mini
            .columns()
            .iter()
            .map(|c| {
                if c.name == x_name {
                    Column::numeric(c.name.clone(), xs.to_vec())
                } else if second_name.as_deref() == Some(c.name.as_str()) {
                    let s = second.unwrap_or_default();
                    match c.as_numeric() {
                        Some(_) => Column::numeric(c.name.clone(), vec![s.parse::<f64>().unwrap_or(f64::NAN); n]),
                        None => Column::text(c.name.clone(), vec![s.to_owned(); n]),
                    }
                } else {
                    match c.as_numeric() {
                        Some(v) => Column::numeric(c.name.clone(), vec![v[0]; n]),
                        None => Column::text(c.name.clone(), vec![c.level_text(0); n]),
                    }
                }
            })
            .collect();
        let table = DataTable::new("curve", columns).ok()?;
        let mut compiled = compile_with_parameters(&table, text, &self.predictor.parameter_names).ok()?;
        let theta = self.fit().estimates();
        let local: Vec<f64> = self.outcome.map[g].iter().map(|&i| theta[i]).collect();
        let rows: Vec<usize> = (0..n).collect();
        let mut out = vec![0.0; n];
        compiled.evaluate(&table, &rows, &local, &mut out).ok()?;
        Some(out)
    }

    fn x_name_of(&self, t: &TemplateSettings) -> Option<String> {
        self.source_name(t.x)
    }

    /// Name of a table column, from the names the analysis kept.
    fn source_name(&self, j: usize) -> Option<String> {
        self.column_names.get(j).cloned()
    }

    /// Data and fitted curves by the level of the second column.
    fn curves_by_second(&self, t: &TemplateSettings) -> Option<ReportSection> {
        let j = t.second?;
        let role = t.template.second()?;
        let second_name = self.source_name(j)?;
        let x_name = self.x_name_of(t)?;
        let levels_of = |r: usize| self.second_text.get(r).cloned().unwrap_or_default();
        let mut labels: Vec<String> = Vec::new();
        for &r in &self.rows_used {
            let l = levels_of(r);
            if !labels.contains(&l) {
                labels.push(l);
            }
        }
        if role.kind == SecondKind::Numeric {
            labels
                .sort_by(|a, b| a.parse::<f64>().unwrap_or(f64::NAN).total_cmp(&b.parse::<f64>().unwrap_or(f64::NAN)));
        }
        let x_of = |r: usize| self.x_values.get(r).copied().unwrap_or(f64::NAN);
        let points: Vec<RowPoint> =
            self.rows_used.iter().zip(&self.y_used).map(|(&row, &y)| RowPoint { row, x: x_of(row), y }).collect();
        let membership: Vec<usize> =
            self.rows_used.iter().map(|&r| labels.iter().position(|l| *l == levels_of(r)).unwrap_or(0)).collect();
        let xs_all: Vec<f64> = points.iter().map(|p| p.x).collect();
        let (lo, hi) = span(&xs_all);
        let gx = grid(lo, hi, 150);
        let mut lines: Vec<Polyline> = Vec::new();
        for l in &labels {
            // Each level's curve in the data set its rows belong to.
            let g =
                self.rows_used.iter().zip(&self.group_used).find(|(r, _)| levels_of(**r) == *l).map_or(0, |(_, g)| *g);
            let v = self.curve_at(t, g, &gx, Some(l)).unwrap_or_default();
            lines.push(gx.iter().zip(v).filter(|(_, v)| v.is_finite()).map(|(x, v)| [*x, v]).collect());
        }
        let spec = PlotSpec {
            x_label: x_name,
            y_label: self.response_name.clone(),
            points,
            groups: Some(PointGroups { title: second_name.clone(), labels, membership, lines }),
            ..PlotSpec::default()
        };
        Some(section(
            SectionKind::TemplateCurves,
            format!("Fitted Curves by {second_name}"),
            SectionContent::Plot(Box::new(spec)),
        ))
    }

    /// Weight of a new observation with fitted value `v`.
    fn weight_at(&self, v: f64) -> f64 {
        match self.request.weighting {
            Weighting::InverseResponseSquared if v != 0.0 => 1.0 / (v * v),
            Weighting::InverseResponse if v != 0.0 => 1.0 / v.abs(),
            _ => 1.0,
        }
    }

    /// Concentrations read off the curve for responses `ys` within
    /// `range` (positive): the first crossing on a log-spaced grid, refined
    /// by bisection in log concentration for all responses at once.
    fn read_off_all(&self, ys: &[f64], range: (f64, f64)) -> Vec<Option<f64>> {
        const GRID: usize = 400;
        let mut out = vec![None; ys.len()];
        let (lo, hi) = range;
        if !(lo > 0.0 && hi > lo) {
            return out;
        }
        let mut ev = self.predictor.evaluator();
        let theta = self.fit().estimates();
        let mut at = |us: &[f64]| {
            let points: Vec<Vec<f64>> = us.iter().map(|u| vec![u.exp()]).collect();
            self.predictor.values_with(&mut ev, 0, &points, &theta)
        };
        let us = grid(lo.ln(), hi.ln(), GRID);
        let Some(f) = at(&us) else { return out };
        // Brackets [a, b] in ln x with the sign of f − y at a.
        let mut brackets: Vec<(usize, f64, f64, bool)> = Vec::new();
        for (k, &y) in ys.iter().enumerate() {
            if !y.is_finite() {
                continue;
            }
            if let Some(i) = (0..GRID - 1).find(|&i| (f[i] - y) * (f[i + 1] - y) <= 0.0) {
                brackets.push((k, us[i], us[i + 1], f[i] - y < 0.0));
            }
        }
        for _ in 0..50 {
            if brackets.is_empty() {
                break;
            }
            let mids: Vec<f64> = brackets.iter().map(|b| 0.5 * (b.1 + b.2)).collect();
            let Some(fm) = at(&mids) else { break };
            for (b, (&m, &v)) in brackets.iter_mut().zip(mids.iter().zip(&fm)) {
                if (v - ys[b.0] < 0.0) == b.3 {
                    b.1 = m;
                } else {
                    b.2 = m;
                }
            }
        }
        for b in brackets {
            out[b.0] = Some((0.5 * (b.1 + b.2)).exp());
        }
        out
    }

    /// Positive standards: `(row, nominal, response)`.
    fn standards(&self) -> Vec<(usize, f64, f64)> {
        self.rows_used
            .iter()
            .zip(self.x_used.iter().zip(&self.y_used))
            .filter(|(_, (x, _))| **x > 0.0)
            .map(|(&r, (&x, &y))| (r, x, y))
            .collect()
    }

    /// Precision profile `(concentration, CV %)` over the standards' range.
    fn precision_profile(&self) -> Vec<(f64, f64)> {
        let s = self.standards();
        let (lo, hi) = span(&s.iter().map(|v| v.1).collect::<Vec<_>>());
        if !(lo > 0.0 && hi > lo) {
            return Vec::new();
        }
        let fit = self.fit();
        let theta = fit.estimates();
        let t = fit.t_critical;
        let mut ev = self.predictor.evaluator();
        let cs: Vec<f64> = grid(lo.ln(), hi.ln(), PROFILE_POINTS).into_iter().map(f64::exp).collect();
        let w = |v: f64| self.weight_at(v);
        // Prediction limits use each point's own weight.
        let mut out = Vec::new();
        let up: Vec<Vec<f64>> = cs.iter().map(|c| vec![c * (1.0 + 1e-4)]).collect();
        let down: Vec<Vec<f64>> = cs.iter().map(|c| vec![c * (1.0 - 1e-4)]).collect();
        let (Some(d_up), Some(d_down)) = (
            self.predictor.values_with(&mut ev, 0, &up, &theta),
            self.predictor.values_with(&mut ev, 0, &down, &theta),
        ) else {
            return Vec::new();
        };
        let points: Vec<Vec<f64>> = cs.iter().map(|&c| vec![c]).collect();
        let Some(limits) = self.predictor.with_limits_with(&mut ev, fit, 0, &points, &w) else { return Vec::new() };
        for (k, &c) in cs.iter().enumerate() {
            let (v, _, (_, p_hi)) = limits[k];
            let slope = (d_up[k] - d_down[k]) / (2e-4 * c);
            let se = (p_hi - v) / t;
            let cv = 100.0 * se / slope.abs() / c;
            out.push((c, if cv.is_finite() { cv } else { f64::NAN }));
        }
        out
    }

    fn standard_curve_sections(&self, t: &TemplateSettings) -> Vec<ReportSection> {
        if self.groups.len() > 1 {
            return vec![section(
                SectionKind::StandardAcceptance,
                "Standard Curve Acceptance",
                SectionContent::Note("Back-calculation, the precision profile and sample concentrations are given for one data set; fit each plate on its own (row filter or data set).".to_owned()),
            )];
        }
        let mut out = Vec::new();
        let standards = self.standards();
        let (lo, hi) = span(&standards.iter().map(|s| s.1).collect::<Vec<_>>());
        // Curve on a log axis.
        let x_name = self.x_name.clone().unwrap_or_default();
        let points: Vec<RowPoint> = standards.iter().map(|&(row, x, y)| RowPoint { row, x: x.log10(), y }).collect();
        let curve: Polyline = if lo > 0.0 && hi > lo {
            let gx: Vec<f64> = grid(lo.ln() - 0.2, hi.ln() + 0.2, 150).into_iter().map(f64::exp).collect();
            self.curve_at(t, 0, &gx, None)
                .map(|v| gx.iter().zip(v).filter(|(_, v)| v.is_finite()).map(|(x, v)| [x.log10(), v]).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        out.push(section(
            SectionKind::TemplateCurves,
            "Standard Curve (log₁₀ concentration; blanks at zero not shown)",
            SectionContent::Plot(Box::new(PlotSpec {
                x_label: format!("log₁₀ {x_name}"),
                y_label: self.response_name.clone(),
                points,
                point_name: Some("Standards".to_owned()),
                line: Some(curve),
                line_name: Some("Fitted curve".to_owned()),
                ..PlotSpec::default()
            })),
        ));
        // Back-calculated standards, read off anywhere within two decades
        // of the standards (a standard beyond its neighbours still counts).
        let back = self.read_off_all(&standards.iter().map(|s| s.2).collect::<Vec<_>>(), (lo / 100.0, hi * 100.0));
        let nominal: Vec<f64> = standards.iter().map(|s| s.1).collect();
        let verdict = assay::standard_curve_acceptance(&nominal, &back, t.acceptance);
        let mut table = TextTable::new([
            "Row",
            "Nominal",
            "Response",
            "Back-calculated",
            "Recovery (%)",
            "Limit (±%)",
            "Within limits",
        ]);
        for (s, b) in standards.iter().zip(&verdict.standards) {
            table.push([
                (s.0 + 1).to_string(),
                num(s.1),
                num(s.2),
                b.back_calculated.map_or_else(|| "not on the curve".to_owned(), num),
                num(b.recovery_percent),
                format_number(b.limit_percent, 4),
                if b.passes { "yes" } else { "no" }.to_owned(),
            ]);
        }
        out.push(section(
            SectionKind::StandardBackCalculation,
            "Back-Calculated Standards",
            SectionContent::Table(table),
        ));
        // Acceptance and quantitation range.
        let profile = self.precision_profile();
        let range = assay::quantitation_range(&profile, t.cv_limit);
        let mut acc = TextTable::new(["Criterion", "Result"]);
        acc.push([
            "Curve accepted".to_owned(),
            if verdict.accepted { "yes".to_owned() } else { format!("no: {}", verdict.failures.join("; ")) },
        ]);
        acc.push(["Standards within limits".to_owned(), format!("{} of {}", verdict.passing.0, verdict.passing.1)]);
        acc.push(["Concentration levels passing".to_owned(), format!("{} of {}", verdict.levels.0, verdict.levels.1)]);
        acc.push([
            "Limits".to_owned(),
            format!(
                "±{}% (±{}% at the lowest and highest standard); at least {}% of standards and {} levels",
                format_number(t.acceptance.tolerance_percent, 4),
                format_number(t.acceptance.end_tolerance_percent, 4),
                format_number(100.0 * t.acceptance.min_fraction, 4),
                t.acceptance.min_levels
            ),
        ]);
        match range {
            Some((a, b)) => {
                acc.push([format!("Lower limit of quantitation (CV ≤ {}%)", format_number(t.cv_limit, 4)), num(a)]);
                acc.push([format!("Upper limit of quantitation (CV ≤ {}%)", format_number(t.cv_limit, 4)), num(b)]);
            }
            None => acc.push([
                "Quantitation range".to_owned(),
                format!("no concentration has CV ≤ {}%", format_number(t.cv_limit, 4)),
            ]),
        }
        out.push(section(
            SectionKind::StandardAcceptance,
            "Standard Curve Acceptance (ICH M10)",
            SectionContent::Table(acc),
        ));
        // Precision profile.
        if !profile.is_empty() {
            let line: Polyline = profile.iter().filter(|p| p.1.is_finite()).map(|&(c, cv)| [c.log10(), cv]).collect();
            let (a, b) = (lo.log10(), hi.log10());
            let mut segments = vec![vec![[a, t.cv_limit], [b, t.cv_limit]]];
            if let Some((q0, q1)) = range {
                let (ymin, ymax) = span(&line.iter().map(|p| p[1]).collect::<Vec<_>>());
                segments.push(vec![[q0.log10(), ymin.min(0.0)], [q0.log10(), ymax]]);
                segments.push(vec![[q1.log10(), ymin.min(0.0)], [q1.log10(), ymax]]);
            }
            out.push(section(
                SectionKind::PrecisionProfile,
                format!(
                    "Precision Profile (thin lines: the {}% limit and the quantitation range)",
                    format_number(t.cv_limit, 4)
                ),
                SectionContent::Plot(Box::new(PlotSpec {
                    x_label: format!("log₁₀ {x_name}"),
                    y_label: "CV of a concentration read off the curve (%)".to_owned(),
                    line: Some(line),
                    line_name: Some("CV (%)".to_owned()),
                    segments,
                    ..PlotSpec::default()
                })),
            ));
        }
        // Samples.
        if !self.inverse.is_empty() {
            let level = percent(self.fit().confidence_level);
            let mut table = TextTable::new(vec![
                "Row".to_owned(),
                "Response".to_owned(),
                "Read off the curve".to_owned(),
                "Dilution factor".to_owned(),
                "Concentration".to_owned(),
                format!("Lower {level}%"),
                format!("Upper {level}%"),
                "Within the quantitation range".to_owned(),
            ]);
            for &(row, _, p) in &self.inverse {
                let df = t
                    .dilution
                    .and_then(|_| self.dilution_values.get(row).copied())
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .unwrap_or(1.0);
                let status = match (p.estimate, range) {
                    (None, _) => "no: not on the curve".to_owned(),
                    (Some(e), Some((a, _))) if e < a => "no: below the lower limit".to_owned(),
                    (Some(e), Some((_, b))) if e > b => "no: above the upper limit".to_owned(),
                    (Some(_), Some(_)) => "yes".to_owned(),
                    (Some(_), None) => "—".to_owned(),
                };
                let scale = |v: Option<f64>| v.map_or_else(|| "open".to_owned(), |v| num(v * df));
                table.push([
                    (row + 1).to_string(),
                    num(p.response),
                    p.estimate.map_or_else(|| "—".to_owned(), num),
                    format_number(df, 6),
                    p.estimate.map_or_else(|| "—".to_owned(), |e| num(e * df)),
                    scale(p.lower),
                    scale(p.upper),
                    status,
                ]);
            }
            out.push(section(
                SectionKind::SampleConcentrations,
                "Sample Concentrations (read off the curve, times the dilution factor; intervals for one new observation)",
                SectionContent::Table(table),
            ));
        }
        out
    }

    fn mechanism_section(&self) -> ReportSection {
        let mut table =
            TextTable::new(["Mechanism", "Parameters", "SSE", "AICc", "ΔAICc", "Akaike weight (%)", "This fit"]);
        let this = self.request.template.as_ref().map(|t| t.template);
        let aicc: Vec<f64> =
            self.template_extras.mechanisms.iter().map(|(_, s)| s.as_ref().map_or(f64::NAN, FitScore::aicc)).collect();
        let best = aicc.iter().copied().filter(|v| v.is_finite()).fold(f64::INFINITY, f64::min);
        let total: f64 = aicc.iter().filter(|v| v.is_finite()).map(|v| (-(v - best) / 2.0).exp()).sum();
        for ((t, s), a) in self.template_extras.mechanisms.iter().zip(&aicc) {
            let mark = if Some(*t) == this { "yes" } else { "" }.to_owned();
            match s {
                Ok(s) => table.push([
                    t.label().to_owned(),
                    s.parameters.to_string(),
                    num(s.sse),
                    num(*a),
                    num(a - best),
                    if a.is_finite() {
                        format_number(100.0 * (-(a - best) / 2.0).exp() / total, 4)
                    } else {
                        "—".to_owned()
                    },
                    mark,
                ]),
                Err(e) => table.push([
                    t.label().to_owned(),
                    "—".to_owned(),
                    format!("could not be fitted: {e}"),
                    "—".to_owned(),
                    "—".to_owned(),
                    "—".to_owned(),
                    mark,
                ]),
            }
        }
        // Noncompetitive is mixed inhibition with alpha = 1: a nested F test.
        let score = |t: CurveTemplate| {
            self.template_extras.mechanisms.iter().find(|(m, _)| *m == t).and_then(|(_, s)| s.as_ref().ok().copied())
        };
        let mut title =
            "Inhibition Mechanism (AICc; the largest Akaike weight is the best-supported mechanism)".to_owned();
        if let (Some(a), Some(b)) = (score(CurveTemplate::Noncompetitive), score(CurveTemplate::Mixed)) {
            if let Ok(f) = extra_sum_of_squares(a, b) {
                title.push_str(&format!(
                    "; noncompetitive (alpha = 1) against mixed: F = {}, p = {}",
                    format_number(f.f, 4),
                    format_p_value(f.p_value)
                ));
            }
        }
        section(SectionKind::InhibitionMechanisms, title, SectionContent::Table(table))
    }

    fn schild_section(&self, t: &TemplateSettings) -> ReportSection {
        let fit = self.fit();
        let est = fit.estimates();
        let (pa2, slope) = (est.get(4).copied().unwrap_or(f64::NAN), est.get(5).copied().unwrap_or(f64::NAN));
        let se_slope = fit.parameters.get(5).map_or(f64::NAN, |p| p.std_error);
        let name = t.second.and_then(|j| self.source_name(j)).unwrap_or_else(|| "antagonist".to_owned());
        let mut note = String::new();
        if se_slope.is_finite() && se_slope > 0.0 && fit.df > 0 {
            let tv = (slope - 1.0) / se_slope;
            let p = StudentsT::new(fit.df as f64).map_or(f64::NAN, |d| 2.0 * d.sf(tv.abs()).unwrap_or(f64::NAN));
            note = format!(
                "Schild slope {} (SE {}); t for slope = 1: {}, p = {}. ",
                format_number(slope, 4),
                format_number(se_slope, 3),
                format_number(tv, 4),
                format_p_value(p)
            );
        }
        let schild = match &self.template_extras.schild {
            Some(Ok(s)) => s,
            Some(Err(e)) => {
                return section(
                    SectionKind::SchildPlot,
                    "Schild Analysis",
                    SectionContent::Note(format!("{note}Separate EC50s could not be fitted: {e}")),
                );
            }
            None => return section(SectionKind::SchildPlot, "Schild Analysis", SectionContent::Note(note)),
        };
        let mut items = Vec::new();
        for &(b, dr) in &schild.ratios {
            if dr > 1.0 {
                items.push((b.log10(), (dr - 1.0).log10(), format!("DR {}", format_number(dr, 4)), 0));
            }
        }
        let xs: Vec<f64> = items.iter().map(|i| i.0).collect();
        let (lo, hi) = span(&xs);
        // Global fit: log(DR − 1) = slope·(log[B] + pA2).
        let line = if lo.is_finite() && hi >= lo {
            let pad = 0.25 * (hi - lo).max(0.5);
            vec![[lo - pad, slope * (lo - pad + pa2)], [hi + pad, slope * (hi + pad + pa2)]]
        } else {
            Vec::new()
        };
        note.push_str(&format!(
            "Points: dose ratios from separate EC50s (Bottom, Top and Hill shared); line: the global Gaddum–Schild fit, pA2 = {}.",
            format_number(pa2, 5)
        ));
        section(
            SectionKind::SchildPlot,
            format!("Schild Plot ({note})"),
            SectionContent::Plot(Box::new(PlotSpec {
                x_label: format!("log₁₀ {name}"),
                y_label: "log₁₀(dose ratio − 1)".to_owned(),
                line: Some(line),
                line_name: Some("Global fit".to_owned()),
                annotations: Some(Annotations { groups: vec!["Dose ratio".to_owned()], items, labels: Vec::new() }),
                ..PlotSpec::default()
            })),
        )
    }

    fn potency_section(&self, t: &TemplateSettings) -> ReportSection {
        let fit = self.fit();
        let theta = fit.estimates();
        let level = percent(fit.confidence_level);
        let standard = t.standard.as_ref().and_then(|s| self.groups.iter().position(|g| g == s)).unwrap_or(0);
        let mut table = TextTable::new(vec![
            "Preparation".to_owned(),
            "log EC50".to_owned(),
            "Relative potency".to_owned(),
            format!("Lower {level}%"),
            format!("Upper {level}%"),
        ]);
        if self.groups.len() < 2 {
            return section(
                SectionKind::RelativePotency,
                "Relative Potency",
                SectionContent::Note(
                    "Relative potency needs a standard and at least one test preparation (data-set column).".to_owned(),
                ),
            );
        }
        let k = 2;
        let is = self.outcome.map[standard][k];
        for (g, label) in self.groups.iter().enumerate() {
            let it = self.outcome.map[g][k];
            if g == standard {
                table.push([
                    format!("{label} (standard)"),
                    num(theta[is]),
                    "1".to_owned(),
                    "—".to_owned(),
                    "—".to_owned(),
                ]);
                continue;
            }
            let cov = |a: usize, b: usize| {
                let v = fit.covariance[(a, b)];
                if v.is_finite() { v } else { 0.0 }
            };
            let se = (cov(is, is) + cov(it, it) - 2.0 * cov(is, it)).max(0.0).sqrt();
            let rp = assay::relative_potency(theta[is], theta[it], se, fit.t_critical);
            table.push([label.clone(), num(theta[it]), num(rp.estimate), num(rp.interval.0), num(rp.interval.1)]);
        }
        section(
            SectionKind::RelativePotency,
            "Relative Potency (EC50 of the standard ÷ EC50 of the preparation; the parallelism test is under Model Comparison)",
            SectionContent::Table(table),
        )
    }
}

impl NonlinearAnalysis {
    /// The template's derived quantities: label, data set (when given per
    /// data set) and value. Empty without a template.
    #[must_use]
    pub fn template_derived(&self) -> Vec<(String, Option<String>, DerivedValue)> {
        let Some(t) = &self.request.template else { return Vec::new() };
        let per = self.per_data_set();
        let sets: Vec<usize> = if per { (0..self.groups.len()).collect() } else { vec![0] };
        let mut out = Vec::new();
        for g in sets {
            for (label, v, _) in self.derived_values(t, g).0 {
                out.push((label.to_owned(), per.then(|| self.groups[g].clone()), v));
            }
        }
        out
    }
}
