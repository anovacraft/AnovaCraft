//! Integration tests: the whole pipeline (frame → every element → marks) and
//! the statistics behind it on degenerate data — no observations, a single
//! observation, zero variance, missing values, extreme magnitudes — must
//! produce finite marks (or none) and never panic.

use stat_core::distribution_fit::FitFamily;
use stat_data::{Column, DataTable, MeasurementLevel, RowStateManager, SelectionMode};
use stat_viz::{
    AxisScale, BarElement, BarLayout, BinElement, BoxPlotElement, ContourElement, DotPlotElement, Element,
    EllipseElement, EncodingPlan, FilterCondition, FitLabels, FitLineElement, FrameOptions, FunctionElement, GraphForm,
    GraphSpec, HeatmapElement, HistogramBins, HistogramElement, JitterKind, LabelPosition, LineConnect, LineElement,
    LocalFilter, Mark, MarkShape, MarkerStyle, PointsElement, ProbabilityPlotElement, Rgba, SmoothMethod,
    SmootherElement, SmoothingSpline, SummaryElement, TreemapSettings, ViolinElement, box_stats, build_frame,
    jitter_offset, poly_fit,
    summary::{ErrorBars, SummaryStat},
};

fn every_element() -> GraphSpec {
    GraphSpec {
        points: Some(PointsElement {
            jitter: JitterKind::Normal,
            jitter_width: 0.3,
            fade_unselected: true,
            label_selected_only: false,
        }),
        smoother: Some(SmootherElement { lambda: 1e-2, band: true, ..SmootherElement::default() }),
        fit: Some(FitLineElement {
            degree: 3,
            band: true,
            prediction: true,
            level: 0.95,
            labels: FitLabels { equation: true, r_squared: true, p_value: true },
        }),
        box_plot: Some(BoxPlotElement { outliers: true, notches: true }),
        violin: Some(ViolinElement::default()),
        summary: Some(SummaryElement { error_bars: ErrorBars::Confidence, ..SummaryElement::default() }),
        ellipse: Some(EllipseElement { levels: vec![0.9, 0.99], correlation: true }),
        contour: Some(ContourElement::default()),
        bins: Some(BinElement::default()),
        function: Some(FunctionElement { formulas: vec!["1 + ln(x)".into(), "1/x".into(), "nonsense(".into()] }),
        bars: Some(BarElement { error_bars: ErrorBars::Confidence, labels: true, ..BarElement::default() }),
        line: Some(LineElement { connect: LineConnect::Summary, ..LineElement::default() }),
        heatmap: Some(HeatmapElement::default()),
        histogram: Some(HistogramElement { stacked: true, density: true, ..HistogramElement::default() }),
        dot_plot: Some(DotPlotElement::default()),
        probability_plot: Some(ProbabilityPlotElement::default()),
        ..GraphSpec::default()
    }
}

/// The same with LOESS and square bins.
fn every_element_alternatives() -> GraphSpec {
    GraphSpec {
        smoother: Some(SmootherElement {
            method: SmoothMethod::Loess,
            band: true,
            local_degree: 1,
            ..SmootherElement::default()
        }),
        bins: Some(BinElement { hexagonal: false, bins: 7 }),
        summary: Some(SummaryElement { stat: SummaryStat::Percentile(0.9), ..SummaryElement::default() }),
        bars: Some(BarElement { layout: BarLayout::Percent, stat: SummaryStat::Sum, ..BarElement::default() }),
        line: Some(LineElement { connect: LineConnect::RowOrder, ..LineElement::default() }),
        histogram: Some(HistogramElement { bins: HistogramBins::Count(3), ..HistogramElement::default() }),
        dot_plot: Some(DotPlotElement { width: Some(0.5) }),
        probability_plot: Some(ProbabilityPlotElement { family: FitFamily::Weibull, band: true, level: 0.99 }),
        ..every_element()
    }
}

/// Build every layer of `plan` for `table` under every scale and check that
/// every coordinate is finite.
fn assert_pipeline_finite(table: &DataTable, plan: &EncodingPlan, state: &RowStateManager) {
    let forms = [
        GraphSpec { form: GraphForm::Treemap(TreemapSettings { size_by_sum: true, labels: true }), ..every_element() },
        GraphSpec { form: GraphForm::Parallel, ..every_element() },
        GraphSpec { form: GraphForm::MultiVari, ..every_element() },
        GraphSpec { form: GraphForm::ScatterMatrix, ..every_element() },
        GraphSpec {
            form: GraphForm::Pie(stat_viz::PieSettings { donut: true, size_by_sum: true, labels: true }),
            ..every_element()
        },
        GraphSpec { form: GraphForm::Pie(stat_viz::PieSettings::default()), ..every_element() },
        GraphSpec {
            form: GraphForm::Radar(stat_viz::RadarSettings { group_means: true, fill: true }),
            ..every_element()
        },
        GraphSpec { form: GraphForm::Radar(stat_viz::RadarSettings::default()), ..every_element() },
        GraphSpec {
            form: GraphForm::Polar(stat_viz::PolarSettings { angle: stat_viz::AngleUnit::Radians, connect: true }),
            ..every_element()
        },
        GraphSpec { form: GraphForm::WindRose(stat_viz::WindRoseSettings::default()), ..every_element() },
        GraphSpec { form: GraphForm::Ternary, ..every_element() },
        GraphSpec { form: GraphForm::Waterfall(stat_viz::WaterfallSettings::default()), ..every_element() },
        GraphSpec { form: GraphForm::Ridgeline(stat_viz::RidgelineSettings::default()), ..every_element() },
        GraphSpec { form: GraphForm::VectorField(stat_viz::VectorSettings::default()), ..every_element() },
    ];
    for mut spec in [every_element(), every_element_alternatives()].into_iter().chain(forms) {
        spec.encoding = plan.clone();
        let mut filter = LocalFilter::default();
        if let Some(c) = FilterCondition::for_column(table, "y") {
            filter.conditions.push(c);
        }
        for (k, (xs, ys)) in [
            (AxisScale::Linear, AxisScale::Linear),
            (AxisScale::Log10, AxisScale::Sqrt),
            (AxisScale::Ln, AxisScale::Log10),
            (AxisScale::Logit, AxisScale::Reciprocal),
        ]
        .into_iter()
        .enumerate()
        {
            let mut options = FrameOptions { x_scale: xs, y_scale: ys, page: k, ..FrameOptions::default() };
            if k % 2 == 1 {
                options.filter = filter.clone();
                options.filter.show_only = k == 3;
            }
            spec.options = options.clone();
            let frame = match spec.form {
                GraphForm::Panels => {
                    build_frame(table, state, plan, &options).unwrap_or_else(|e| panic!("{plan:?}: {e}"))
                }
                // Whole-panel forms reject plans they cannot draw.
                _ => match spec.frame(table, state) {
                    Ok(f) => f,
                    Err(_) => continue,
                },
            };
            for layers in spec.layers(&frame, state) {
                for layer in layers {
                    assert!(layer.all_finite(), "{plan:?} {xs:?}/{ys:?}: {layer:?}");
                }
            }
        }
    }
}

fn degenerate_tables() -> Vec<DataTable> {
    let mut nominal = Column::numeric("cat", vec![1.0; 6]);
    nominal.level = MeasurementLevel::Nominal;
    vec![
        // No rows at all.
        DataTable::new(
            "empty",
            vec![Column::numeric("x", vec![]), Column::numeric("y", vec![]), Column::text("g", vec![])],
        )
        .unwrap(),
        // A single observation.
        DataTable::new(
            "one",
            vec![Column::numeric("x", vec![2.0]), Column::numeric("y", vec![3.0]), Column::text("g", vec!["a".into()])],
        )
        .unwrap(),
        // Zero variance in both, plus a constant category.
        DataTable::new(
            "flat",
            vec![
                Column::numeric("x", vec![5.0; 6]),
                Column::numeric("y", vec![7.0; 6]),
                Column::text("g", vec!["a".into(); 6]),
                nominal,
            ],
        )
        .unwrap(),
        // Missing values everywhere, negatives (outside log domains), huge and tiny magnitudes.
        DataTable::new(
            "messy",
            vec![
                Column::numeric("x", vec![f64::NAN, -1.0, 0.0, 1e-300, 1e300, 3.0, 3.0, f64::NAN]),
                Column::numeric("y", vec![1.0, f64::NAN, -2.0, 0.0, 1e300, 4.0, 4.0, 5.0]),
                Column::text("g", ["a", "", "b", "a", "b", "a", "b", ""].map(String::from).to_vec()),
            ],
        )
        .unwrap(),
    ]
}

#[test]
fn every_element_is_finite_on_degenerate_data() {
    for table in degenerate_tables() {
        let n = table.n_rows();
        let mut state = RowStateManager::new(n);
        if n > 1 {
            state.select([0], SelectionMode::Replace);
            state.set_excluded([n - 1], true);
        }
        let plans = [
            EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], ..Default::default() },
            EncodingPlan { x: vec!["x".into()], color: Some("g".into()), freq: Some("y".into()), ..Default::default() },
            EncodingPlan { x: vec!["g".into()], y: vec!["y".into()], overlay: Some("g".into()), ..Default::default() },
            EncodingPlan { y: vec!["y".into()], color: Some("x".into()), size: Some("y".into()), ..Default::default() },
            EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], wrap: Some("g".into()), ..Default::default() },
            EncodingPlan {
                x: vec!["x".into()],
                y: vec!["y".into()],
                label: Some("g".into()),
                page: Some("g".into()),
                freq: Some("x".into()),
                weight: Some("y".into()),
                ..Default::default()
            },
            EncodingPlan {
                x: vec!["x".into(), "y".into()],
                y: vec!["y".into()],
                group_y: Some("g".into()),
                color: Some("g".into()),
                ..Default::default()
            },
            // Two columns on each shelf (vector fields), a category up (ridgelines).
            EncodingPlan { x: vec!["x".into(), "y".into()], y: vec!["y".into(), "x".into()], ..Default::default() },
            EncodingPlan { x: vec!["x".into()], y: vec!["g".into()], color: Some("g".into()), ..Default::default() },
        ];
        for plan in &plans {
            assert_pipeline_finite(&table, plan, &state);
        }
    }
}

#[test]
fn statistics_on_edge_cases() {
    // Box plots.
    assert!(box_stats(&[]).is_none());
    let single = box_stats(&[42.0]).unwrap();
    assert_eq!((single.q1, single.median, single.q3), (42.0, 42.0, 42.0));
    assert!(single.outliers.is_empty());
    let constant = box_stats(&[1.5; 10]).unwrap();
    assert_eq!((constant.lower_whisker, constant.upper_whisker), (1.5, 1.5));
    let with_nan = box_stats(&[f64::NAN, 1.0, 2.0, f64::INFINITY]).unwrap();
    assert_eq!(with_nan.n, 2);
    // Jitter stays bounded for any width, including zero and absurd values.
    for kind in JitterKind::ALL {
        for width in [0.0, 1e-12, 0.3, 1e9, -0.3, f64::INFINITY] {
            for row in [0, 1, usize::MAX] {
                let o = jitter_offset(kind, width, row, 0);
                assert!(o.is_finite(), "{kind:?} {width} {row}");
                if width.is_finite() {
                    assert!(o.abs() <= width.abs() + 1e-12);
                }
            }
        }
    }
    // Polynomial fits: zero variance, single observation, too few distinct x.
    assert!(poly_fit(&[], &[], 1, 0.95).is_none());
    assert!(poly_fit(&[1.0], &[2.0], 2, 0.95).is_none());
    assert!(poly_fit(&[3.0; 4], &[1.0, 2.0, 3.0, 4.0], 1, 0.95).is_none());
    let flat_y = poly_fit(&[1.0, 2.0, 3.0, 4.0], &[2.0; 4], 2, 0.95).unwrap();
    assert!(flat_y.curve.iter().all(|p| p[0].is_finite() && (p[1] - 2.0).abs() < 1e-9));
    let tiny = poly_fit(&[1e-300, 2e-300, 3e-300], &[1.0, 2.0, 3.0], 1, 0.95);
    if let Some(f) = tiny {
        assert!(f.curve.iter().all(|p| p[1].is_finite()));
    }
    // Smoothing spline: degenerate inputs.
    assert!(SmoothingSpline::fit(&[], &[], 0.1).is_none());
    for lambda in [0.0, 1e-12, 1.0, 1e12, f64::INFINITY, -1.0] {
        let s = SmoothingSpline::fit(&[1.0, 2.0, 3.0, 4.0], &[1.0, 4.0, 9.0, 16.0], lambda);
        if let Some(s) = s {
            assert!(s.curve(20).iter().all(|p| p[1].is_finite()), "λ = {lambda}");
        }
    }
}

#[test]
fn multi_element_pipeline_draws_each_layer() {
    // Three groups with a clear trend: every element contributes marks to the
    // same panel, in drawing order (boxes, fits, smoother, points).
    let n = 60;
    let g: Vec<String> = (0..n).map(|i| ["a", "b", "c"][i % 3].to_owned()).collect();
    let x: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let y: Vec<f64> = (0..n).map(|i| 2.0 * i as f64 + (i % 7) as f64).collect();
    let table =
        DataTable::new("t", vec![Column::numeric("x", x), Column::numeric("y", y), Column::text("g", g)]).unwrap();
    let state = RowStateManager::new(n);
    // Continuous x: fit, smoother and points (boxes report they need categories).
    let mut spec = GraphSpec {
        points: Some(PointsElement::default()),
        smoother: Some(SmootherElement { lambda: 1e-2, ..SmootherElement::default() }),
        fit: Some(FitLineElement { degree: 3, ..FitLineElement::default() }),
        box_plot: Some(BoxPlotElement::default()),
        ..GraphSpec::default()
    };
    spec.encoding =
        EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], overlay: Some("g".into()), ..Default::default() };
    let frame = spec.frame(&table, &state).unwrap();
    let layers = spec.layers(&frame, &state);
    assert_eq!(layers.len(), 1);
    let kinds: Vec<usize> = layers[0].iter().map(|l| l.marks.len()).collect();
    assert!(kinds[0] == 0 && !layers[0][0].notes.is_empty(), "boxes need a categorical axis");
    assert_eq!(layers[0][1].marks.iter().filter(|m| matches!(m, Mark::Line { .. })).count(), 3, "one fit per subgroup");
    assert_eq!(layers[0][1].marks.iter().filter(|m| matches!(m, Mark::Text { .. })).count(), 3, "R² labels");
    assert_eq!(layers[0][2].marks.len(), 3, "one smoother per subgroup");
    assert!(!layers[0][3].marks.is_empty());
    // Categorical x: boxes per subgroup, points jittered within their slots.
    spec.encoding.x = vec!["g".into()];
    let frame = spec.frame(&table, &state).unwrap();
    let layers = spec.layers(&frame, &state);
    let boxes = layers[0][0].marks.iter().filter(|m| matches!(m, Mark::Polygon { .. })).count();
    assert_eq!(boxes, 3);
    let points = spec.points.unwrap();
    for p in &frame.panels[0].points {
        let at = points.position(&frame.panels[0], p);
        assert!((at[0] - p.x).abs() <= frame.panels[0].slot_half_width + 1e-12);
    }
    // Selection: the selection gets rings; unselected points stay opaque
    // unless fading is switched on, and then stay clearly visible.
    let mut state = RowStateManager::new(n);
    state.select([0, 1, 2], SelectionMode::Replace);
    let alphas = |points: &PointsElement| -> Vec<u8> {
        points
            .layer(&frame.panels[0], &state)
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Points { color, filled: true, name, .. } if name == "Rows" => Some(color.0[3]),
                _ => None,
            })
            .collect()
    };
    assert!(alphas(&points).iter().all(|a| *a == 255), "no fading by default");
    let fading = PointsElement { fade_unselected: true, ..points };
    let faded = alphas(&fading);
    assert!(faded.iter().any(|a| *a < 255) && faded.iter().all(|a| *a >= 128), "{faded:?}");
    let layer = points.layer(&frame.panels[0], &state);
    let rings = layer.marks.iter().find_map(|m| match m {
        Mark::Points { filled: false, points, .. } => Some(points.len()),
        _ => None,
    });
    assert_eq!(rings, Some(3));
}

#[test]
fn pages_filters_frequencies_and_weights() {
    let table = DataTable::new(
        "t",
        vec![
            Column::numeric("x", (0..12).map(f64::from).collect()),
            Column::numeric("y", (0..12).map(|i| f64::from(i) * 2.0 + f64::from(i % 3)).collect()),
            Column::text("g", (0..12).map(|i| ["p", "q", "r"][i % 3].to_owned()).collect()),
            Column::numeric("f", (0..12).map(|i| if i == 5 { 0.0 } else { 1.0 + f64::from(i % 2) }).collect()),
        ],
    )
    .unwrap();
    let state = RowStateManager::new(12);
    let mut spec = GraphSpec {
        encoding: EncodingPlan {
            x: vec!["x".into()],
            y: vec!["y".into()],
            page: Some("g".into()),
            freq: Some("f".into()),
            ..Default::default()
        },
        fit: Some(FitLineElement::default()),
        ..GraphSpec::default()
    };
    // Pages: one level at a time, clamped.
    let frame = spec.frame(&table, &state).unwrap();
    assert_eq!(frame.pages, ["p", "q", "r"]);
    assert_eq!(frame.page_title(&spec.encoding).as_deref(), Some("g: p"));
    assert!(frame.panels[0].points.iter().all(|p| p.row % 3 == 0));
    spec.options.page = 9;
    let frame = spec.frame(&table, &state).unwrap();
    assert_eq!(frame.page, 2);
    assert!(frame.panels[0].points.iter().all(|p| p.row % 3 == 2));
    // Frequencies travel with the points; a zero frequency leaves the analysis.
    let five = frame.panels[0].points.iter().find(|p| p.row == 5).unwrap();
    assert!(five.freq == 0.0 && !five.in_analysis());
    assert_eq!(frame.panels[0].points.iter().find(|p| p.row == 8).unwrap().freq, 1.0);

    // Filter: show only x ≤ 5, or keep the rest drawn but out of the fit.
    spec.encoding.page = None;
    spec.options.filter.conditions = vec![FilterCondition::Range { column: "x".into(), min: 0.0, max: 5.0 }];
    let frame = spec.frame(&table, &state).unwrap();
    assert_eq!(frame.panels[0].points.len(), 6);
    assert!(frame.notes.iter().any(|n| n.contains("outside the filter")));
    spec.options.filter.show_only = false;
    let frame = spec.frame(&table, &state).unwrap();
    let p = &frame.panels[0];
    assert_eq!(p.points.len(), 12);
    assert_eq!(p.points.iter().filter(|q| q.filtered).count(), 6);
    let fit = spec.fit.unwrap().fits(p)[0].clone().unwrap();
    assert_eq!(fit.n, 5, "rows 0–5 except the zero-frequency row 5");
    // Hollow markers for rows outside the filter.
    let layer = PointsElement::default().layer(p, &state);
    assert!(
        layer
            .marks
            .iter()
            .any(|m| matches!(m, Mark::Points { filled: false, name, .. } if name == "Outside the filter"))
    );
    // The graph knows every column it uses, and follows renames.
    spec.switchers.push(stat_viz::ColumnSwitcher {
        role: stat_viz::Role::Y,
        slot: 0,
        choices: vec!["y".into(), "f".into()],
    });
    assert_eq!(spec.columns(), ["x", "y", "f"]);
    spec.rename("x", "x2");
    assert_eq!(spec.encoding.x, ["x2"]);
    assert_eq!(spec.options.filter.columns(), ["x2"]);
}

#[test]
fn distribution_density_summary_and_formula_layers() {
    let n = 90;
    let g: Vec<String> = (0..n).map(|i| ["a", "b", "c"][i % 3].to_owned()).collect();
    let x: Vec<f64> = (0..n).map(|i| 1.0 + (i as f64 * 0.37).sin().abs() * 9.0).collect();
    let y: Vec<f64> = x.iter().enumerate().map(|(i, v)| 3.0 * v + (i % 5) as f64 + (i % 3) as f64 * 4.0).collect();
    let table =
        DataTable::new("t", vec![Column::numeric("x", x), Column::numeric("y", y), Column::text("g", g)]).unwrap();
    let state = RowStateManager::new(n);
    let kinds = |spec: &GraphSpec| -> Vec<stat_viz::ElementKind> { spec.elements().iter().map(|e| e.kind()).collect() };

    // Categories across: violins (band + outline + 3 quartile lines each), notched
    // boxes, summary diamonds with error bars.
    let spec = GraphSpec {
        encoding: EncodingPlan { x: vec!["g".into()], y: vec!["y".into()], ..Default::default() },
        violin: Some(ViolinElement::default()),
        box_plot: Some(BoxPlotElement { outliers: true, notches: true }),
        summary: Some(SummaryElement { error_bars: ErrorBars::StdErr, ..SummaryElement::default() }),
        points: None,
        ..GraphSpec::default()
    };
    assert_eq!(
        kinds(&spec),
        [stat_viz::ElementKind::Violin, stat_viz::ElementKind::BoxPlot, stat_viz::ElementKind::Summary]
    );
    let frame = spec.frame(&table, &state).unwrap();
    let layers = spec.layers(&frame, &state);
    let [violin, boxes, summary] = &layers[0][..] else { panic!() };
    assert_eq!(violin.marks.iter().filter(|m| matches!(m, Mark::Band { .. })).count(), 3);
    assert_eq!(boxes.marks.iter().filter(|m| matches!(m, Mark::Band { .. })).count(), 3, "notched outlines");
    let Some(Mark::Points { points, .. }) = summary.marks.iter().find(|m| matches!(m, Mark::Points { .. })) else {
        panic!("summary points")
    };
    assert_eq!(points.len(), 3);
    assert_eq!(summary.marks.iter().filter(|m| matches!(m, Mark::Line { .. })).count(), 9, "bar and two caps each");
    assert!(!summary.key.is_empty() && !boxes.key.is_empty() && !violin.key.is_empty());

    // Numbers on both axes: ellipses, contours, hexagonal bins, LOESS band, formulas.
    let spec = GraphSpec {
        encoding: EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], ..Default::default() },
        ellipse: Some(EllipseElement { levels: vec![0.5, 0.95], correlation: true }),
        contour: Some(ContourElement::default()),
        bins: Some(BinElement::default()),
        smoother: Some(SmootherElement { method: SmoothMethod::Loess, band: true, ..SmootherElement::default() }),
        fit: Some(FitLineElement {
            prediction: true,
            labels: FitLabels { equation: true, r_squared: true, p_value: true },
            ..FitLineElement::default()
        }),
        function: Some(FunctionElement { formulas: vec!["3*x + 4".into(), "ln(x - 5)".into(), "bad +".into()] }),
        points: None,
        ..GraphSpec::default()
    };
    let frame = spec.frame(&table, &state).unwrap();
    let layers = spec.layers(&frame, &state);
    let by_kind = |k: stat_viz::ElementKind| &layers[0][kinds(&spec).iter().position(|x| *x == k).unwrap()];
    let bins = by_kind(stat_viz::ElementKind::Bins);
    let cells: f64 = bins.marks.iter().filter(|m| matches!(m, Mark::Polygon { .. })).count() as f64;
    assert!(cells > 3.0 && bins.ramp.is_some());
    let ramp = bins.ramp.as_ref().unwrap();
    assert!(ramp.max >= ramp.min && ramp.reversed);
    let contours = by_kind(stat_viz::ElementKind::Contour);
    assert!(contours.marks.iter().all(|m| matches!(m, Mark::Segments { .. })) && contours.marks.len() >= 3);
    let ellipses = by_kind(stat_viz::ElementKind::Ellipse);
    assert_eq!(ellipses.marks.iter().filter(|m| matches!(m, Mark::Line { .. })).count(), 2);
    assert!(ellipses.marks.iter().any(|m| matches!(m, Mark::Text { text, .. } if text.starts_with("r = "))));
    let smooth = by_kind(stat_viz::ElementKind::Smoother);
    assert!(smooth.marks.iter().any(|m| matches!(m, Mark::Band { .. })));
    let fit = by_kind(stat_viz::ElementKind::FitLine);
    let label = fit.marks.iter().find_map(|m| if let Mark::Text { text, .. } = m { Some(text.clone()) } else { None });
    let label = label.unwrap();
    assert!(label.starts_with("y = ") && label.contains("R² = ") && label.contains("p < 0.0001"), "{label}");
    assert_eq!(
        fit.marks.iter().filter(|m| matches!(m, Mark::Line { pattern: stat_viz::LinePattern::Dashed, .. })).count(),
        2
    );
    let curves = by_kind(stat_viz::ElementKind::Function);
    // The line covers the range; ln(x − 5) only where x > 5; the bad formula is a note.
    assert!(curves.marks.len() >= 2);
    assert!(curves.notes.iter().any(|n| n.contains("bad +")));
    assert_eq!(curves.key.len(), 2);
    for l in &layers[0] {
        assert!(l.all_finite());
    }
}

#[test]
fn bars_lines_and_heat_maps() {
    let table = DataTable::new(
        "t",
        vec![
            Column::text("g", ["a", "a", "b", "b", "b", "c"].map(String::from).to_vec()),
            Column::text("h", ["p", "q", "p", "q", "q", "p"].map(String::from).to_vec()),
            Column::numeric("y", vec![1.0, 3.0, 2.0, 4.0, 6.0, 5.0]),
            Column::numeric("v", vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0]),
        ],
    )
    .unwrap();
    let mut state = RowStateManager::new(6);
    let polygons = |l: &stat_viz::Layer| -> Vec<Vec<[f64; 2]>> {
        l.marks
            .iter()
            .filter_map(|m| match m {
                Mark::Polygon { points, name, .. } if name.ends_with("bar") => Some(points.clone()),
                _ => None,
            })
            .collect()
    };
    let top = |p: &[[f64; 2]]| p.iter().map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max);
    // Counts with no vertical column: a, b, c = 2, 3, 1.
    let counts = GraphSpec {
        encoding: EncodingPlan { x: vec!["g".into()], ..Default::default() },
        bars: Some(BarElement::default()),
        points: None,
        ..GraphSpec::default()
    };
    let f = counts.frame(&table, &state).unwrap();
    let l = &counts.layers(&f, &state)[0][0];
    let tops: Vec<f64> = polygons(l).iter().map(|p| top(p)).collect();
    assert_eq!(tops, [2.0, 3.0, 1.0]);
    assert_eq!(l.regions.len(), 3);
    assert_eq!(stat_viz::region_rows(std::slice::from_ref(l), [2.0, 1.0]), Some(vec![2, 3, 4]));
    // Means side by side with subgroups, then stacked sums, then 100%.
    let mut spec = GraphSpec {
        encoding: EncodingPlan {
            x: vec!["g".into()],
            y: vec!["y".into()],
            overlay: Some("h".into()),
            ..Default::default()
        },
        bars: Some(BarElement { stat: SummaryStat::Sum, ..BarElement::default() }),
        points: None,
        ..GraphSpec::default()
    };
    let f = spec.frame(&table, &state).unwrap();
    assert_eq!(polygons(&spec.layers(&f, &state)[0][0]).len(), 5);
    spec.bars.as_mut().unwrap().layout = BarLayout::Stacked;
    let l = &spec.layers(&f, &state)[0][0];
    let b_tops: Vec<f64> =
        polygons(l).iter().filter(|p| (p[0][0] - 1.65).abs() < 1.0 && p[0][0] > 1.5).map(|p| top(p)).collect();
    assert_eq!(b_tops, [2.0, 12.0], "b stacks p (2) under q (4 + 6)");
    spec.bars.as_mut().unwrap().layout = BarLayout::Percent;
    let l = &spec.layers(&f, &state)[0][0];
    assert!(polygons(l).iter().all(|p| top(p) <= 100.0 + 1e-9));
    assert!(polygons(l).iter().any(|p| (top(p) - 100.0).abs() < 1e-9));
    // The selected share is filled.
    state.select([4], stat_data::SelectionMode::Replace);
    spec.bars.as_mut().unwrap().layout = BarLayout::SideBySide;
    let l = &spec.layers(&f, &state)[0][0];
    assert!(l.marks.iter().any(|m| matches!(m, Mark::Polygon { name, .. } if name == "Selected rows")));
    // Lines: row order joins all rows; summaries one vertex per position.
    let mut lines = GraphSpec {
        encoding: EncodingPlan { x: vec!["v".into()], y: vec!["y".into()], ..Default::default() },
        line: Some(LineElement::default()),
        points: None,
        ..GraphSpec::default()
    };
    let f = lines.frame(&table, &state).unwrap();
    let l = &lines.layers(&f, &state)[0][0];
    assert!(matches!(&l.marks[0], Mark::Line { points, .. } if points.len() == 6));
    lines.encoding.x = vec!["g".into()];
    lines.line = Some(LineElement { connect: LineConnect::Summary, stat: SummaryStat::Max });
    let f = lines.frame(&table, &state).unwrap();
    let l = &lines.layers(&f, &state)[0][0];
    let Some(Mark::Line { points, .. }) = l.marks.iter().find(|m| matches!(m, Mark::Line { .. })) else { panic!() };
    assert_eq!(points.iter().map(|p| p[1]).collect::<Vec<_>>(), [3.0, 6.0, 5.0]);
    // Heat map: counts per (g, h) cell, then the mean of a color column.
    let mut heat = GraphSpec {
        encoding: EncodingPlan { x: vec!["g".into()], y: vec!["h".into()], ..Default::default() },
        heatmap: Some(HeatmapElement { stat: SummaryStat::Mean, labels: true }),
        points: None,
        ..GraphSpec::default()
    };
    let f = heat.frame(&table, &state).unwrap();
    let l = &heat.layers(&f, &state)[0][0];
    assert_eq!(l.regions.len(), 5);
    assert_eq!(l.ramp.as_ref().map(|r| (r.min, r.max)), Some((1.0, 2.0)));
    assert!(l.marks.iter().any(|m| matches!(m, Mark::Text { text, .. } if text == "2")));
    heat.encoding.color = Some("v".into());
    let f = heat.frame(&table, &state).unwrap();
    let l = &heat.layers(&f, &state)[0][0];
    assert_eq!(l.ramp.as_ref().map(|r| (r.min, r.max)), Some((10.0, 60.0)));
    assert!(l.marks.iter().any(|m| matches!(m, Mark::Text { text, .. } if text == "45")), "b·q mean of 40 and 50");
}

#[test]
fn point_labels() {
    let table = DataTable::new(
        "t",
        vec![
            Column::numeric("x", vec![1.0, 2.0, 3.0]),
            Column::numeric("y", vec![1.0, 4.0, 9.0]),
            Column::text("name", ["one", "", "three"].map(String::from).to_vec()),
        ],
    )
    .unwrap();
    let mut state = RowStateManager::new(3);
    let mut spec = GraphSpec {
        encoding: EncodingPlan {
            x: vec!["x".into()],
            y: vec!["y".into()],
            label: Some("name".into()),
            ..Default::default()
        },
        ..GraphSpec::default()
    };
    let texts = |spec: &GraphSpec, state: &RowStateManager| -> Vec<String> {
        let f = spec.frame(&table, state).unwrap();
        spec.layers(&f, state)[0]
            .iter()
            .flat_map(|l| l.marks.iter())
            .filter_map(|m| if let Mark::Text { text, .. } = m { Some(text.trim().to_owned()) } else { None })
            .collect()
    };
    assert_eq!(texts(&spec, &state), ["one", "three"], "blank labels are skipped");
    spec.points.as_mut().unwrap().label_selected_only = true;
    assert!(texts(&spec, &state).is_empty());
    state.select([2], SelectionMode::Replace);
    assert_eq!(texts(&spec, &state), ["three"]);
}

#[test]
fn marker_style_fit_colors_and_moved_labels() {
    let table = DataTable::new(
        "t",
        vec![
            Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0]),
            Column::numeric("y", vec![1.0, 2.1, 2.9, 4.2, 2.0, 2.2, 2.5, 2.4]),
            Column::text("g", ["a", "a", "a", "a", "b", "b", "b", "b"].map(String::from).to_vec()),
        ],
    )
    .unwrap();
    let state = RowStateManager::new(8);
    let red = Rgba::rgb(200, 30, 30);
    let mut spec = GraphSpec {
        encoding: EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], ..Default::default() },
        fit: Some(FitLineElement { prediction: true, ..FitLineElement::default() }),
        ..GraphSpec::default()
    };
    spec.options.style =
        MarkerStyle { size: Some(7.0), color: Some(red), shape: Some(MarkShape::Square), ..Default::default() };
    let f = spec.frame(&table, &state).unwrap();
    let p = &f.panels[0];
    assert!(p.points.iter().all(|q| q.color == red && q.shape == MarkShape::Square && (q.radius - 7.0).abs() < 1e-6));
    let layers = spec.layers(&f, &state);
    let fit_colors: Vec<Rgba> = layers[0]
        .iter()
        .flat_map(|l| l.marks.iter())
        .filter_map(|m| match m {
            Mark::Line { color, .. } | Mark::Text { color, .. } => Some(*color),
            Mark::Band { color, .. } => Some(color.with_alpha(1.0)),
            _ => None,
        })
        .collect();
    assert!(fit_colors.len() >= 4, "line, two prediction limits, label");
    assert!(fit_colors.iter().all(|c| c.0[..3] == red.0[..3]), "{fit_colors:?}");

    // Per-level overrides on a colour column; each subgroup's fit follows its markers.
    spec.encoding.color = Some("g".into());
    let blue = Rgba::rgb(10, 40, 220);
    spec.options.style.level_colors.insert(MarkerStyle::key("g", "b"), blue);
    spec.options.style.level_shapes.insert(MarkerStyle::key("g", "b"), MarkShape::Cross);
    let f = spec.frame(&table, &state).unwrap();
    let b: Vec<_> = f.panels[0].points.iter().filter(|q| q.row >= 4).collect();
    assert!(b.iter().all(|q| q.color == blue && q.shape == MarkShape::Cross));
    let layers = spec.layers(&f, &state);
    let labels: Vec<(String, Rgba, [f64; 2])> = layers[0]
        .iter()
        .flat_map(|l| l.marks.iter())
        .filter_map(|m| match m {
            Mark::Text { movable: Some(id), color, at, .. } => Some((id.clone(), *color, *at)),
            _ => None,
        })
        .collect();
    assert_eq!(labels.len(), 2);
    assert!(labels.iter().any(|(id, c, _)| id == "fit/1" && *c == blue));

    // A moved label lands at its fraction of the view; others stay put.
    spec.place_label(0, "fit/1", [0.25, 0.75]);
    spec.place_label(0, "fit/1", [0.1, 1.5]);
    assert_eq!(spec.label_positions, [LabelPosition { panel: 0, id: "fit/1".into(), at: [0.1, 1.0] }]);
    let view = ([0.0, 10.0], [-2.0, 2.0]);
    let placed = stat_viz::place_labels(&layers[0], &spec.label_positions, 0, view);
    let at = |ls: &[stat_viz::Layer], want: &str| {
        ls.iter()
            .flat_map(|l| l.marks.iter())
            .find_map(|m| match m {
                Mark::Text { movable: Some(id), at, .. } if id == want => Some(*at),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(at(&placed, "fit/1"), [1.0, 2.0]);
    assert_eq!(at(&placed, "fit/0"), labels.iter().find(|l| l.0 == "fit/0").unwrap().2);
    assert_eq!(stat_viz::place_labels(&layers[0], &spec.label_positions, 1, view), layers[0], "other panels untouched");
    assert_eq!(stat_viz::view::view_fraction([1.0, 2.0], view), [0.1, 1.0]);
}

#[test]
fn marker_styles_per_panel_and_level_sizes() {
    let table = DataTable::new(
        "t",
        vec![
            Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0]),
            Column::numeric("y", vec![1.0, 2.0, 3.0, 4.0, 2.0, 2.5, 2.5, 3.0]),
            Column::text("g", ["a", "b", "a", "b", "a", "b", "a", "b"].map(String::from).to_vec()),
            Column::text("p", ["L", "L", "L", "L", "R", "R", "R", "R"].map(String::from).to_vec()),
        ],
    )
    .unwrap();
    let state = RowStateManager::new(8);
    let mut spec = GraphSpec {
        encoding: EncodingPlan {
            x: vec!["x".into()],
            y: vec!["y".into()],
            color: Some("g".into()),
            group_x: Some("p".into()),
            ..Default::default()
        },
        fit: Some(FitLineElement::default()),
        ..GraphSpec::default()
    };
    let red = Rgba::rgb(220, 0, 0);
    spec.options.style.level_sizes.insert(MarkerStyle::key("g", "a"), 11.0);
    let mut right = MarkerStyle::default();
    right.level_colors.insert(MarkerStyle::key("g", "b"), red);
    right.level_shapes.insert(MarkerStyle::key("g", "b"), MarkShape::Plus);
    right.level_sizes.insert(MarkerStyle::key("g", "a"), 4.0);
    spec.options.style.panels.insert(1, right);
    let f = spec.frame(&table, &state).unwrap();
    assert_eq!(f.panels.len(), 2);
    let point = |panel: usize, row: usize| *f.panels[panel].points.iter().find(|p| p.row == row).unwrap();
    // Graph-wide level size; panel 1 overrides it and recolors level b there only.
    assert_eq!((point(0, 0).radius, point(1, 4).radius), (11.0, 4.0));
    assert_eq!(point(0, 1).radius, stat_viz::DEFAULT_RADIUS);
    assert_ne!(point(0, 1).color, red);
    assert_eq!((point(1, 5).color, point(1, 5).shape), (red, MarkShape::Plus));
    // The fit for level b in panel 1 follows its markers.
    let layers = spec.layers(&f, &state);
    let fit_colors: Vec<Rgba> = layers[1]
        .iter()
        .flat_map(|l| l.marks.iter())
        .filter_map(|m| if let Mark::Line { color, .. } = m { Some(*color) } else { None })
        .collect();
    assert!(fit_colors.contains(&red));
    let left: Vec<Rgba> = layers[0]
        .iter()
        .flat_map(|l| l.marks.iter())
        .filter_map(|m| if let Mark::Line { color, .. } = m { Some(*color) } else { None })
        .collect();
    assert!(!left.contains(&red));
    // The key keeps the graph-wide styles.
    assert!(spec.options.style.for_panel(0).level_colors.is_empty());
    assert_eq!(spec.options.style.for_panel(1).level_size("g", "a"), Some(4.0));
}

/// A frequency of k must draw the same smoother (curve and band) as the row
/// listed k times, for both the spline and LOESS.
#[test]
fn smoother_frequencies_count_observations() {
    let x: Vec<f64> = (0..16).map(|i| f64::from(i) * 0.5).collect();
    let y: Vec<f64> = x.iter().enumerate().map(|(i, v)| v.sin() + [0.3, -0.2, 0.1, -0.25][i % 4]).collect();
    let f: Vec<f64> = (0..16).map(|i| f64::from(1 + i % 3)).collect();
    let (mut xe, mut ye) = (Vec::new(), Vec::new());
    for i in 0..16 {
        for _ in 0..(1 + i % 3) {
            xe.push(x[i]);
            ye.push(y[i]);
        }
    }
    let compressed =
        DataTable::new("c", vec![Column::numeric("x", x), Column::numeric("y", y), Column::numeric("f", f)]).unwrap();
    let expanded = DataTable::new("e", vec![Column::numeric("x", xe), Column::numeric("y", ye)]).unwrap();
    let marks = |table: &DataTable, freq: Option<&str>, method: SmoothMethod| -> Vec<Mark> {
        let spec = GraphSpec {
            encoding: EncodingPlan {
                x: vec!["x".into()],
                y: vec!["y".into()],
                freq: freq.map(str::to_owned),
                ..Default::default()
            },
            smoother: Some(SmootherElement {
                method,
                band: true,
                lambda: 1e-3,
                span: 0.6,
                ..SmootherElement::default()
            }),
            ..GraphSpec::default()
        };
        let state = RowStateManager::new(table.n_rows());
        let frame = spec.frame(table, &state).unwrap();
        spec.layers(&frame, &state)
            .remove(0)
            .into_iter()
            .flat_map(|l| l.marks)
            .filter(|m| matches!(m, Mark::Band { .. } | Mark::Line { .. }))
            .collect()
    };
    for method in [SmoothMethod::Spline, SmoothMethod::Loess] {
        let a = marks(&compressed, Some("f"), method);
        let b = marks(&expanded, None, method);
        let coords = |m: &[Mark]| -> Vec<[f64; 2]> { m.iter().flat_map(Mark::coordinates).collect() };
        let (ca, cb) = (coords(&a), coords(&b));
        assert!(a.iter().any(|m| matches!(m, Mark::Band { .. })), "{method:?} has a band");
        assert_eq!(ca.len(), cb.len(), "{method:?}");
        for (p, q) in ca.iter().zip(&cb) {
            assert!((p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9, "{method:?}: {p:?} vs {q:?}");
        }
    }
}
