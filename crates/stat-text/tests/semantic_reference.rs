//! Document–term counts, TF-IDF weights with unit-length rows and latent
//! semantic analysis compared with an independent NumPy computation
//! (`tests/data/semantic_reference.py` writes `semantic_reference.json`).

use stat_data::{JsonValue, parse_json};
use stat_text::document_term::{DocumentTermMatrix, VocabularyOptions, Weighting};
use stat_text::semantic::latent_semantic_analysis;

fn matrix(v: &JsonValue, key: &str) -> Vec<Vec<f64>> {
    v.get(key)
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect())
        .collect()
}

fn dense(rows: &[Vec<(usize, f64)>], n_terms: usize) -> Vec<Vec<f64>> {
    rows.iter()
        .map(|r| {
            let mut out = vec![0.0; n_terms];
            r.iter().for_each(|&(j, x)| out[j] = x);
            out
        })
        .collect()
}

fn close(got: &[Vec<f64>], want: &[Vec<f64>], tol: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what} rows");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert_eq!(g.len(), w.len(), "{what} row {i} length");
        for (j, (a, b)) in g.iter().zip(w).enumerate() {
            assert!((a - b).abs() <= tol, "{what}[{i}][{j}]: {a} vs {b}");
        }
    }
}

#[test]
fn counts_tfidf_and_lsa_match_numpy() {
    let v = parse_json(include_str!("data/semantic_reference.json")).unwrap();
    let docs: Vec<&str> = v.get("documents").unwrap().as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
    let terms: Vec<&str> = v.get("terms").unwrap().as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
    let m = DocumentTermMatrix::build(&docs, &VocabularyOptions::default()).unwrap();
    assert_eq!(m.terms, terms);
    let t = m.n_terms();
    close(&dense(&m.rows, t), &matrix(&v, "counts"), 0.0, "counts");
    let w = m.weighted(Weighting::TfIdf, true);
    close(&dense(&w, t), &matrix(&v, "tfidf_normalized"), 1e-14, "TF-IDF");
    let s = latent_semantic_analysis(&w, t, 3, 1).unwrap();
    let sv: Vec<f64> =
        v.get("singular_values").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    for i in 0..3 {
        assert!((s.singular_values[i] - sv[i]).abs() <= 1e-12 * sv[i], "σ{i}");
    }
    let total = v.get("total_sum_squares").unwrap().as_f64().unwrap();
    assert!((s.total_sum_squares - total).abs() <= 1e-12 * total);
    close(&s.term_loadings, &matrix(&v, "term_loadings"), 1e-10, "term loadings");
    close(&s.document_scores, &matrix(&v, "document_scores"), 1e-10, "document scores");
}
