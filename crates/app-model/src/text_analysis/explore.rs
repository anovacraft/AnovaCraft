//! Term frequencies, phrases, word cloud, latent semantic dimensions and
//! document clusters of a text column.

use stat_core::Result;
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_multivariate::{KMeansOptions, k_means};
use stat_text::document_term::{DocumentTermMatrix, PhraseCount, SparseRow, Weighting, phrase_counts};
use stat_text::semantic::{SemanticSpace, latent_semantic_analysis};

use super::{Corpus, TextPreparation, invalid, section};
use crate::format::format_number;
use crate::report::{
    Annotations, BarChartSpec, BarItem, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind,
    TextTable,
};
use crate::report_columns::ReportColumn;
use crate::word_cloud::WordCloudSpec;

/// Settings of a text exploration.
#[derive(Debug, Clone, PartialEq)]
pub struct TextExplorationRequest {
    /// The column and how its words become terms.
    pub text: TextPreparation,
    /// Weighting of the document–term matrix for the semantic dimensions,
    /// clusters and the weight column of the term table.
    pub weighting: Weighting,
    /// Rows of the term table (0: every term).
    pub table_terms: usize,
    /// Bars of the word-frequency chart.
    pub chart_terms: usize,
    /// Words in the word cloud.
    pub cloud_words: usize,
    /// Longest phrase counted, in words (below 2: no phrase table).
    pub phrase_length: usize,
    /// Fewest occurrences of a phrase listed.
    pub min_phrase_count: usize,
    /// Latent semantic dimensions (0: none).
    pub dimensions: usize,
    /// Document clusters by k-means on the semantic dimensions (0 or 1: none).
    pub clusters: usize,
    /// Seed of the clustering starts and the randomized decomposition.
    pub seed: u64,
}

impl TextExplorationRequest {
    /// Defaults for `column`: TF-IDF, 100 table rows, 20 bars, 80 cloud
    /// words, phrases of 2–3 words seen at least twice, two dimensions, no
    /// clusters, seed 1.
    #[must_use]
    pub fn new(column: usize) -> Self {
        Self {
            text: TextPreparation::new(column),
            weighting: Weighting::TfIdf,
            table_terms: 100,
            chart_terms: 20,
            cloud_words: 80,
            phrase_length: 3,
            min_phrase_count: 2,
            dimensions: 2,
            clusters: 0,
            seed: 1,
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

    /// The project-file form (`"analysis": "text-exploration"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        use crate::project::s;
        let mut m = vec![("analysis".to_owned(), s("text-exploration"))];
        m.extend(self.text.json_members(table));
        let n = |x: usize| JsonValue::Number(x as f64);
        m.extend([
            ("weighting".to_owned(), s(self.weighting.key())),
            ("table_terms".to_owned(), n(self.table_terms)),
            ("chart_terms".to_owned(), n(self.chart_terms)),
            ("cloud_words".to_owned(), n(self.cloud_words)),
            ("phrase_length".to_owned(), n(self.phrase_length)),
            ("min_phrase_count".to_owned(), n(self.min_phrase_count)),
            ("dimensions".to_owned(), n(self.dimensions)),
            ("clusters".to_owned(), n(self.clusters)),
            ("seed".to_owned(), JsonValue::Number(self.seed as f64)),
        ]);
        JsonValue::Object(m)
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// A missing or unknown column, or an unknown weighting.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let text = TextPreparation::from_json(v, table)?;
        let d = Self::new(text.column);
        let count = |key: &str, default: usize| v.get(key).and_then(JsonValue::as_usize).unwrap_or(default);
        let weighting = match v.get("weighting").and_then(JsonValue::as_str) {
            None => d.weighting,
            Some(k) => Weighting::from_key(k).ok_or_else(|| invalid(format!("unknown weighting {k:?}")))?,
        };
        Ok(Self {
            text,
            weighting,
            table_terms: count("table_terms", d.table_terms),
            chart_terms: count("chart_terms", d.chart_terms),
            cloud_words: count("cloud_words", d.cloud_words),
            phrase_length: count("phrase_length", d.phrase_length),
            min_phrase_count: count("min_phrase_count", d.min_phrase_count),
            dimensions: count("dimensions", d.dimensions),
            clusters: count("clusters", d.clusters),
            seed: v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(d.seed),
        })
    }
}

/// A computed text exploration.
#[derive(Debug, Clone)]
pub struct TextExploration {
    request: TextExplorationRequest,
    corpus: Corpus,
    matrix: DocumentTermMatrix,
    weighted: Vec<SparseRow>,
    phrases: Vec<PhraseCount>,
    space: Option<SemanticSpace>,
    /// Cluster of each document.
    clusters: Option<Vec<usize>>,
}

impl TextExploration {
    /// Count the terms and fit the requested dimensions and clusters.
    ///
    /// # Errors
    /// A missing column, no non-empty documents, no term remaining, more
    /// dimensions than documents or terms allow, or more clusters than
    /// documents.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &TextExplorationRequest) -> Result<Self> {
        let corpus = Corpus::read(table, states, request.text.column)?;
        let matrix = corpus.matrix(&request.text)?;
        let weighted = matrix.weighted(request.weighting, true);
        let docs: Vec<&str> = corpus.texts.iter().map(String::as_str).collect();
        let phrases = if request.phrase_length >= 2 {
            phrase_counts(&docs, &request.text.token_options(), request.phrase_length, request.min_phrase_count)
        } else {
            Vec::new()
        };
        let space = if request.dimensions > 0 {
            let most = matrix.n_documents().min(matrix.n_terms());
            if request.dimensions > most {
                return Err(invalid(format!(
                    "at most {most} semantic dimensions are possible with {} documents and {} terms",
                    matrix.n_documents(),
                    matrix.n_terms()
                )));
            }
            Some(latent_semantic_analysis(&weighted, matrix.n_terms(), request.dimensions, request.seed)?)
        } else {
            None
        };
        let clusters = match (&space, request.clusters) {
            (Some(s), k) if k >= 2 => {
                if k > corpus.rows.len() {
                    return Err(invalid(format!("{k} clusters need at least {k} documents")));
                }
                // Cosine geometry: cluster the directions of the documents.
                let points: Vec<Vec<f64>> = s
                    .document_scores
                    .iter()
                    .map(|p| {
                        let norm = p.iter().map(|x| x * x).sum::<f64>().sqrt();
                        if norm > 0.0 { p.iter().map(|x| x / norm).collect() } else { p.clone() }
                    })
                    .collect();
                let fit = k_means(&points, &KMeansOptions { seed: request.seed, ..KMeansOptions::new(k) })?;
                Some(fit.assignments)
            }
            (None, k) if k >= 2 => {
                return Err(invalid("document clusters are found in the semantic dimensions; choose at least one"));
            }
            _ => None,
        };
        Ok(Self { request: request.clone(), corpus, matrix, weighted, phrases, space, clusters })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &TextExplorationRequest {
        &self.request
    }

    /// The document–term matrix.
    #[must_use]
    pub fn matrix(&self) -> &DocumentTermMatrix {
        &self.matrix
    }

    /// The semantic space, when dimensions were requested.
    #[must_use]
    pub fn space(&self) -> Option<&SemanticSpace> {
        self.space.as_ref()
    }

    /// Dataset rows of the documents.
    #[must_use]
    pub fn rows(&self) -> &[usize] {
        &self.corpus.rows
    }

    /// The phrases found.
    #[must_use]
    pub fn phrases(&self) -> &[PhraseCount] {
        &self.phrases
    }

    /// Cluster of each document (from 0), when clusters were requested.
    #[must_use]
    pub fn clusters(&self) -> Option<&[usize]> {
        self.clusters.as_deref()
    }

    /// Report title, e.g. `"Text Exploration: Review"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Text Exploration: {}", self.corpus.name)
    }

    /// Columns the report can add: `dimension_1`, … (document coordinates)
    /// and `cluster`.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let mut out = Vec::new();
        if let Some(s) = &self.space {
            for i in 0..s.singular_values.len() {
                out.push(ReportColumn::continuous(
                    &format!("dimension_{}", i + 1),
                    &format!("Save semantic dimension {}", i + 1),
                    format!("Semantic dimension {}", i + 1),
                    self.corpus.per_row(s.document_scores.iter().map(|p| p[i])),
                ));
            }
        }
        if let Some(c) = &self.clusters {
            let mut col = ReportColumn::continuous(
                "cluster",
                "Save document cluster",
                "Document cluster".to_owned(),
                self.corpus.per_row(c.iter().map(|&k| (k + 1) as f64)),
            );
            col.level = MeasurementLevel::Nominal;
            out.push(col);
        }
        out
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.term_table(), self.term_chart(), self.cloud()];
        if self.request.phrase_length >= 2 {
            out.push(self.phrase_table());
        }
        if let Some(s) = &self.space {
            out.push(self.dimension_table(s));
            out.push(self.loading_table(s));
            if s.singular_values.len() >= 2 {
                out.push(self.document_map(s));
            }
        }
        if let Some(c) = &self.clusters {
            out.push(self.cluster_table(c));
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let m = &self.matrix;
        let mut t = TextTable::new(["Statistic", "Value"]);
        t.push(["Text column".to_owned(), self.corpus.name.clone()]);
        for (k, v) in self.corpus.summary_rows() {
            t.push([k, v]);
        }
        let words: usize = m.word_counts.iter().sum();
        let terms: usize = m.term_occurrences.iter().sum();
        t.push(["Words".to_owned(), words.to_string()]);
        t.push(["Mean words per document".to_owned(), format_number(words as f64 / m.n_documents() as f64, 4)]);
        t.push(["Term occurrences".to_owned(), terms.to_string()]);
        t.push(["Distinct terms".to_owned(), m.distinct_terms.to_string()]);
        t.push(["Terms kept".to_owned(), m.n_terms().to_string()]);
        let empty = m.rows.iter().filter(|r| r.is_empty()).count();
        if empty > 0 {
            t.push(["Documents with no kept term".to_owned(), empty.to_string()]);
        }
        t.push(["Terms".to_owned(), self.request.text.describe()]);
        t.push(["Weighting".to_owned(), format!("{}, unit-length rows", self.request.weighting.label())]);
        section(SectionKind::TextSummary, "Summary", SectionContent::Table(t))
    }

    fn documents_with(&self, term: usize) -> Vec<usize> {
        self.matrix
            .rows
            .iter()
            .zip(&self.corpus.rows)
            .filter(|(row, _)| row.binary_search_by_key(&term, |e| e.0).is_ok())
            .map(|(_, &r)| r)
            .collect()
    }

    fn term_table(&self) -> ReportSection {
        let m = &self.matrix;
        let sums = DocumentTermMatrix::column_sums(&self.weighted, m.n_terms());
        let n_docs = m.n_documents() as f64;
        let mut cols = vec!["Rank", "Term"];
        if self.request.text.stem {
            cols.push("Stem");
        }
        cols.extend(["Count", "Documents", "% of documents", "Weight total"]);
        let weight_heading = format!("{} total", self.request.weighting.label());
        let mut t = TextTable::new(cols.iter().map(|c| if *c == "Weight total" { weight_heading.as_str() } else { c }));
        let shown = if self.request.table_terms == 0 { m.n_terms() } else { self.request.table_terms.min(m.n_terms()) };
        for j in 0..shown {
            let mut row = vec![(j + 1).to_string(), m.labels[j].clone()];
            if self.request.text.stem {
                row.push(m.terms[j].clone());
            }
            row.extend([
                format_number(m.term_count[j], 8),
                m.document_frequency[j].to_string(),
                format!("{:.1}%", 100.0 * m.document_frequency[j] as f64 / n_docs),
                format_number(sums[j], 4),
            ]);
            t.push(row);
        }
        let title = if shown < m.n_terms() {
            format!("Term Frequencies ({shown} most frequent of {})", m.n_terms())
        } else {
            "Term Frequencies".to_owned()
        };
        section(SectionKind::TermFrequencies, &title, SectionContent::Table(t))
    }

    fn term_chart(&self) -> ReportSection {
        let m = &self.matrix;
        let shown = self.request.chart_terms.clamp(1, 60).min(m.n_terms());
        let bars = (0..shown)
            .map(|j| {
                let rows = self.documents_with(j);
                let unlinked = (m.term_count[j] as usize).saturating_sub(rows.len());
                BarItem { center: (j + 1) as f64, width: 0.8, rows, unlinked }
            })
            .collect();
        let spec = BarChartSpec {
            x_label: "Term".to_owned(),
            y_label: "Occurrences".to_owned(),
            bars,
            x_ticks: Some((0..shown).map(|j| ((j + 1) as f64, m.labels[j].clone())).collect()),
            curve: None,
            references: Vec::new(),
            x_time: None,
        };
        section(
            SectionKind::WordFrequencyChart,
            &format!("Word Frequencies ({shown} most frequent terms)"),
            SectionContent::Bars(spec),
        )
    }

    fn cloud(&self) -> ReportSection {
        let m = &self.matrix;
        let shown = self.request.cloud_words.clamp(1, 300).min(m.n_terms());
        let words =
            (0..shown).map(|j| (m.labels[j].clone(), m.term_count[j] as usize, self.documents_with(j))).collect();
        let spec = WordCloudSpec::layout("Occurrences", words);
        section(
            SectionKind::WordCloud,
            &format!("Word Cloud ({shown} most frequent terms; size shows the count)"),
            SectionContent::WordCloud(spec),
        )
    }

    fn phrase_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Phrase", "Words", "Count", "Documents"]);
        for p in self.phrases.iter().take(100) {
            t.push([p.label.clone(), p.length.to_string(), p.count.to_string(), p.documents.to_string()]);
        }
        let title = format!(
            "Phrases (2 to {} words, seen at least {} times)",
            self.request.phrase_length,
            self.request.min_phrase_count.max(1)
        );
        if t.rows.is_empty() {
            return section(SectionKind::Phrases, &title, SectionContent::Note("No phrase occurs that often.".into()));
        }
        section(SectionKind::Phrases, &title, SectionContent::Table(t))
    }

    fn dimension_table(&self, s: &SemanticSpace) -> ReportSection {
        let mut t = TextTable::new(["Dimension", "Singular value", "Share", "Cumulative share"]);
        let mut cumulative = 0.0;
        for (i, (sv, share)) in s.singular_values.iter().zip(s.shares()).enumerate() {
            cumulative += share;
            t.push([
                (i + 1).to_string(),
                format_number(*sv, 6),
                format!("{:.2}%", 100.0 * share),
                format!("{:.2}%", 100.0 * cumulative),
            ]);
        }
        let method = if s.randomized { "randomized truncated SVD" } else { "SVD" };
        section(
            SectionKind::SemanticDimensions,
            &format!("Semantic Dimensions ({} matrix, {method})", self.request.weighting.label()),
            SectionContent::Table(t),
        )
    }

    fn loading_table(&self, s: &SemanticSpace) -> ReportSection {
        let mut t = TextTable::new(["Dimension", "Highest loadings", "Lowest loadings"]);
        let m = &self.matrix;
        for i in 0..s.singular_values.len() {
            let mut order: Vec<usize> = (0..m.n_terms()).collect();
            order.sort_by(|&a, &b| s.term_loadings[b][i].total_cmp(&s.term_loadings[a][i]));
            let list = |it: &mut dyn Iterator<Item = &usize>| {
                it.take(8)
                    .map(|&j| format!("{} ({})", m.labels[j], format_number(s.term_loadings[j][i], 3)))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            t.push([(i + 1).to_string(), list(&mut order.iter()), list(&mut order.iter().rev())]);
        }
        section(SectionKind::SemanticTerms, "Terms by Dimension", SectionContent::Table(t))
    }

    fn document_map(&self, s: &SemanticSpace) -> ReportSection {
        let points: Vec<RowPoint> = s
            .document_scores
            .iter()
            .zip(&self.corpus.rows)
            .map(|(p, &row)| RowPoint { row, x: p[0], y: p[1] })
            .collect();
        let groups = self.clusters.as_ref().map(|c| {
            let k = c.iter().max().map_or(0, |m| m + 1);
            PointGroups {
                title: "Cluster".to_owned(),
                labels: (1..=k).map(|i| format!("Cluster {i}")).collect(),
                membership: c.clone(),
                lines: vec![Vec::new(); k],
            }
        });
        // The ten terms farthest from the origin, scaled into the documents'
        // range so both read on one map.
        let m = &self.matrix;
        let reach = |rows: &mut dyn Iterator<Item = [f64; 2]>| rows.map(|p| p[0].hypot(p[1])).fold(0.0_f64, f64::max);
        let doc_reach = reach(&mut s.document_scores.iter().map(|p| [p[0], p[1]]));
        let term_xy: Vec<[f64; 2]> =
            s.term_loadings.iter().map(|v| [v[0] * s.singular_values[0], v[1] * s.singular_values[1]]).collect();
        let term_reach = reach(&mut term_xy.iter().copied());
        let scale = if term_reach > 0.0 { 0.9 * doc_reach / term_reach } else { 1.0 };
        let mut order: Vec<usize> = (0..m.n_terms()).collect();
        order.sort_by(|&a, &b| term_xy[b][0].hypot(term_xy[b][1]).total_cmp(&term_xy[a][0].hypot(term_xy[a][1])));
        let items = order
            .iter()
            .take(10)
            .map(|&j| (term_xy[j][0] * scale, term_xy[j][1] * scale, m.labels[j].clone(), 0))
            .collect();
        let plot = PlotSpec {
            x_label: "Semantic dimension 1".to_owned(),
            y_label: "Semantic dimension 2".to_owned(),
            points,
            point_name: Some("Documents".to_owned()),
            groups,
            annotations: Some(Annotations { groups: vec!["Terms (scaled)".to_owned()], items, labels: Vec::new() }),
            ..PlotSpec::default()
        };
        section(SectionKind::DocumentMap, "Document Map", SectionContent::Plot(Box::new(plot)))
    }

    fn cluster_table(&self, clusters: &[usize]) -> ReportSection {
        let k = clusters.iter().max().map_or(0, |m| m + 1);
        let n_terms = self.matrix.n_terms();
        let mut t = TextTable::new(["Cluster", "Documents", "Share", "Most characteristic terms"]);
        for c in 0..k {
            let members: Vec<&SparseRow> =
                self.weighted.iter().zip(clusters).filter(|(_, g)| **g == c).map(|(r, _)| r).collect();
            let sums =
                DocumentTermMatrix::column_sums(&members.iter().map(|r| (*r).clone()).collect::<Vec<_>>(), n_terms);
            let mut order: Vec<usize> = (0..n_terms).filter(|&j| sums[j] > 0.0).collect();
            order.sort_by(|&a, &b| sums[b].total_cmp(&sums[a]));
            let terms = order.iter().take(8).map(|&j| self.matrix.labels[j].clone()).collect::<Vec<_>>().join(", ");
            t.push([
                (c + 1).to_string(),
                members.len().to_string(),
                format!("{:.1}%", 100.0 * members.len() as f64 / clusters.len() as f64),
                terms,
            ]);
        }
        section(
            SectionKind::DocumentClusters,
            &format!("Document Clusters (k-means on the {} semantic dimensions)", self.request.dimensions),
            SectionContent::Table(t),
        )
    }
}
