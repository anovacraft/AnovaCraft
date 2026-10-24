//! Summarize by group (`stat_data::table_ops::summarize`) against the NIST StRD
//! univariate datasets: with one group it must reach the same accuracy as
//! the Describe statistics (`univariate_certified.rs`), including the
//! documented NumAcc3/NumAcc4 representation floors; split into groups, each
//! group must equal the statistics of its rows computed directly.

use stat_core::descriptive::{QuantileMethod, moments};
use stat_data::table_ops::{Statistic, SummarizeOptions, summarize};
use stat_data::{Column, DataTable};
use stat_strd::{log_relative_error, univariate_datasets};

fn options(groups: Vec<usize>) -> SummarizeOptions {
    SummarizeOptions {
        groups,
        columns: vec![0],
        statistics: vec![Statistic::Count, Statistic::Mean, Statistic::StdDev],
        quantile_method: QuantileMethod::default(),
    }
}

fn value(t: &DataTable, name: &str, row: usize) -> f64 {
    t.column(t.find(name).unwrap()).unwrap().as_numeric().unwrap()[row]
}

/// Required LRE (mean, sd), as for Describe.
fn required(name: &str) -> (f64, f64) {
    match name {
        "NumAcc3" => (13.0, 9.0),
        "NumAcc4" => (13.0, 8.0),
        _ => (13.0, 13.0),
    }
}

#[test]
fn summarize_matches_nist_certified_values() {
    let mut failures = Vec::new();
    for d in univariate_datasets() {
        let n = d.values.len();
        let t = DataTable::new(d.name, vec![Column::numeric("y", d.values.clone())]).unwrap();
        let rows: Vec<usize> = (0..n).collect();
        let s = summarize(&t, &rows, &options(vec![]), "s").unwrap().table;
        assert_eq!(value(&s, "y N", 0), n as f64);
        let (need_mean, need_sd) = required(d.name);
        let lre_mean = log_relative_error(value(&s, "y mean", 0), d.mean);
        let lre_sd = log_relative_error(value(&s, "y SD", 0), d.std_dev);
        println!("{:<9} mean LRE {lre_mean:.1}  sd LRE {lre_sd:.1}", d.name);
        if lre_mean < need_mean {
            failures.push(format!("{} mean: LRE {lre_mean:.1} < {need_mean}", d.name));
        }
        if lre_sd < need_sd {
            failures.push(format!("{} sd: LRE {lre_sd:.1} < {need_sd}", d.name));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn each_group_equals_its_rows_summarized_directly() {
    for d in univariate_datasets() {
        let n = d.values.len();
        // Three groups by position (row mod 3), labelled as text.
        let labels: Vec<String> = (0..n).map(|r| format!("g{}", r % 3)).collect();
        let t =
            DataTable::new(d.name, vec![Column::numeric("y", d.values.clone()), Column::text("g", labels)]).unwrap();
        let rows: Vec<usize> = (0..n).collect();
        let s = summarize(&t, &rows, &options(vec![1]), "s").unwrap().table;
        for g in 0..s.n_rows() {
            let members: Vec<f64> = (0..n).filter(|r| r % 3 == g).map(|r| d.values[r]).collect();
            let m = moments(&members).unwrap();
            assert_eq!(value(&s, "y N", g), members.len() as f64, "{} group {g}", d.name);
            assert_eq!(value(&s, "y mean", g).to_bits(), m.mean.to_bits(), "{} group {g}", d.name);
            let sd = value(&s, "y SD", g);
            assert!(m.std_dev.map_or(sd.is_nan(), |x| x.to_bits() == sd.to_bits()), "{} group {g}", d.name);
        }
    }
}
