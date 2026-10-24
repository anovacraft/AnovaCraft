//! Cause-and-effect (fishbone) diagram drawn from columns of causes and
//! sub-causes (Ishikawa, *Guide to Quality Control*, 1976, chapter 3).
//!
//! Each row is one cause path: its value in the first column names a main
//! branch (a category of causes), the second column a cause on that branch
//! and the third, if given, a sub-cause of that cause. Repeated paths are
//! merged (and counted); an empty cell ends the path there. Branches, causes
//! and sub-causes keep the order in which they first appear, so the table
//! controls the drawing. Every label carries the rows behind it, so clicking
//! it selects them in every linked view.
//!
//! [`CauseEffectDiagram::layout`] places everything in page points, shared by
//! the screen and by figure export: a horizontal spine ending in the effect,
//! main branches slanting off it alternately above and below, each cause on
//! a short horizontal rib off its branch and each sub-cause indented under
//! its cause.

use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};

/// What to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CauseEffectRequest {
    /// One to three columns: main branches, causes and sub-causes.
    pub causes: Vec<usize>,
    /// The effect (problem) written at the head of the diagram.
    pub effect: String,
}

/// One branch, cause or sub-cause of a [`CauseEffectDiagram`].
#[derive(Debug, Clone, PartialEq)]
pub struct CauseNode {
    /// Text.
    pub label: String,
    /// Dataset rows whose path passes through the node.
    pub rows: Vec<usize>,
    /// Causes of a branch, or sub-causes of a cause.
    pub children: Vec<CauseNode>,
}

/// A cause-and-effect diagram for display.
#[derive(Debug, Clone, PartialEq)]
pub struct CauseEffectDiagram {
    /// The effect at the head.
    pub effect: String,
    /// Main branches.
    pub branches: Vec<CauseNode>,
    /// Names of the columns the levels come from.
    pub levels: Vec<String>,
}

/// A line of a [`FishboneLayout`].
#[derive(Debug, Clone, PartialEq)]
pub struct FishboneLine {
    /// Vertices in page points (y up).
    pub points: Vec<[f64; 2]>,
    /// Width in points.
    pub width: f64,
}

/// A piece of text of a [`FishboneLayout`].
#[derive(Debug, Clone, PartialEq)]
pub struct FishboneLabel {
    /// Anchor in page points (y up).
    pub at: [f64; 2],
    /// Text.
    pub text: String,
    /// Size in points.
    pub size: f64,
    /// Horizontal alignment: 0 starts at the anchor, 0.5 centres, 1 ends there.
    pub align: f64,
    /// Vertical alignment: 0 hangs below the anchor, 0.5 centres, 1 sits on it.
    pub valign: f64,
    /// Bold weight.
    pub bold: bool,
    /// Path of child indices to the node from the branches (empty for the effect).
    pub node: Vec<usize>,
}

/// Where everything of a fishbone diagram is drawn, in page points with the
/// origin at the bottom left and y up.
#[derive(Debug, Clone, PartialEq)]
pub struct FishboneLayout {
    /// Page width.
    pub width: f64,
    /// Page height.
    pub height: f64,
    /// Spine, branches, ribs and connectors.
    pub lines: Vec<FishboneLine>,
    /// Arrowhead at the end of the spine (a closed triangle).
    pub arrow: Vec<[f64; 2]>,
    /// The effect's box `[x0, y0, x1, y1]`.
    pub head: [f64; 4],
    /// Text, the effect first.
    pub labels: Vec<FishboneLabel>,
}

/// Text sizes in points.
const EFFECT_SIZE: f64 = 11.0;
const BRANCH_SIZE: f64 = 10.0;
const CAUSE_SIZE: f64 = 9.0;
const SUB_SIZE: f64 = 8.0;
/// Height of one line of causes along a branch (a cause's label beside its
/// rib and a sub-cause's label centred on its line both fit in it).
const LINE: f64 = 18.0;
/// Height of a line of text, as a multiple of its size.
pub const LINE_HEIGHT: f64 = 1.25;
/// Horizontal run of a branch per point of rise.
const SLOPE: f64 = 0.5;
/// Indent of sub-causes under their cause.
const SUB_INDENT: f64 = 14.0;
/// Margin around the drawing.
const MARGIN: f64 = 12.0;

/// Estimated width of `text` at `size` points (an average glyph is a little
/// over half the size wide).
#[must_use]
pub fn text_width(text: &str, size: f64) -> f64 {
    text.chars().count() as f64 * size * 0.56
}

impl CauseEffectDiagram {
    /// The node at `path` (child indices from the branches).
    #[must_use]
    pub fn node(&self, path: &[usize]) -> Option<&CauseNode> {
        let (first, rest) = path.split_first()?;
        let mut node = self.branches.get(*first)?;
        for &i in rest {
            node = node.children.get(i)?;
        }
        Some(node)
    }

    /// Place the diagram (see the module documentation).
    #[must_use]
    pub fn layout(&self) -> FishboneLayout {
        // Each branch: its entries (cause or sub-cause, with their paths) from
        // the tip toward the spine, the rib length and the height.
        struct Branch {
            entries: Vec<(Vec<usize>, bool)>,
            rib: f64,
            height: f64,
            reach: f64,
        }
        let branches: Vec<Branch> = self
            .branches
            .iter()
            .enumerate()
            .map(|(b, branch)| {
                let mut entries = Vec::new();
                let (mut cause_w, mut sub_w) = (0.0_f64, 0.0_f64);
                for (c, cause) in branch.children.iter().enumerate() {
                    entries.push((vec![b, c], false));
                    cause_w = cause_w.max(text_width(&cause.label, CAUSE_SIZE));
                    for (s, sub) in cause.children.iter().enumerate() {
                        entries.push((vec![b, c, s], true));
                        sub_w = sub_w.max(text_width(&sub.label, SUB_SIZE));
                    }
                }
                let rib = (cause_w + 12.0).max(sub_w + SUB_INDENT + 10.0).max(48.0);
                let height = (entries.len() as f64 + 1.0) * LINE + 10.0;
                // How far left of its root the branch reaches: its ribs at the
                // tip, or its label.
                let tip = height * SLOPE;
                let label_half = text_width(&branch.label, BRANCH_SIZE) / 2.0;
                let reach = if entries.is_empty() { tip + label_half } else { (tip + rib).max(tip + label_half) };
                Branch { entries, rib, height, reach }
            })
            .collect();
        // Branch k sits in column k / 2, above the spine when k is even.
        let columns = branches.len().div_ceil(2);
        let mut roots = Vec::with_capacity(columns);
        let mut x = MARGIN;
        for c in 0..columns {
            let reach = branches[2 * c..(2 * c + 2).min(branches.len())].iter().map(|b| b.reach).fold(0.0, f64::max);
            x += reach + if c == 0 { 0.0 } else { 18.0 };
            roots.push(x);
        }
        let above = branches.iter().step_by(2).map(|b| b.height).fold(LINE, f64::max) + 2.0 * BRANCH_SIZE;
        let below = branches.iter().skip(1).step_by(2).map(|b| b.height).fold(LINE, f64::max) + 2.0 * BRANCH_SIZE;
        let spine_y = MARGIN + below;
        let head_x = roots.last().copied().unwrap_or(MARGIN) + 40.0;
        let head_w = text_width(&self.effect, EFFECT_SIZE) + 20.0;
        let head_h = 32.0;
        let width = head_x + head_w + MARGIN;
        let height = spine_y + above + MARGIN;
        let mut lines = vec![FishboneLine { points: vec![[MARGIN, spine_y], [head_x - 9.0, spine_y]], width: 2.4 }];
        let arrow = vec![[head_x - 10.0, spine_y - 5.0], [head_x, spine_y], [head_x - 10.0, spine_y + 5.0]];
        let head = [head_x, spine_y - head_h / 2.0, head_x + head_w, spine_y + head_h / 2.0];
        let mut labels = vec![FishboneLabel {
            at: [head_x + head_w / 2.0, spine_y],
            text: self.effect.clone(),
            size: EFFECT_SIZE,
            align: 0.5,
            valign: 0.5,
            bold: true,
            node: Vec::new(),
        }];
        for (k, (branch, node)) in branches.iter().zip(&self.branches).enumerate() {
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            let root = roots[k / 2];
            let bone_x = |rise: f64| root - rise * SLOPE;
            let tip = [bone_x(branch.height), spine_y + side * branch.height];
            lines.push(FishboneLine { points: vec![[root, spine_y], tip], width: 1.6 });
            labels.push(FishboneLabel {
                at: [tip[0], tip[1] + side * 3.0],
                text: node.label.clone(),
                size: BRANCH_SIZE,
                align: 0.5,
                valign: if side > 0.0 { 1.0 } else { 0.0 },
                bold: true,
                node: vec![k],
            });
            let mut cause_start = [0.0, 0.0];
            for (j, (path, sub)) in branch.entries.iter().enumerate() {
                let rise = branch.height - (j as f64 + 1.0) * LINE;
                let y = spine_y + side * rise;
                let Some(n) = self.node(path) else { continue };
                if *sub {
                    let x = cause_start[0] + 6.0;
                    lines.push(FishboneLine { points: vec![[x, cause_start[1]], [x, y], [x + 6.0, y]], width: 0.7 });
                    labels.push(FishboneLabel {
                        at: [cause_start[0] + SUB_INDENT, y],
                        text: n.label.clone(),
                        size: SUB_SIZE,
                        align: 0.0,
                        valign: 0.5,
                        bold: false,
                        node: path.clone(),
                    });
                } else {
                    let end = bone_x(rise);
                    cause_start = [end - branch.rib, y];
                    lines.push(FishboneLine { points: vec![cause_start, [end, y]], width: 1.0 });
                    // The label on the side of its rib away from the spine, so
                    // the sub-causes below it (toward the spine) keep clear.
                    labels.push(FishboneLabel {
                        at: [cause_start[0], y + side * 1.5],
                        text: n.label.clone(),
                        size: CAUSE_SIZE,
                        align: 0.0,
                        valign: if side > 0.0 { 1.0 } else { 0.0 },
                        bold: false,
                        node: path.clone(),
                    });
                }
            }
        }
        FishboneLayout { width, height, lines, arrow, head, labels }
    }

    /// The diagram as an outline table: branch, cause, sub-cause and the
    /// number of rows behind each.
    #[must_use]
    pub fn table(&self) -> TextTable {
        let mut columns: Vec<String> = self.levels.clone();
        columns.push("Rows".to_owned());
        let mut t = TextTable::new(columns.clone());
        let width = columns.len() - 1;
        let mut push = |cells: Vec<&str>, n: usize| {
            let mut row: Vec<String> = cells.iter().map(|s| (*s).to_owned()).collect();
            row.resize(width, String::new());
            row.push(n.to_string());
            t.push(row);
        };
        for b in &self.branches {
            push(vec![&b.label], b.rows.len());
            for c in &b.children {
                push(vec![&b.label, &c.label], c.rows.len());
                for s in &c.children {
                    push(vec![&b.label, &c.label, &s.label], s.rows.len());
                }
            }
        }
        t
    }
}

/// A computed cause-and-effect diagram.
#[derive(Debug, Clone)]
pub struct CauseEffectAnalysis {
    request: CauseEffectRequest,
    diagram: CauseEffectDiagram,
    n_excluded: usize,
    n_missing: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

impl CauseEffectAnalysis {
    /// Gather the cause paths of the rows that are not excluded and have a
    /// main branch.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] without one to three distinct columns,
    /// for a missing column, or when no row names a main branch.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &CauseEffectRequest) -> Result<Self> {
        if request.causes.is_empty() || request.causes.len() > 3 {
            return Err(invalid("a cause-and-effect diagram takes one to three cause columns"));
        }
        let mut seen = request.causes.clone();
        seen.sort_unstable();
        seen.dedup();
        if seen.len() != request.causes.len() {
            return Err(invalid("the cause columns must differ"));
        }
        let columns = request
            .causes
            .iter()
            .map(|&i| table.column(i).ok_or_else(|| invalid(format!("no column {i}"))))
            .collect::<Result<Vec<_>>>()?;
        let mut branches: Vec<CauseNode> = Vec::new();
        let (mut n_excluded, mut n_missing) = (0, 0);
        for r in 0..table.n_rows() {
            let path: Vec<String> =
                columns.iter().map(|c| c.level_text(r).trim().to_owned()).take_while(|s| !s.is_empty()).collect();
            if path.is_empty() {
                n_missing += 1;
                continue;
            }
            if states.is_excluded(r) {
                n_excluded += 1;
                continue;
            }
            let mut level = &mut branches;
            for label in path {
                let k = match level.iter().position(|n| n.label == label) {
                    Some(k) => k,
                    None => {
                        level.push(CauseNode { label, rows: Vec::new(), children: Vec::new() });
                        level.len() - 1
                    }
                };
                level[k].rows.push(r);
                level = &mut level[k].children;
            }
        }
        if branches.is_empty() {
            return Err(invalid("no rows name a main branch of causes"));
        }
        let effect =
            if request.effect.trim().is_empty() { "Effect".to_owned() } else { request.effect.trim().to_owned() };
        let levels = columns.iter().map(|c| c.name.clone()).collect();
        Ok(Self {
            request: request.clone(),
            diagram: CauseEffectDiagram { effect, branches, levels },
            n_excluded,
            n_missing,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &CauseEffectRequest {
        &self.request
    }

    /// The diagram.
    #[must_use]
    pub fn diagram(&self) -> &CauseEffectDiagram {
        &self.diagram
    }

    /// Report title, e.g. `"Cause and Effect: Late deliveries"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Cause and Effect: {}", self.diagram.effect)
    }

    /// Report sections: the diagram and its outline.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut outline = self.diagram.table();
        let mut notes = Vec::new();
        if self.n_excluded > 0 {
            notes.push(format!("{} excluded rows left out", self.n_excluded));
        }
        if self.n_missing > 0 {
            notes.push(format!("{} rows without a main branch", self.n_missing));
        }
        if !notes.is_empty() {
            let mut row = vec![String::new(); outline.columns.len()];
            row[0] = format!("({})", notes.join("; "));
            outline.push(row);
        }
        vec![
            ReportSection {
                kind: SectionKind::CauseEffectDiagram,
                title: "Cause-and-Effect Diagram".to_owned(),
                content: SectionContent::CauseEffect(self.diagram.clone()),
            },
            ReportSection {
                kind: SectionKind::CauseEffectOutline,
                title: "Causes".to_owned(),
                content: SectionContent::Table(outline),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn causes() -> DataTable {
        let text = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        DataTable::new(
            "causes",
            vec![
                Column::text("Category", text(&["Machine", "Machine", "Method", "Machine", "Material", ""])),
                Column::text("Cause", text(&["Nozzle wear", "Belt speed", "Profile", "Nozzle wear", "", "x"])),
                Column::text("Sub-cause", text(&["Clogging", "", "Too short", "Clogging", "", ""])),
            ],
        )
        .unwrap()
    }

    #[test]
    fn rows_merge_into_a_tree_in_order_of_appearance() {
        let t = causes();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([2], true);
        let a = CauseEffectAnalysis::run(
            &t,
            &rows,
            &CauseEffectRequest { causes: vec![0, 1, 2], effect: " Voids ".into() },
        )
        .unwrap();
        let d = a.diagram();
        assert_eq!(d.effect, "Voids");
        let names: Vec<&str> = d.branches.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(names, ["Machine", "Material"], "the excluded Method row is left out");
        assert_eq!(d.branches[0].rows, [0, 1, 3]);
        let causes: Vec<&str> = d.branches[0].children.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(causes, ["Nozzle wear", "Belt speed"]);
        assert_eq!(d.node(&[0, 0, 0]).map(|n| n.rows.clone()), Some(vec![0, 3]));
        assert!(d.branches[1].children.is_empty(), "an empty cause ends the path");
        let outline = d.table();
        assert_eq!(outline.columns, ["Category", "Cause", "Sub-cause", "Rows"]);
        assert_eq!(outline.rows[2], ["Machine", "Nozzle wear", "Clogging", "2"]);
        let s = a.sections();
        let SectionContent::Table(t) = &s[1].content else { panic!() };
        assert!(t.rows.last().unwrap()[0].contains("1 excluded rows left out; 1 rows without a main branch"));
        assert_eq!(a.title(), "Cause and Effect: Voids");
    }

    #[test]
    fn layout_places_every_label_inside_the_page_without_overlap() {
        let t = causes();
        let rows = RowStateManager::new(t.n_rows());
        let a =
            CauseEffectAnalysis::run(&t, &rows, &CauseEffectRequest { causes: vec![0, 1, 2], effect: String::new() })
                .unwrap();
        let l = a.diagram().layout();
        // Effect, three branches, three causes and two sub-causes.
        assert_eq!(l.labels.len(), 1 + 3 + 3 + 2);
        assert_eq!(l.labels[0].text, "Effect");
        // Branches alternate above and below the spine.
        let spine = l.lines[0].points[0][1];
        let tips: Vec<f64> = l.labels.iter().filter(|x| x.node.len() == 1).map(|x| x.at[1] - spine).collect();
        assert!(tips[0] > 0.0 && tips[1] < 0.0 && tips[2] > 0.0, "{tips:?}");
        // Text boxes stay on the page and do not overlap.
        let boxes: Vec<[f64; 4]> = l
            .labels
            .iter()
            .map(|x| {
                let (w, h) = (text_width(&x.text, x.size), LINE_HEIGHT * x.size);
                let x0 = x.at[0] - x.align * w;
                let y0 = x.at[1] - x.valign * h;
                [x0, y0, x0 + w, y0 + h]
            })
            .collect();
        for b in &boxes {
            assert!(b[0] >= 0.0 && b[1] >= 0.0 && b[2] <= l.width && b[3] <= l.height, "{b:?} in {l:?}");
        }
        for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                let (a, b) = (boxes[i], boxes[j]);
                let overlap = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0) * (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
                assert!(overlap < 1e-9, "{} / {}", l.labels[i].text, l.labels[j].text);
            }
        }
    }

    #[test]
    fn bad_requests_are_refused() {
        let t = causes();
        let rows = RowStateManager::new(t.n_rows());
        let run = |causes: Vec<usize>| {
            CauseEffectAnalysis::run(&t, &rows, &CauseEffectRequest { causes, effect: "e".into() })
        };
        assert!(run(vec![]).is_err());
        assert!(run(vec![0, 0]).is_err());
        assert!(run(vec![0, 1, 2, 1]).is_err());
        assert!(run(vec![7]).is_err());
    }
}
