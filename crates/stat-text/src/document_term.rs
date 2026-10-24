//! Document–term matrices, term weighting and phrase counts.
//!
//! # Method
//! Each document is split into terms ([`crate::tokenize`]). The vocabulary is
//! every term found in at least `min_document_frequency` documents, ordered
//! by decreasing total count (ties alphabetically) and cut to the
//! `max_terms` most frequent. The matrix holds the count `tf(d, t)` of term
//! `t` in document `d`, stored sparsely by document.
//!
//! Weightings (Salton & Buckley 1988, "Term-weighting approaches in
//! automatic text retrieval", *Information Processing & Management* 24(5);
//! Manning, Raghavan & Schütze 2008, *Introduction to Information
//! Retrieval*, §6.2–6.4), with `N` documents and document frequency `df(t)`:
//!
//! | Weighting | `w(d, t)` |
//! |---|---|
//! | Count | `tf` |
//! | Binary | `1` when `tf > 0` |
//! | Log count | `1 + ln tf` when `tf > 0` |
//! | TF-IDF | `tf · ln(N / df)` |
//! | Log TF-IDF | `(1 + ln tf) · ln(N / df)` |
//!
//! With normalisation every document row is scaled to unit Euclidean length
//! (rows that are all zero stay zero), so long and short documents compare
//! by their mix of terms rather than their length (cosine normalisation).
//!
//! Stems are labelled by the surface form that occurs most often for them
//! (ties: the alphabetically first), so "machin" reads as "machine".

use std::collections::HashMap;

use crate::tokenize::{TokenOptions, Word, phrases, words};
use crate::{Result, StatError};

/// How the vocabulary is chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct VocabularyOptions {
    /// How words become terms.
    pub tokens: TokenOptions,
    /// Fewest documents a term must occur in (default 1).
    pub min_document_frequency: usize,
    /// Most terms kept, the most frequent first (0 keeps every term).
    pub max_terms: usize,
}

impl Default for VocabularyOptions {
    fn default() -> Self {
        Self { tokens: TokenOptions::default(), min_document_frequency: 1, max_terms: 0 }
    }
}

/// Term weighting of a document–term matrix (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weighting {
    /// Raw counts.
    Count,
    /// Presence (1) or absence (0).
    Binary,
    /// `1 + ln tf`.
    LogCount,
    /// `tf · ln(N / df)`.
    TfIdf,
    /// `(1 + ln tf) · ln(N / df)`.
    LogTfIdf,
}

impl Weighting {
    /// Every weighting, in menu order.
    pub const ALL: [Self; 5] = [Self::TfIdf, Self::LogTfIdf, Self::Count, Self::LogCount, Self::Binary];

    /// A short name, e.g. `"TF-IDF"`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Count => "Count",
            Self::Binary => "Binary",
            Self::LogCount => "Log count",
            Self::TfIdf => "TF-IDF",
            Self::LogTfIdf => "Log TF-IDF",
        }
    }

    /// The project-file key, e.g. `"tf-idf"`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Binary => "binary",
            Self::LogCount => "log-count",
            Self::TfIdf => "tf-idf",
            Self::LogTfIdf => "log-tf-idf",
        }
    }

    /// The weighting with this project-file key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.key() == key)
    }
}

/// A sparse row: `(term index, value)` pairs in increasing term order.
pub type SparseRow = Vec<(usize, f64)>;

/// The terms of a document collection and their counts.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentTermMatrix {
    /// Vocabulary keys (normalised, possibly stemmed), most frequent first.
    pub terms: Vec<String>,
    /// Display label of each term (its most common surface form, case-folded
    /// as the options say).
    pub labels: Vec<String>,
    /// Counts per document, sparse.
    pub rows: Vec<SparseRow>,
    /// Total count of each term.
    pub term_count: Vec<f64>,
    /// Number of documents containing each term.
    pub document_frequency: Vec<usize>,
    /// Term occurrences of each document in vocabulary order of appearance
    /// (vocabulary indices), the input of topic models.
    pub sequences: Vec<Vec<usize>>,
    /// Words in each document (terms or not).
    pub word_counts: Vec<usize>,
    /// Terms in each document before the vocabulary was cut.
    pub term_occurrences: Vec<usize>,
    /// Distinct terms before the vocabulary was cut.
    pub distinct_terms: usize,
}

impl DocumentTermMatrix {
    /// Count the terms of `documents`.
    ///
    /// # Errors
    /// [`StatError::InsufficientData`] with no documents;
    /// [`StatError::InvalidArgument`] when no term remains (every word was a
    /// stop word, too short, or below the document-frequency cut).
    pub fn build(documents: &[&str], options: &VocabularyOptions) -> Result<Self> {
        if documents.is_empty() {
            return Err(StatError::InsufficientData { required: 1, found: 0 });
        }
        let parsed: Vec<Vec<Word>> = documents.iter().map(|d| words(d, &options.tokens)).collect();
        // Per term: total count, documents, surface counts.
        let mut index: HashMap<&str, usize> = HashMap::new();
        let mut keys: Vec<&str> = Vec::new();
        let mut total: Vec<usize> = Vec::new();
        let mut docs: Vec<usize> = Vec::new();
        let mut surfaces: Vec<HashMap<String, usize>> = Vec::new();
        let mut last_doc: Vec<usize> = Vec::new();
        let mut term_occurrences = Vec::with_capacity(documents.len());
        for (d, ws) in parsed.iter().enumerate() {
            let mut n = 0;
            for w in ws.iter().filter(|w| w.is_term) {
                n += 1;
                let id = *index.entry(w.form.as_str()).or_insert_with(|| {
                    keys.push(w.form.as_str());
                    total.push(0);
                    docs.push(0);
                    surfaces.push(HashMap::new());
                    last_doc.push(usize::MAX);
                    keys.len() - 1
                });
                total[id] += 1;
                if last_doc[id] != d {
                    last_doc[id] = d;
                    docs[id] += 1;
                }
                let surface = if options.tokens.case_fold { w.surface.to_lowercase() } else { w.surface.clone() };
                *surfaces[id].entry(surface).or_insert(0) += 1;
            }
            term_occurrences.push(n);
        }
        let distinct_terms = keys.len();
        let mut kept: Vec<usize> =
            (0..keys.len()).filter(|&i| docs[i] >= options.min_document_frequency.max(1)).collect();
        kept.sort_by(|&a, &b| total[b].cmp(&total[a]).then_with(|| keys[a].cmp(keys[b])));
        if options.max_terms > 0 {
            kept.truncate(options.max_terms);
        }
        if kept.is_empty() {
            return Err(StatError::InvalidArgument {
                message: "no term remains: every word is a stop word, too short, a number, or in too few documents"
                    .into(),
            });
        }
        let mut position = vec![usize::MAX; keys.len()];
        for (j, &i) in kept.iter().enumerate() {
            position[i] = j;
        }
        let terms: Vec<String> = kept.iter().map(|&i| keys[i].to_owned()).collect();
        let labels = kept
            .iter()
            .map(|&i| {
                let mut best: Vec<(&String, &usize)> = surfaces[i].iter().collect();
                best.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
                best.first().map_or_else(|| keys[i].to_owned(), |(s, _)| (*s).clone())
            })
            .collect();
        let mut rows = Vec::with_capacity(parsed.len());
        let mut sequences = Vec::with_capacity(parsed.len());
        for ws in &parsed {
            let seq: Vec<usize> = ws
                .iter()
                .filter(|w| w.is_term)
                .filter_map(|w| index.get(w.form.as_str()).map(|&i| position[i]))
                .filter(|&j| j != usize::MAX)
                .collect();
            let mut counts: HashMap<usize, f64> = HashMap::new();
            for &j in &seq {
                *counts.entry(j).or_insert(0.0) += 1.0;
            }
            let mut row: SparseRow = counts.into_iter().collect();
            row.sort_unstable_by_key(|e| e.0);
            rows.push(row);
            sequences.push(seq);
        }
        Ok(Self {
            terms,
            labels,
            rows,
            term_count: kept.iter().map(|&i| total[i] as f64).collect(),
            document_frequency: kept.iter().map(|&i| docs[i]).collect(),
            sequences,
            word_counts: parsed.iter().map(Vec::len).collect(),
            term_occurrences,
            distinct_terms,
        })
    }

    /// Number of documents.
    #[must_use]
    pub fn n_documents(&self) -> usize {
        self.rows.len()
    }

    /// Number of vocabulary terms.
    #[must_use]
    pub fn n_terms(&self) -> usize {
        self.terms.len()
    }

    /// Inverse document frequency `ln(N / df)` of each term.
    #[must_use]
    pub fn idf(&self) -> Vec<f64> {
        let n = self.n_documents() as f64;
        self.document_frequency.iter().map(|&df| (n / df as f64).ln()).collect()
    }

    /// The weighted matrix, optionally with unit-length rows.
    #[must_use]
    pub fn weighted(&self, weighting: Weighting, normalize: bool) -> Vec<SparseRow> {
        let idf = matches!(weighting, Weighting::TfIdf | Weighting::LogTfIdf).then(|| self.idf());
        self.rows
            .iter()
            .map(|row| {
                let mut out: SparseRow = row
                    .iter()
                    .map(|&(j, tf)| {
                        let w = match weighting {
                            Weighting::Count => tf,
                            Weighting::Binary => 1.0,
                            Weighting::LogCount => 1.0 + tf.ln(),
                            Weighting::TfIdf => tf * idf.as_ref().map_or(1.0, |v| v[j]),
                            Weighting::LogTfIdf => (1.0 + tf.ln()) * idf.as_ref().map_or(1.0, |v| v[j]),
                        };
                        (j, w)
                    })
                    .collect();
                if normalize {
                    let norm = out.iter().map(|e| e.1 * e.1).sum::<f64>().sqrt();
                    if norm > 0.0 {
                        out.iter_mut().for_each(|e| e.1 /= norm);
                    }
                }
                out
            })
            .collect()
    }

    /// Sum of each term's weights over the documents.
    #[must_use]
    pub fn column_sums(rows: &[SparseRow], n_terms: usize) -> Vec<f64> {
        let mut out = vec![0.0; n_terms];
        for row in rows {
            for &(j, w) in row {
                out[j] += w;
            }
        }
        out
    }
}

/// A phrase and how often it occurs.
#[derive(Debug, Clone, PartialEq)]
pub struct PhraseCount {
    /// The phrase (normalised forms joined by spaces).
    pub phrase: String,
    /// Display label (the most common surface form).
    pub label: String,
    /// Words in the phrase.
    pub length: usize,
    /// Occurrences.
    pub count: usize,
    /// Documents containing it.
    pub documents: usize,
}

/// Phrases of 2 to `max_length` words occurring at least `min_count` times,
/// most frequent first (ties: shorter, then alphabetical).
#[must_use]
pub fn phrase_counts(
    documents: &[&str],
    tokens: &TokenOptions,
    max_length: usize,
    min_count: usize,
) -> Vec<PhraseCount> {
    struct Tally {
        length: usize,
        count: usize,
        documents: usize,
        last_document: usize,
        surfaces: HashMap<String, usize>,
    }
    let mut found: HashMap<String, Tally> = HashMap::new();
    for (d, text) in documents.iter().enumerate() {
        let ws = words(text, tokens);
        let as_written: Vec<Word> = ws
            .iter()
            .map(|w| Word {
                form: if tokens.case_fold { w.surface.to_lowercase() } else { w.surface.clone() },
                ..w.clone()
            })
            .collect();
        for n in 2..=max_length {
            for (p, s) in phrases(&ws, n).into_iter().zip(phrases(&as_written, n)) {
                let t = found.entry(p).or_insert_with(|| Tally {
                    length: n,
                    count: 0,
                    documents: 0,
                    last_document: usize::MAX,
                    surfaces: HashMap::new(),
                });
                t.count += 1;
                if t.last_document != d {
                    t.last_document = d;
                    t.documents += 1;
                }
                *t.surfaces.entry(s).or_insert(0) += 1;
            }
        }
    }
    let mut out: Vec<PhraseCount> = found
        .into_iter()
        .filter(|(_, t)| t.count >= min_count.max(1))
        .map(|(phrase, t)| {
            let mut best: Vec<(&String, &usize)> = t.surfaces.iter().collect();
            best.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            let label = best.first().map_or_else(|| phrase.clone(), |(s, _)| (*s).clone());
            PhraseCount { phrase, label, length: t.length, count: t.count, documents: t.documents }
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.length.cmp(&b.length)).then_with(|| a.phrase.cmp(&b.phrase)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs() -> Vec<&'static str> {
        vec![
            "The grinder is loud. Loud grinder, great coffee.",
            "Great coffee and a quiet machine.",
            "The water tank leaks; water tank cracked.",
            "",
            "Loud grinder again.",
        ]
    }

    #[test]
    fn counts_vocabulary_order_and_frequencies() {
        let m = DocumentTermMatrix::build(&docs(), &VocabularyOptions::default()).unwrap();
        // grinder 3, loud 3, then coffee, great, tank and water 2 each, then singletons.
        assert_eq!(m.n_documents(), 5);
        assert_eq!(&m.terms[..6], ["grinder", "loud", "coffee", "great", "tank", "water"]);
        assert_eq!(m.term_count[0], 3.0);
        assert_eq!(m.document_frequency[0], 2);
        assert_eq!(m.document_frequency[4], 1);
        assert!(m.rows[3].is_empty() && m.sequences[3].is_empty());
        assert_eq!(m.word_counts[0], 8);
        assert_eq!(m.term_occurrences[0], 6);
        let j = |t: &str| m.terms.iter().position(|x| x == t).unwrap();
        assert_eq!(m.sequences[1], [j("great"), j("coffee"), j("quiet"), j("machine")]);
        let cut = VocabularyOptions { min_document_frequency: 2, ..VocabularyOptions::default() };
        let m2 = DocumentTermMatrix::build(&docs(), &cut).unwrap();
        assert_eq!(m2.terms, ["grinder", "loud", "coffee", "great"]);
        let top = VocabularyOptions { max_terms: 3, ..VocabularyOptions::default() };
        assert_eq!(DocumentTermMatrix::build(&docs(), &top).unwrap().n_terms(), 3);
        assert!(DocumentTermMatrix::build(&["the a of"], &VocabularyOptions::default()).is_err());
        assert!(DocumentTermMatrix::build(&[], &VocabularyOptions::default()).is_err());
    }

    #[test]
    fn weightings_by_hand() {
        let m = DocumentTermMatrix::build(&docs(), &VocabularyOptions::default()).unwrap();
        let j = |t: &str| m.terms.iter().position(|x| x == t).unwrap();
        let get = |rows: &[SparseRow], d: usize, t: usize| rows[d].iter().find(|e| e.0 == t).map_or(0.0, |e| e.1);
        let n = 5.0_f64;
        let w = m.weighted(Weighting::TfIdf, false);
        assert!((get(&w, 0, j("grinder")) - 2.0 * (n / 2.0).ln()).abs() < 1e-15);
        assert!((get(&w, 0, j("coffee")) - (n / 2.0).ln()).abs() < 1e-15);
        let w = m.weighted(Weighting::LogTfIdf, false);
        assert!((get(&w, 2, j("tank")) - (1.0 + 2.0_f64.ln()) * 5.0_f64.ln()).abs() < 1e-15);
        let w = m.weighted(Weighting::LogCount, false);
        assert!((get(&w, 2, j("water")) - (1.0 + 2.0_f64.ln())).abs() < 1e-15);
        assert_eq!(get(&m.weighted(Weighting::Binary, false), 0, j("loud")), 1.0);
        let w = m.weighted(Weighting::Count, true);
        let norm: f64 = w[0].iter().map(|e| e.1 * e.1).sum();
        assert!((norm - 1.0).abs() < 1e-15);
        // grinder 2, loud 2, great 1, coffee 1: norm √10.
        assert!((get(&w, 0, j("grinder")) - 2.0 / 10.0_f64.sqrt()).abs() < 1e-15);
        assert!(w[3].is_empty());
        for wt in Weighting::ALL {
            assert_eq!(Weighting::from_key(wt.key()), Some(wt));
        }
    }

    #[test]
    fn stems_take_the_commonest_surface_label() {
        let o = VocabularyOptions {
            tokens: TokenOptions { stem: true, ..TokenOptions::default() },
            ..VocabularyOptions::default()
        };
        let m = DocumentTermMatrix::build(&["Machines machines machine"], &o).unwrap();
        assert_eq!(m.labels[0], "machines");
    }

    #[test]
    fn phrases_counted_with_documents() {
        let p = phrase_counts(&docs(), &TokenOptions::default(), 3, 2);
        let names: Vec<&str> = p.iter().map(|x| x.phrase.as_str()).collect();
        assert_eq!(names, ["great coffee", "loud grinder", "water tank"]);
        assert_eq!(p[0].documents, 2);
        assert_eq!(p[2].documents, 1);
        assert_eq!(p[2].count, 2);
    }
}
