//! Lexicon sentiment scores of a text column.

use std::collections::BTreeMap;

use stat_core::Result;
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_text::sentiment::{DocumentSentiment, SentimentOptions, score_text};

use super::{Corpus, excerpt, invalid, section};
use crate::format::format_number;
use crate::report::{BarChartSpec, BarItem, ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;

/// Settings of a sentiment analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct SentimentRequest {
    /// The text column.
    pub column: usize,
    /// Reverse and damp sentiment words after a negation (default true).
    pub negation: bool,
    /// Words before a sentiment word checked for a negation (default 3).
    pub negation_window: usize,
    /// More lexicon entries `(word, valence on −4…4)`, overriding bundled ones.
    pub extra: Vec<(String, f64)>,
    /// Compound scores at or above this are positive, at or below its
    /// negative negative, and between neutral (default 0.05).
    pub threshold: f64,
}

impl SentimentRequest {
    /// Defaults for `column`.
    #[must_use]
    pub fn new(column: usize) -> Self {
        Self { column, negation: true, negation_window: 3, extra: Vec::new(), threshold: 0.05 }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        vec![self.column]
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.column = f(self.column);
    }

    fn options(&self) -> SentimentOptions {
        SentimentOptions {
            negation: self.negation,
            negation_window: self.negation_window,
            extra: self.extra.clone(),
            ..SentimentOptions::default()
        }
    }

    /// The project-file form (`"analysis": "sentiment"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        use crate::project::{obj, s};
        obj([
            ("analysis", s("sentiment")),
            ("column", s(table.column(self.column).map_or("", |c| c.name.as_str()))),
            ("negation", JsonValue::Bool(self.negation)),
            ("negation_window", JsonValue::Number(self.negation_window as f64)),
            (
                "lexicon",
                JsonValue::Object(self.extra.iter().map(|(w, v)| (w.clone(), JsonValue::Number(*v))).collect()),
            ),
            ("threshold", JsonValue::Number(self.threshold)),
        ])
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// A missing or unknown column.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let column = crate::time_series_data::json_column(v, "column", table)?
            .ok_or_else(|| invalid("the sentiment report needs a \"column\""))?;
        let d = Self::new(column);
        let extra = match v.get("lexicon") {
            Some(JsonValue::Object(m)) => m.iter().filter_map(|(w, x)| x.as_f64().map(|x| (w.clone(), x))).collect(),
            _ => Vec::new(),
        };
        Ok(Self {
            negation: v.get("negation").and_then(JsonValue::as_bool).unwrap_or(d.negation),
            negation_window: v.get("negation_window").and_then(JsonValue::as_usize).unwrap_or(d.negation_window),
            extra,
            threshold: v.get("threshold").and_then(JsonValue::as_f64).unwrap_or(d.threshold),
            ..d
        })
    }
}

/// Parse lexicon entries written `word = valence`, separated by commas,
/// semicolons or lines.
///
/// # Errors
/// An entry without `=` or with a valence that is not a number in −4…4.
pub fn parse_lexicon_entries(text: &str) -> Result<Vec<(String, f64)>> {
    super::parse_word_list(text)
        .into_iter()
        .map(|entry| {
            let (w, v) = entry
                .split_once('=')
                .ok_or_else(|| invalid(format!("write lexicon entries as word = valence (got {entry:?})")))?;
            let v: f64 = v.trim().replace('−', "-").parse().map_err(|_| invalid(format!("{v:?} is not a number")))?;
            if !(-4.0..=4.0).contains(&v) {
                return Err(invalid(format!("valences run from −4 to 4 (got {v})")));
            }
            Ok((w.trim().to_lowercase(), v))
        })
        .collect()
}

/// The category of a compound score.
fn category(compound: f64, threshold: f64) -> &'static str {
    if compound >= threshold {
        "Positive"
    } else if compound <= -threshold {
        "Negative"
    } else {
        "Neutral"
    }
}

/// Computed sentiment scores.
#[derive(Debug, Clone)]
pub struct SentimentAnalysis {
    request: SentimentRequest,
    corpus: Corpus,
    scores: Vec<DocumentSentiment>,
}

impl SentimentAnalysis {
    /// Score every document.
    ///
    /// # Errors
    /// A missing column, no documents, or a negative or non-finite threshold.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SentimentRequest) -> Result<Self> {
        if !(request.threshold >= 0.0 && request.threshold < 1.0) {
            return Err(invalid("the neutral band's threshold must be at least 0 and below 1"));
        }
        let corpus = Corpus::read(table, states, request.column)?;
        let options = request.options();
        let scores = corpus.texts.iter().map(|t| score_text(t, &options)).collect();
        Ok(Self { request: request.clone(), corpus, scores })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &SentimentRequest {
        &self.request
    }

    /// Scores of each document.
    #[must_use]
    pub fn scores(&self) -> &[DocumentSentiment] {
        &self.scores
    }

    /// Dataset rows of the documents.
    #[must_use]
    pub fn rows(&self) -> &[usize] {
        &self.corpus.rows
    }

    /// Report title, e.g. `"Sentiment Scores: Review"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Sentiment Scores: {}", self.corpus.name)
    }

    /// Columns the report can add: `compound`, `sum`, `sentiment` (category),
    /// `positive_words`, `negative_words`.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let mut category_text = vec![String::new(); self.corpus.n_rows];
        for (s, &r) in self.scores.iter().zip(&self.corpus.rows) {
            category_text[r] = category(s.compound, self.request.threshold).to_owned();
        }
        let mut cat = ReportColumn::text("sentiment", "Save sentiment category", "Sentiment".to_owned(), category_text);
        cat.level = MeasurementLevel::Nominal;
        vec![
            ReportColumn::continuous(
                "compound",
                "Save sentiment score",
                "Sentiment score".to_owned(),
                self.corpus.per_row(self.scores.iter().map(|s| s.compound)),
            ),
            ReportColumn::continuous(
                "sum",
                "Save sum of valences",
                "Sentiment sum".to_owned(),
                self.corpus.per_row(self.scores.iter().map(|s| s.score)),
            ),
            cat,
            ReportColumn::continuous(
                "positive_words",
                "Save positive word count",
                "Positive words".to_owned(),
                self.corpus.per_row(self.scores.iter().map(|s| s.n_positive as f64)),
            ),
            ReportColumn::continuous(
                "negative_words",
                "Save negative word count",
                "Negative words".to_owned(),
                self.corpus.per_row(self.scores.iter().map(|s| s.n_negative as f64)),
            ),
        ]
    }

    /// Report sections: summary, score distribution, sentiment words, and
    /// the most positive and most negative documents.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![self.summary(), self.distribution(), self.words(), self.extremes(true), self.extremes(false)]
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        t.push(["Text column".to_owned(), self.corpus.name.clone()]);
        for (k, v) in self.corpus.summary_rows() {
            t.push([k, v]);
        }
        let n = self.scores.len() as f64;
        let mean = self.scores.iter().map(|s| s.compound).sum::<f64>() / n;
        let mut sorted: Vec<f64> = self.scores.iter().map(|s| s.compound).collect();
        sorted.sort_by(f64::total_cmp);
        let median =
            stat_core::descriptive::quantile_sorted(&sorted, 0.5, stat_core::descriptive::QuantileMethod::default())
                .unwrap_or(f64::NAN);
        t.push(["Mean score".to_owned(), format_number(mean, 4)]);
        t.push(["Median score".to_owned(), format_number(median, 4)]);
        for c in ["Positive", "Neutral", "Negative"] {
            let k = self.scores.iter().filter(|s| category(s.compound, self.request.threshold) == c).count();
            t.push([format!("{c} documents"), format!("{k} ({:.1}%)", 100.0 * k as f64 / n)]);
        }
        let matched: usize = self.scores.iter().map(|s| s.n_matched).sum();
        let words: usize = self.scores.iter().map(|s| s.n_tokens).sum();
        t.push([
            "Words found in the lexicon".to_owned(),
            format!("{matched} of {words} ({:.1}%)", 100.0 * matched as f64 / words.max(1) as f64),
        ]);
        t.push([
            "Lexicon".to_owned(),
            format!(
                "{} words (Hutto & Gilbert 2014, MIT licence){}",
                stat_text::sentiment::lexicon_len(),
                if self.request.extra.is_empty() {
                    String::new()
                } else {
                    format!(", {} entries of your own", self.request.extra.len())
                }
            ),
        ]);
        t.push([
            "Negation".to_owned(),
            if self.request.negation {
                format!("a negation within {} words before reverses and damps (× −0.74)", self.request.negation_window)
            } else {
                "not applied".to_owned()
            },
        ]);
        t.push([
            "Score".to_owned(),
            format!(
                "sum s of valences scaled to s / √(s² + 15); positive ≥ {0}, negative ≤ −{0}",
                format_number(self.request.threshold, 3)
            ),
        ]);
        section(SectionKind::SentimentSummary, "Summary", SectionContent::Table(t))
    }

    fn distribution(&self) -> ReportSection {
        let width = 0.1;
        let bars = (0..20)
            .map(|b| {
                let lo = -1.0 + b as f64 * width;
                let rows = self
                    .scores
                    .iter()
                    .zip(&self.corpus.rows)
                    .filter(|(s, _)| {
                        let i = (((s.compound + 1.0) / width).floor() as usize).min(19);
                        i == b
                    })
                    .map(|(_, &r)| r)
                    .collect();
                BarItem { center: lo + width / 2.0, width, rows, unlinked: 0 }
            })
            .collect();
        let t = self.request.threshold;
        let spec = BarChartSpec {
            x_label: "Sentiment score".to_owned(),
            y_label: "Documents".to_owned(),
            bars,
            x_ticks: None,
            curve: None,
            references: if t > 0.0 {
                let v = format_number(t, 3);
                vec![(-t, format!("Neutral band −{v}")), (t, format!("+{v}"))]
            } else {
                vec![(0.0, "0".to_owned())]
            },
            x_time: None,
        };
        section(SectionKind::SentimentDistribution, "Score Distribution", SectionContent::Bars(spec))
    }

    fn words(&self) -> ReportSection {
        // word → (valence, occurrences, negated, total)
        let mut tally: BTreeMap<String, (f64, usize, usize, f64)> = BTreeMap::new();
        for s in &self.scores {
            for c in &s.contributions {
                let e = tally.entry(c.word.clone()).or_insert((c.valence, 0, 0, 0.0));
                e.1 += 1;
                e.2 += usize::from(c.negated);
                e.3 += c.contribution;
            }
        }
        let mut list: Vec<(String, (f64, usize, usize, f64))> = tally.into_iter().collect();
        list.sort_by(|a, b| b.1.3.abs().total_cmp(&a.1.3.abs()).then_with(|| a.0.cmp(&b.0)));
        let mut t = TextTable::new(["Word", "Valence", "Occurrences", "Negated", "Total contribution"]);
        for (w, (v, n, neg, total)) in list.iter().take(40) {
            t.push([w.clone(), format_number(*v, 3), n.to_string(), neg.to_string(), format_number(*total, 4)]);
        }
        if t.rows.is_empty() {
            return section(
                SectionKind::SentimentWords,
                "Sentiment Words",
                SectionContent::Note("No word of the documents is in the lexicon.".into()),
            );
        }
        section(
            SectionKind::SentimentWords,
            "Sentiment Words (largest total contribution first)",
            SectionContent::Table(t),
        )
    }

    fn extremes(&self, positive: bool) -> ReportSection {
        let mut order: Vec<usize> = (0..self.scores.len()).collect();
        order.sort_by(|&a, &b| {
            let (x, y) = (self.scores[a].compound, self.scores[b].compound);
            if positive { y.total_cmp(&x) } else { x.total_cmp(&y) }
        });
        let mut t = TextTable::new(["Row", "Score", "Text"]);
        for &d in order.iter().take(5) {
            t.push([
                (self.corpus.rows[d] + 1).to_string(),
                format_number(self.scores[d].compound, 4),
                excerpt(&self.corpus.texts[d], 140),
            ]);
        }
        let (kind, title) = if positive {
            (SectionKind::MostPositive, "Most Positive Documents")
        } else {
            (SectionKind::MostNegative, "Most Negative Documents")
        };
        section(kind, title, SectionContent::Table(t))
    }
}
