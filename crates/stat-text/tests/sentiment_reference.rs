//! Lexicon sentiment scores against the published vaderSentiment package.
//!
//! `tests/data/sentiment_reference.tsv` is written by
//! `tests/data/sentiment_reference.py`, which runs vaderSentiment 3.3.2
//! (Hutto & Gilbert 2014) on 69 sentences inside the lexicon-and-negation
//! subset stat-text implements, including empty text, words outside the
//! lexicon, several sentiment words, negation at distances 1–3 (and 4, which
//! must not negate), double negation and "no".
//!
//! Tolerances: the raw sum and the unrounded compound score and proportions
//! (captured from VADER's own valence list) agree to 1e-9 absolute; VADER's
//! printed output, rounded to 4 decimals (compound) and 3 (proportions),
//! agrees to 1e-3 (half a unit in the third decimal plus margin).

use stat_text::sentiment::{SentimentOptions, score_text};

struct Case {
    sentence: String,
    exact: [f64; 5],
    rounded: [f64; 4],
}

fn cases() -> Vec<Case> {
    let text = include_str!("data/sentiment_reference.tsv");
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 10, "bad row: {line:?}");
            let num = |i: usize| -> f64 { fields[i].parse().expect("number") };
            Case {
                sentence: fields[0].to_string(),
                exact: [num(1), num(2), num(3), num(4), num(5)],
                rounded: [num(6), num(7), num(8), num(9)],
            }
        })
        .collect()
}

#[test]
fn agrees_with_vader_on_lexicon_and_negation_subset() {
    let cases = cases();
    assert_eq!(cases.len(), 69);
    let options = SentimentOptions::default();
    for case in &cases {
        let d = score_text(&case.sentence, &options);
        let ours = [d.score, d.compound, d.positive, d.negative, d.neutral];
        for (k, (a, b)) in ours.iter().zip(case.exact).enumerate() {
            assert!((a - b).abs() < 1e-9, "{:?}: quantity {k}: ours {a}, VADER {b}", case.sentence);
        }
        let ours = [d.compound, d.positive, d.negative, d.neutral];
        for (k, (a, b)) in ours.iter().zip(case.rounded).enumerate() {
            assert!((a - b).abs() <= 1e-3, "{:?}: rounded quantity {k}: ours {a}, VADER {b}", case.sentence);
        }
    }
}

#[test]
fn reference_covers_negation_and_neutral_cases() {
    let cases = cases();
    let negated = cases
        .iter()
        .filter(|c| score_text(&c.sentence, &SentimentOptions::default()).contributions.iter().any(|t| t.negated))
        .count();
    assert!(negated >= 25, "only {negated} sentences exercise negation");
    assert!(cases.iter().any(|c| c.sentence.is_empty()));
    assert!(cases.iter().any(|c| c.exact[0] == 0.0 && c.exact[4] == 1.0));
}
