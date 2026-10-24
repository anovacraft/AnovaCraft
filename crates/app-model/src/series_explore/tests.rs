use super::*;
use crate::project::{Project, SavedReport, load_project, report_from_json, report_to_json, save_project};
use crate::report::TextTable;
use crate::sample::{advertising_sales, electricity_demand, product_adoption};

fn table_of(sections: &[ReportSection], kind: SectionKind) -> &TextTable {
    match &sections.iter().find(|s| s.kind == kind).unwrap_or_else(|| panic!("{kind:?} missing")).content {
        SectionContent::Table(t) => t,
        other => panic!("{kind:?} is not a table: {other:?}"),
    }
}

fn cell(t: &TextTable, row: usize, column: &str) -> f64 {
    t.cell(row, column).unwrap_or_else(|| panic!("no {column}")).replace('−', "-").parse().unwrap()
}

fn demand_request(method: ExploreMethod) -> (DataTable, SeriesExploreRequest) {
    let t = electricity_demand();
    let series = SeriesInput { column: t.find("Demand (GWh)").unwrap(), time: t.find("Month"), period: None };
    (t, SeriesExploreRequest { series, method })
}

fn run(t: &DataTable, r: &SeriesExploreRequest) -> SeriesExploreAnalysis {
    SeriesExploreAnalysis::run(t, &RowStateManager::new(t.n_rows()), r).unwrap()
}

#[test]
fn description_shows_seasonal_dependence() {
    let (t, r) = demand_request(ExploreMethod::Describe(DescribeSettings::default()));
    let a = run(&t, &r);
    assert_eq!(a.title(), "Series Description: Demand (GWh)");
    let s = a.sections();
    let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
    for k in [
        SectionKind::SeriesSummary,
        SectionKind::SeriesPlot,
        SectionKind::SeriesAcfPlot,
        SectionKind::SeriesPacfPlot,
        SectionKind::SeriesAutocorrelationTable,
        SectionKind::SeriesLjungBox,
        SectionKind::SeriesUnitRoot,
    ] {
        assert!(kinds.contains(&k), "{k:?} missing");
    }
    // No transformation, no spectrum by default.
    assert!(!kinds.contains(&SectionKind::SeriesTransformedPlot));
    assert!(!kinds.contains(&SectionKind::SeriesSpectrumPlot));
    // Monthly dates give period 12 and lags up to 24.
    let acf = table_of(&s, SectionKind::SeriesAutocorrelationTable);
    assert_eq!(acf.rows.len(), 24);
    assert!(cell(acf, 11, "Autocorrelation") > 0.5, "lag 12 autocorrelation");
    assert!(cell(acf, 5, "Autocorrelation") < cell(acf, 11, "Autocorrelation"));
    let lb = table_of(&s, SectionKind::SeriesLjungBox);
    assert_eq!(lb.cell(0, "Up to lag"), Some("12"));
    assert!(cell(lb, 0, "p-value") < 1e-3);
    let ur = table_of(&s, SectionKind::SeriesUnitRoot);
    assert_eq!(ur.rows.len(), 3);
    // The plot is tied to the table rows and carries the date axis.
    let SectionContent::Plot(p) = &s.iter().find(|x| x.kind == SectionKind::SeriesPlot).unwrap().content else {
        panic!()
    };
    assert_eq!(p.points.len(), 120);
    assert!(p.x_time.is_some());
}

#[test]
fn transformation_and_spectrum() {
    let settings = DescribeSettings {
        box_cox: BoxCoxChoice::Guerrero,
        differences: 1,
        seasonal_differences: 1,
        bartlett: true,
        spectrum: true,
        ..DescribeSettings::default()
    };
    let (t, r) = demand_request(ExploreMethod::Describe(settings.clone()));
    let s = run(&t, &r).sections();
    let summary = table_of(&s, SectionKind::SeriesSummary);
    let analysed = summary.rows.iter().find(|row| row[0] == "Analysed as").unwrap();
    assert!(analysed[1].contains("Guerrero") && analysed[1].contains("seasonal difference"), "{analysed:?}");
    let values = summary.rows.iter().find(|row| row[0] == "Values after transformation").unwrap();
    assert_eq!(values[1], "107");
    assert!(table_of(&s, SectionKind::SeriesAutocorrelationTable).columns.iter().any(|c| c.contains("Bartlett")));
    // The spectrum of the untransformed series peaks at the 12-month cycle.
    let plain = DescribeSettings { spectrum: true, ..DescribeSettings::default() };
    let (t, r) = demand_request(ExploreMethod::Describe(plain));
    let s = run(&t, &r).sections();
    let peaks = table_of(&s, SectionKind::SeriesSpectrumPeaks);
    let period = cell(peaks, 0, "Period (observations per cycle)");
    assert!((period - 12.0).abs() < 1.0, "strongest cycle {period}");
    // Seasonal differencing without a period is refused.
    let t2 = electricity_demand();
    let bad = SeriesExploreRequest {
        series: SeriesInput::new(1),
        method: ExploreMethod::Describe(DescribeSettings { seasonal_differences: 1, ..DescribeSettings::default() }),
    };
    assert!(SeriesExploreAnalysis::run(&t2, &RowStateManager::new(120), &bad).is_err());
}

#[test]
fn multiplicative_decomposition_finds_the_seasons() {
    let (t, r) = demand_request(ExploreMethod::Decompose(DecomposeSettings::default()));
    let a = run(&t, &r);
    let s = a.sections();
    let ix = table_of(&s, SectionKind::DecompositionIndices);
    assert_eq!(ix.rows.len(), 12);
    assert_eq!(ix.cell(0, "Season"), Some("Jan"));
    let index = |m: usize| cell(ix, m, "Seasonal index (factor)");
    assert!(index(0) > 1.12 && index(0) < 1.25, "January {}", index(0));
    assert!(index(6) > index(3) && index(6) > index(9) && index(3) < 1.0, "a summer peak");
    let mean: f64 = (0..12).map(index).sum::<f64>() / 12.0;
    assert!((mean - 1.0).abs() < 1e-4, "indices average 1 (to the digits shown)");
    let strength = table_of(&s, SectionKind::DecompositionSummary);
    assert!(cell(strength, 1, "Value") > 0.8, "seasonality");
    assert!(cell(strength, 0, "Value") > 0.8, "trend");
    // Saved components: the trend is undefined at the ends of a classical decomposition.
    let cols = a.saveable_columns();
    let keys: Vec<&str> = cols.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["trend", "seasonal", "remainder", "adjusted"]);
    assert!(cols[0].values[0].is_nan() && cols[0].values[60].is_finite());
    assert!(cols[3].values.iter().all(|v| v.is_finite()));
    // STL, periodic and robust.
    for robust in [false, true] {
        let (t, r) = demand_request(ExploreMethod::Decompose(DecomposeSettings {
            method: DecomposeMethod::Stl,
            seasonal_window: if robust { Some(13) } else { None },
            robust,
        }));
        let s = run(&t, &r).sections();
        assert!(!s.iter().any(|x| x.kind == SectionKind::DecompositionIndices));
        assert!(cell(table_of(&s, SectionKind::DecompositionSummary), 1, "Value") > 0.8);
    }
    // A period of 1 cannot be decomposed.
    let mut r1 = r;
    r1.series.period = Some(1);
    let e = SeriesExploreAnalysis::run(&t, &RowStateManager::new(120), &r1).unwrap_err();
    assert!(e.to_string().contains("seasonal period of at least 2"), "{e}");
}

#[test]
fn logistic_trend_recovers_the_adoption_curve() {
    let t = product_adoption();
    let r = SeriesExploreRequest {
        series: SeriesInput { column: 1, time: Some(0), period: None },
        method: ExploreMethod::Trend(TrendSettings { model: TrendChoice::Logistic, ..TrendSettings::default() }),
    };
    let a = run(&t, &r);
    let s = a.sections();
    let coef = table_of(&s, SectionKind::TrendModelCoefficients);
    let c = cell(coef, 0, "Estimate");
    assert!((c - 52_000.0).abs() < 0.08 * 52_000.0, "asymptote {c}");
    let mid = cell(coef, 3, "Estimate");
    assert!((mid - 80f64.ln() / 0.16).abs() < 2.0, "midpoint {mid}");
    let fc = table_of(&s, SectionKind::TrendModelForecast);
    assert_eq!(fc.rows.len(), 12);
    assert_eq!(fc.columns, ["Periods ahead", "Time", "Forecast"]);
    assert_eq!(fc.cell(0, "Time"), Some("2025-01-01"));
    let acc = table_of(&s, SectionKind::TrendModelAccuracy);
    assert!(cell(acc, 0, "Value") < 5.0, "MAPE");
    // Linear, quadratic and exponential trends give prediction limits.
    for model in [TrendChoice::Linear, TrendChoice::Quadratic, TrendChoice::Exponential] {
        let r = SeriesExploreRequest {
            method: ExploreMethod::Trend(TrendSettings { model, horizon: 4, ..TrendSettings::default() }),
            ..r.clone()
        };
        let s = run(&t, &r).sections();
        let fc = table_of(&s, SectionKind::TrendModelForecast);
        assert_eq!(fc.columns.len(), 5, "{model:?}");
        assert!(cell(fc, 0, "Lower 95%") < cell(fc, 0, "Forecast"));
    }
    // A moving average forecasts flat; its accuracy uses n − span forecasts.
    let r = SeriesExploreRequest {
        method: ExploreMethod::Trend(TrendSettings {
            model: TrendChoice::MovingAverage,
            span: 4,
            horizon: 3,
            confidence: 0.95,
        }),
        ..r
    };
    let a = run(&t, &r);
    let s = a.sections();
    let fc = table_of(&s, SectionKind::TrendModelForecast);
    assert_eq!(fc.cell(0, "Forecast"), fc.cell(2, "Forecast"));
    assert_eq!(table_of(&s, SectionKind::TrendModelAccuracy).cell(5, "Value"), Some("44"));
    let fitted = &a.saveable_columns()[0].values;
    assert!(fitted[3].is_nan() && fitted[4].is_finite());
}

#[test]
fn cross_correlation_peaks_at_the_delay() {
    let t = advertising_sales();
    for prewhiten in [Prewhiten::None, Prewhiten::Aic, Prewhiten::Order(1)] {
        let r = SeriesExploreRequest {
            series: SeriesInput { column: 2, time: Some(0), period: None },
            method: ExploreMethod::CrossCorrelation(CrossSettings {
                max_lag: Some(6),
                prewhiten,
                ..CrossSettings::new(1)
            }),
        };
        let a = run(&t, &r);
        assert_eq!(a.title(), "Cross-Correlation: Advertising (k$) → Sales (k$)");
        let s = a.sections();
        let ccf = table_of(&s, SectionKind::CrossCorrelationTable);
        assert_eq!(ccf.rows.len(), 13);
        assert_eq!(ccf.cell(0, "Lag"), Some("-6"));
        let best =
            (0..13).max_by(|&a, &b| cell(ccf, a, "Correlation").total_cmp(&cell(ccf, b, "Correlation"))).unwrap();
        assert_eq!(ccf.cell(best, "Lag"), Some("2"), "{prewhiten:?}");
        assert_eq!(ccf.cell(best, "Beyond the limits"), Some("yes"));
        let has_filter = s.iter().any(|x| x.kind == SectionKind::CrossCorrelationFilter);
        assert_eq!(has_filter, prewhiten != Prewhiten::None);
        let SectionContent::Note(note) = &s[0].content else { panic!() };
        assert!(note.contains("Sales (k$) follows Advertising (k$) by 2 periods"), "{note}");
    }
    // The same column in both roles is refused.
    let r = SeriesExploreRequest {
        series: SeriesInput::new(1),
        method: ExploreMethod::CrossCorrelation(CrossSettings::new(1)),
    };
    assert!(SeriesExploreAnalysis::run(&t, &RowStateManager::new(104), &r).is_err());
}

#[test]
fn gaps_are_reported_and_ends_trimmed() {
    let (t, r) = demand_request(ExploreMethod::Describe(DescribeSettings::default()));
    let mut rows = RowStateManager::new(120);
    rows.set_excluded([0, 1, 119], true);
    let a = SeriesExploreAnalysis::run(&t, &rows, &r).unwrap();
    let s = a.sections();
    assert!(s[0].title.contains("3 leading or trailing rows left out"), "{}", s[0].title);
    rows.set_excluded([50], true);
    let e = SeriesExploreAnalysis::run(&t, &rows, &r).unwrap_err().to_string();
    assert!(e.contains("gap at row 51"), "{e}");
}

#[test]
fn requests_round_trip_and_reload_identically() {
    let t = electricity_demand();
    let requests = vec![
        SeriesExploreRequest {
            series: SeriesInput { column: 1, time: Some(0), period: Some(6) },
            method: ExploreMethod::Describe(DescribeSettings {
                box_cox: BoxCoxChoice::Lambda(0.0),
                differences: 1,
                seasonal_differences: 1,
                max_lag: Some(18),
                bartlett: true,
                confidence: 0.9,
                adf: stat_timeseries::unit_root::AdfDeterministic::None,
                unit_root: false,
                spectrum: true,
                spans: vec![5],
                period_axis: true,
            }),
        },
        SeriesExploreRequest {
            series: SeriesInput { column: 1, time: None, period: Some(12) },
            method: ExploreMethod::Decompose(DecomposeSettings {
                method: DecomposeMethod::Stl,
                seasonal_window: Some(9),
                robust: true,
            }),
        },
        SeriesExploreRequest {
            series: SeriesInput { column: 1, time: Some(0), period: None },
            method: ExploreMethod::Decompose(DecomposeSettings {
                method: DecomposeMethod::ClassicalAdditive,
                ..DecomposeSettings::default()
            }),
        },
        SeriesExploreRequest {
            series: SeriesInput::new(1),
            method: ExploreMethod::Trend(TrendSettings {
                model: TrendChoice::Quadratic,
                span: 5,
                horizon: 7,
                confidence: 0.8,
            }),
        },
        SeriesExploreRequest {
            series: SeriesInput::new(1),
            method: ExploreMethod::CrossCorrelation(CrossSettings {
                max_lag: Some(4),
                prewhiten: Prewhiten::Order(2),
                confidence: 0.99,
                input: 0,
            }),
        },
    ];
    for r in &requests {
        let back = SeriesExploreRequest::from_json(r.method.key(), &r.to_json(&t), &t).unwrap();
        assert_eq!(&back, r);
        let saved = SavedReport::SeriesExplore { request: r.clone() };
        assert_eq!(report_from_json(&report_to_json(&saved, &t), &t).unwrap(), saved);
    }
    // A project of the samples reproduces every section after reloading.
    for sample in ["Monthly electricity demand", "Product adoption", "Advertising and sales"] {
        let info = crate::sample::catalog().into_iter().find(|s| s.name.starts_with(sample)).unwrap();
        let s = (info.build)();
        let mut rows = RowStateManager::new(s.table.n_rows());
        rows.set_excluded([0], true);
        let project =
            Project { external: None, origin: None, scripts: Vec::new(), reports: s.reports, table: s.table, rows };
        let back = load_project(&save_project(&project)).unwrap();
        assert_eq!(back.reports, project.reports);
        for (a, b) in project.reports.iter().zip(&back.reports) {
            let one = crate::run_report(a, &project.table, &project.rows).unwrap();
            let two = crate::run_report(b, &back.table, &back.rows).unwrap();
            assert_eq!(format!("{:?}", one.sections), format!("{:?}", two.sections));
        }
    }
}
