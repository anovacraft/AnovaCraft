//! MARS against reference fits from the R package earth (see
//! `tests/data/mars_reference.R`).

// Reference values are pasted with every digit R printed.
#![allow(clippy::excessive_precision)]

use stat_predictive::Feature;
use stat_predictive::mars::{Factor, MarsFit, MarsOptions, fit_mars};

fn read_csv(name: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines();
    let header = lines.next().unwrap().split(',').map(|s| s.trim_matches('"').to_owned()).collect();
    let rows = lines.map(|l| l.split(',').map(|s| s.trim_matches('"').to_owned()).collect()).collect();
    (header, rows)
}

/// Earth-style names of the selected terms (`h(x-t)`, `h(t-x)`, products with `*`).
fn describe(fit: &MarsFit, names: &[&str]) -> Vec<String> {
    fit.selected
        .iter()
        .map(|&t| {
            let parts: Vec<String> = fit.basis[t]
                .factors
                .iter()
                .map(|f| match *f {
                    Factor::Hinge { feature, knot, direction } => match direction {
                        stat_predictive::mars::Direction::Right => format!("h({}-{})", names[feature], fmt(knot)),
                        stat_predictive::mars::Direction::Left => format!("h({}-{})", fmt(knot), names[feature]),
                    },
                    Factor::Linear { feature } => names[feature].to_owned(),
                    Factor::Level { feature, level } => format!("{}{}", names[feature], ["a", "b", "c"][level]),
                })
                .collect();
            if parts.is_empty() { "(Intercept)".to_owned() } else { parts.join("*") }
        })
        .collect()
}

fn friedman1() -> (Vec<Feature>, Vec<f64>) {
    let (_, rows) = read_csv("mars_friedman1.csv");
    let col = |j: usize| rows.iter().map(|r| r[j].parse::<f64>().unwrap()).collect::<Vec<_>>();
    ((0..10).map(|j| Feature::Numeric(col(j))).collect(), col(10))
}

fn fmt(t: f64) -> String {
    let s = format!("{t:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn categorical() -> (Vec<Feature>, Vec<f64>) {
    let (_, rows) = read_csv("mars_categorical.csv");
    let num = |j: usize| rows.iter().map(|r| r[j].parse::<f64>().unwrap()).collect::<Vec<_>>();
    let g = Feature::Categorical {
        codes: rows.iter().map(|r| ["a", "b", "c"].iter().position(|l| *l == r[3])).collect(),
        levels: 3,
    };
    (vec![Feature::Numeric(num(0)), Feature::Numeric(num(1)), Feature::Numeric(num(2)), g], num(4))
}

const X: [&str; 10] = ["x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10"];

fn fit(features: &[Feature], y: &[f64], options: MarsOptions) -> MarsFit {
    let rows: Vec<usize> = (0..y.len()).collect();
    fit_mars(features, &vec![false; features.len()], y, &rows, &options).unwrap()
}

fn spans(degree: usize) -> MarsOptions {
    MarsOptions { max_degree: degree, max_terms: 21, min_span: 1, end_span: 5, ..MarsOptions::default() }
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

fn first_predictions(fit: &MarsFit, features: &[Feature]) -> Vec<f64> {
    (0..5).map(|r| fit.predict(&|j| features[j].value(r))).collect()
}

/// Additive model: the forward pass, the pruning and the coefficients agree
/// with earth (`degree=1, minspan=1, endspan=5, nk=21`) to rounding.
#[test]
fn friedman1_additive_matches_earth() {
    let (features, y) = friedman1();
    let f = fit(&features, &y, spans(1));
    assert_eq!(f.basis.len(), 19, "forward terms (two redundant partners dropped)");
    assert!(close(f.rss(), 407.103498811684, 1e-10), "{}", f.rss());
    assert!(close(f.gcv(), 2.65863509428038, 1e-10), "{}", f.gcv());
    assert!(close(f.r_squared(), 0.908990479836542, 1e-10));
    assert!(close(f.generalized_r_squared(), 0.882316146677777, 1e-10));
    let earth = [
        ("(Intercept)", 47.45617103525069),
        ("h(x4-0.307224)", 123.98328997076729),
        ("h(0.307224-x4)", -124.81835623366364),
        ("h(x1-0.482817)", 2.89031111871112),
        ("h(0.482817-x1)", -13.36376406131567),
        ("h(0.431559-x2)", -16.03082493755940),
        ("h(x3-0.502713)", 9.59212986226649),
        ("h(0.502713-x3)", 9.11854969464287),
        ("h(0.932098-x5)", -3.87075992582062),
        ("h(x7-0.628618)", -4.62505718394534),
        ("h(0.628618-x7)", -2.09920235583565),
        ("h(0.259406-x6)", -3.88869662162551),
        ("h(x4-0.028049)", -114.42420722856532),
    ];
    let names = describe(&f, &X);
    assert_eq!(names.len(), earth.len());
    for (name, b) in earth {
        let k = names.iter().position(|n| n == name).unwrap_or_else(|| panic!("{name} not in {names:?}"));
        assert!(close(f.coefficients[k], b, 1e-9), "{name}: {} vs {b}", f.coefficients[k]);
    }
    let p = first_predictions(&f, &features);
    for (a, b) in
        p.iter().zip([14.3458659924890, 12.6341884213402, 16.9154706893519, 15.6439968819228, 12.4067203658191])
    {
        assert!(close(*a, b, 1e-11), "{a} vs {b}");
    }
}

/// The backward pass and the importance against nested backward
/// elimination written with `lm` on earth's forward basis (earth's own
/// pruning uses leaps, whose best subsets of each size need not be nested).
#[test]
fn friedman1_backward_and_importance() {
    let (features, y) = friedman1();
    let f = fit(&features, &y, spans(1));
    let rss = [
        4473.196848862678,
        3374.217076281353,
        2176.674587262162,
        1335.800584040283,
        1218.149618256170,
        915.061945648954,
        633.330969031238,
        494.103209376984,
        480.593481188615,
        456.546280209383,
        431.109147688555,
        417.808104568176,
        407.103498811684,
        398.708535067512,
        393.213452834853,
        386.028854782335,
        383.387072884521,
        382.538089765687,
        382.069675520625,
    ];
    assert_eq!(f.subsets.len(), rss.len());
    for (s, r) in f.subsets.iter().zip(rss) {
        assert!(close(s.rss, r, 1e-10), "{} vs {r}", s.rss);
    }
    assert_eq!(f.selected.len(), 13);
    let imp = f.importance(10);
    let subsets = [11, 10, 9, 12, 7, 2, 5, 0, 0, 0];
    let gcv = [
        14.7302029395847125,
        8.7900149373417680,
        4.5136411227470941,
        19.9326976946021155,
        2.4647486052182548,
        0.0323492550754767,
        0.2287466990109750,
        0.0,
        0.0,
        0.0,
    ];
    for j in 0..10 {
        assert_eq!(imp[j].subsets, subsets[j], "x{}", j + 1);
        assert!((imp[j].gcv - gcv[j]).abs() < 1e-9, "x{}: {} vs {}", j + 1, imp[j].gcv, gcv[j]);
    }
}

/// With Friedman's default spans earth's grid of candidate knots differs
/// slightly (its spacing is anchored differently), so knots move a little
/// and the fit agrees to within a fraction of a percent; the spans
/// themselves match earth's (minspan 6, endspan 10 for 10 predictors and
/// 200 rows).
#[test]
fn friedman1_default_spans_close_to_earth() {
    let (features, y) = friedman1();
    let f = fit(&features, &y, MarsOptions { max_terms: 21, ..MarsOptions::default() });
    assert_eq!((f.min_span, f.end_span), (6, 10));
    assert!(close(f.rss(), 417.981676845779, 0.005), "{}", f.rss());
    assert!(close(f.gcv(), 2.66833717543349, 0.005), "{}", f.gcv());
    assert_eq!(f.selected.len(), 12);
    let p = first_predictions(&f, &features);
    for (a, b) in
        p.iter().zip([14.3392414924358, 12.6309940407653, 16.9927546453641, 15.8238639588174, 12.3736568512910])
    {
        assert!((a - b).abs() < 0.2, "{a} vs {b}");
    }
}

/// Interactions (degree 2, penalty 3): the same terms on the same
/// predictors and parents, with three knots of interaction terms a little
/// apart. Friedman (1991, Algorithm 2) places knots on a parent at values of
/// the cases where the parent is nonzero; earth also tries values of other
/// cases (e.g. its h(x1-0.482817)*h(x2-0.49851), where 0.49851 belongs to a
/// case with x1 < 0.482817), and skips some knots that would reduce the RSS
/// more. RSS, GCV and predictions agree to 0.2%.
#[test]
fn friedman1_interactions_close_to_earth() {
    let (features, y) = friedman1();
    for (penalty, gcv) in [(None, 1.31988189721713), (Some(2.0), 1.18739678226323)] {
        let f = fit(&features, &y, MarsOptions { penalty, ..spans(2) });
        assert_eq!(f.basis.len(), 21);
        assert_eq!(f.selected.len(), 18);
        assert!(close(f.rss(), 161.634386985582, 0.002), "{}", f.rss());
        assert!(close(f.gcv(), gcv, 0.002), "{}", f.gcv());
        let names = describe(&f, &X);
        for exact in [
            "h(x4-0.307224)",
            "h(x1-0.482817)",
            "h(0.431559-x2)",
            "h(0.932098-x5)",
            "h(x7-0.450091)",
            "h(x1-0.482817)*h(0.075253-x3)",
        ] {
            assert!(names.iter().any(|n| n == exact), "{exact} not in {names:?}");
        }
        let shape = |n: &str| n.replace(|c: char| c.is_ascii_digit() || c == '.', "");
        let mut ours: Vec<String> = names.iter().map(|n| shape(n)).collect();
        ours.sort();
        let mut theirs: Vec<String> = [
            "(Intercept)",
            "h(x-)",
            "h(-x)",
            "h(x-)",
            "h(-x)",
            "h(x-)",
            "h(-x)",
            "h(x-)",
            "h(-x)",
            "h(-x)",
            "h(x-)*h(x-)",
            "h(x-)*h(-x)",
            "h(-x)*h(x-)",
            "h(-x)*h(-x)",
            "h(x-)",
            "h(-x)",
            "h(-x)*h(-x)",
            "h(x-)*h(-x)",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        theirs.sort();
        assert_eq!(ours, theirs);
        let p = first_predictions(&f, &features);
        for (a, b) in
            p.iter().zip([13.6134786592234, 12.3625462829251, 16.9005343824308, 16.0296077453928, 12.3818300781813])
        {
            assert!(close(*a, b, 0.002), "{a} vs {b}");
        }
    }
}

/// A categorical predictor (levels a, b, c) enters through indicators of b
/// and c, as earth's treatment-contrast dummies gb and gc: the additive fit
/// matches exactly, the degree-2 fit to 1% (knots of interaction terms, as
/// above).
#[test]
fn categorical_matches_earth() {
    let (features, y) = categorical();
    let names = ["u", "v", "w", "g"];
    let f = fit(&features, &y, spans(1));
    assert!(close(f.rss(), 8.64730153980894, 1e-10), "{}", f.rss());
    assert!(close(f.gcv(), 0.067133959472664, 1e-10), "{}", f.gcv());
    let earth = [
        ("(Intercept)", -0.0537030552937447),
        ("gb", 1.4500526771090727),
        ("h(u-0.454453)", 3.9495391746018553),
        ("h(0.561681-v)", -2.2163615637406946),
        ("gc", -0.7283406561680273),
        ("h(u-0.931951)", 6.8215742993110187),
    ];
    let ours = describe(&f, &names);
    assert_eq!(ours.len(), earth.len());
    for (name, b) in earth {
        let k = ours.iter().position(|n| n == name).unwrap_or_else(|| panic!("{name} not in {ours:?}"));
        assert!(close(f.coefficients[k], b, 1e-9), "{name}: {} vs {b}", f.coefficients[k]);
    }
    let imp = f.importance(4);
    assert_eq!(imp.iter().map(|i| i.subsets).collect::<Vec<_>>(), [4, 3, 0, 5], "u, v, w, g (gb and gc)");

    let f2 = fit(&features, &y, spans(2));
    assert!(close(f2.rss(), 5.47695123742676, 0.01), "{}", f2.rss());
    assert!(close(f2.gcv(), 0.0457531012259977, 0.01), "{}", f2.gcv());
    assert_eq!(f2.selected.len(), 7);
    let p = first_predictions(&f2, &features);
    for (a, b) in p.iter().zip([
        2.32818500394376304,
        0.00443044157514899,
        -0.65060567203273389,
        -1.86930377303772266,
        0.89757150516735940,
    ]) {
        assert!((a - b).abs() < 0.03, "{a} vs {b}");
    }
}
