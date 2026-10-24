//! Simulated responses for the design held in a table: a [`SimulateRequest`]
//! names the factor columns (coded −1 and +1 as for a declared design), a
//! model and the prior of [`stat_doe::simulate`], and
//! [`simulate_design_responses`] returns new response columns, one per
//! simulation, and a table of the true coefficients that made them.
//!
//! A numeric continuous factor is coded from its low and high settings (by
//! default the smallest and largest values among the rows); a categorical
//! factor must have exactly two levels, coded −1 and +1 in sorted order.
//! Rows without every factor setting get missing responses.

use crate::declared_design::DeclaredFactor;
use crate::project::invalid;
use stat_core::Result;
use stat_data::{Column, DataTable, MeasurementLevel, table_ops::natural_order};
use stat_doe::ModelFormula;
use stat_doe::simulate::{SparsityPrior, simulate_responses};

/// What to simulate.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulateRequest {
    /// Factor columns and their coding.
    pub factors: Vec<DeclaredFactor>,
    /// Model over the factor column names (see [`ModelFormula::parse`]).
    pub formula: String,
    /// Probabilities and sizes of the coefficients.
    pub prior: SparsityPrior,
    /// Number of responses to simulate.
    pub count: usize,
    /// Seed.
    pub seed: u64,
    /// Name of the response columns (numbered when there are several).
    pub name: String,
}

impl SimulateRequest {
    /// One response named "Simulated Y" for `factors` with the
    /// main-effects-and-interactions model and the default prior, seed 1.
    #[must_use]
    pub fn new(factors: Vec<DeclaredFactor>) -> Self {
        Self {
            factors,
            formula: "interactions".to_owned(),
            prior: SparsityPrior::default(),
            count: 1,
            seed: 1,
            name: "Simulated Y".to_owned(),
        }
    }
}

/// The simulated columns and their coefficients.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulateOutcome {
    /// One numeric column per simulation, named from the request (not
    /// clashing with the table's columns).
    pub columns: Vec<Column>,
    /// One row per simulated column: its name, the number of active terms,
    /// the active terms, and every term's true coefficient.
    pub coefficients: DataTable,
    /// Notes for the user.
    pub notes: Vec<String>,
}

/// A simulation request from script settings (JSON): `factors` (column
/// names, or `#{ column, low, high }`), `model` (formula over the factor
/// names, or `"main effects"`, `"interactions"` (default), `"quadratic"`),
/// `count` (1), `seed` (1), `name` (`"Simulated Y"`), and the prior:
/// `main`, `all_parents`, `some_parents`, `no_parents` (0.25, 0.25, 0.1,
/// 0.01), `effect_sd` (2), `inactive_sd` (0), `noise_sd` (1), `intercept` (0).
///
/// # Errors
/// Missing columns or malformed settings.
pub fn simulate_request_from_json(v: &stat_data::JsonValue, table: &DataTable) -> Result<SimulateRequest> {
    use stat_data::JsonValue;
    let declared = crate::DesignDeclarationRequest::from_json(v, table)?;
    let mut request = SimulateRequest::new(declared.factors);
    if let Some(m) = v.get("model").and_then(JsonValue::as_str) {
        m.clone_into(&mut request.formula);
    }
    if let Some(name) = v.get("name").and_then(JsonValue::as_str) {
        name.clone_into(&mut request.name);
    }
    match v.get("count") {
        None | Some(JsonValue::Null) => {}
        Some(j) => request.count = j.as_usize().ok_or_else(|| invalid("count: expected a whole number"))?,
    }
    match v.get("seed") {
        None | Some(JsonValue::Null) => {}
        Some(j) => {
            request.seed = crate::seed::seed_from_json(j).ok_or_else(|| invalid("seed: expected a whole number"))?
        }
    }
    let p = &mut request.prior;
    for (key, slot) in [
        ("main", &mut p.main),
        ("all_parents", &mut p.all_parents),
        ("some_parents", &mut p.some_parents),
        ("no_parents", &mut p.no_parents),
        ("effect_sd", &mut p.effect_sd),
        ("inactive_sd", &mut p.inactive_sd),
        ("noise_sd", &mut p.noise_sd),
        ("intercept", &mut p.intercept),
    ] {
        match v.get(key) {
            None | Some(JsonValue::Null) => {}
            Some(j) => *slot = j.as_f64().ok_or_else(|| invalid(format!("{key}: expected a number")))?,
        }
    }
    Ok(request)
}

/// Simulate responses for the design in `table`.
///
/// # Errors
/// No factors, a missing column or one used twice, a categorical factor
/// without exactly two levels, equal low and high settings, an invalid
/// model, and the errors of [`simulate_responses`].
pub fn simulate_design_responses(table: &DataTable, request: &SimulateRequest) -> Result<SimulateOutcome> {
    if request.factors.is_empty() {
        return Err(invalid("choose at least one factor column"));
    }
    let cols: Vec<usize> = request.factors.iter().map(|f| f.column).collect();
    if cols.iter().enumerate().any(|(i, c)| cols[..i].contains(c)) {
        return Err(invalid("each column can be chosen once"));
    }
    let n = table.n_rows();
    let mut names = Vec::new();
    let mut coded: Vec<Vec<f64>> = Vec::new();
    for f in &request.factors {
        let c = table.column(f.column).ok_or_else(|| invalid(format!("no column {}", f.column)))?;
        names.push(c.name.clone());
        match c.as_numeric().filter(|_| c.level == MeasurementLevel::Continuous) {
            Some(v) => {
                let (lo, hi) = v
                    .iter()
                    .filter(|x| x.is_finite())
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
                let (low, high) = (f.low.unwrap_or(lo), f.high.unwrap_or(hi));
                if !(low.is_finite() && high.is_finite()) || low == high {
                    return Err(invalid(format!("{} needs different low and high settings (it may not vary)", c.name)));
                }
                coded.push(v.iter().map(|x| (2.0 * x - (low + high)) / (high - low)).collect());
            }
            None => {
                let text: Vec<String> = (0..n).map(|r| c.level_text(r)).collect();
                let mut distinct: Vec<&String> = text.iter().filter(|t| !t.is_empty()).collect();
                distinct.sort_by(|a, b| natural_order(a, b));
                distinct.dedup();
                if distinct.len() != 2 {
                    return Err(invalid(format!(
                        "{} is categorical with {} levels: a categorical factor needs exactly two",
                        c.name,
                        distinct.len()
                    )));
                }
                let lo = distinct[0].clone();
                coded.push(
                    text.iter()
                        .map(|t| {
                            if t.is_empty() {
                                f64::NAN
                            } else if *t == lo {
                                -1.0
                            } else {
                                1.0
                            }
                        })
                        .collect(),
                );
            }
        }
    }
    let rows: Vec<usize> = (0..n).filter(|&r| coded.iter().all(|v| v[r].is_finite())).collect();
    if rows.is_empty() {
        return Err(invalid("no row has a setting for every factor"));
    }
    let runs: Vec<Vec<f64>> = rows.iter().map(|&r| coded.iter().map(|v| v[r]).collect()).collect();
    let model = ModelFormula::parse(&request.formula, &names)?;
    let sims = simulate_responses(&runs, &model, &request.prior, request.count, request.seed)?;
    // Column names that are free in the table.
    let stem = if request.name.trim().is_empty() { "Simulated Y" } else { request.name.trim() };
    let free = |c: &String| table.find(c).is_none();
    let column_names: Vec<String> = if request.count == 1 {
        vec![if free(&stem.to_owned()) { stem.to_owned() } else { table.unused_column_name(stem) }]
    } else {
        (1..).map(|i| format!("{stem} {i}")).filter(free).take(request.count).collect()
    };
    let columns: Vec<Column> = sims
        .iter()
        .zip(&column_names)
        .map(|(s, name)| {
            let mut values = vec![f64::NAN; n];
            for (&r, &y) in rows.iter().zip(&s.values) {
                values[r] = y;
            }
            Column::numeric(name.clone(), values)
        })
        .collect();
    let labels: Vec<String> = model.terms().iter().map(|t| t.label(&names)).collect();
    let mut table_columns = vec![
        Column::text("Response", column_names.clone()),
        Column::numeric(
            "Active terms",
            sims.iter().map(|s| (s.active.iter().filter(|&&a| a).count() - 1) as f64).collect(),
        ),
        Column::text(
            "Active",
            sims.iter()
                .map(|s| {
                    let list: Vec<&str> =
                        labels.iter().zip(&s.active).skip(1).filter(|(_, a)| **a).map(|(l, _)| l.as_str()).collect();
                    if list.is_empty() { "(none)".to_owned() } else { list.join(", ") }
                })
                .collect(),
        ),
    ];
    for (t, label) in labels.iter().enumerate() {
        table_columns.push(Column::numeric(label.clone(), sims.iter().map(|s| s.coefficients[t]).collect()));
    }
    let coefficients = DataTable::new(format!("{} (simulated coefficients)", table.name()), table_columns)?;
    let p = &request.prior;
    let mut notes = vec![
        format!("Model: {} ({} terms).", model.describe(&names), model.n_terms()),
        format!(
            "Active with probability {} (main effects); higher-order terms {} with all parents active, {} with some, \
             {} with none.",
            p.main, p.all_parents, p.some_parents, p.no_parents
        ),
        format!(
            "Active coefficients ~ N(0, {}²) in coded units, inactive {}; noise SD {}; intercept {}; seed {}.",
            p.effect_sd,
            if p.inactive_sd > 0.0 { format!("~ N(0, {}²)", p.inactive_sd) } else { "0".to_owned() },
            p.noise_sd,
            p.intercept,
            request.seed
        ),
    ];
    if rows.len() < n {
        notes.push(format!("{} row(s) without every factor setting get missing responses.", n - rows.len()));
    }
    Ok(SimulateOutcome { columns, coefficients, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_columns_follow_their_coefficients() {
        let mut a = Vec::new();
        let mut b = Vec::new();
        let mut c = Vec::new();
        for i in 0..8 {
            a.push(if i & 1 == 0 { 10.0 } else { 20.0 });
            b.push(if i & 2 == 0 { 1.0 } else { 3.0 });
            c.push(if i & 4 == 0 { "low" } else { "high" }.to_owned());
        }
        a.push(f64::NAN);
        b.push(2.0);
        c.push("low".into());
        let t = DataTable::new(
            "Design",
            vec![
                Column::numeric("A", a.clone()),
                Column::numeric("B", b.clone()),
                Column::text("C", c.clone()),
                Column::numeric("Simulated Y 1", vec![0.0; 9]),
            ],
        )
        .unwrap();
        let factors = (0..3).map(|column| DeclaredFactor { column, low: None, high: None }).collect();
        let mut request = SimulateRequest::new(factors);
        request.count = 3;
        request.prior = SparsityPrior { main: 0.7, noise_sd: 0.0, ..SparsityPrior::default() };
        let out = simulate_design_responses(&t, &request).unwrap();
        let names: Vec<&str> = out.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Simulated Y 2", "Simulated Y 3", "Simulated Y 4"], "names in use are skipped");
        let k = &out.coefficients;
        assert_eq!(k.n_rows(), 3);
        let headers: Vec<&str> = k.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(&headers[..4], &["Response", "Active terms", "Active", "Intercept"]);
        // Without noise each response is exactly its coefficients' model ("high" < "low", so high is −1).
        for (s, col) in out.columns.iter().enumerate() {
            let y = col.as_numeric().unwrap();
            let coef = |name: &str| k.column(k.find(name).unwrap()).unwrap().as_numeric().unwrap()[s];
            for r in 0..8 {
                let (x1, x2, x3) = ((a[r] - 15.0) / 5.0, b[r] - 2.0, if c[r] == "high" { -1.0 } else { 1.0 });
                let want = coef("Intercept")
                    + coef("A") * x1
                    + coef("B") * x2
                    + coef("C") * x3
                    + coef("A × B") * x1 * x2
                    + coef("A × C") * x1 * x3
                    + coef("B × C") * x2 * x3;
                assert!((y[r] - want).abs() < 1e-12);
            }
            assert!(y[8].is_nan(), "a row without every setting");
        }
        assert!(out.notes.iter().any(|n| n.contains("1 row(s) without every factor setting")));
        request.factors = vec![DeclaredFactor { column: 2, low: None, high: None }; 2];
        assert!(simulate_design_responses(&t, &request).is_err());
    }
}
