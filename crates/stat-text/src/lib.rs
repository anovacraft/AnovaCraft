//! # stat-text
//!
//! Analysis of a collection of short documents (one text cell per row).
//!
//! * [`tokenize`] — Unicode word segmentation (UAX #29), case folding,
//!   stop words, minimum word length, phrases (n-grams within clauses).
//! * [`stop_words`] — the bundled English stop-word list.
//! * [`porter`] — the Porter (1980) suffix-stripping stemmer.
//! * [`document_term`] — document–term matrices with count, binary, log
//!   and TF-IDF weightings.
//! * [`semantic`] — latent semantic analysis by truncated singular value
//!   decomposition of the weighted matrix.
//! * [`topic_model`] — latent Dirichlet allocation fitted by collapsed Gibbs
//!   sampling (Griffiths & Steyvers 2004).
//! * [`sentiment`] — lexicon-based sentiment scores with negation handling.

pub mod document_term;
pub mod porter;
pub mod semantic;
pub mod sentiment;
pub mod stop_words;
pub mod tokenize;
pub mod topic_model;

pub use stat_core::error::{Result, StatError};
