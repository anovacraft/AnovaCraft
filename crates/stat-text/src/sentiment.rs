//! Lexicon-based sentiment scores with negation handling.
//!
//! # Method
//!
//! The scores follow the lexicon and negation part of VADER (Hutto, C. J. &
//! Gilbert, E. E. (2014), "VADER: A parsimonious rule-based model for
//! sentiment analysis of social media text", *Proceedings of the Eighth
//! International AAAI Conference on Weblogs and Social Media*, 216–225).
//!
//! 1. The text is split into words by Unicode word segmentation (UAX #29,
//!    [`UnicodeSegmentation::unicode_words`]) and lower-cased. Contractions
//!    such as "don't" stay one word; punctuation is dropped.
//! 2. Each word found in the lexicon contributes its valence *v*, the mean
//!    human rating of the word on a −4 (extremely negative) … +4 (extremely
//!    positive) scale. Words not in the lexicon contribute 0.
//! 3. **Negation.** For each of the `negation_window` (default 3) words
//!    before a lexicon word that is a negator ([`is_negator`]), *v* is
//!    multiplied by the negation scalar *η* = −0.74 (Hutto & Gilbert's
//!    empirically derived value), so a word preceded by *k* negators in the
//!    window contributes *v ηᵏ*: "not good" scores 1.9 × −0.74 = −1.406 and
//!    "not not good" 1.9 × 0.5476. A negator that is itself a lexicon word
//!    ("no", valence −1.2) contributes nothing when the word right after it
//!    is a lexicon word, since it then acts as a negator ("no problem")
//!    rather than as a sentiment word ("no thanks" still negates "thanks").
//! 4. The document score is the sum *s* = Σ contributions (left to right),
//!    and the compound score is *s* / √(*s*² + α) with α = 15, which lies in
//!    (−1, 1) and is ±0.5 at *s* = ±√5.
//! 5. The proportions follow VADER: with *P* = Σ (cᵢ + 1) over positive
//!    contributions, *N* = Σ (|cᵢ| + 1) over negative ones and *Z* the number
//!    of words contributing exactly 0 (including words outside the lexicon),
//!    positive = *P*/T, negative = *N*/T and neutral = *Z*/T with
//!    T = *P* + *N* + *Z*. The +1 per sentiment word compensates for each
//!    neutral word counting 1. All three are 0 for a text with no words.
//!
//! # Scope
//!
//! VADER's other heuristics are not applied: ALL-CAPS emphasis (text is
//! lower-cased), "!" and "?" emphasis, booster and dampener words ("very",
//! "slightly"), the contrastive "but", "least", idioms and special phrases,
//! "never so/this", "without doubt", emoticons and emoji (word segmentation
//! drops them). On text without those features the scores agree with the
//! published vaderSentiment package (`tests/sentiment_reference.rs`), except
//! that "no" negates a word three positions on (VADER: only after "or"/"nor").
//!
//! # Lexicon
//!
//! The bundled lexicon (`data/sentiment_lexicon.tsv`, 7,520 lines) is the
//! VADER lexicon of Hutto & Gilbert, MIT licence
//! (`data/LICENSE-sentiment-lexicon.txt`). Fourteen words appear twice; as in
//! VADER the later line wins, leaving 7,506 distinct entries.

use std::collections::HashMap;
use std::sync::OnceLock;

use unicode_segmentation::UnicodeSegmentation;

/// The bundled lexicon: one `word<TAB>valence` line per entry.
const LEXICON_TSV: &str = include_str!("../data/sentiment_lexicon.tsv");

/// Hutto & Gilbert's (2014) empirical negation scalar.
pub const DEFAULT_NEGATION_SCALAR: f64 = -0.74;

/// Hutto & Gilbert's (2014) normalisation constant α in s / √(s² + α).
pub const NORMALIZATION_ALPHA: f64 = 15.0;

/// Words that negate a following sentiment word.
///
/// Standard English negation words and negative contractions, with and
/// without the apostrophe ("dont" is common in informal text). VADER's list
/// also counts "rarely", "seldom", "despite", "nope" and "uh-uh" ("uhuh"),
/// which are kept so the scores agree; "no" and "nobody" are added. Any
/// other word containing "n't" (or "n’t") is a negator too ([`is_negator`]).
pub const NEGATORS: &[&str] = &[
    "not",
    "no",
    "never",
    "none",
    "nobody",
    "nothing",
    "neither",
    "nor",
    "nowhere",
    "cannot",
    "without",
    "nope",
    "uhuh",
    "uh-uh",
    "rarely",
    "seldom",
    "despite",
    "ain't",
    "aint",
    "aren't",
    "arent",
    "can't",
    "cant",
    "couldn't",
    "couldnt",
    "daren't",
    "darent",
    "didn't",
    "didnt",
    "doesn't",
    "doesnt",
    "don't",
    "dont",
    "hadn't",
    "hadnt",
    "hasn't",
    "hasnt",
    "haven't",
    "havent",
    "isn't",
    "isnt",
    "mightn't",
    "mightnt",
    "mustn't",
    "mustnt",
    "needn't",
    "neednt",
    "oughtn't",
    "oughtnt",
    "shan't",
    "shant",
    "shouldn't",
    "shouldnt",
    "wasn't",
    "wasnt",
    "weren't",
    "werent",
    "won't",
    "wont",
    "wouldn't",
    "wouldnt",
];

/// Options for [`score_text`] and [`score_tokens`].
#[derive(Debug, Clone, PartialEq)]
pub struct SentimentOptions {
    /// Apply negation (default `true`). When `false` every lexicon word
    /// contributes its own valence.
    pub negation: bool,
    /// How many words before a lexicon word are checked for negators
    /// (default 3, as in VADER). 0 disables negation.
    pub negation_window: usize,
    /// Factor applied once per negator in the window (default −0.74).
    pub negation_scalar: f64,
    /// User lexicon entries `(word, valence)`, matched case-insensitively.
    /// They override bundled valences and add new words; a later duplicate
    /// overrides an earlier one. Entries with a non-finite valence are
    /// ignored. Valences are expected on the lexicon's −4 … +4 scale.
    pub extra: Vec<(String, f64)>,
}

impl Default for SentimentOptions {
    fn default() -> Self {
        Self { negation: true, negation_window: 3, negation_scalar: DEFAULT_NEGATION_SCALAR, extra: Vec::new() }
    }
}

/// One lexicon word's contribution to a document score.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenContribution {
    /// Position of the word among the document's tokens (0-based).
    pub token_index: usize,
    /// The word, lower-cased.
    pub word: String,
    /// Its lexicon valence (user entries take precedence).
    pub valence: f64,
    /// What it adds to the score: `valence × scalarᵏ` for *k* negators in
    /// the window, or 0 for a lexicon negator acting on the next word.
    pub contribution: f64,
    /// Whether at least one negator preceded it within the window (k ≥ 1).
    pub negated: bool,
}

/// Sentiment of one document.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentSentiment {
    /// Raw score *s*: the sum of the contributions.
    pub score: f64,
    /// Normalised score *s* / √(*s*² + 15), in (−1, 1).
    pub compound: f64,
    /// Proportion of positive sentiment (VADER's definition; see module docs).
    pub positive: f64,
    /// Proportion of negative sentiment.
    pub negative: f64,
    /// Proportion of neutral words.
    pub neutral: f64,
    /// Number of tokens (words) in the document.
    pub n_tokens: usize,
    /// Number of tokens found in the lexicon.
    pub n_matched: usize,
    /// Number of positive contributions.
    pub n_positive: usize,
    /// Number of negative contributions.
    pub n_negative: usize,
    /// One entry per lexicon token, in document order.
    pub contributions: Vec<TokenContribution>,
}

/// The bundled lexicon, parsed on first use.
fn lexicon() -> &'static HashMap<&'static str, f64> {
    static LEXICON: OnceLock<HashMap<&'static str, f64>> = OnceLock::new();
    LEXICON.get_or_init(|| parse_lexicon(LEXICON_TSV))
}

/// Parses `word<TAB>valence` lines; blank or malformed lines are skipped and
/// a repeated word keeps its last valence.
fn parse_lexicon(text: &str) -> HashMap<&str, f64> {
    let mut map = HashMap::with_capacity(8192);
    for line in text.lines() {
        let mut fields = line.split('\t');
        let (Some(word), Some(value)) = (fields.next(), fields.next()) else {
            continue;
        };
        if let Ok(v) = value.trim().parse::<f64>() {
            if !word.is_empty() && v.is_finite() {
                map.insert(word, v);
            }
        }
    }
    map
}

/// Valence of `word` in the bundled lexicon (−4 … +4), or `None` when the
/// word is not listed. The lookup is exact: lexicon words are lower case
/// (a few emoticons such as ":D" contain capitals), so lower-case the word
/// first when scoring ordinary text.
pub fn lexicon_valence(word: &str) -> Option<f64> {
    lexicon().get(word).copied()
}

/// Number of distinct entries in the bundled lexicon (7,506).
pub fn lexicon_len() -> usize {
    lexicon().len()
}

/// Whether a lower-case `word` negates a following sentiment word: it is in
/// [`NEGATORS`] or contains "n't" (straight or typographic apostrophe).
pub fn is_negator(word: &str) -> bool {
    NEGATORS.contains(&word) || word.contains("n't") || word.contains("n\u{2019}t")
}

/// Scores `text`: Unicode word segmentation, lower-casing, then
/// [`score_tokens`]. Empty text (or text with no words) scores 0 with all
/// proportions 0.
pub fn score_text(text: &str, options: &SentimentOptions) -> DocumentSentiment {
    let tokens: Vec<String> = text.unicode_words().map(str::to_lowercase).collect();
    score_tokens(&tokens, options)
}

/// Scores an already tokenised document (see the module documentation for
/// the method). Tokens are lower-cased for lexicon and negator lookup; the
/// tokens are used as given otherwise, so a caller's tokenizer (stop-word
/// removal, stemming) changes the result. Stop-word removal in particular
/// drops negators such as "not", so score unfiltered tokens.
pub fn score_tokens(tokens: &[String], options: &SentimentOptions) -> DocumentSentiment {
    let extra: HashMap<String, f64> =
        options.extra.iter().filter(|(_, v)| v.is_finite()).map(|(w, v)| (w.to_lowercase(), *v)).collect();
    let lower: Vec<String> = tokens.iter().map(|t| t.to_lowercase()).collect();
    let valence_of = |w: &str| extra.get(w).copied().or_else(|| lexicon_valence(w));
    let window = if options.negation { options.negation_window } else { 0 };

    let mut contributions = Vec::new();
    for (i, word) in lower.iter().enumerate() {
        let Some(valence) = valence_of(word) else {
            continue;
        };
        let acts_as_negator =
            window > 0 && is_negator(word) && lower.get(i + 1).is_some_and(|next| valence_of(next).is_some());
        let mut contribution = if acts_as_negator { 0.0 } else { valence };
        let mut negations = 0;
        for prev in lower[i.saturating_sub(window)..i].iter().rev() {
            if is_negator(prev) {
                contribution *= options.negation_scalar;
                negations += 1;
            }
        }
        contributions.push(TokenContribution {
            token_index: i,
            word: word.clone(),
            valence,
            contribution,
            negated: negations > 0,
        });
    }
    summarize(tokens.len(), contributions)
}

/// Sums the contributions and forms the compound score and proportions.
fn summarize(n_tokens: usize, contributions: Vec<TokenContribution>) -> DocumentSentiment {
    let mut score = 0.0;
    let (mut pos_sum, mut neg_sum) = (0.0, 0.0);
    let (mut n_positive, mut n_negative) = (0, 0);
    for c in &contributions {
        score += c.contribution;
        if c.contribution > 0.0 {
            pos_sum += c.contribution + 1.0;
            n_positive += 1;
        } else if c.contribution < 0.0 {
            neg_sum += 1.0 - c.contribution;
            n_negative += 1;
        }
    }
    let neutral_count = (n_tokens - n_positive - n_negative) as f64;
    let total = pos_sum + neg_sum + neutral_count;
    let (positive, negative, neutral) =
        if total > 0.0 { (pos_sum / total, neg_sum / total, neutral_count / total) } else { (0.0, 0.0, 0.0) };
    DocumentSentiment {
        score,
        compound: score / (score * score + NORMALIZATION_ALPHA).sqrt(),
        positive,
        negative,
        neutral,
        n_tokens,
        n_matched: contributions.len(),
        n_positive,
        n_negative,
        contributions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> SentimentOptions {
        SentimentOptions::default()
    }

    #[test]
    fn lexicon_parses_every_entry() {
        assert_eq!(LEXICON_TSV.lines().count(), 7520);
        assert_eq!(lexicon_len(), 7506);
        assert_eq!(lexicon_valence("good"), Some(1.9));
        assert_eq!(lexicon_valence("bad"), Some(-2.5));
        assert_eq!(lexicon_valence("love"), Some(3.2));
        assert_eq!(lexicon_valence("no"), Some(-1.2));
        assert_eq!(lexicon_valence(":D"), Some(2.3));
        assert_eq!(lexicon_valence("table"), None);
        // Repeated entries keep the later line, as VADER's dictionary does.
        assert_eq!(lexicon_valence("lol"), Some(1.8));
        assert_eq!(lexicon_valence("ok"), Some(1.2));
    }

    #[test]
    fn parser_skips_malformed_lines() {
        let map = parse_lexicon("a\t1.5\n\nb\tx\nc\n\t2\nd\t-0.5\textra\na\t2\n");
        assert_eq!(map.len(), 2);
        assert_eq!(map["a"], 2.0);
        assert_eq!(map["d"], -0.5);
    }

    #[test]
    fn single_word_and_compound() {
        let d = score_text("The food was GOOD.", &opts());
        assert_eq!(d.n_tokens, 4);
        assert_eq!(d.n_matched, 1);
        assert_eq!(d.score, 1.9);
        assert!((d.compound - 1.9 / (1.9f64 * 1.9 + 15.0).sqrt()).abs() < 1e-15);
        // P = 2.9, Z = 3.
        assert!((d.positive - 2.9 / 5.9).abs() < 1e-15);
        assert!((d.neutral - 3.0 / 5.9).abs() < 1e-15);
        assert_eq!(d.negative, 0.0);
    }

    #[test]
    fn negation_window_counts_each_negator() {
        let at = |text: &str| score_text(text, &opts()).score;
        assert!((at("not good") - 1.9 * -0.74).abs() < 1e-15);
        assert!((at("not a good") - 1.9 * -0.74).abs() < 1e-15);
        assert!((at("not a nice good") - (1.8 + 1.9) * -0.74).abs() < 1e-12);
        assert!((at("not one two good") - 1.9 * -0.74).abs() < 1e-15);
        assert_eq!(at("not one two three good"), 1.9);
        assert!((at("not not good") - 1.9 * 0.74 * 0.74).abs() < 1e-15);
        assert!((at("it isn\u{2019}t good") - 1.9 * -0.74).abs() < 1e-15);
        let mut narrow = opts();
        narrow.negation_window = 1;
        assert_eq!(score_text("not a good", &narrow).score, 1.9);
    }

    #[test]
    fn lexicon_negator_before_a_lexicon_word() {
        let d = score_text("no problem", &opts());
        assert_eq!(d.contributions[0].contribution, 0.0);
        assert!(!d.contributions[0].negated);
        assert!(d.contributions[1].negated);
        assert!((d.score - -1.7 * -0.74).abs() < 1e-15);
        // Standing alone, "no" keeps its own valence.
        assert_eq!(score_text("no way", &opts()).score, -1.2);
    }

    #[test]
    fn negation_off() {
        let mut off = opts();
        off.negation = false;
        let d = score_text("no problem not good", &off);
        assert!((d.score - (-1.2 - 1.7 + 1.9)).abs() < 1e-12);
        assert!(d.contributions.iter().all(|c| !c.negated));
        assert_eq!(d.n_positive, 1);
        assert_eq!(d.n_negative, 2);
    }

    #[test]
    fn extra_entries_override_and_add() {
        let mut o = opts();
        o.extra =
            vec![("Flaky".into(), -2.0), ("good".into(), 1.0), ("good".into(), 0.5), ("qwertyword".into(), f64::NAN)];
        let d = score_text("flaky but good qwertyword", &o);
        assert_eq!(d.n_matched, 2);
        assert_eq!(d.contributions[0].valence, -2.0);
        assert_eq!(d.contributions[1].valence, 0.5);
        assert!((d.score - -1.5).abs() < 1e-15);
        // The bundled lexicon is untouched.
        assert_eq!(lexicon_valence("good"), Some(1.9));
    }

    #[test]
    fn empty_and_neutral_text() {
        let d = score_text("", &opts());
        assert_eq!((d.n_tokens, d.score, d.compound), (0, 0.0, 0.0));
        assert_eq!((d.positive, d.negative, d.neutral), (0.0, 0.0, 0.0));
        let d = score_text("  ... !!! ", &opts());
        assert_eq!(d.n_tokens, 0);
        let d = score_text("the table is on the left", &opts());
        assert_eq!((d.score, d.neutral), (0.0, 1.0));
    }

    #[test]
    fn tokens_are_lower_cased_for_lookup() {
        let tokens: Vec<String> = ["NOT", "Good"].iter().map(|s| s.to_string()).collect();
        let d = score_tokens(&tokens, &opts());
        assert_eq!(d.contributions[0].word, "good");
        assert!((d.score - 1.9 * -0.74).abs() < 1e-15);
    }

    #[test]
    fn negator_list_is_lower_case_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for w in NEGATORS {
            assert_eq!(*w, w.to_lowercase());
            assert!(seen.insert(*w), "duplicate negator {w}");
            assert!(is_negator(w));
        }
        assert!(is_negator("shouldn't've"));
        assert!(!is_negator("note"));
    }
}
