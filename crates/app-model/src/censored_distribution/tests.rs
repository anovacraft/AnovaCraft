//! Reference values from R (fitdistrplus 1.x `fitdistcens`, survival
//! `survfit`); script: `tests/data/censored_distribution_reference.R`. The fits
//! use `optim` with `reltol = 1e-15`: at R's default tolerance the estimates
//! stop about 3·10⁻⁴ short of the maximum.

use super::*;
use crate::project::{report_from_json, report_to_json};
use crate::sample::well_arsenic;
use crate::{DistributionAnalysis, DistributionOptions, DistributionRequest, SavedReport};
use stat_data::RowStateManager;

fn request(t: &DataTable) -> DistributionRequest {
    DistributionRequest {
        censoring: Some(Censoring::Indicator {
            column: t.find("Non-detect").unwrap(),
            direction: CensorDirection::Left,
        }),
        ..DistributionRequest::new(t.find("Arsenic (µg/L)").unwrap())
    }
}

fn analysis() -> DistributionAnalysis {
    let t = well_arsenic();
    DistributionAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &request(&t)).unwrap()
}

fn close(a: f64, b: f64, rel: f64) {
    assert!((a - b).abs() <= rel * b.abs().max(1e-300), "{a} vs {b}");
}

#[test]
fn fits_match_fitdistcens() {
    let a = analysis();
    let c = a.censored().unwrap();
    // (family, first parameter, second parameter, log-likelihood) in this
    // crate's parametrization; R's rates are inverted.
    let cases = [
        (FitFamily::Normal, 3.17380947907871, Some(5.49460026803304), -56.042121154033),
        (FitFamily::Lognormal, 0.891195045772975, Some(1.096550618206111), -53.8809889454099),
        (FitFamily::Weibull, 0.918385192143761, Some(3.939879627256525), -53.4145351804254),
        (FitFamily::Gamma, 0.861032292236016, Some(1.0 / 0.210151511924602), -53.4187378129833),
        (FitFamily::Exponential, 1.0 / 0.242743063155323, None, -53.5207474904395),
        (FitFamily::Logistic, 2.94178202964498, Some(2.85886649443222), -55.1959936916899),
    ];
    for (family, p1, p2, ll) in cases {
        let f = c.fit(family).unwrap_or_else(|| panic!("{family:?} not fitted"));
        let params = f.distribution.parameters();
        close(params[0].value, p1, 1e-5);
        if let Some(p2) = p2 {
            close(params[1].value, p2, 1e-5);
        }
        assert!((f.log_likelihood() - ll).abs() < 1e-8, "{family:?}: {} vs {ll}", f.log_likelihood());
    }
}

#[test]
fn nonparametric_estimate_is_the_reversed_kaplan_meier() {
    let a = analysis();
    let np = a.censored().unwrap().nonparametric().unwrap();
    // F(x) = P(X ≤ x) at each detected value, from survfit on the flipped data.
    let reference = [
        (1.2, 0.3125),
        (1.6, 0.375),
        (2.0, 0.421875),
        (2.2, 0.46875),
        (2.4, 0.515625),
        (2.9, 0.5625),
        (3.2, 0.609375),
        (3.9, 0.65625),
        (4.1, 0.703125),
        (4.8, 0.75),
        (6.1, 0.791666666666667),
        (7.3, 0.833333333333333),
        (8.8, 0.875),
        (9.7, 0.916666666666667),
        (12.5, 0.958333333333333),
        (18.4, 1.0),
    ];
    for (x, f) in reference {
        let got = np.cdf_at(x).unwrap();
        assert!((got - f).abs() < 1e-9, "F({x}) = {got}, reference {f}");
    }
    // Below the lowest detection limit only the total is known.
    assert!((np.cdf_at(1.0).unwrap() - 0.25).abs() < 1e-9);
    assert_eq!(percentile_text(np, 0.1), "< 1");
    assert_eq!(percentile_text(np, 0.5), "2.4");
}

#[test]
fn mixed_censoring_matches_fitdistcens() {
    let mut t = well_arsenic();
    // Interval layout: lower end in a new column, upper end in another.
    let value = t.find("Arsenic (µg/L)").unwrap();
    let nd = t.find("Non-detect").unwrap();
    let v = t.column(value).unwrap().as_numeric().unwrap().to_vec();
    let flags = t.column(nd).unwrap().as_numeric().unwrap().to_vec();
    let mut lower: Vec<f64> = v.iter().zip(&flags).map(|(x, f)| if *f != 0.0 { f64::NAN } else { *x }).collect();
    let mut upper = v.clone();
    lower[19] = 15.0;
    upper[19] = f64::NAN;
    lower[2] = 3.0;
    upper[2] = 4.0;
    t.insert_column(t.n_cols(), stat_data::Column::numeric("Lower", lower)).unwrap();
    t.insert_column(t.n_cols(), stat_data::Column::numeric("Upper", upper)).unwrap();
    let request = DistributionRequest {
        censoring: Some(Censoring::Interval { upper: t.find("Upper").unwrap() }),
        ..DistributionRequest::new(t.find("Lower").unwrap())
    };
    let a = DistributionAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &request).unwrap();
    let f = a.censored().unwrap().fit(FitFamily::Lognormal).unwrap();
    let params = f.distribution.parameters();
    close(params[0].value, 0.896048760609651, 1e-5);
    close(params[1].value, 1.138342632249389, 1e-5);
    assert!((f.log_likelihood() - -51.3646721733105).abs() < 1e-8);
    let sections = a.sections(&DistributionOptions::default());
    assert!(sections.iter().any(|s| s.title.contains("Turnbull")));
}

#[test]
fn report_has_every_section_and_round_trips() {
    let t = well_arsenic();
    let a = analysis();
    let sections = a.sections(&DistributionOptions::default());
    let kinds: Vec<SectionKind> = sections.iter().map(|s| s.kind).collect();
    for k in [
        SectionKind::CensoredSummary,
        SectionKind::CensoredNonparametric,
        SectionKind::CensoredFits,
        SectionKind::CensoredParameters,
        SectionKind::CensoredPlot,
    ] {
        assert!(kinds.contains(&k), "missing {k:?}");
    }
    let saved = SavedReport::Distribution { request: request(&t), options: DistributionOptions::default() };
    let json = report_to_json(&saved, &t);
    assert_eq!(report_from_json(&json, &t).unwrap(), saved);
    // Without the plot option the plot is left out.
    let no_plot = DistributionOptions { show_chart: false, ..DistributionOptions::default() };
    assert!(!a.sections(&no_plot).iter().any(|s| s.kind == SectionKind::CensoredPlot));
}

#[test]
fn counts_with_detection_limits_get_count_fits() {
    let t = stat_data::DataTable::new(
        "counts",
        vec![
            stat_data::Column::numeric("Colonies", vec![3.0, 0.0, 5.0, 2.0, 2.0, 7.0, 1.0, 4.0, 2.0, 6.0, 3.0, 2.0]),
            stat_data::Column::numeric("Below", vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
        ],
    )
    .unwrap();
    let request = DistributionRequest {
        censoring: Some(Censoring::Indicator { column: 1, direction: CensorDirection::Left }),
        ..DistributionRequest::new(0)
    };
    let a = DistributionAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &request).unwrap();
    let sections = a.sections(&DistributionOptions::default());
    assert!(sections.iter().any(|s| s.kind == SectionKind::CensoredCountFits));
    // A left-censored count below 2 is 0 or 1.
    let o = count_observation(&LifeObservation::left(2.0, 1.0));
    assert_eq!((o.lower, o.upper), (0.0, 1.0));
}
