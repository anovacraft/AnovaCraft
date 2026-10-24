//! Latent Dirichlet allocation checked against independent references:
//!
//! * the closed-form `log p(w | z)`, `log p(z)`, `φ` and `θ` for fixed
//!   assignments, against `tests/data/topic_model_reference.py` (numpy and
//!   `scipy.special.gammaln`), to 1e-12 relative;
//! * recovery of known topics in the "bars" experiment of Griffiths &
//!   Steyvers (2004, *PNAS* 101, Fig. 1): 25 terms on a 5 × 5 grid, ten
//!   topics that are its five rows and five columns, documents of 100 words
//!   with proportions drawn from `Dirichlet(1)`.

use stat_core::rng::Rng;
use stat_data::{JsonValue, parse_json};
use stat_text::topic_model::{
    TopicModelOptions, estimates_from_assignments, fit_topic_model, log_likelihood_of, log_prior_of,
};

fn index_lists(v: &JsonValue) -> Vec<Vec<usize>> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_array().unwrap().iter().map(|x| x.as_usize().unwrap()).collect())
        .collect()
}

fn matrix(v: &JsonValue) -> Vec<Vec<f64>> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect())
        .collect()
}

fn close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= 1e-12 * b.abs(), "{what}: {a} vs {b} (relative {:e})", (a - b).abs() / b.abs());
}

#[test]
fn closed_forms_match_python_reference() {
    let root = parse_json(include_str!("data/topic_model_reference.json")).unwrap();
    let cases = root.get("cases").unwrap().as_array().unwrap();
    assert_eq!(cases.len(), 3);
    for (c, case) in cases.iter().enumerate() {
        let docs = index_lists(case.get("documents").unwrap());
        let z = index_lists(case.get("assignments").unwrap());
        let v = case.get("n_terms").unwrap().as_usize().unwrap();
        let k = case.get("topics").unwrap().as_usize().unwrap();
        let alpha = case.get("alpha").unwrap().as_f64().unwrap();
        let beta = case.get("beta").unwrap().as_f64().unwrap();
        let ll = log_likelihood_of(&docs, &z, v, k, beta).unwrap();
        close(ll, case.get("log_likelihood").unwrap().as_f64().unwrap(), &format!("case {c} log p(w|z)"));
        let lp = log_prior_of(&docs, &z, k, alpha).unwrap();
        close(lp, case.get("log_prior").unwrap().as_f64().unwrap(), &format!("case {c} log p(z)"));
        let est = estimates_from_assignments(&docs, &z, v, k, alpha, beta).unwrap();
        for (name, got, want) in [
            ("phi", &est.topic_term, matrix(case.get("phi").unwrap())),
            ("theta", &est.document_topic, matrix(case.get("theta").unwrap())),
        ] {
            assert_eq!(got.len(), want.len());
            for (r, (g, w)) in got.iter().zip(&want).enumerate() {
                assert_eq!(g.len(), w.len());
                for (j, (&a, &b)) in g.iter().zip(w).enumerate() {
                    close(a, b, &format!("case {c} {name}[{r}][{j}]"));
                }
            }
        }
    }
}

/// The ten bar topics on a 5 × 5 grid: rows, then columns.
fn bars() -> Vec<Vec<f64>> {
    let mut topics = Vec::new();
    for r in 0..5 {
        topics.push((0..25).map(|w| if w / 5 == r { 0.2 } else { 0.0 }).collect());
    }
    for c in 0..5 {
        topics.push((0..25).map(|w| if w % 5 == c { 0.2 } else { 0.0 }).collect());
    }
    topics
}

/// Documents of `length` words from the bar topics with `θ ~ Dirichlet(1)`
/// (normalised standard exponentials); returns the documents and their `θ`.
fn bars_corpus(documents: usize, length: usize, seed: u64) -> (Vec<Vec<usize>>, Vec<Vec<f64>>) {
    let mut rng = Rng::new(seed);
    let mut thetas = Vec::new();
    let docs = (0..documents)
        .map(|_| {
            let e: Vec<f64> = (0..10).map(|_| -(1.0 - rng.next_f64()).ln()).collect();
            let total: f64 = e.iter().sum();
            thetas.push(e.iter().map(|x| x / total).collect());
            (0..length)
                .map(|_| {
                    let mut u = rng.next_f64() * total;
                    let mut topic = 9;
                    for (k, &x) in e.iter().enumerate() {
                        if u < x {
                            topic = k;
                            break;
                        }
                        u -= x;
                    }
                    let cell = rng.below(5);
                    if topic < 5 { topic * 5 + cell } else { cell * 5 + (topic - 5) }
                })
                .collect()
        })
        .collect();
    (docs, thetas)
}

/// Perplexity of `documents` under proportions `theta` and topics `phi`.
fn perplexity_under(documents: &[Vec<usize>], theta: &[Vec<f64>], phi: &[Vec<f64>]) -> f64 {
    let mut log_sum = 0.0;
    let mut n = 0;
    for (doc, t) in documents.iter().zip(theta) {
        for &w in doc {
            log_sum += t.iter().zip(phi).map(|(a, f)| a * f[w]).sum::<f64>().ln();
            n += 1;
        }
    }
    (-log_sum / f64::from(n)).exp()
}

fn total_variation(a: &[f64], b: &[f64]) -> f64 {
    0.5 * a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f64>()
}

/// Minimum-total-cost one-to-one matching of rows to columns of a square
/// cost matrix (dynamic programming over subsets); returns `assign[row]`.
fn optimal_matching(cost: &[Vec<f64>]) -> Vec<usize> {
    let n = cost.len();
    let full = 1usize << n;
    let mut best = vec![f64::INFINITY; full];
    let mut choice = vec![0usize; full];
    best[0] = 0.0;
    for mask in 0..full {
        let row = mask.count_ones() as usize;
        if row >= n || best[mask].is_infinite() {
            continue;
        }
        for col in 0..n {
            if mask & (1 << col) == 0 {
                let next = mask | (1 << col);
                let c = best[mask] + cost[row][col];
                if c < best[next] {
                    best[next] = c;
                    choice[next] = col;
                }
            }
        }
    }
    let mut assign = vec![0; n];
    let mut mask = full - 1;
    for row in (0..n).rev() {
        let col = choice[mask];
        assign[row] = col;
        mask &= !(1 << col);
    }
    assign
}

/// Tolerance on the total variation distance between each recovered topic
/// and its matched bar (optimal one-to-one matching). With 500 documents of
/// 100 words each bar holds about 5000 tokens; most of the remaining
/// distance is the posterior's genuine uncertainty about tokens of the cell
/// a row and a column share. With 600 sweeps (400 burn-in, 20 averaged
/// samples) the worst topic over chain seeds 1–8 was at most 0.063 (mean
/// distances 0.03–0.04), so 0.1 leaves a wide margin; at 300 sweeps the
/// worst seed reached 0.083.
const BAR_TOLERANCE: f64 = 0.1;

#[test]
fn recovers_bar_topics() {
    let truth = bars();
    for seed in [1u64, 2, 3, 4] {
        let (docs, theta) = bars_corpus(500, 100, 100 + seed);
        let options = TopicModelOptions {
            alpha: 1.0,
            beta: 0.1,
            iterations: 600,
            burn_in: 400,
            thin: 10,
            seed,
            ..TopicModelOptions::new(10)
        };
        let fit = fit_topic_model(&docs, 25, &options).unwrap();
        let cost: Vec<Vec<f64>> =
            fit.topic_term.iter().map(|phi| truth.iter().map(|t| total_variation(phi, t)).collect()).collect();
        let assign = optimal_matching(&cost);
        let worst = (0..10).map(|k| cost[k][assign[k]]).fold(0.0, f64::max);
        eprintln!("seed {seed}: worst total variation {worst:.4}");
        let mean = (0..10).map(|k| cost[k][assign[k]]).sum::<f64>() / 10.0;
        eprintln!("seed {seed}: mean total variation {mean:.4}");
        assert!(worst < BAR_TOLERANCE, "seed {seed}: worst total variation {worst}");
        // The chain has climbed from its random start.
        assert!(fit.log_likelihood.last().unwrap().1 > fit.log_likelihood[0].1);
        // Perplexity within 5% of the generating model's (the fit may be
        // slightly lower, having been tuned to these very documents).
        let p = fit.perplexity(&docs).unwrap();
        let oracle = perplexity_under(&docs, &theta, &truth);
        eprintln!("seed {seed}: perplexity {p:.3}, generating model {oracle:.3}");
        assert!((p / oracle - 1.0).abs() < 0.05, "perplexity {p} vs generating model {oracle}");
    }
}

#[test]
fn optimal_matching_beats_greedy() {
    // Greedy would take (0,0)=1 then (1,1)=10; optimal is (0,1)+(1,0)=4.
    let cost = vec![vec![1.0, 2.0], vec![2.0, 10.0]];
    assert_eq!(optimal_matching(&cost), vec![1, 0]);
}
