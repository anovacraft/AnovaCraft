//! Dialogs for text exploration, topic models and sentiment scores in the
//! Multivariate menu's Text group.

use crate::dialog_kit::ComboExt as _;
use app_model::text_analysis::Weighting;
use app_model::text_analysis::{parse_lexicon_entries, parse_word_list};
use app_model::{SavedReport, SentimentRequest, TextExplorationRequest, TextPreparation, TopicModelRequest};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which analysis the dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    Explore,
    Topics,
    Sentiment,
}

#[derive(Clone)]
pub struct TextDialog {
    /// The report being edited (`None`: a new report).
    edit: Option<u64>,
    kind: TextKind,
    column: Option<usize>,
    // Term preparation.
    case_fold: bool,
    remove_stop_words: bool,
    extra_stop_words: String,
    stem: bool,
    min_length: usize,
    keep_numbers: bool,
    min_document_frequency: usize,
    max_terms: usize,
    // Exploration.
    weighting: Weighting,
    cloud_words: usize,
    chart_terms: usize,
    phrases: bool,
    phrase_length: usize,
    min_phrase_count: usize,
    dimensions: usize,
    clusters: usize,
    // Topics.
    topics: usize,
    custom_alpha: bool,
    alpha: f64,
    beta: f64,
    iterations: usize,
    burn_in: usize,
    // Sentiment.
    negation: bool,
    negation_window: usize,
    lexicon: String,
    threshold: f64,
    seed: u64,
}

/// The text column most likely meant: the first text column whose cells
/// average at least 15 characters, else the first text column.
fn likely_text_column(table: &DataTable) -> Option<usize> {
    let text: Vec<usize> = (0..table.n_cols()).filter(|&i| !kit::is_numeric(table, i)).collect();
    let mean_len = |i: usize| {
        let c = table.column(i).expect("column");
        let n = table.n_rows().max(1);
        (0..table.n_rows()).map(|r| c.cell_text(r).chars().count()).sum::<usize>() as f64 / n as f64
    };
    text.iter().copied().find(|&i| mean_len(i) >= 15.0).or_else(|| text.first().copied())
}

impl TextDialog {
    pub fn new(kind: TextKind, table: &DataTable) -> Self {
        let prep = TextPreparation::new(0);
        let e = TextExplorationRequest::new(0);
        let t = TopicModelRequest::new(0);
        let s = SentimentRequest::new(0);
        Self {
            edit: None,
            kind,
            column: likely_text_column(table),
            case_fold: prep.case_fold,
            remove_stop_words: prep.remove_stop_words,
            extra_stop_words: String::new(),
            stem: prep.stem,
            min_length: prep.min_length,
            keep_numbers: prep.keep_numbers,
            min_document_frequency: prep.min_document_frequency,
            max_terms: prep.max_terms,
            weighting: e.weighting,
            cloud_words: e.cloud_words,
            chart_terms: e.chart_terms,
            phrases: e.phrase_length >= 2,
            phrase_length: e.phrase_length.max(2),
            min_phrase_count: e.min_phrase_count,
            dimensions: e.dimensions,
            clusters: e.clusters,
            topics: t.topics,
            custom_alpha: false,
            alpha: t.effective_alpha(),
            beta: t.beta,
            iterations: t.iterations,
            burn_in: t.burn_in,
            negation: s.negation,
            negation_window: s.negation_window,
            lexicon: String::new(),
            threshold: s.threshold,
            seed: 1,
        }
    }

    fn with_prep(self, p: &TextPreparation, table: &DataTable) -> Self {
        Self {
            column: kit::valid(table, p.column),
            case_fold: p.case_fold,
            remove_stop_words: p.remove_stop_words,
            extra_stop_words: p.extra_stop_words.join(", "),
            stem: p.stem,
            min_length: p.min_length,
            keep_numbers: p.keep_numbers,
            min_document_frequency: p.min_document_frequency,
            max_terms: p.max_terms,
            ..self
        }
    }

    /// The dialog filled from a saved text report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        Some(match saved {
            SavedReport::TextExploration { request: r } => Self {
                weighting: r.weighting,
                cloud_words: r.cloud_words,
                chart_terms: r.chart_terms,
                phrases: r.phrase_length >= 2,
                phrase_length: r.phrase_length.max(2),
                min_phrase_count: r.min_phrase_count,
                dimensions: r.dimensions,
                clusters: r.clusters,
                seed: r.seed,
                ..Self::new(TextKind::Explore, table)
            }
            .with_prep(&r.text, table),
            SavedReport::TopicModel { request: r } => Self {
                topics: r.topics,
                custom_alpha: r.alpha.is_some(),
                alpha: r.effective_alpha(),
                beta: r.beta,
                iterations: r.iterations,
                burn_in: r.burn_in,
                seed: r.seed,
                ..Self::new(TextKind::Topics, table)
            }
            .with_prep(&r.text, table),
            SavedReport::Sentiment { request: r } => Self {
                column: kit::valid(table, r.column),
                negation: r.negation,
                negation_window: r.negation_window,
                lexicon: r.extra.iter().map(|(w, v)| format!("{w} = {v}")).collect::<Vec<_>>().join(", "),
                threshold: r.threshold,
                ..Self::new(TextKind::Sentiment, table)
            },
            _ => return None,
        })
    }

    fn title(&self) -> &'static str {
        match self.kind {
            TextKind::Explore => "Text exploration",
            TextKind::Topics => "Topic model",
            TextKind::Sentiment => "Sentiment scores",
        }
    }

    fn intro(&self) -> &'static str {
        match self.kind {
            TextKind::Explore => {
                "Each non-empty cell of a text column is a document. Counts the words and phrases, draws a word \
                 cloud, and can place the documents on semantic dimensions (latent semantic analysis) and group \
                 them into clusters."
            }
            TextKind::Topics => {
                "Finds topics, each a mix of words, and describes every document as a mix of topics (latent \
                 Dirichlet allocation, fitted by Gibbs sampling)."
            }
            TextKind::Sentiment => {
                "Scores each document from a lexicon of about 7,500 words rated from −4 (very negative) to +4 \
                 (very positive), reversing words after a negation such as \"not\"."
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<ReportRequest> {
        let dialog = Dialog::new(self.title()).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| {
            ui.set_max_width(kit::em(ui) * 32.0);
            kit::note(ui, self.intro());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Text column:");
                kit::column_choice(ui, "text_column", table, &mut self.column, |i| !kit::is_numeric(table, i), None);
            });
            ui.separator();
            match self.kind {
                TextKind::Explore => self.explore(ui),
                TextKind::Topics => self.topics(ui),
                TextKind::Sentiment => self.sentiment(ui),
            }
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.kind, table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn term_options(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.remove_stop_words, "Leave out common words (the, and, of, …)");
        ui.checkbox(&mut self.stem, "Combine word forms (Porter stems: price, prices, priced)");
        ui.horizontal(|ui| {
            ui.label("Keep terms found in at least");
            ui.add(egui::DragValue::new(&mut self.min_document_frequency).range(1..=1000));
            ui.label("documents");
        });
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("More words to leave out:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.extra_stop_words)
                        .hint_text("comma-separated")
                        .desired_width(kit::em(ui) * 16.0),
                );
            });
            ui.checkbox(&mut self.case_fold, "Lower-case every word");
            ui.checkbox(&mut self.keep_numbers, "Keep numbers as terms");
            ui.horizontal(|ui| {
                ui.label("Shortest term (characters):");
                ui.add(egui::DragValue::new(&mut self.min_length).range(1..=20));
            });
            ui.horizontal(|ui| {
                ui.label("Most terms kept (0 for all):");
                ui.add(egui::DragValue::new(&mut self.max_terms).range(0..=100_000));
            });
            ui.horizontal(|ui| {
                ui.label("Random seed:");
                ui.add(kit::seed_value(&mut self.seed));
            });
        });
    }

    fn prep(&self, column: usize) -> TextPreparation {
        TextPreparation {
            column,
            case_fold: self.case_fold,
            remove_stop_words: self.remove_stop_words,
            extra_stop_words: parse_word_list(&self.extra_stop_words),
            stem: self.stem,
            min_length: self.min_length,
            keep_numbers: self.keep_numbers,
            min_document_frequency: self.min_document_frequency,
            max_terms: self.max_terms,
        }
    }

    fn explore(&mut self, ui: &mut egui::Ui) -> Readiness<ReportRequest> {
        ui.horizontal(|ui| {
            ui.label("Words in the cloud:");
            ui.add(egui::DragValue::new(&mut self.cloud_words).range(5..=300));
            ui.label("Bars in the chart:");
            ui.add(egui::DragValue::new(&mut self.chart_terms).range(5..=60));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.phrases, "List phrases of up to");
            ui.add_enabled(self.phrases, egui::DragValue::new(&mut self.phrase_length).range(2..=6));
            ui.label("words seen at least");
            ui.add_enabled(self.phrases, egui::DragValue::new(&mut self.min_phrase_count).range(1..=1000));
            ui.label("times");
        });
        ui.horizontal(|ui| {
            ui.label("Semantic dimensions (0 for none):");
            ui.add(egui::DragValue::new(&mut self.dimensions).range(0..=50))
                .on_hover_text("Latent semantic analysis: a truncated SVD of the weighted document–term matrix");
        });
        ui.add_enabled_ui(self.dimensions > 0, |ui| {
            ui.horizontal(|ui| {
                ui.label("Document clusters (0 for none):");
                ui.add(egui::DragValue::new(&mut self.clusters).range(0..=30))
                    .on_hover_text("k-means on the documents' directions in the semantic dimensions");
            });
            ui.horizontal(|ui| {
                ui.label("Term weighting:");
                egui::ComboBox::from_id_salt("text_weighting").selected_text(self.weighting.label()).show_list(
                    ui,
                    |ui| {
                        for w in Weighting::ALL {
                            ui.selectable_value(&mut self.weighting, w, w.label());
                        }
                    },
                );
            });
        });
        ui.separator();
        self.term_options(ui);
        let Some(column) = self.column else { return Err("Choose a text column".into()) };
        if self.clusters == 1 {
            return Err("Ask for at least two clusters, or 0 for none".into());
        }
        let request = TextExplorationRequest {
            text: self.prep(column),
            weighting: self.weighting,
            chart_terms: self.chart_terms,
            cloud_words: self.cloud_words,
            phrase_length: if self.phrases { self.phrase_length } else { 0 },
            min_phrase_count: self.min_phrase_count,
            dimensions: self.dimensions,
            clusters: if self.dimensions > 0 { self.clusters } else { 0 },
            seed: self.seed,
            ..TextExplorationRequest::new(column)
        };
        kit::ready(ReportRequest::TextExploration(request), "Term frequencies and word cloud")
    }

    fn topics(&mut self, ui: &mut egui::Ui) -> Readiness<ReportRequest> {
        ui.horizontal(|ui| {
            ui.label("Topics:");
            ui.add(egui::DragValue::new(&mut self.topics).range(2..=app_model::TopicModelAnalysis::MAX_TOPICS));
        });
        ui.separator();
        self.term_options(ui);
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.custom_alpha, "Document–topic prior α:");
                if !self.custom_alpha {
                    self.alpha = 50.0 / self.topics.max(1) as f64;
                }
                ui.add_enabled(
                    self.custom_alpha,
                    egui::DragValue::new(&mut self.alpha).range(0.001..=100.0).speed(0.01),
                )
                .on_hover_text("Smaller values let each document lean on fewer topics (default 50 ÷ topics)");
            });
            ui.horizontal(|ui| {
                ui.label("Topic–term prior β:");
                ui.add(egui::DragValue::new(&mut self.beta).range(0.001..=10.0).speed(0.005))
                    .on_hover_text("Smaller values give topics fewer, more distinct terms");
            });
            ui.horizontal(|ui| {
                ui.label("Sweeps:");
                ui.add(egui::DragValue::new(&mut self.iterations).range(50..=20_000));
                ui.label("discarding the first");
                ui.add(egui::DragValue::new(&mut self.burn_in).range(0..=20_000));
            });
        });
        let Some(column) = self.column else { return Err("Choose a text column".into()) };
        if self.burn_in >= self.iterations {
            return Err("Discard fewer sweeps than are run".into());
        }
        let request = TopicModelRequest {
            text: self.prep(column),
            topics: self.topics,
            alpha: self.custom_alpha.then_some(self.alpha),
            beta: self.beta,
            iterations: self.iterations,
            burn_in: self.burn_in,
            seed: self.seed,
            ..TopicModelRequest::new(column)
        };
        kit::ready(ReportRequest::TopicModel(request), format!("{} topics", self.topics))
    }

    fn sentiment(&mut self, ui: &mut egui::Ui) -> Readiness<ReportRequest> {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.negation, "Reverse words after a negation within");
            ui.add_enabled(self.negation, egui::DragValue::new(&mut self.negation_window).range(1..=6));
            ui.label("words");
        });
        ui.horizontal(|ui| {
            ui.label("Neutral band: scores between ±");
            ui.add(egui::DragValue::new(&mut self.threshold).range(0.0..=0.95).speed(0.01));
        });
        kit::more_options(ui, |ui| {
            ui.label("Words of your own (they replace the lexicon's valence):");
            ui.add(
                egui::TextEdit::multiline(&mut self.lexicon)
                    .hint_text("for example: crema = 2, limescale = −1.5")
                    .desired_rows(2)
                    .desired_width(kit::em(ui) * 28.0),
            );
        });
        let extra = parse_lexicon_entries(&self.lexicon).map_err(|e| e.to_string())?;
        let Some(column) = self.column else { return Err("Choose a text column".into()) };
        let request = SentimentRequest {
            negation: self.negation,
            negation_window: self.negation_window,
            extra,
            threshold: self.threshold,
            ..SentimentRequest::new(column)
        };
        kit::ready(ReportRequest::Sentiment(request), "Sentiment of each document")
    }
}

impl Remember for TextDialog {
    fn memory_key(&self) -> String {
        self.title().to_owned()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        if let Some(&c) = columns.iter().find(|&&c| c < table.n_cols() && !kit::is_numeric(table, c)) {
            self.column = Some(c);
        }
    }
}
