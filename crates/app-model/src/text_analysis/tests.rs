use stat_data::RowStateManager;
use stat_text::document_term::Weighting;

use super::*;
use crate::project::{SavedReport, report_from_json, report_to_json};
use crate::report::SectionContent;
use crate::sample::espresso_reviews;

fn reviews() -> (DataTable, RowStateManager, usize) {
    let t = espresso_reviews();
    let rows = RowStateManager::new(t.n_rows());
    let c = t.find("Review text").unwrap();
    (t, rows, c)
}

fn column_values(t: &DataTable, name: &str) -> Vec<String> {
    let c = t.column(t.find(name).unwrap()).unwrap();
    (0..t.n_rows()).map(|r| c.level_text(r).to_string()).collect()
}

#[test]
fn exploration_sections_terms_and_clusters() {
    let (t, rows, c) = reviews();
    let req = TextExplorationRequest { dimensions: 6, clusters: 6, ..TextExplorationRequest::new(c) };
    let a = TextExploration::run(&t, &rows, &req).unwrap();
    assert_eq!(a.title(), "Text Exploration: Review text");
    let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        [
            SectionKind::TextSummary,
            SectionKind::TermFrequencies,
            SectionKind::WordFrequencyChart,
            SectionKind::WordCloud,
            SectionKind::Phrases,
            SectionKind::SemanticDimensions,
            SectionKind::SemanticTerms,
            SectionKind::DocumentMap,
            SectionKind::DocumentClusters,
        ]
    );
    let m = a.matrix();
    assert_eq!(m.n_documents(), 300);
    assert!(m.terms.iter().all(|w| !crate::text_analysis::tests::is_stop(w)));
    assert!(m.document_frequency.iter().all(|&d| d >= 2));
    // The bars brush the documents that contain each term.
    let s = a.sections();
    let SectionContent::Bars(b) = &s[2].content else { panic!("bars") };
    assert_eq!(b.bars[0].rows.len(), m.document_frequency[0]);
    assert_eq!(b.bars[0].count() as f64, m.term_count[0]);
    let SectionContent::WordCloud(w) = &s[3].content else { panic!("cloud") };
    assert_eq!(w.words.len(), 80);
    assert_eq!(w.words[0].text, m.labels[0]);
    let SectionContent::Plot(p) = &s[7].content else { panic!("map") };
    assert_eq!(p.points.len(), 300);
    assert!(p.groups.is_some());
    // Clusters follow the simulated themes: most clusters are dominated by one.
    let theme = column_values(&t, "Simulated main theme");
    let clusters = a.clusters().unwrap();
    let mut pure = 0;
    for k in 0..6 {
        let members: Vec<&String> = clusters.iter().zip(&theme).filter(|(c, _)| **c == k).map(|(_, t)| t).collect();
        let top = members.iter().map(|m| members.iter().filter(|x| *x == m).count()).max().unwrap_or(0);
        if top as f64 >= 0.6 * members.len() as f64 {
            pure += 1;
        }
    }
    assert!(pure >= 4, "{pure} clusters dominated by one theme");
    let cols = a.saveable_columns();
    assert_eq!(cols.len(), 7);
    assert_eq!(cols[6].key, "cluster");
}

pub(super) fn is_stop(w: &str) -> bool {
    stat_text::stop_words::is_stop_word(w)
}

#[test]
fn topics_recover_the_simulated_themes() {
    let (t, rows, c) = reviews();
    let req = TopicModelRequest { topics: 6, iterations: 400, burn_in: 200, ..TopicModelRequest::new(c) };
    let a = TopicModelAnalysis::run(&t, &rows, &req).unwrap();
    assert_eq!(a.title(), "Topic Model (6 topics): Review text");
    assert_eq!(a.sections().len(), 5);
    let theme = column_values(&t, "Simulated main theme");
    let main = a.main_topics();
    // Each document's main topic predicts its simulated main theme well
    // after mapping each topic to its commonest theme.
    let mut correct = 0;
    for k in 0..6 {
        let members: Vec<&String> = main.iter().zip(&theme).filter(|(m, _)| **m == k).map(|(_, t)| t).collect();
        correct += members.iter().map(|m| members.iter().filter(|x| *x == m).count()).max().unwrap_or(0);
    }
    assert!(correct as f64 >= 0.7 * 300.0, "{correct} of 300 documents in their theme's topic");
    let cols = a.saveable_columns();
    assert_eq!(cols.len(), 7);
    for r in 0..t.n_rows() {
        let total: f64 = cols[1..].iter().map(|c| c.values[r]).sum();
        assert!((total - 1.0).abs() < 1e-9);
    }
    assert!(TopicModelAnalysis::run(&t, &rows, &TopicModelRequest { topics: 1, ..req.clone() }).is_err());
}

#[test]
fn sentiment_follows_the_rating() {
    let (t, rows, c) = reviews();
    let a = SentimentAnalysis::run(&t, &rows, &SentimentRequest::new(c)).unwrap();
    assert_eq!(a.sections().len(), 5);
    let rating = t.column(t.find("Rating").unwrap()).unwrap().as_numeric().unwrap().to_vec();
    let score: Vec<f64> = a.scores().iter().map(|s| s.compound).collect();
    let r = stat_core::correlation::pearson(&rating, &score, 0.95).unwrap().correlation.estimate;
    assert!(r > 0.5, "correlation of score and rating {r}");
    let cols = a.saveable_columns();
    assert_eq!(
        cols.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
        ["compound", "sum", "sentiment", "positive_words", "negative_words"]
    );
    // "not happy" is negated.
    let row = (0..t.n_rows()).find(|&r| column_values(&t, "Review text")[r].contains("Not happy")).unwrap();
    let d = a.rows().iter().position(|&x| x == row).unwrap();
    assert!(a.scores()[d].contributions.iter().any(|c| c.word == "happy" && c.negated && c.contribution < 0.0));
    let entries = parse_lexicon_entries("crema = 2, bitter=−3").unwrap();
    assert_eq!(entries, [("crema".to_owned(), 2.0), ("bitter".to_owned(), -3.0)]);
    assert!(parse_lexicon_entries("crema").is_err() && parse_lexicon_entries("x = 9").is_err());
}

#[test]
fn excluded_and_empty_rows_are_left_out() {
    let (t, mut rows, c) = reviews();
    rows.set_excluded([0, 1, 2], true);
    let a = SentimentAnalysis::run(&t, &rows, &SentimentRequest::new(c)).unwrap();
    assert_eq!(a.rows().len(), 297);
    let cols = a.saveable_columns();
    assert!(cols[0].values[0].is_nan());
    assert_eq!(cols[2].text.as_ref().unwrap()[0], "");
    // A numeric column's empty cells are empty documents.
    let rating = t.find("Rating").unwrap();
    assert!(TextExploration::run(&t, &rows, &TextExplorationRequest::new(rating)).is_err(), "no terms in numbers");
}

#[test]
fn requests_round_trip_through_project_json() {
    let (t, _, c) = reviews();
    let mut prep = TextPreparation::new(c);
    prep.stem = true;
    prep.extra_stop_words = vec!["machine".into()];
    prep.min_length = 3;
    prep.max_terms = 500;
    let reports = [
        SavedReport::TextExploration {
            request: TextExplorationRequest {
                text: prep.clone(),
                weighting: Weighting::LogTfIdf,
                dimensions: 4,
                clusters: 3,
                phrase_length: 2,
                seed: 9,
                ..TextExplorationRequest::new(c)
            },
        },
        SavedReport::TopicModel {
            request: TopicModelRequest {
                text: prep,
                topics: 4,
                alpha: Some(0.5),
                seed: 3,
                ..TopicModelRequest::new(c)
            },
        },
        SavedReport::Sentiment {
            request: SentimentRequest {
                negation: false,
                extra: vec![("crema".into(), 2.0)],
                threshold: 0.1,
                ..SentimentRequest::new(c)
            },
        },
    ];
    for r in reports {
        let back = report_from_json(&report_to_json(&r, &t), &t).unwrap();
        assert_eq!(back, r);
    }
}

#[test]
fn word_lists_and_excerpts() {
    assert_eq!(parse_word_list(" a, b;c\n\n d "), ["a", "b", "c", "d"]);
    assert_eq!(excerpt("one  two\nthree", 50), "one two three");
    assert_eq!(excerpt("abcdefghij", 5), "abcd…");
}
