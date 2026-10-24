//! Table operations against independent results computed with pandas and
//! numpy (`tests/data/table_ops/reference_cases.py`, which documents where it
//! adapts pandas to the SQL and ordering rules `stat-data` follows).
//!
//! Every cell must agree: text exactly, numbers and date-times (as seconds)
//! within 1e-12 relative, missing values in the same places, and each column's
//! storage type (text, number, date-time) as expected.

use stat_core::descriptive::QuantileMethod;
use stat_data::table_ops::{
    JoinKind, Relationship, SortKey, Statistic, SummarizeOptions, combine_rows, join, sorted, stack, summarize,
    transpose, unstack, without_duplicates,
};
use stat_data::{Column, DataTable, JsonValue, TemporalDisplay, TemporalFormat, parse_json};

const CASES: &str = include_str!("data/table_ops/reference_cases.json");

fn table(name: &str, columns: &JsonValue) -> DataTable {
    let columns = columns
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let name = c.get("name").and_then(JsonValue::as_str).unwrap();
            let values = c.get("values").and_then(JsonValue::as_array).unwrap();
            let numbers = || values.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect::<Vec<_>>();
            match c.get("type").and_then(JsonValue::as_str).unwrap() {
                "number" => Column::numeric(name, numbers()),
                "date" => Column::date_time(name, numbers(), TemporalDisplay::new(TemporalFormat::Date)),
                _ => Column::text(name, values.iter().map(|v| v.as_str().unwrap().to_owned()).collect()),
            }
        })
        .collect();
    DataTable::new(name, columns).unwrap()
}

fn strings(case: &JsonValue, key: &str) -> Vec<String> {
    case.get(key).and_then(JsonValue::as_array).unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect()
}

fn indices(t: &DataTable, names: &[String]) -> Vec<usize> {
    names.iter().map(|n| t.find(n).unwrap_or_else(|| panic!("no column {n}"))).collect()
}

fn assert_same(got: &DataTable, expected: &DataTable, label: &str) {
    let names = |t: &DataTable| t.columns().iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(got), names(expected), "{label}: columns");
    assert_eq!(got.n_rows(), expected.n_rows(), "{label}: rows");
    for (g, e) in got.columns().iter().zip(expected.columns()) {
        assert_eq!(g.temporal().is_some(), e.temporal().is_some(), "{label}: {} date-time", e.name);
        match (g.as_numeric(), e.as_numeric()) {
            (Some(gv), Some(ev)) => {
                for (r, (a, b)) in gv.iter().zip(ev).enumerate() {
                    let ok = (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-12 * b.abs().max(1e-300);
                    assert!(ok, "{label}: {} row {}: got {a}, expected {b}", e.name, r + 1);
                }
            }
            (None, None) => {
                for r in 0..e.data.len() {
                    assert_eq!(g.cell_text(r), e.cell_text(r), "{label}: {} row {}", e.name, r + 1);
                }
            }
            _ => panic!("{label}: column {} has the wrong type", e.name),
        }
    }
}

fn quantile_method(t: usize) -> QuantileMethod {
    match t {
        2 => QuantileMethod::AveragedInverseCdf,
        6 => QuantileMethod::Weibull,
        7 => QuantileMethod::Linear,
        8 => QuantileMethod::MedianUnbiased,
        _ => panic!("quantile type {t}"),
    }
}

#[test]
fn table_operations_match_pandas() {
    let doc = parse_json(CASES).unwrap();
    let cases = doc.get("cases").and_then(JsonValue::as_array).unwrap();
    assert_eq!(cases.len(), 19);
    for (i, case) in cases.iter().enumerate() {
        let op = case.get("op").and_then(JsonValue::as_str).unwrap();
        let label = format!("case {} ({op})", i + 1);
        let inputs = case.get("inputs").and_then(JsonValue::as_array).unwrap();
        let names = case.get("names").map_or_else(|| vec!["input".to_owned()], |_| strings(case, "names"));
        let tables: Vec<DataTable> = inputs.iter().zip(&names).map(|(c, n)| table(n, c)).collect();
        let t = &tables[0];
        let got = match op {
            "sort" => {
                let keys = indices(t, &strings(case, "keys"));
                let desc = case.get("descending").and_then(JsonValue::as_array).unwrap();
                let keys: Vec<SortKey> = keys
                    .iter()
                    .zip(desc)
                    .map(|(&column, d)| SortKey { column, descending: d.as_bool().unwrap() })
                    .collect();
                sorted(t, &keys, "out").unwrap()
            }
            "duplicates" => without_duplicates(t, &indices(t, &strings(case, "columns")), "out").unwrap(),
            "combine" => {
                let source = case.get("source").and_then(JsonValue::as_str);
                combine_rows(&tables.iter().collect::<Vec<_>>(), source, "out").unwrap()
            }
            "join" => {
                let pairs: Vec<(usize, usize)> = case
                    .get("keys")
                    .and_then(JsonValue::as_array)
                    .unwrap()
                    .iter()
                    .map(|p| {
                        let p = p.as_array().unwrap();
                        (t.find(p[0].as_str().unwrap()).unwrap(), tables[1].find(p[1].as_str().unwrap()).unwrap())
                    })
                    .collect();
                let kind = match case.get("how").and_then(JsonValue::as_str).unwrap() {
                    "inner" => JoinKind::Inner,
                    "left" => JoinKind::Left,
                    "right" => JoinKind::Right,
                    _ => JoinKind::Full,
                };
                join(t, &tables[1], &pairs, kind, Relationship::Any, "out").unwrap()
            }
            "stack" => {
                let label_name = case.get("label").and_then(JsonValue::as_str).unwrap();
                let value_name = case.get("value").and_then(JsonValue::as_str).unwrap();
                stack(t, &indices(t, &strings(case, "columns")), label_name, value_name, "out").unwrap()
            }
            "unstack" => {
                let label_col = t.find(case.get("label").and_then(JsonValue::as_str).unwrap()).unwrap();
                unstack(t, label_col, &indices(t, &strings(case, "values")), &indices(t, &strings(case, "ids")), "out")
                    .unwrap()
            }
            "transpose" => {
                let label_col = t.find(case.get("label").and_then(JsonValue::as_str).unwrap());
                transpose(t, &indices(t, &strings(case, "columns")), label_col, "out").unwrap()
            }
            "summarize" => {
                let qtype = case.get("quantile_type").and_then(JsonValue::as_usize).unwrap();
                let options = SummarizeOptions {
                    groups: indices(t, &strings(case, "groups")),
                    columns: indices(t, &strings(case, "columns")),
                    statistics: vec![
                        Statistic::Count,
                        Statistic::Missing,
                        Statistic::Mean,
                        Statistic::StdDev,
                        Statistic::Min,
                        Statistic::Max,
                        Statistic::Median,
                        Statistic::Quantile(0.1),
                        Statistic::Quantile(0.25),
                        Statistic::Quantile(0.9),
                        Statistic::Sum,
                    ],
                    quantile_method: quantile_method(qtype),
                };
                let rows: Vec<usize> = (0..t.n_rows()).collect();
                summarize(t, &rows, &options, "out").unwrap()
            }
            other => panic!("unknown operation {other}"),
        };
        let expected = table("expected", case.get("expected").unwrap());
        assert_same(&got.table, &expected, &label);
    }
}
