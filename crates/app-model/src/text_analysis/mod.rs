//! Text analysis of a text column, bound to the row state
//! ([`stat_text`]).
//!
//! Each non-empty cell of the chosen column on a row that is not excluded is
//! a document. Three reports read it:
//!
//! * [`TextExploration`] — term and phrase frequencies, a word-frequency
//!   chart and a word cloud, and optionally latent semantic dimensions with
//!   a document map and k-means clusters of the documents.
//! * [`TopicModelAnalysis`] — latent Dirichlet allocation: top terms per
//!   topic, topic shares, the sampler's log-likelihood trace, and each
//!   document's topic proportions (saved as columns).
//! * [`SentimentAnalysis`] — lexicon sentiment scores with negation, the
//!   words that drive them, the most positive and negative documents, and
//!   scores saved as columns.
//!
//! The first two share [`TextPreparation`]: how words become terms (case
//! folding, stop words, stemming, minimum length, numbers) and which terms
//! the vocabulary keeps.

mod explore;
mod sentiment;
mod topics;

#[cfg(test)]
mod tests;

pub use explore::{TextExploration, TextExplorationRequest};
pub use sentiment::{SentimentAnalysis, SentimentRequest, parse_lexicon_entries};
pub use stat_text::document_term::Weighting;
pub use topics::{TopicModelAnalysis, TopicModelRequest};

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_text::document_term::{DocumentTermMatrix, VocabularyOptions};
use stat_text::tokenize::TokenOptions;

use crate::report::{ReportSection, SectionContent, SectionKind};

/// How the documents of a text column become terms.
#[derive(Debug, Clone, PartialEq)]
pub struct TextPreparation {
    /// The text column.
    pub column: usize,
    /// Lower-case every word (default true).
    pub case_fold: bool,
    /// Drop the English stop words (default true).
    pub remove_stop_words: bool,
    /// More words to drop.
    pub extra_stop_words: Vec<String>,
    /// Reduce terms to Porter stems (default false).
    pub stem: bool,
    /// Shortest term, in characters (default 2).
    pub min_length: usize,
    /// Keep numbers as terms (default false).
    pub keep_numbers: bool,
    /// Fewest documents a term must occur in (default 2).
    pub min_document_frequency: usize,
    /// Most terms kept, the most frequent first (0: all; default 0).
    pub max_terms: usize,
}

impl TextPreparation {
    /// Default preparation of `column`.
    #[must_use]
    pub fn new(column: usize) -> Self {
        let t = TokenOptions::default();
        Self {
            column,
            case_fold: t.case_fold,
            remove_stop_words: t.remove_stop_words,
            extra_stop_words: Vec::new(),
            stem: t.stem,
            min_length: t.min_length,
            keep_numbers: t.keep_numbers,
            min_document_frequency: 2,
            max_terms: 0,
        }
    }

    /// The tokenizer options.
    #[must_use]
    pub fn token_options(&self) -> TokenOptions {
        TokenOptions {
            case_fold: self.case_fold,
            remove_stop_words: self.remove_stop_words,
            extra_stop_words: self.extra_stop_words.clone(),
            stem: self.stem,
            min_length: self.min_length,
            keep_numbers: self.keep_numbers,
        }
    }

    /// The vocabulary options.
    #[must_use]
    pub fn vocabulary_options(&self) -> VocabularyOptions {
        VocabularyOptions {
            tokens: self.token_options(),
            min_document_frequency: self.min_document_frequency,
            max_terms: self.max_terms,
        }
    }

    /// Project-file members (the column and every option).
    pub(crate) fn json_members(&self, table: &DataTable) -> Vec<(String, JsonValue)> {
        use crate::project::s;
        let name = table.column(self.column).map_or("", |c| c.name.as_str());
        vec![
            ("column".into(), s(name)),
            ("case_fold".into(), JsonValue::Bool(self.case_fold)),
            ("remove_stop_words".into(), JsonValue::Bool(self.remove_stop_words)),
            ("extra_stop_words".into(), JsonValue::Array(self.extra_stop_words.iter().map(|w| s(w)).collect())),
            ("stem".into(), JsonValue::Bool(self.stem)),
            ("min_length".into(), JsonValue::Number(self.min_length as f64)),
            ("keep_numbers".into(), JsonValue::Bool(self.keep_numbers)),
            ("min_document_frequency".into(), JsonValue::Number(self.min_document_frequency as f64)),
            ("max_terms".into(), JsonValue::Number(self.max_terms as f64)),
        ]
    }

    /// Read the project-file members; missing options take their defaults.
    ///
    /// # Errors
    /// A missing or unknown column.
    pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let column = crate::time_series_data::json_column(v, "column", table)?
            .ok_or_else(|| invalid("the text report needs a \"column\""))?;
        let d = Self::new(column);
        let flag = |key: &str, default: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(default);
        let count = |key: &str, default: usize| v.get(key).and_then(JsonValue::as_usize).unwrap_or(default);
        Ok(Self {
            case_fold: flag("case_fold", d.case_fold),
            remove_stop_words: flag("remove_stop_words", d.remove_stop_words),
            extra_stop_words: string_list(v, "extra_stop_words"),
            stem: flag("stem", d.stem),
            min_length: count("min_length", d.min_length),
            keep_numbers: flag("keep_numbers", d.keep_numbers),
            min_document_frequency: count("min_document_frequency", d.min_document_frequency),
            max_terms: count("max_terms", d.max_terms),
            ..d
        })
    }

    /// A one-line description of the settings, e.g. `"stop words removed,
    /// stemmed, terms in ≥ 2 documents"`.
    #[must_use]
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        parts.push(if self.case_fold { "lower-cased" } else { "case kept" }.to_owned());
        parts.push(if self.remove_stop_words { "stop words removed" } else { "stop words kept" }.to_owned());
        if !self.extra_stop_words.is_empty() {
            parts.push(format!("{} more stop words", self.extra_stop_words.len()));
        }
        if self.stem {
            parts.push("Porter stems".to_owned());
        }
        parts.push(format!("terms of ≥ {} characters", self.min_length.max(1)));
        if self.keep_numbers {
            parts.push("numbers kept".to_owned());
        }
        parts.push(format!("in ≥ {} documents", self.min_document_frequency.max(1)));
        if self.max_terms > 0 {
            parts.push(format!("at most {} terms", self.max_terms));
        }
        parts.join(", ")
    }
}

/// A list of strings at `v[key]` (empty when absent).
pub(crate) fn string_list(v: &JsonValue, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(JsonValue::as_array)
        .map(|a| a.iter().filter_map(JsonValue::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Split a comma-, semicolon- or line-separated list of words.
#[must_use]
pub fn parse_word_list(text: &str) -> Vec<String> {
    text.split([',', ';', '\n']).map(str::trim).filter(|w| !w.is_empty()).map(str::to_owned).collect()
}

pub(crate) fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

pub(crate) fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

/// The documents of a text column.
#[derive(Debug, Clone)]
pub(crate) struct Corpus {
    /// Name of the column.
    pub name: String,
    /// Rows of the table.
    pub n_rows: usize,
    /// Dataset row of each document.
    pub rows: Vec<usize>,
    /// Text of each document.
    pub texts: Vec<String>,
    /// Rows left out because they are excluded.
    pub n_excluded: usize,
    /// Rows left out because their cell is empty.
    pub n_empty: usize,
}

impl Corpus {
    /// Read the documents of `column`.
    ///
    /// # Errors
    /// A missing column, or no documents.
    pub fn read(table: &DataTable, states: &RowStateManager, column: usize) -> Result<Self> {
        let c = table.column(column).ok_or_else(|| invalid(format!("no column {column}")))?;
        let (mut n_excluded, mut n_empty) = (0, 0);
        let mut rows = Vec::new();
        let mut texts = Vec::new();
        for r in 0..table.n_rows() {
            if states.is_excluded(r) {
                n_excluded += 1;
                continue;
            }
            let text = c.level_text(r);
            if text.trim().is_empty() {
                n_empty += 1;
            } else {
                rows.push(r);
                texts.push(text.to_string());
            }
        }
        if rows.is_empty() {
            return Err(StatError::InsufficientData { required: 1, found: 0 });
        }
        Ok(Self { name: c.name.clone(), n_rows: table.n_rows(), rows, texts, n_excluded, n_empty })
    }

    /// The document–term matrix under `prep`.
    ///
    /// # Errors
    /// No term remaining.
    pub fn matrix(&self, prep: &TextPreparation) -> Result<DocumentTermMatrix> {
        let docs: Vec<&str> = self.texts.iter().map(String::as_str).collect();
        DocumentTermMatrix::build(&docs, &prep.vocabulary_options())
    }

    /// One value per table row from one value per document (NaN elsewhere).
    pub fn per_row(&self, values: impl IntoIterator<Item = f64>) -> Vec<f64> {
        let mut out = vec![f64::NAN; self.n_rows];
        for (v, &r) in values.into_iter().zip(&self.rows) {
            out[r] = v;
        }
        out
    }

    /// Rows-left-out lines for a summary table.
    pub fn summary_rows(&self) -> Vec<(String, String)> {
        let mut out = vec![("Documents analysed".to_owned(), self.rows.len().to_string())];
        if self.n_empty > 0 {
            out.push(("Rows with empty text (left out)".to_owned(), self.n_empty.to_string()));
        }
        if self.n_excluded > 0 {
            out.push(("Rows excluded".to_owned(), self.n_excluded.to_string()));
        }
        out
    }
}

/// The first `limit` characters of `text` on one line, with "…" when cut.
#[must_use]
pub fn excerpt(text: &str, limit: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        flat
    } else {
        let cut: String = flat.chars().take(limit.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    }
}
