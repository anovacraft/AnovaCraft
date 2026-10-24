//! Word clouds: the most frequent terms drawn with type sized by frequency,
//! packed around a centre without overlapping.
//!
//! # Layout
//! Words are placed from the most frequent down. A word's type size grows
//! with the square root of its count between [`MIN_SIZE`] and [`MAX_SIZE`]
//! points, so the *area* of the type follows the count (as for proportional
//! symbols; Cleveland & McGill 1984 on area judgements). Each word starts at
//! the centre and walks out along an Archimedean spiral `r = a·θ`, stretched
//! horizontally to suit a landscape frame, until its box (width estimated
//! from its characters at the type size, height 1.2 × the size, plus a small
//! margin) overlaps no word already placed (Feinberg's spiral placement, as
//! described by Viégas, Wattenberg & Feinberg 2009, "Participatory
//! visualization with Wordle", *IEEE TVCG* 15(6)). Every word is horizontal.
//! The layout is deterministic. Size is the only encoding of frequency; the
//! report's term table lists the counts, and hovering a word names its
//! count.

use stat_viz::export::{ChartPanel, Label};
use stat_viz::{Mark, Rgba};

/// Smallest type size, in points.
pub const MIN_SIZE: f64 = 10.0;
/// Largest type size, in points.
pub const MAX_SIZE: f64 = 44.0;
/// Average advance of a character, as a fraction of the type size (a little
/// wider than the bundled text face's, so estimated boxes contain the words).
pub const CHAR_WIDTH: f64 = 0.6;
/// Margin around each word's box, in points.
const MARGIN: f64 = 2.0;

/// One placed word.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudWord {
    /// The word or phrase shown.
    pub text: String,
    /// Its count (occurrences).
    pub count: usize,
    /// Documents (dataset rows) containing it, for selection.
    pub rows: Vec<usize>,
    /// Type size in points.
    pub size: f64,
    /// Centre of the word in layout points (x right, y down), the cloud
    /// centred near the origin.
    pub center: [f64; 2],
}

impl CloudWord {
    /// Estimated width of the word's box (without margin).
    #[must_use]
    pub fn width(&self) -> f64 {
        estimated_width(&self.text, self.size)
    }

    /// Height of the word's box (without margin).
    #[must_use]
    pub fn height(&self) -> f64 {
        1.2 * self.size
    }
}

/// A laid-out word cloud.
#[derive(Debug, Clone, PartialEq)]
pub struct WordCloudSpec {
    /// What the counts are, e.g. `"Occurrences"`, for hover text.
    pub measure: String,
    /// The words, most frequent first.
    pub words: Vec<CloudWord>,
    /// Bounding box `[x_min, y_min, x_max, y_max]` of every word's box.
    pub bounds: [f64; 4],
}

/// Width of `text` at `size` points, estimated from its characters.
#[must_use]
pub fn estimated_width(text: &str, size: f64) -> f64 {
    text.chars().count() as f64 * CHAR_WIDTH * size
}

fn overlaps(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

impl WordCloudSpec {
    /// Lay out `words` (`text`, `count`, rows), most frequent first after
    /// sorting. Words with a zero count are left out.
    #[must_use]
    pub fn layout(measure: &str, words: Vec<(String, usize, Vec<usize>)>) -> Self {
        let mut words: Vec<(String, usize, Vec<usize>)> = words.into_iter().filter(|w| w.1 > 0).collect();
        words.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let (lo, hi) = match (words.last(), words.first()) {
            (Some(l), Some(h)) => (l.1 as f64, h.1 as f64),
            _ => (0.0, 0.0),
        };
        let size_of = |c: usize| {
            if hi > lo {
                MIN_SIZE + (MAX_SIZE - MIN_SIZE) * ((c as f64 - lo) / (hi - lo)).sqrt()
            } else {
                0.6 * MAX_SIZE
            }
        };
        let mut boxes: Vec<[f64; 4]> = Vec::new();
        let mut placed = Vec::new();
        for (text, count, rows) in words {
            let size = size_of(count);
            let (w, h) = (estimated_width(&text, size) + 2.0 * MARGIN, 1.2 * size + 2.0 * MARGIN);
            let mut theta = 0.0_f64;
            let center = loop {
                let r = 1.5 * theta;
                let c = [1.6 * r * theta.cos(), r * theta.sin()];
                let b = [c[0] - w / 2.0, c[1] - h / 2.0, c[0] + w / 2.0, c[1] + h / 2.0];
                if !boxes.iter().any(|o| overlaps(o, &b)) {
                    boxes.push(b);
                    break c;
                }
                // Steps of about 2 points along the spiral.
                theta += (2.0 / r.max(2.0)).min(0.5);
            };
            placed.push(CloudWord { text, count, rows, size, center });
        }
        let mut bounds = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for b in &boxes {
            bounds = [bounds[0].min(b[0]), bounds[1].min(b[1]), bounds[2].max(b[2]), bounds[3].max(b[3])];
        }
        if boxes.is_empty() {
            bounds = [0.0; 4];
        }
        Self { measure: measure.to_owned(), words: placed, bounds }
    }

    /// The words as a table: word, count and documents.
    #[must_use]
    pub fn table(&self) -> crate::report::TextTable {
        let mut t = crate::report::TextTable::new(["Word", self.measure.as_str(), "Documents"]);
        for w in &self.words {
            t.push([w.text.clone(), w.count.to_string(), w.rows.len().to_string()]);
        }
        t
    }

    /// Width and height of the bounding box.
    #[must_use]
    pub fn extent(&self) -> (f64, f64) {
        (self.bounds[2] - self.bounds[0], self.bounds[3] - self.bounds[1])
    }

    /// The cloud as a figure panel for vector export: each word a label at
    /// its place (the vertical axis flipped so the page reads as the
    /// screen), with no axes. The most frequent tenth of the words are set in
    /// bold, in the accent ink.
    #[must_use]
    pub fn chart_panel(&self) -> ChartPanel {
        let mut c = ChartPanel { bare: true, ..ChartPanel::default() };
        let accent = Rgba::rgb(37, 99, 235);
        let ink = Rgba::rgb(30, 41, 59);
        let strong = self.words.len().div_ceil(10);
        for (i, w) in self.words.iter().enumerate() {
            c.labels.push(Label {
                at: [w.center[0], -w.center[1]],
                text: w.text.clone(),
                size: w.size,
                color: if i < strong { accent } else { ink },
                align: 0.5,
                valign: 0.5,
                bold: i < strong,
            });
        }
        // Invisible corners keep the frame around every word.
        let [x0, y0, x1, y1] = self.bounds;
        c.marks.push(Mark::Polygon {
            points: vec![[x0, -y1], [x1, -y1], [x1, -y0], [x0, -y0]],
            fill: Rgba([0, 0, 0, 0]),
            stroke: Rgba([0, 0, 0, 0]),
            name: String::new(),
        });
        c.x_range = (x0, x1);
        c.y_range = (-y1, -y0);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_do_not_overlap_and_sizes_follow_counts() {
        let words: Vec<(String, usize, Vec<usize>)> =
            (1..=60).map(|i| (format!("word{i}"), 61 - i, vec![i])).chain([("zero".into(), 0, vec![])]).collect();
        let c = WordCloudSpec::layout("Occurrences", words);
        assert_eq!(c.words.len(), 60);
        assert_eq!(c.words[0].text, "word1");
        assert!((c.words[0].size - MAX_SIZE).abs() < 1e-12 && (c.words[59].size - MIN_SIZE).abs() < 1e-12);
        assert!(c.words.windows(2).all(|w| w[0].size >= w[1].size));
        let boxes: Vec<[f64; 4]> = c
            .words
            .iter()
            .map(|w| {
                let (hw, hh) = (w.width() / 2.0, w.height() / 2.0);
                [w.center[0] - hw, w.center[1] - hh, w.center[0] + hw, w.center[1] + hh]
            })
            .collect();
        for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                assert!(!overlaps(&boxes[i], &boxes[j]), "{i} and {j} overlap");
            }
            let b = boxes[i];
            assert!(b[0] >= c.bounds[0] && b[2] <= c.bounds[2] && b[1] >= c.bounds[1] && b[3] <= c.bounds[3]);
        }
        assert_eq!(c.words[0].center, [0.0, 0.0]);
        let (w, h) = c.extent();
        assert!(w > h, "landscape");
        let p = c.chart_panel();
        assert_eq!(p.labels.len(), 60);
        assert!(p.labels[0].bold && !p.labels[59].bold);
    }

    #[test]
    fn single_and_empty() {
        let c = WordCloudSpec::layout("Occurrences", vec![("only".into(), 3, vec![0])]);
        assert_eq!(c.words.len(), 1);
        assert!(WordCloudSpec::layout("Occurrences", Vec::new()).words.is_empty());
    }
}
