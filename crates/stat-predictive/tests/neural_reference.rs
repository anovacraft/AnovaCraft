//! Neural networks against reference fits from scikit-learn (see
//! `tests/data/neural_reference.py`): at scikit-learn's fitted weights the
//! objective, its gradient (scikit-learn's back-propagation) and the
//! predictions agree, so both programs minimize the same function.

use stat_predictive::{Activation, Layer, Network, NetworkTask, Output};

struct Case {
    activation: Activation,
    alpha: f64,
    x: Vec<Vec<f64>>,
    y: Vec<f64>,
    layers: Vec<Layer>,
    gradient: Vec<f64>,
    loss: f64,
    predictions: Vec<Vec<f64>>,
    classification: bool,
}

fn read(name: &str) -> Case {
    let path = format!("{}/tests/data/neural_{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let mut case = Case {
        activation: Activation::Tanh,
        alpha: 0.0,
        x: Vec::new(),
        y: Vec::new(),
        layers: Vec::new(),
        gradient: Vec::new(),
        loss: 0.0,
        predictions: Vec::new(),
        classification: false,
    };
    let nums = |rest: &[&str]| rest.iter().map(|v| v.parse::<f64>().unwrap()).collect::<Vec<_>>();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts[0] {
            "kind" => case.classification = parts[1] == "classification",
            "activation" => case.activation = Activation::from_key(parts[1]).unwrap(),
            "alpha" => case.alpha = parts[1].parse().unwrap(),
            "x" => {
                let mut v = nums(&parts[1..]);
                case.y.push(v.pop().unwrap());
                case.x.push(v);
            }
            "layer" => {
                // sklearn's coefs_ shape is (inputs, outputs).
                let (inputs, outputs) = (parts[1].parse().unwrap(), parts[2].parse().unwrap());
                case.layers.push(Layer { inputs, outputs, weights: Vec::new(), bias: Vec::new() });
            }
            "w" => case.layers.last_mut().unwrap().weights = nums(&parts[1..]),
            "b" => case.layers.last_mut().unwrap().bias = nums(&parts[1..]),
            // Each layer's weight gradient, then its bias gradient: the
            // same order as Network::evaluate.
            "gw" | "gb" => case.gradient.extend(nums(&parts[1..])),
            "loss" => case.loss = parts[1].parse().unwrap(),
            "p" => case.predictions.push(nums(&parts[1..])),
            _ => {}
        }
    }
    case
}

fn check(name: &str) {
    let case = read(name);
    let rows: Vec<&[f64]> = case.x.iter().map(Vec::as_slice).collect();
    let (task, output) = if case.classification {
        let class: Vec<usize> = case.y.iter().map(|&v| v as usize).collect();
        let classes = class.iter().max().unwrap() + 1;
        let output = if classes == 2 { Output::Logistic } else { Output::Softmax(classes) };
        (NetworkTask::Classification { class, classes }, output)
    } else {
        (NetworkTask::Regression { target: case.y.clone() }, Output::Linear)
    };
    let net = Network::from_layers(case.layers, case.activation, output).unwrap();
    let (objective, gradient) = net.evaluate(&rows, &task, case.alpha).unwrap();
    assert!((objective - case.loss).abs() < 1e-10 * case.loss, "{name}: objective {objective} vs {}", case.loss);
    assert_eq!(gradient.len(), case.gradient.len());
    for (k, (a, b)) in gradient.iter().zip(&case.gradient).enumerate() {
        assert!((a - b).abs() < 1e-10 * (1e-3 + b.abs()), "{name}: gradient {k}: {a} vs {b}");
    }
    for (r, expected) in rows.iter().zip(&case.predictions) {
        let p = net.predict(r);
        // sklearn returns both class probabilities for two classes.
        assert_eq!(p.len(), expected.len());
        for (a, b) in p.iter().zip(expected) {
            assert!((a - b).abs() < 1e-12 * (1.0 + b.abs()), "{name}: prediction {a} vs {b}");
        }
    }
}

#[test]
fn regression_tanh_one_hidden_layer() {
    check("regression");
}

#[test]
fn two_classes_logistic_two_hidden_layers() {
    check("binary");
}

#[test]
fn three_classes_relu_softmax() {
    check("multiclass");
}

/// Fitted from random starts, L-BFGS reaches an objective at least as small
/// as scikit-learn's on each case (both find local minima, so a start can do
/// better; the bound allows 2% above).
#[test]
fn fits_reach_reference_objective() {
    use stat_predictive::{NetworkOptions, fit_network};
    for (name, hidden) in [("regression", vec![4]), ("binary", vec![3, 2]), ("multiclass", vec![5])] {
        let case = read(name);
        let rows: Vec<&[f64]> = case.x.iter().map(Vec::as_slice).collect();
        let task = if case.classification {
            let class: Vec<usize> = case.y.iter().map(|&v| v as usize).collect();
            let classes = class.iter().max().unwrap() + 1;
            NetworkTask::Classification { class, classes }
        } else {
            NetworkTask::Regression { target: case.y.clone() }
        };
        let options = NetworkOptions {
            hidden,
            activation: case.activation,
            penalty: case.alpha,
            starts: 10,
            max_iter: 5000,
            ..NetworkOptions::default()
        };
        let net = fit_network(&rows, &task, &options).unwrap();
        assert!(net.objective <= case.loss * 1.02, "{name}: {} vs reference {}", net.objective, case.loss);
    }
}
