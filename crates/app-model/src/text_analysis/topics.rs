//! Topic model (latent Dirichlet allocation) of a text column.

use stat_core::Result;
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_text::document_term::DocumentTermMatrix;
use stat_text::topic_model::{TopicModel, TopicModelOptions, fit_topic_model};

use super::{Corpus, TextPreparation, excerpt, invalid, section};
use crate::format::format_number;
use crate::report::{BarChartSpec, BarItem, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;

/// Settings of a topic model.
#[derive(Debug, Clone, PartialEq)]
pub struct TopicModelRequest {
    /// The column and how its words become terms.
    pub text: TextPreparation,
    /// Number of topics.
    pub topics: usize,
    /// Document–topic prior α (`None`: 50 / topics, Griffiths & Steyvers).
    pub alpha: Option<f64>,
    /// Topic–term prior β.
    pub beta: f64,
    /// Gibbs sweeps.
    pub iterations: usize,
    /// Sweeps discarded before averaging.
    pub burn_in: usize,
    /// Seed of the sampler.
    pub seed: u64,
    /// Terms listed per topic.
    pub top_terms: usize,
}

impl TopicModelRequest {
    /// Defaults for `column`: 5 topics, α = 50/K, β = 0.1, 1000 sweeps with
    /// 200 burn-in, seed 1, 8 terms per topic.
    #[must_use]
    pub fn new(column: usize) -> Self {
        let o = TopicModelOptions::new(5);
        Self {
            text: TextPreparation::new(column),
            topics: o.topics,
            alpha: None,
            beta: o.beta,
            iterations: o.iterations,
            burn_in: o.burn_in,
            seed: o.seed,
            top_terms: 8,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        vec![self.text.column]
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.text.column = f(self.text.column);
    }

    /// The α used: the request's, or 50 / topics.
    #[must_use]
    pub fn effective_alpha(&self) -> f64 {
        self.alpha.unwrap_or(50.0 / self.topics.max(1) as f64)
    }

    /// The project-file form (`"analysis": "topic-model"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        use crate::project::s;
        let mut m = vec![("analysis".to_owned(), s("topic-model"))];
        m.extend(self.text.json_members(table));
        let n = |x: usize| JsonValue::Number(x as f64);
        m.extend([
            ("topics".to_owned(), n(self.topics)),
            ("alpha".to_owned(), self.alpha.map_or(JsonValue::Null, JsonValue::Number)),
            ("beta".to_owned(), JsonValue::Number(self.beta)),
            ("iterations".to_owned(), n(self.iterations)),
            ("burn_in".to_owned(), n(self.burn_in)),
            ("seed".to_owned(), JsonValue::Number(self.seed as f64)),
            ("top_terms".to_owned(), n(self.top_terms)),
        ]);
        JsonValue::Object(m)
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// A missing or unknown column.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let text = TextPreparation::from_json(v, table)?;
        let d = Self::new(text.column);
        let count = |key: &str, default: usize| v.get(key).and_then(JsonValue::as_usize).unwrap_or(default);
        Ok(Self {
            text,
            topics: count("topics", d.topics),
            alpha: v.get("alpha").and_then(JsonValue::as_f64),
            beta: v.get("beta").and_then(JsonValue::as_f64).unwrap_or(d.beta),
            iterations: count("iterations", d.iterations),
            burn_in: count("burn_in", d.burn_in),
            seed: v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(d.seed),
            top_terms: count("top_terms", d.top_terms),
        })
    }
}

/// A fitted topic model.
#[derive(Debug, Clone)]
pub struct TopicModelAnalysis {
    request: TopicModelRequest,
    corpus: Corpus,
    matrix: DocumentTermMatrix,
    model: TopicModel,
    perplexity: f64,
}

impl TopicModelAnalysis {
    /// Most topics allowed.
    pub const MAX_TOPICS: usize = 50;

    /// Fit the topic model.
    ///
    /// # Errors
    /// A missing column, no documents or terms, fewer than 2 or more than
    /// [`TopicModelAnalysis::MAX_TOPICS`] topics, or invalid priors.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &TopicModelRequest) -> Result<Self> {
        if !(2..=Self::MAX_TOPICS).contains(&request.topics) {
            return Err(invalid(format!("choose between 2 and {} topics", Self::MAX_TOPICS)));
        }
        let corpus = Corpus::read(table, states, request.text.column)?;
        let matrix = corpus.matrix(&request.text)?;
        let options = TopicModelOptions {
            topics: request.topics,
            alpha: request.effective_alpha(),
            beta: request.beta,
            iterations: request.iterations,
            burn_in: request.burn_in,
            seed: request.seed,
            ..TopicModelOptions::new(request.topics)
        };
        let model = fit_topic_model(&matrix.sequences, matrix.n_terms(), &options)?;
        let perplexity = model.perplexity(&matrix.sequences)?;
        Ok(Self { request: request.clone(), corpus, matrix, model, perplexity })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &TopicModelRequest {
        &self.request
    }

    /// The fitted model.
    #[must_use]
    pub fn model(&self) -> &TopicModel {
        &self.model
    }

    /// Display labels of the vocabulary.
    #[must_use]
    pub fn term_labels(&self) -> &[String] {
        &self.matrix.labels
    }

    /// Dataset rows of the documents.
    #[must_use]
    pub fn rows(&self) -> &[usize] {
        &self.corpus.rows
    }

    /// Most likely topic of each document (from 0).
    #[must_use]
    pub fn main_topics(&self) -> Vec<usize> {
        self.model
            .document_topic
            .iter()
            .map(|t| t.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(&a.0))).map_or(0, |e| e.0))
            .collect()
    }

    /// Report title, e.g. `"Topic Model (5 topics): Review"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Topic Model ({} topics): {}", self.request.topics, self.corpus.name)
    }

    fn topic_label(&self, k: usize) -> String {
        let words: Vec<String> =
            self.model.top_terms(k, 3).iter().map(|&(j, _)| self.matrix.labels[j].clone()).collect();
        format!("{} ({})", k + 1, words.join(", "))
    }

    /// Columns the report can add: `topic` (most likely topic, nominal) and
    /// `topic_1`, `topic_2`, … (topic proportions).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let mut main = ReportColumn::continuous(
            "topic",
            "Save most likely topic",
            "Topic".to_owned(),
            self.corpus.per_row(self.main_topics().into_iter().map(|k| (k + 1) as f64)),
        );
        main.level = MeasurementLevel::Nominal;
        let mut out = vec![main];
        out.extend((0..self.request.topics).map(|k| {
            ReportColumn::continuous(
                &format!("topic_{}", k + 1),
                &format!("Save proportion of topic {}", k + 1),
                format!("Topic {} proportion", k + 1),
                self.corpus.per_row(self.model.document_topic.iter().map(|t| t[k])),
            )
        }));
        out
    }

    /// Report sections: summary, top terms by topic, topic shares, the
    /// log-likelihood trace and representative documents.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![self.summary(), self.terms(), self.shares(), self.trace(), self.documents()]
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        t.push(["Text column".to_owned(), self.corpus.name.clone()]);
        for (k, v) in self.corpus.summary_rows() {
            t.push([k, v]);
        }
        let tokens: usize = self.matrix.sequences.iter().map(Vec::len).sum();
        t.push(["Term occurrences modelled".to_owned(), tokens.to_string()]);
        t.push(["Vocabulary".to_owned(), self.matrix.n_terms().to_string()]);
        t.push(["Terms".to_owned(), self.request.text.describe()]);
        t.push(["Topics".to_owned(), self.request.topics.to_string()]);
        t.push(["α (document–topic prior)".to_owned(), format_number(self.request.effective_alpha(), 6)]);
        t.push(["β (topic–term prior)".to_owned(), format_number(self.request.beta, 6)]);
        let o = &self.model.options;
        t.push([
            "Gibbs sweeps".to_owned(),
            format!(
                "{} (first {} discarded; {} samples averaged, every {}th sweep; seed {})",
                o.iterations, o.burn_in, self.model.samples, o.thin, o.seed
            ),
        ]);
        if let Some(&(_, ll)) = self.model.log_likelihood.last() {
            t.push(["Final log p(words | topics)".to_owned(), format_number(ll, 8)]);
        }
        t.push(["Perplexity (lower is better)".to_owned(), format_number(self.perplexity, 6)]);
        section(SectionKind::TopicSummary, "Summary", SectionContent::Table(t))
    }

    fn terms(&self) -> ReportSection {
        let total: usize = self.model.topic_tokens.iter().sum();
        let mut t = TextTable::new(["Topic", "Share of terms", "Top terms (probability)"]);
        for k in 0..self.request.topics {
            let terms = self
                .model
                .top_terms(k, self.request.top_terms.max(1))
                .iter()
                .map(|&(j, p)| format!("{} ({p:.3})", self.matrix.labels[j]))
                .collect::<Vec<_>>()
                .join(", ");
            t.push([
                (k + 1).to_string(),
                format!("{:.1}%", 100.0 * self.model.topic_tokens[k] as f64 / total.max(1) as f64),
                terms,
            ]);
        }
        section(SectionKind::TopicTerms, "Top Terms by Topic", SectionContent::Table(t))
    }

    fn shares(&self) -> ReportSection {
        let main = self.main_topics();
        let bars = (0..self.request.topics)
            .map(|k| BarItem {
                center: (k + 1) as f64,
                width: 0.8,
                rows: main.iter().zip(&self.corpus.rows).filter(|(t, _)| **t == k).map(|(_, &r)| r).collect(),
                unlinked: 0,
            })
            .collect();
        let spec = BarChartSpec {
            x_label: "Topic".to_owned(),
            y_label: "Documents".to_owned(),
            bars,
            x_ticks: Some((0..self.request.topics).map(|k| ((k + 1) as f64, self.topic_label(k))).collect()),
            curve: None,
            references: Vec::new(),
            x_time: None,
        };
        section(SectionKind::TopicShares, "Documents by Most Likely Topic", SectionContent::Bars(spec))
    }

    fn trace(&self) -> ReportSection {
        let line: Vec<[f64; 2]> = self.model.log_likelihood.iter().map(|&(i, ll)| [i as f64, ll]).collect();
        let plot = PlotSpec {
            x_label: "Sweep".to_owned(),
            y_label: "log p(words | topics)".to_owned(),
            line: Some(line),
            line_name: Some("Log-likelihood".to_owned()),
            ..PlotSpec::default()
        };
        section(
            SectionKind::TopicTrace,
            "Sampler Log-Likelihood (should level off after the burn-in)",
            SectionContent::Plot(Box::new(plot)),
        )
    }

    fn documents(&self) -> ReportSection {
        let mut t = TextTable::new(["Topic", "Row", "Proportion", "Text"]);
        for k in 0..self.request.topics {
            let mut order: Vec<usize> = (0..self.corpus.rows.len()).collect();
            order.sort_by(|&a, &b| self.model.document_topic[b][k].total_cmp(&self.model.document_topic[a][k]));
            for &d in order.iter().take(3) {
                t.push([
                    (k + 1).to_string(),
                    (self.corpus.rows[d] + 1).to_string(),
                    format_number(self.model.document_topic[d][k], 3),
                    excerpt(&self.corpus.texts[d], 120),
                ]);
            }
        }
        section(SectionKind::TopicDocuments, "Most Representative Documents (3 per topic)", SectionContent::Table(t))
    }
}
