//! Splitting text into words, terms and phrases.
//!
//! # Method
//! Text is segmented at Unicode word boundaries (Unicode Standard Annex #29,
//! *Unicode Text Segmentation*, default word boundary rules), so "don't",
//! "3.5" and "naïve" stay whole while spaces and punctuation separate words.
//! A segment is a word when it contains a letter or digit. Curly apostrophes
//! are read as straight ones, and with case folding each word is lower-cased
//! with Unicode's full lower-case mapping.
//!
//! Punctuation other than hyphens, apostrophes, slashes and ampersands, and
//! line breaks, end a *clause*: phrases never span a clause boundary, so
//! "price. Coffee" yields no phrase "price coffee".
//!
//! A word becomes a *term* unless it is a stop word (when stop words are
//! removed), shorter than the minimum length (in characters), or made only of
//! digits and number punctuation (unless numbers are kept). With stemming,
//! terms are reduced to their Porter stems ([`crate::porter`]).
//!
//! A *phrase* of length `n ≥ 2` is a run of `n` consecutive words of one
//! clause whose first and last words would be terms (stop words may sit in
//! the middle, as in "out of stock"), written with single spaces and with the
//! same case folding and stemming as terms (Justeson & Katz 1995, "Technical
//! terminology: some linguistic properties and an algorithm for
//! identification in text", *Natural Language Engineering* 1(1), take the
//! same view of phrase boundaries).

use unicode_segmentation::UnicodeSegmentation;

use crate::porter::porter_stem;
use crate::stop_words::is_stop_word;

/// How words become terms.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenOptions {
    /// Lower-case every word (default true).
    pub case_fold: bool,
    /// Drop the English stop words ([`crate::stop_words::ENGLISH`]; default true).
    pub remove_stop_words: bool,
    /// More words to drop, compared after case folding.
    pub extra_stop_words: Vec<String>,
    /// Reduce terms to their Porter stems (default false).
    pub stem: bool,
    /// Shortest term kept, in characters (default 2).
    pub min_length: usize,
    /// Keep words made only of digits (default false).
    pub keep_numbers: bool,
}

impl Default for TokenOptions {
    fn default() -> Self {
        Self {
            case_fold: true,
            remove_stop_words: true,
            extra_stop_words: Vec::new(),
            stem: false,
            min_length: 2,
            keep_numbers: false,
        }
    }
}

/// One word of a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// The word as written (curly apostrophes straightened).
    pub surface: String,
    /// The normalised form: case-folded and, for terms with stemming, stemmed.
    pub form: String,
    /// Whether the word is a term under the options.
    pub is_term: bool,
    /// Index of the clause the word belongs to (from 0).
    pub clause: usize,
}

/// Whether a non-word segment ends a clause.
fn breaks_clause(segment: &str) -> bool {
    segment.chars().any(|c| {
        matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
            || !(c.is_whitespace() || matches!(c, '-' | '\u{2010}' | '\u{2011}' | '\'' | '\u{2019}' | '/' | '&'))
    })
}

/// Whether a word is made only of digits and number punctuation.
fn is_number(word: &str) -> bool {
    word.chars().all(|c| c.is_numeric() || matches!(c, '.' | ',' | '\''))
}

/// The words of `text` (every word, terms or not) in order.
#[must_use]
pub fn words(text: &str, options: &TokenOptions) -> Vec<Word> {
    let mut out = Vec::new();
    let mut clause = 0;
    let mut pending_break = false;
    for segment in text.split_word_bounds() {
        if !segment.chars().any(char::is_alphanumeric) {
            if breaks_clause(segment) {
                pending_break = true;
            }
            continue;
        }
        if pending_break && !out.is_empty() {
            clause += 1;
        }
        pending_break = false;
        let surface = segment.replace('\u{2019}', "'");
        let folded = if options.case_fold { surface.to_lowercase() } else { surface.clone() };
        let lower = if options.case_fold { folded.clone() } else { folded.to_lowercase() };
        let stop = options.remove_stop_words && is_stop_word(&lower)
            || options.extra_stop_words.iter().any(|w| w.trim().to_lowercase() == lower);
        let is_term = !stop
            && folded.chars().count() >= options.min_length.max(1)
            && (options.keep_numbers || !is_number(&folded));
        let form = if is_term && options.stem { stem_preserving_case(&folded) } else { folded };
        out.push(Word { surface, form, is_term, clause });
    }
    out
}

/// Porter stem of a word; words with upper-case letters (when case folding
/// is off) are stemmed in lower case.
fn stem_preserving_case(word: &str) -> String {
    if word.chars().any(char::is_uppercase) { porter_stem(&word.to_lowercase()) } else { porter_stem(word) }
}

/// The terms of `text`, in order.
#[must_use]
pub fn terms(text: &str, options: &TokenOptions) -> Vec<String> {
    words(text, options).into_iter().filter(|w| w.is_term).map(|w| w.form).collect()
}

/// The phrases of length `n` (`n ≥ 2`) in `words`, in order (see the module
/// documentation). Returns nothing for `n < 2`.
#[must_use]
pub fn phrases(words: &[Word], n: usize) -> Vec<String> {
    if n < 2 {
        return Vec::new();
    }
    words
        .windows(n)
        .filter(|w| w[0].clause == w[n - 1].clause && w[0].is_term && w[n - 1].is_term)
        .map(|w| w.iter().map(|x| x.form.as_str()).collect::<Vec<_>>().join(" "))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_words_case_and_apostrophes() {
        let o = TokenOptions { remove_stop_words: false, min_length: 1, ..TokenOptions::default() };
        let w = words("Don’t stop — naïve CAFÉ users paid 3.50, OK?", &o);
        let forms: Vec<&str> = w.iter().map(|x| x.form.as_str()).collect();
        assert_eq!(forms, ["don't", "stop", "naïve", "café", "users", "paid", "3.50", "ok"]);
        assert!(!w[6].is_term, "numbers are dropped by default");
        assert_eq!(w[0].surface, "Don't");
        // The dash and the comma end clauses.
        let clauses: Vec<usize> = w.iter().map(|x| x.clause).collect();
        assert_eq!(clauses, [0, 0, 1, 1, 1, 1, 1, 2]);
    }

    #[test]
    fn stop_words_length_numbers_and_extra() {
        let o = TokenOptions { extra_stop_words: vec!["Coffee".into()], ..TokenOptions::default() };
        assert_eq!(terms("The coffee is a bit bitter in 2024 x", &o), ["bit", "bitter"]);
        let keep = TokenOptions { keep_numbers: true, remove_stop_words: false, min_length: 1, ..o };
        assert_eq!(terms("The x 2024", &keep), ["the", "x", "2024"]);
        let no_fold = TokenOptions { case_fold: false, ..TokenOptions::default() };
        assert_eq!(terms("The Grinder", &no_fold), ["Grinder"]);
    }

    #[test]
    fn phrases_respect_clauses_and_stop_word_ends() {
        let o = TokenOptions::default();
        let w = words("Water tank leaks. Out of stock again; water tank cracked", &o);
        assert_eq!(phrases(&w, 2), ["water tank", "tank leaks", "water tank", "tank cracked"]);
        assert_eq!(phrases(&w, 3), ["water tank leaks", "water tank cracked"]);
        let w = words("out of stock", &TokenOptions { remove_stop_words: false, ..o.clone() });
        assert_eq!(phrases(&w, 3), ["out of stock"]);
        assert!(phrases(&w, 1).is_empty());
    }

    #[test]
    fn empty_and_punctuation_only() {
        assert!(words("", &TokenOptions::default()).is_empty());
        assert!(words(" ... !! -- ", &TokenOptions::default()).is_empty());
    }
}
