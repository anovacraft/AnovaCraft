//! Neural networks: fully connected feed-forward networks (multilayer
//! perceptrons) with one or two hidden layers, fitted by penalized maximum
//! likelihood.
//!
//! **Model.** Each hidden layer computes `a = h(W a₋ + b)` from the layer
//! before it, with the activation `h` one of tanh, the logistic function
//! `1 / (1 + e⁻ᶻ)`, the rectifier `max(0, z)` or the identity. The output
//! layer is linear for a continuous response, one logistic unit for two
//! classes (the probability of the second class) and a softmax over `K`
//! units for `K > 2` classes (Bishop 1995, *Neural Networks for Pattern
//! Recognition*, ch. 4 and 6; Ripley 1996, *Pattern Recognition and Neural
//! Networks*, ch. 5).
//!
//! **Objective.** With `n` training rows and weights `W` (biases are not
//! penalized), the network minimizes
//!
//! ```text
//! E = (1/n) Σᵢ L(yᵢ, f(xᵢ)) + (α / 2n) Σ w²,
//! ```
//!
//! where `L` is half the squared error `½(y − ŷ)²` for a continuous response,
//! and the cross-entropy `−Σₖ yₖ ln pₖ` (with probabilities clipped at 10⁻¹⁵)
//! for classes. These output–loss pairs are canonical, so the error signal at
//! the output is `ŷ − y` in every case (Bishop 1995, §6.9); gradients follow
//! by back-propagation (Rumelhart, Hinton & Williams 1986). The penalty `α`
//! is weight decay (Hastie, Tibshirani & Friedman 2009, *The Elements of
//! Statistical Learning*, §11.5.2).
//!
//! **Solvers.** Limited-memory BFGS (Liu & Nocedal 1989; two-loop recursion
//! with ten correction pairs and a backtracking Armijo line search; Nocedal &
//! Wright 2006, *Numerical Optimization*, alg. 7.4–7.5) on the full training
//! set, or Adam (Kingma & Ba 2015) on shuffled mini-batches. L-BFGS stops when
//! the largest gradient component falls below 10⁻⁵ or an iteration lowers
//! the objective by less than 10⁻⁹ relative to its size; Adam stops when ten
//! consecutive passes fail to lower the epoch's mean objective by 10⁻⁴.
//! [`Solver`] `None` picks L-BFGS up to 2,000 rows and Adam above.
//!
//! **Starts.** The objective has many local minima, so the network is fitted
//! from several random starts and the start with the smallest final
//! objective is kept. Starting weights and biases are uniform on
//! `±√(6 / (fan-in + fan-out))` (Glorot & Bengio 2010), with 2 in place of 6
//! for the logistic activation. Each start's generator is seeded from the
//! request's seed and the start's number, and starts run in parallel with
//! their results combined in a fixed order, so a fit repeats exactly.
//!
//! **Failure modes.** No hidden layer or more than two, a layer without
//! nodes, a negative penalty, no rows, inputs of unequal width or with a
//! missing value, a non-finite target, or a class code outside the classes.

use rayon::prelude::*;
use stat_core::rng::Rng;
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Rows per parallel chunk of the gradient.
const CHUNK: usize = 256;
/// Correction pairs kept by L-BFGS.
const MEMORY: usize = 10;
/// L-BFGS stops when the largest gradient component is below this.
const GRADIENT_TOLERANCE: f64 = 1e-5;
/// L-BFGS stops when an iteration's relative decrease is below this.
const OBJECTIVE_TOLERANCE: f64 = 1e-9;
/// Adam's improvement threshold per pass.
const ADAM_TOLERANCE: f64 = 1e-4;
/// Adam passes without improvement before it stops.
const ADAM_PATIENCE: usize = 10;
/// Rows up to which [`Solver`] `None` uses L-BFGS.
const LBFGS_ROWS: usize = 2000;
/// Smallest probability in the cross-entropy.
const PROBABILITY_FLOOR: f64 = 1e-15;

/// The activation of the hidden nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// `tanh z`.
    Tanh,
    /// `1 / (1 + e⁻ᶻ)`.
    Logistic,
    /// `max(0, z)` (rectified linear unit).
    Relu,
    /// `z`.
    Identity,
}

impl Activation {
    /// Every activation, in menu order.
    pub const ALL: [Self; 4] = [Self::Tanh, Self::Logistic, Self::Relu, Self::Identity];

    /// Name for reports and project files.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::Tanh => "tanh",
            Self::Logistic => "logistic",
            Self::Relu => "relu",
            Self::Identity => "identity",
        }
    }

    /// The activation named `key` (see [`key`](Self::key)).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == key)
    }

    /// The formula, for reports.
    #[must_use]
    pub fn formula(&self) -> &'static str {
        match self {
            Self::Tanh => "tanh(z)",
            Self::Logistic => "1 / (1 + exp(−z))",
            Self::Relu => "max(0, z)",
            Self::Identity => "z",
        }
    }

    /// `h(z)`.
    #[must_use]
    pub fn apply(&self, z: f64) -> f64 {
        match self {
            Self::Tanh => z.tanh(),
            Self::Logistic => logistic(z),
            Self::Relu => z.max(0.0),
            Self::Identity => z,
        }
    }

    /// `h′(z)` written in terms of `a = h(z)`.
    fn derivative(&self, a: f64) -> f64 {
        match self {
            Self::Tanh => 1.0 - a * a,
            Self::Logistic => a * (1.0 - a),
            Self::Relu => f64::from(u8::from(a > 0.0)),
            Self::Identity => 1.0,
        }
    }
}

fn logistic(z: f64) -> f64 {
    if z >= 0.0 {
        1.0 / (1.0 + (-z).exp())
    } else {
        let e = z.exp();
        e / (1.0 + e)
    }
}

/// What the network predicts.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkTask {
    /// A continuous target (linear output, squared error).
    Regression {
        /// Target per row.
        target: Vec<f64>,
    },
    /// Classes `0..classes` (logistic output for two, softmax for more;
    /// cross-entropy).
    Classification {
        /// Class code per row.
        class: Vec<usize>,
        /// Number of classes (at least 2).
        classes: usize,
    },
}

impl NetworkTask {
    fn len(&self) -> usize {
        match self {
            Self::Regression { target } => target.len(),
            Self::Classification { class, .. } => class.len(),
        }
    }

    fn output(&self) -> Output {
        match self {
            Self::Regression { .. } => Output::Linear,
            Self::Classification { classes: 2, .. } => Output::Logistic,
            Self::Classification { classes, .. } => Output::Softmax(*classes),
        }
    }

    /// The same task on the rows `rows`.
    #[must_use]
    pub fn subset(&self, rows: &[usize]) -> Self {
        match self {
            Self::Regression { target } => Self::Regression { target: rows.iter().map(|&r| target[r]).collect() },
            Self::Classification { class, classes } => {
                Self::Classification { class: rows.iter().map(|&r| class[r]).collect(), classes: *classes }
            }
        }
    }
}

/// The output layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// One linear unit.
    Linear,
    /// One logistic unit: the probability of class 1 of two.
    Logistic,
    /// A softmax over this many classes.
    Softmax(usize),
}

impl Output {
    fn units(self) -> usize {
        match self {
            Self::Linear | Self::Logistic => 1,
            Self::Softmax(k) => k,
        }
    }
}

/// The optimizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Solver {
    /// Limited-memory BFGS on the full training set.
    Lbfgs,
    /// Adam on mini-batches.
    Adam,
}

impl Solver {
    /// Name for reports and project files.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::Lbfgs => "lbfgs",
            Self::Adam => "adam",
        }
    }

    /// The solver named `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [Self::Lbfgs, Self::Adam].into_iter().find(|s| s.key() == key)
    }

    /// Name for reports.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Lbfgs => "L-BFGS",
            Self::Adam => "Adam",
        }
    }
}

/// Network shape and fitting settings.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkOptions {
    /// Nodes per hidden layer (one or two layers).
    pub hidden: Vec<usize>,
    /// Activation of the hidden nodes.
    pub activation: Activation,
    /// Weight-decay penalty α ≥ 0.
    pub penalty: f64,
    /// Optimizer (`None`: L-BFGS up to 2,000 rows, Adam above).
    pub solver: Option<Solver>,
    /// Random starts (at least 1).
    pub starts: usize,
    /// Most L-BFGS iterations, or Adam passes over the data.
    pub max_iter: usize,
    /// Adam's step size.
    pub learning_rate: f64,
    /// Adam's mini-batch size (capped at the number of rows).
    pub batch_size: usize,
    /// Seed of the starting weights (and Adam's shuffles).
    pub seed: u64,
}

impl Default for NetworkOptions {
    fn default() -> Self {
        Self {
            hidden: vec![3],
            activation: Activation::Tanh,
            penalty: 1e-4,
            solver: None,
            starts: 3,
            max_iter: 2000,
            learning_rate: 1e-3,
            batch_size: 200,
            seed: 1,
        }
    }
}

/// One fully connected layer: `z = W a + b`.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Nodes feeding the layer.
    pub inputs: usize,
    /// Nodes of the layer.
    pub outputs: usize,
    /// `W`, row-major: `weights[o * inputs + i]` joins input `i` to node `o`.
    pub weights: Vec<f64>,
    /// `b`, one per node.
    pub bias: Vec<f64>,
}

impl Layer {
    /// The weight from input `i` to node `o`.
    #[must_use]
    pub fn weight(&self, o: usize, i: usize) -> f64 {
        self.weights[o * self.inputs + i]
    }
}

/// Layer sizes and where each layer's parameters sit in the flat vector
/// (each layer's weights, then its biases).
#[derive(Debug, Clone, PartialEq)]
struct Shape {
    sizes: Vec<usize>,
}

impl Shape {
    fn layers(&self) -> usize {
        self.sizes.len() - 1
    }

    fn parameters(&self) -> usize {
        self.sizes.windows(2).map(|w| w[0] * w[1] + w[1]).sum()
    }

    /// Offsets of layer `l`'s weights and biases.
    fn offsets(&self, l: usize) -> (usize, usize) {
        let start: usize = self.sizes.windows(2).take(l).map(|w| w[0] * w[1] + w[1]).sum();
        (start, start + self.sizes[l] * self.sizes[l + 1])
    }

    fn weight_ranges(&self) -> Vec<std::ops::Range<usize>> {
        (0..self.layers())
            .map(|l| {
                let (w, b) = self.offsets(l);
                w..b
            })
            .collect()
    }
}

/// Activations of every layer for one row, reused across rows.
struct Workspace {
    acts: Vec<Vec<f64>>,
    deltas: Vec<Vec<f64>>,
}

impl Workspace {
    fn new(shape: &Shape) -> Self {
        Self {
            acts: shape.sizes.iter().map(|&s| vec![0.0; s]).collect(),
            deltas: shape.sizes.iter().map(|&s| vec![0.0; s]).collect(),
        }
    }
}

/// The network's function: shape, activation and output layer.
#[derive(Debug, Clone, PartialEq)]
struct Architecture {
    shape: Shape,
    activation: Activation,
    output: Output,
}

impl Architecture {
    /// Forward pass of `x`, leaving every layer's values in `ws.acts`.
    fn forward(&self, params: &[f64], x: &[f64], ws: &mut Workspace) {
        ws.acts[0].copy_from_slice(x);
        let last = self.shape.layers() - 1;
        for l in 0..self.shape.layers() {
            let (wo, bo) = self.shape.offsets(l);
            let (n_in, n_out) = (self.shape.sizes[l], self.shape.sizes[l + 1]);
            let (before, after) = ws.acts.split_at_mut(l + 1);
            let input = &before[l];
            let out = &mut after[0];
            for o in 0..n_out {
                let w = &params[wo + o * n_in..wo + (o + 1) * n_in];
                let z = params[bo + o] + w.iter().zip(input).map(|(a, b)| a * b).sum::<f64>();
                out[o] = if l < last { self.activation.apply(z) } else { z };
            }
        }
        let out = &mut ws.acts[last + 1];
        match self.output {
            Output::Linear => {}
            Output::Logistic => out[0] = logistic(out[0]),
            Output::Softmax(_) => {
                let m = out.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let mut s = 0.0;
                for v in out.iter_mut() {
                    *v = (*v - m).exp();
                    s += *v;
                }
                out.iter_mut().for_each(|v| *v /= s);
            }
        }
    }

    /// Loss of row `r` after [`forward`](Self::forward); sets the output
    /// error signal `ŷ − y` in `ws.deltas`.
    fn loss(&self, task: &NetworkTask, r: usize, ws: &mut Workspace) -> f64 {
        let last = self.shape.layers();
        let out = &ws.acts[last];
        let delta = &mut ws.deltas[last];
        match (task, self.output) {
            (NetworkTask::Regression { target }, _) => {
                let e = out[0] - target[r];
                delta[0] = e;
                0.5 * e * e
            }
            (NetworkTask::Classification { class, .. }, Output::Logistic) => {
                let y = f64::from(u8::from(class[r] == 1));
                let p = out[0];
                delta[0] = p - y;
                let q = if y > 0.5 { p } else { 1.0 - p };
                -q.max(PROBABILITY_FLOOR).ln()
            }
            (NetworkTask::Classification { class, .. }, _) => {
                for (k, d) in delta.iter_mut().enumerate() {
                    *d = out[k] - f64::from(u8::from(class[r] == k));
                }
                -out[class[r]].max(PROBABILITY_FLOOR).ln()
            }
        }
    }

    /// Adds row's gradient (from the error signal in `ws.deltas`) to `grad`.
    fn backward(&self, params: &[f64], ws: &mut Workspace, grad: &mut [f64]) {
        for l in (0..self.shape.layers()).rev() {
            let (wo, bo) = self.shape.offsets(l);
            let (n_in, n_out) = (self.shape.sizes[l], self.shape.sizes[l + 1]);
            for o in 0..n_out {
                let d = ws.deltas[l + 1][o];
                if d == 0.0 {
                    continue;
                }
                grad[bo + o] += d;
                let g = &mut grad[wo + o * n_in..wo + (o + 1) * n_in];
                for (gi, a) in g.iter_mut().zip(&ws.acts[l]) {
                    *gi += d * a;
                }
            }
            if l > 0 {
                for i in 0..n_in {
                    let s: f64 = (0..n_out).map(|o| params[wo + o * n_in + i] * ws.deltas[l + 1][o]).sum();
                    ws.deltas[l][i] = s * self.activation.derivative(ws.acts[l][i]);
                }
            }
        }
    }

    /// Mean loss over `rows` plus `(α / 2m) Σ w²`, and its gradient.
    fn objective(
        &self,
        params: &[f64],
        x: &[&[f64]],
        task: &NetworkTask,
        rows: &[usize],
        penalty: f64,
        penalty_rows: f64,
    ) -> (f64, Vec<f64>) {
        let p = self.shape.parameters();
        let parts: Vec<(f64, Vec<f64>)> = rows
            .par_chunks(CHUNK)
            .map(|chunk| {
                let mut ws = Workspace::new(&self.shape);
                let mut grad = vec![0.0; p];
                let mut loss = 0.0;
                for &r in chunk {
                    self.forward(params, x[r], &mut ws);
                    loss += self.loss(task, r, &mut ws);
                    self.backward(params, &mut ws, &mut grad);
                }
                (loss, grad)
            })
            .collect();
        let m = rows.len().max(1) as f64;
        let mut loss = 0.0;
        let mut grad = vec![0.0; p];
        for (l, g) in parts {
            loss += l;
            grad.iter_mut().zip(&g).for_each(|(a, b)| *a += b);
        }
        loss /= m;
        grad.iter_mut().for_each(|g| *g /= m);
        if penalty > 0.0 {
            let k = penalty / penalty_rows;
            for range in self.shape.weight_ranges() {
                for i in range {
                    loss += 0.5 * k * params[i] * params[i];
                    grad[i] += k * params[i];
                }
            }
        }
        (loss, grad)
    }

    /// Glorot-uniform starting parameters.
    fn initial(&self, rng: &mut Rng) -> Vec<f64> {
        let mut params = vec![0.0; self.shape.parameters()];
        let factor = if self.activation == Activation::Logistic { 2.0 } else { 6.0 };
        for l in 0..self.shape.layers() {
            let (wo, bo) = self.shape.offsets(l);
            let (n_in, n_out) = (self.shape.sizes[l], self.shape.sizes[l + 1]);
            let bound = (factor / (n_in + n_out) as f64).sqrt();
            for v in &mut params[wo..bo + n_out] {
                *v = bound * (2.0 * rng.next_f64() - 1.0);
            }
        }
        params
    }
}

/// The outcome of one optimizer run.
struct Run {
    params: Vec<f64>,
    objective: f64,
    iterations: usize,
    converged: bool,
}

/// An objective returning its value and gradient.
type Objective<'a> = dyn Fn(&[f64]) -> (f64, Vec<f64>) + 'a;

/// L-BFGS from `x0` on `f` (returning the value and gradient).
fn lbfgs(f: &Objective<'_>, x0: Vec<f64>, max_iter: usize) -> Run {
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let mut x = x0;
    let (mut fx, mut g) = f(&x);
    let mut s_list: Vec<Vec<f64>> = Vec::new();
    let mut y_list: Vec<Vec<f64>> = Vec::new();
    let mut iterations = 0;
    let mut converged = false;
    while iterations < max_iter {
        if !fx.is_finite() {
            break;
        }
        let gmax = g.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        if gmax < GRADIENT_TOLERANCE {
            converged = true;
            break;
        }
        // Two-loop recursion: d = −H g.
        let mut q = g.clone();
        let k = s_list.len();
        let mut alpha = vec![0.0; k];
        for j in (0..k).rev() {
            let rho = 1.0 / dot(&y_list[j], &s_list[j]);
            alpha[j] = rho * dot(&s_list[j], &q);
            q.iter_mut().zip(&y_list[j]).for_each(|(qi, yi)| *qi -= alpha[j] * yi);
        }
        let scale = if k > 0 {
            dot(&s_list[k - 1], &y_list[k - 1]) / dot(&y_list[k - 1], &y_list[k - 1])
        } else {
            1.0 / dot(&g, &g).sqrt().max(1.0)
        };
        q.iter_mut().for_each(|v| *v *= scale);
        for j in 0..k {
            let rho = 1.0 / dot(&y_list[j], &s_list[j]);
            let beta = rho * dot(&y_list[j], &q);
            q.iter_mut().zip(&s_list[j]).for_each(|(qi, si)| *qi += (alpha[j] - beta) * si);
        }
        let mut d: Vec<f64> = q.iter().map(|v| -v).collect();
        let mut slope = dot(&g, &d);
        if slope >= 0.0 || !slope.is_finite() {
            s_list.clear();
            y_list.clear();
            let n = dot(&g, &g).sqrt().max(1.0);
            d = g.iter().map(|v| -v / n).collect();
            slope = dot(&g, &d);
        }
        // Backtracking Armijo line search.
        let mut t = 1.0;
        let mut accepted = None;
        for _ in 0..40 {
            let xn: Vec<f64> = x.iter().zip(&d).map(|(a, b)| a + t * b).collect();
            let (fnew, gnew) = f(&xn);
            if fnew.is_finite() && fnew <= fx + 1e-4 * t * slope {
                accepted = Some((xn, fnew, gnew));
                break;
            }
            t *= 0.5;
        }
        iterations += 1;
        let Some((xn, fnew, gnew)) = accepted else {
            // No decrease along the direction: a minimum to working precision.
            converged = true;
            break;
        };
        let s: Vec<f64> = xn.iter().zip(&x).map(|(a, b)| a - b).collect();
        let y: Vec<f64> = gnew.iter().zip(&g).map(|(a, b)| a - b).collect();
        let sy = dot(&s, &y);
        if sy > 1e-12 * dot(&s, &s).sqrt() * dot(&y, &y).sqrt() {
            if s_list.len() == MEMORY {
                s_list.remove(0);
                y_list.remove(0);
            }
            s_list.push(s);
            y_list.push(y);
        }
        let decrease = fx - fnew;
        x = xn;
        g = gnew;
        let previous = fx;
        fx = fnew;
        if decrease <= OBJECTIVE_TOLERANCE * previous.abs().max(fx.abs()).max(1.0) {
            converged = true;
            break;
        }
    }
    Run { params: x, objective: fx, iterations, converged }
}

/// Adam on shuffled mini-batches of `rows`.
fn adam(
    arch: &Architecture,
    x: &[&[f64]],
    task: &NetworkTask,
    penalty: f64,
    options: &NetworkOptions,
    params: Vec<f64>,
    rng: &mut Rng,
) -> Run {
    const BETA1: f64 = 0.9;
    const BETA2: f64 = 0.999;
    const EPS: f64 = 1e-8;
    let n = task.len();
    let mut params = params;
    let mut m = vec![0.0; params.len()];
    let mut v = vec![0.0; params.len()];
    let mut order: Vec<usize> = (0..n).collect();
    let batch = options.batch_size.clamp(1, n);
    let mut step = 0i32;
    let mut best = f64::INFINITY;
    let mut stale = 0;
    let mut iterations = 0;
    let mut converged = false;
    while iterations < options.max_iter {
        rng.shuffle(&mut order);
        let mut total = 0.0;
        for chunk in order.chunks(batch) {
            let (loss, grad) = arch.objective(&params, x, task, chunk, penalty, n as f64);
            total += loss * chunk.len() as f64;
            step += 1;
            let c1 = 1.0 - BETA1.powi(step);
            let c2 = 1.0 - BETA2.powi(step);
            for k in 0..params.len() {
                m[k] = BETA1 * m[k] + (1.0 - BETA1) * grad[k];
                v[k] = BETA2 * v[k] + (1.0 - BETA2) * grad[k] * grad[k];
                params[k] -= options.learning_rate * (m[k] / c1) / ((v[k] / c2).sqrt() + EPS);
            }
        }
        iterations += 1;
        let epoch = total / n as f64;
        if !epoch.is_finite() {
            break;
        }
        if epoch > best - ADAM_TOLERANCE {
            stale += 1;
        } else {
            stale = 0;
        }
        best = best.min(epoch);
        if stale > ADAM_PATIENCE {
            converged = true;
            break;
        }
    }
    let rows: Vec<usize> = (0..n).collect();
    let (objective, _) = arch.objective(&params, x, task, &rows, penalty, n as f64);
    Run { params, objective, iterations, converged }
}

/// A fitted network.
#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    arch: Architecture,
    /// The layers, input side first (the last is the output layer).
    pub layers: Vec<Layer>,
    /// The penalty α it was fitted with.
    pub penalty: f64,
    /// The objective `E` at the kept start.
    pub objective: f64,
    /// The mean loss (without the penalty) on the training rows.
    pub loss: f64,
    /// The optimizer used.
    pub solver: Solver,
    /// The final objective of every start, in start order.
    pub start_objectives: Vec<f64>,
    /// The kept start (index into [`start_objectives`](Self::start_objectives)).
    pub best_start: usize,
    /// Iterations (L-BFGS) or passes (Adam) of the kept start.
    pub iterations: usize,
    /// Whether the kept start met its stopping rule before the iteration limit.
    pub converged: bool,
}

impl Network {
    /// A network with the given layers (input side first), for evaluating
    /// weights fitted elsewhere.
    ///
    /// # Errors
    /// No layers, more than three, or layers whose sizes do not chain or
    /// whose output layer does not match `task`'s outputs.
    pub fn from_layers(layers: Vec<Layer>, activation: Activation, output: Output) -> Result<Self> {
        if layers.is_empty() || layers.len() > 3 {
            return Err(invalid("a network has one to three weight layers"));
        }
        let mut sizes = vec![layers[0].inputs];
        for l in &layers {
            if l.inputs != *sizes.last().unwrap_or(&0)
                || l.weights.len() != l.inputs * l.outputs
                || l.bias.len() != l.outputs
            {
                return Err(invalid("layer sizes do not chain"));
            }
            sizes.push(l.outputs);
        }
        if sizes.last() != Some(&output.units()) {
            return Err(invalid("the output layer does not match the output"));
        }
        Ok(Self {
            arch: Architecture { shape: Shape { sizes }, activation, output },
            layers,
            penalty: 0.0,
            objective: f64::NAN,
            loss: f64::NAN,
            solver: Solver::Lbfgs,
            start_objectives: Vec::new(),
            best_start: 0,
            iterations: 0,
            converged: true,
        })
    }

    fn params(&self) -> Vec<f64> {
        let mut p = Vec::with_capacity(self.arch.shape.parameters());
        for l in &self.layers {
            p.extend(&l.weights);
            p.extend(&l.bias);
        }
        p
    }

    /// The activation of the hidden nodes.
    #[must_use]
    pub fn activation(&self) -> Activation {
        self.arch.activation
    }

    /// The output layer.
    #[must_use]
    pub fn output(&self) -> Output {
        self.arch.output
    }

    /// Nodes per layer, inputs first and outputs last.
    #[must_use]
    pub fn sizes(&self) -> &[usize] {
        &self.arch.shape.sizes
    }

    /// Weights and biases in the network.
    #[must_use]
    pub fn parameter_count(&self) -> usize {
        self.arch.shape.parameters()
    }

    /// The prediction for `x`: `[ŷ]` for a continuous response, otherwise
    /// one probability per class.
    #[must_use]
    pub fn predict(&self, x: &[f64]) -> Vec<f64> {
        let mut ws = Workspace::new(&self.arch.shape);
        self.arch.forward(&self.params(), x, &mut ws);
        let out = &ws.acts[self.arch.shape.layers()];
        match self.arch.output {
            Output::Linear => vec![out[0]],
            Output::Logistic => vec![1.0 - out[0], out[0]],
            Output::Softmax(_) => out.clone(),
        }
    }

    /// The objective `E` (mean loss over every row of `task` plus the
    /// penalty `penalty`) and its gradient with respect to the parameters
    /// (each layer's weights row by row, then its biases).
    ///
    /// # Errors
    /// Inputs that do not match the network or the task.
    pub fn evaluate(&self, x: &[&[f64]], task: &NetworkTask, penalty: f64) -> Result<(f64, Vec<f64>)> {
        check_data(x, task, self.arch.shape.sizes[0])?;
        if task.output() != self.arch.output {
            return Err(invalid("the task does not match the network's output"));
        }
        let rows: Vec<usize> = (0..task.len()).collect();
        Ok(self.arch.objective(&self.params(), x, task, &rows, penalty, rows.len() as f64))
    }
}

fn check_data(x: &[&[f64]], task: &NetworkTask, width: usize) -> Result<()> {
    if x.is_empty() {
        return Err(invalid("a neural network needs at least one row"));
    }
    if x.len() != task.len() {
        return Err(invalid("inputs and targets differ in length"));
    }
    if x.iter().any(|r| r.len() != width || r.iter().any(|v| !v.is_finite())) {
        return Err(invalid("every row needs the same number of finite inputs"));
    }
    match task {
        NetworkTask::Regression { target } => {
            if target.iter().any(|t| !t.is_finite()) {
                return Err(invalid("targets must be finite"));
            }
        }
        NetworkTask::Classification { class, classes } => {
            if *classes < 2 {
                return Err(invalid("classification needs at least two classes"));
            }
            if class.iter().any(|&c| c >= *classes) {
                return Err(invalid("a class code is outside the classes"));
            }
        }
    }
    Ok(())
}

/// Fit a network to rows `x` (one input vector per row) and `task`.
///
/// # Errors
/// See the module's failure modes.
pub fn fit_network(x: &[&[f64]], task: &NetworkTask, options: &NetworkOptions) -> Result<Network> {
    if options.hidden.is_empty() || options.hidden.len() > 2 {
        return Err(invalid("a neural network has one or two hidden layers"));
    }
    if options.hidden.iter().any(|&h| h == 0 || h > 1000) {
        return Err(invalid("each hidden layer needs 1 to 1,000 nodes"));
    }
    if !(options.penalty >= 0.0 && options.penalty.is_finite()) {
        return Err(invalid("the penalty must not be negative"));
    }
    if options.starts == 0 || options.max_iter == 0 {
        return Err(invalid("at least one start and one iteration are needed"));
    }
    if !(options.learning_rate > 0.0 && options.learning_rate.is_finite()) {
        return Err(invalid("the learning rate must be positive"));
    }
    let width = x.first().map_or(0, |r| r.len());
    if width == 0 {
        return Err(invalid("a neural network needs at least one input"));
    }
    check_data(x, task, width)?;
    let output = task.output();
    let mut sizes = vec![width];
    sizes.extend(&options.hidden);
    sizes.push(output.units());
    let arch = Architecture { shape: Shape { sizes }, activation: options.activation, output };
    let n = task.len();
    let solver = options.solver.unwrap_or(if n <= LBFGS_ROWS { Solver::Lbfgs } else { Solver::Adam });
    let rows: Vec<usize> = (0..n).collect();
    let runs: Vec<Run> = (0..options.starts)
        .into_par_iter()
        .map(|s| {
            let mut rng = Rng::new(options.seed ^ (s as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03));
            let start = arch.initial(&mut rng);
            match solver {
                Solver::Lbfgs => {
                    let f = |p: &[f64]| arch.objective(p, x, task, &rows, options.penalty, n as f64);
                    lbfgs(&f, start, options.max_iter)
                }
                Solver::Adam => adam(&arch, x, task, options.penalty, options, start, &mut rng),
            }
        })
        .collect();
    let start_objectives: Vec<f64> = runs.iter().map(|r| r.objective).collect();
    let best_start = (0..runs.len())
        .min_by(|&a, &b| {
            let fa = if runs[a].objective.is_finite() { runs[a].objective } else { f64::INFINITY };
            let fb = if runs[b].objective.is_finite() { runs[b].objective } else { f64::INFINITY };
            fa.total_cmp(&fb)
        })
        .unwrap_or(0);
    let best = &runs[best_start];
    if !best.objective.is_finite() {
        return Err(invalid("the network's fit diverged; try a larger penalty or a smaller learning rate"));
    }
    let layers = (0..arch.shape.layers())
        .map(|l| {
            let (wo, bo) = arch.shape.offsets(l);
            let (n_in, n_out) = (arch.shape.sizes[l], arch.shape.sizes[l + 1]);
            Layer {
                inputs: n_in,
                outputs: n_out,
                weights: best.params[wo..bo].to_vec(),
                bias: best.params[bo..bo + n_out].to_vec(),
            }
        })
        .collect();
    let (loss, _) = arch.objective(&best.params, x, task, &rows, 0.0, n as f64);
    Ok(Network {
        arch,
        layers,
        penalty: options.penalty,
        objective: best.objective,
        loss,
        solver,
        start_objectives,
        best_start,
        iterations: best.iterations,
        converged: best.converged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(n: usize, seed: u64) -> (Vec<Vec<f64>>, Vec<f64>, Vec<usize>) {
        let mut rng = Rng::new(seed);
        let mut x = Vec::new();
        let mut y = Vec::new();
        let mut c = Vec::new();
        for _ in 0..n {
            let a = 4.0 * rng.next_f64() - 2.0;
            let b = 4.0 * rng.next_f64() - 2.0;
            x.push(vec![a, b]);
            y.push((1.5 * a).sin() + 0.5 * b * b + 0.05 * rng.normal());
            c.push(if a * a + b * b < 1.5 {
                0
            } else if a > 0.0 {
                1
            } else {
                2
            });
        }
        (x, y, c)
    }

    #[test]
    fn gradient_matches_finite_differences() {
        let (x, y, c) = data(30, 3);
        let rows: Vec<&[f64]> = x.iter().map(Vec::as_slice).collect();
        let tasks = [
            NetworkTask::Regression { target: y },
            NetworkTask::Classification { class: c.iter().map(|&k| usize::from(k > 0)).collect(), classes: 2 },
            NetworkTask::Classification { class: c, classes: 3 },
        ];
        for task in &tasks {
            for activation in Activation::ALL {
                let output = task.output();
                let arch = Architecture { shape: Shape { sizes: vec![2, 4, 3, output.units()] }, activation, output };
                let mut rng = Rng::new(9);
                let p = arch.initial(&mut rng);
                let all: Vec<usize> = (0..30).collect();
                let (_, g) = arch.objective(&p, &rows, task, &all, 0.7, 30.0);
                for k in 0..p.len() {
                    let h = 1e-6;
                    let mut up = p.clone();
                    up[k] += h;
                    let mut down = p.clone();
                    down[k] -= h;
                    let fd = (arch.objective(&up, &rows, task, &all, 0.7, 30.0).0
                        - arch.objective(&down, &rows, task, &all, 0.7, 30.0).0)
                        / (2.0 * h);
                    // ReLU kinks make finite differences unreliable only exactly at 0.
                    assert!((fd - g[k]).abs() < 1e-6 * (1.0 + fd.abs()), "{activation:?} {k}: {fd} vs {}", g[k]);
                }
            }
        }
    }

    #[test]
    fn regression_fits_and_repeats() {
        let (x, y, _) = data(300, 5);
        let rows: Vec<&[f64]> = x.iter().map(Vec::as_slice).collect();
        let task = NetworkTask::Regression { target: y.clone() };
        let options = NetworkOptions { hidden: vec![6], ..NetworkOptions::default() };
        let a = fit_network(&rows, &task, &options).unwrap();
        assert_eq!(a.solver, Solver::Lbfgs);
        assert_eq!(a.start_objectives.len(), 3);
        assert_eq!(a.objective, a.start_objectives.iter().copied().fold(f64::INFINITY, f64::min));
        let sse: f64 = rows.iter().zip(&y).map(|(r, t)| (a.predict(r)[0] - t).powi(2)).sum();
        let mean = y.iter().sum::<f64>() / y.len() as f64;
        let sst: f64 = y.iter().map(|t| (t - mean).powi(2)).sum();
        assert!(1.0 - sse / sst > 0.98, "R² {}", 1.0 - sse / sst);
        let b = fit_network(&rows, &task, &options).unwrap();
        assert_eq!(a, b, "same seed, same fit");
        // Adam reaches a comparable fit.
        let adam = NetworkOptions { solver: Some(Solver::Adam), learning_rate: 0.01, starts: 1, ..options };
        let c = fit_network(&rows, &task, &adam).unwrap();
        assert_eq!(c.solver, Solver::Adam);
        let sse: f64 = rows.iter().zip(&y).map(|(r, t)| (c.predict(r)[0] - t).powi(2)).sum();
        assert!(1.0 - sse / sst > 0.9, "Adam R² {}", 1.0 - sse / sst);
    }

    #[test]
    fn classification_probabilities() {
        let (x, _, c) = data(300, 8);
        let rows: Vec<&[f64]> = x.iter().map(Vec::as_slice).collect();
        let task = NetworkTask::Classification { class: c.clone(), classes: 3 };
        let net =
            fit_network(&rows, &task, &NetworkOptions { hidden: vec![5, 3], ..NetworkOptions::default() }).unwrap();
        assert_eq!(net.sizes(), &[2, 5, 3, 3]);
        let mut wrong = 0;
        for (r, &k) in rows.iter().zip(&c) {
            let p = net.predict(r);
            assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            let best = (0..3).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap();
            wrong += usize::from(best != k);
        }
        assert!(wrong < 20, "{wrong} misclassified");
        // A larger penalty shrinks the weights.
        let heavy = fit_network(&rows, &task, &NetworkOptions { penalty: 50.0, ..NetworkOptions::default() }).unwrap();
        let light = fit_network(&rows, &task, &NetworkOptions { penalty: 0.0, ..NetworkOptions::default() }).unwrap();
        let norm = |n: &Network| n.layers.iter().flat_map(|l| &l.weights).map(|w| w * w).sum::<f64>();
        assert!(norm(&heavy) < norm(&light));
    }

    #[test]
    fn rejects_bad_settings() {
        let x = [vec![1.0], vec![2.0]];
        let rows: Vec<&[f64]> = x.iter().map(Vec::as_slice).collect();
        let task = NetworkTask::Regression { target: vec![1.0, 2.0] };
        for o in [
            NetworkOptions { hidden: vec![], ..NetworkOptions::default() },
            NetworkOptions { hidden: vec![2, 2, 2], ..NetworkOptions::default() },
            NetworkOptions { hidden: vec![0], ..NetworkOptions::default() },
            NetworkOptions { penalty: -1.0, ..NetworkOptions::default() },
            NetworkOptions { starts: 0, ..NetworkOptions::default() },
        ] {
            assert!(fit_network(&rows, &task, &o).is_err());
        }
        let bad = NetworkTask::Classification { class: vec![0, 2], classes: 2 };
        assert!(fit_network(&rows, &bad, &NetworkOptions::default()).is_err());
    }
}
