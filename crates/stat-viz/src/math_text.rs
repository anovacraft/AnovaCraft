//! Math in labels: a small markup for titles, axis titles, key entries and
//! text labels, parsed once into a layout tree and set the same way on
//! screen (egui) and in SVG, PDF, EPS and PNG figures.
//!
//! The markup (documented for users in the Reference Manual, "Math in
//! labels"):
//!
//! | Written | Shown |
//! |---|---|
//! | `x_1`, `x_{ij}` | subscript (one character, or a `{…}` group) |
//! | `x^2`, `e^{-t}` | superscript |
//! | `x_i^2` | subscript and superscript stacked at the same place |
//! | `\alpha`, `\mu`, `\Delta`, … | Greek letters by name (lower case, and capitals by a capital first letter) |
//! | `\pm`, `\times`, `\cdot`, `\le`, `\ge`, `\ne`, `\approx`, `\infty`, `\partial`, `\deg` | a few symbols |
//! | `\frac{a}{b}` | a fraction stacked over a rule |
//! | `\sqrt{x}` | a square root: a radical sign as tall as `x` with a bar over it |
//! | `\sum`, `\prod` | Σ and ∏ |
//! | `$…$` | a math span: inside it `a/b` is also a stacked fraction (outside, `/` is a plain slash) |
//! | `\_`, `\^`, `\{`, `\}`, `\$`, `\/`, `\\` | the character itself |
//!
//! Text with none of these characters is a single plain run, drawn exactly
//! as before the markup existed. An unknown command (`\foo`), a `_` or `^`
//! with nothing after it, and an unmatched `$` are kept as typed.
//!
//! Layout follows the conventions of mathematical typesetting (Knuth, *The
//! TeXbook*, 1984, Appendix G, in a much reduced form): scripts at 70% of the
//! size of their base, subscripts dropped by 0.22 em and superscripts raised
//! by 0.40 em; a fraction's numerator and denominator at 80% size, centred
//! over and under a rule on the math axis (0.27 em above the baseline), the
//! rule as wide as the wider of the two plus 0.1 em on each side; a root's
//! radical sign scaled to span its radicand, with a bar (the vinculum) 0.1 em
//! above the radicand.

/// One piece of a label.
#[derive(Debug, Clone, PartialEq)]
pub enum MathNode {
    /// Text set in the current size.
    Text(String),
    /// A subscript.
    Sub(Vec<MathNode>),
    /// A superscript.
    Sup(Vec<MathNode>),
    /// A fraction: numerator over denominator.
    Frac(Vec<MathNode>, Vec<MathNode>),
    /// A square root of its contents.
    Root(Vec<MathNode>),
}

/// A parsed label: a sequence of [`MathNode`]s.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MathText {
    /// The pieces, left to right.
    pub nodes: Vec<MathNode>,
}

/// A run of text placed by [`MathText::layout`].
#[derive(Debug, Clone, PartialEq)]
pub struct MathRun {
    /// The characters.
    pub text: String,
    /// Left edge, from the start of the label, in points.
    pub x: f64,
    /// Height of the run's baseline above the label's baseline, in points
    /// (negative below it).
    pub rise: f64,
    /// Font size in points.
    pub size: f64,
}

/// A horizontal rule placed by [`MathText::layout`] (a fraction bar).
#[derive(Debug, Clone, PartialEq)]
pub struct MathRule {
    /// Left end, from the start of the label.
    pub x0: f64,
    /// Right end.
    pub x1: f64,
    /// Height of the rule's centre above the baseline.
    pub rise: f64,
    /// Thickness in points.
    pub thickness: f64,
}

/// A label laid out in points: runs and rules relative to its start on the
/// baseline, with its extent.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MathLayout {
    /// Text runs.
    pub runs: Vec<MathRun>,
    /// Fraction rules.
    pub rules: Vec<MathRule>,
    /// Advance width.
    pub width: f64,
    /// Height above the baseline.
    pub ascent: f64,
    /// Depth below the baseline (positive).
    pub descent: f64,
}

/// Size of scripts relative to their base.
pub const SCRIPT_SCALE: f64 = 0.7;
/// Size of a fraction's parts relative to the surrounding text.
pub const FRACTION_SCALE: f64 = 0.8;
/// Subscript drop, in ems of the base size.
pub const SUB_DROP: f64 = 0.22;
/// Superscript raise, in ems of the base size.
pub const SUP_RISE: f64 = 0.40;
/// Height of the math axis (fraction rules), in ems.
pub const MATH_AXIS: f64 = 0.27;
/// Height of capitals above the baseline, in ems (for extents).
const ASCENT: f64 = 0.74;
/// Depth of descenders below the baseline, in ems.
const DESCENT: f64 = 0.22;
/// Height of the radical sign √ from its foot on the baseline, in ems (Inter,
/// JetBrains Mono: 0.73).
const RADICAL_HEIGHT: f64 = 0.73;

/// The Greek letters and symbols written as `\name`.
const COMMANDS: &[(&str, char)] = &[
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ε'),
    ("varepsilon", 'ε'),
    ("zeta", 'ζ'),
    ("eta", 'η'),
    ("theta", 'θ'),
    ("vartheta", 'ϑ'),
    ("iota", 'ι'),
    ("kappa", 'κ'),
    ("lambda", 'λ'),
    ("mu", 'μ'),
    ("nu", 'ν'),
    ("xi", 'ξ'),
    ("omicron", 'ο'),
    ("pi", 'π'),
    ("rho", 'ρ'),
    ("sigma", 'σ'),
    ("varsigma", 'ς'),
    ("tau", 'τ'),
    ("upsilon", 'υ'),
    ("phi", 'φ'),
    ("varphi", 'φ'),
    ("chi", 'χ'),
    ("psi", 'ψ'),
    ("omega", 'ω'),
    ("Alpha", 'Α'),
    ("Beta", 'Β'),
    ("Gamma", 'Γ'),
    ("Delta", 'Δ'),
    ("Epsilon", 'Ε'),
    ("Zeta", 'Ζ'),
    ("Eta", 'Η'),
    ("Theta", 'Θ'),
    ("Iota", 'Ι'),
    ("Kappa", 'Κ'),
    ("Lambda", 'Λ'),
    ("Mu", 'Μ'),
    ("Nu", 'Ν'),
    ("Xi", 'Ξ'),
    ("Omicron", 'Ο'),
    ("Pi", 'Π'),
    ("Rho", 'Ρ'),
    ("Sigma", 'Σ'),
    ("Tau", 'Τ'),
    ("Upsilon", 'Υ'),
    ("Phi", 'Φ'),
    ("Chi", 'Χ'),
    ("Psi", 'Ψ'),
    ("Omega", 'Ω'),
    ("pm", '±'),
    ("times", '×'),
    ("cdot", '·'),
    ("le", '≤'),
    ("ge", '≥'),
    ("ne", '≠'),
    ("approx", '≈'),
    ("infty", '∞'),
    ("partial", '∂'),
    ("deg", '°'),
    ("sum", 'Σ'),
    ("prod", '∏'),
];

/// The character `\name` stands for, if any.
#[must_use]
pub fn command_char(name: &str) -> Option<char> {
    COMMANDS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// Characters that a backslash makes literal.
const ESCAPABLE: &[char] = &['\\', '_', '^', '{', '}', '$', '/', ' '];

/// Recursive-descent parser over the characters of a label.
struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Parse nodes until `stop` (a closing `}` or `$`) or the end. In a
    /// math span, `a/b` makes a fraction of the atoms either side.
    fn sequence(&mut self, stop: Option<char>, math: bool) -> Vec<MathNode> {
        let mut out: Vec<MathNode> = Vec::new();
        // Atoms of the current math span, so `/` can take the one before it.
        while let Some(c) = self.peek() {
            if Some(c) == stop {
                break;
            }
            match c {
                '\\' => {
                    let node = self.command(math);
                    push(&mut out, node);
                }
                '_' | '^' => {
                    self.pos += 1;
                    match self.script_argument(math) {
                        Some(arg) => out.push(if c == '_' { MathNode::Sub(arg) } else { MathNode::Sup(arg) }),
                        None => push(&mut out, MathNode::Text(c.to_string())),
                    }
                }
                '{' => {
                    // A group: braces are not shown.
                    let start = self.pos;
                    self.pos += 1;
                    let inner = self.sequence(Some('}'), math);
                    if self.peek() == Some('}') {
                        self.pos += 1;
                        if math {
                            // Keep the group as one atom for a following `/`.
                            out.push(MathNode::Frac(inner, Vec::new()));
                        } else {
                            for n in inner {
                                push(&mut out, n);
                            }
                        }
                    } else {
                        // Unclosed: the brace is text.
                        self.pos = start + 1;
                        push(&mut out, MathNode::Text("{".into()));
                    }
                }
                '$' if !math => {
                    let start = self.pos;
                    self.pos += 1;
                    let inner = self.sequence(Some('$'), true);
                    if self.peek() == Some('$') {
                        self.pos += 1;
                        for n in resolve_groups(inner) {
                            push(&mut out, n);
                        }
                    } else {
                        self.pos = start + 1;
                        push(&mut out, MathNode::Text("$".into()));
                    }
                }
                '/' if math => {
                    self.pos += 1;
                    let numerator = take_atom(&mut out);
                    let denominator = self.math_atom();
                    match (numerator, denominator) {
                        (Some(n), Some(d)) => out.push(MathNode::Frac(n, d)),
                        (n, d) => {
                            // Nothing on one side: a plain slash.
                            for part in n.into_iter().flatten() {
                                push(&mut out, part);
                            }
                            push(&mut out, MathNode::Text("/".into()));
                            for part in d.into_iter().flatten() {
                                push(&mut out, part);
                            }
                        }
                    }
                }
                _ => {
                    self.pos += 1;
                    push(&mut out, MathNode::Text(c.to_string()));
                }
            }
        }
        out
    }

    /// `\…` at the cursor: an escaped character, a Greek letter or symbol,
    /// `\frac{a}{b}`, or (unknown) the text as typed.
    fn command(&mut self, math: bool) -> MathNode {
        self.pos += 1; // the backslash
        let Some(next) = self.peek() else { return MathNode::Text("\\".into()) };
        if ESCAPABLE.contains(&next) {
            self.pos += 1;
            return MathNode::Text(next.to_string());
        }
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        let name: String = self.chars[start..self.pos].iter().collect();
        if name == "frac" {
            let save = self.pos;
            if let (Some(a), Some(b)) = (self.group(math), self.group(math)) {
                return MathNode::Frac(a, b);
            }
            self.pos = save;
        }
        if name == "sqrt" {
            let save = self.pos;
            if let Some(a) = self.group(math) {
                return MathNode::Root(a);
            }
            self.pos = save;
        }
        // A space after a command is kept: labels are text, not formulas.
        match command_char(&name) {
            Some(c) => MathNode::Text(c.to_string()),
            None => MathNode::Text(format!("\\{name}")),
        }
    }

    /// A `{…}` group at the cursor (skipping spaces before it).
    fn group(&mut self, math: bool) -> Option<Vec<MathNode>> {
        while self.peek() == Some(' ') {
            self.pos += 1;
        }
        if self.peek() != Some('{') {
            return None;
        }
        let start = self.pos;
        self.pos += 1;
        let inner = self.sequence(Some('}'), math);
        if self.peek() == Some('}') {
            self.pos += 1;
            Some(resolve_groups(inner))
        } else {
            self.pos = start;
            None
        }
    }

    /// The argument of `_` or `^`: a group, a command, or one character.
    fn script_argument(&mut self, math: bool) -> Option<Vec<MathNode>> {
        match self.peek()? {
            '{' => {
                let save = self.pos;
                let g = self.group(math);
                if g.is_none() {
                    self.pos = save;
                }
                g
            }
            '\\' => Some(vec![self.command(math)]),
            ' ' | '}' | '$' => None,
            c => {
                self.pos += 1;
                Some(vec![MathNode::Text(c.to_string())])
            }
        }
    }

    /// The denominator after `/` in a math span: a group, or a run of
    /// letters and digits (with any scripts on it), or one command.
    fn math_atom(&mut self) -> Option<Vec<MathNode>> {
        match self.peek()? {
            '{' => self.group(true),
            '\\' => Some(vec![self.command(true)]),
            c if c.is_alphanumeric() || c == '.' => {
                let start = self.pos;
                while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '.') {
                    self.pos += 1;
                }
                let text: String = self.chars[start..self.pos].iter().collect();
                let mut atom = vec![MathNode::Text(text)];
                // Scripts directly on the denominator belong to it.
                while let Some(c @ ('_' | '^')) = self.peek() {
                    self.pos += 1;
                    match self.script_argument(true) {
                        Some(arg) => atom.push(if c == '_' { MathNode::Sub(arg) } else { MathNode::Sup(arg) }),
                        None => {
                            self.pos -= 1;
                            break;
                        }
                    }
                }
                Some(atom)
            }
            _ => None,
        }
    }
}

/// Append `node`, merging adjacent text.
fn push(out: &mut Vec<MathNode>, node: MathNode) {
    if let (Some(MathNode::Text(last)), MathNode::Text(t)) = (out.last_mut(), &node) {
        last.push_str(t);
        return;
    }
    out.push(node);
}

/// In a math span, a `{…}` group is kept as `Frac(inner, [])` until the
/// span ends so that `/` can take it whole; afterwards it is spliced back.
fn resolve_groups(nodes: Vec<MathNode>) -> Vec<MathNode> {
    let mut out = Vec::new();
    for n in nodes {
        match n {
            MathNode::Frac(inner, d) if d.is_empty() => {
                for m in resolve_groups(inner) {
                    push(&mut out, m);
                }
            }
            MathNode::Frac(a, b) => out.push(MathNode::Frac(resolve_groups(a), resolve_groups(b))),
            MathNode::Sub(a) => out.push(MathNode::Sub(resolve_groups(a))),
            MathNode::Sup(a) => out.push(MathNode::Sup(resolve_groups(a))),
            MathNode::Root(a) => out.push(MathNode::Root(resolve_groups(a))),
            t @ MathNode::Text(_) => push(&mut out, t),
        }
    }
    out
}

/// Remove and return the atom before a `/` in a math span: a group, or the
/// trailing run of letters and digits of the last text (with the scripts
/// after it).
fn take_atom(out: &mut Vec<MathNode>) -> Option<Vec<MathNode>> {
    // Scripts attached to the atom come with it.
    let mut scripts = Vec::new();
    while matches!(out.last(), Some(MathNode::Sub(_) | MathNode::Sup(_))) {
        scripts.push(out.pop()?);
    }
    scripts.reverse();
    let atom = match out.pop() {
        Some(MathNode::Frac(inner, d)) if d.is_empty() => Some(resolve_groups(inner)),
        Some(MathNode::Text(t)) => {
            let cut = t
                .char_indices()
                .rev()
                .take_while(|(_, c)| {
                    c.is_alphanumeric() || *c == '.' || "αβγδεζηθικλμνξοπρστυφχψωΔΣΩΓΛΦΨΘΠ".contains(*c)
                })
                .last()
                .map(|(i, _)| i);
            match cut {
                Some(i) => {
                    let (head, tail) = t.split_at(i);
                    if !head.is_empty() {
                        out.push(MathNode::Text(head.to_owned()));
                    }
                    Some(vec![MathNode::Text(tail.to_owned())])
                }
                None => {
                    out.push(MathNode::Text(t));
                    None
                }
            }
        }
        Some(other) => {
            out.push(other);
            None
        }
        None => None,
    };
    match atom {
        Some(mut a) => {
            a.extend(scripts);
            Some(a)
        }
        None => {
            out.extend(scripts);
            None
        }
    }
}

impl MathText {
    /// Parse `text` written in the markup (see the module documentation).
    /// Never fails: anything that is not markup is text.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut p = Parser { chars: text.chars().collect(), pos: 0 };
        let mut nodes = Vec::new();
        loop {
            for n in resolve_groups(p.sequence(None, false)) {
                push(&mut nodes, n);
            }
            // A stray `}` at the top level is text.
            match p.peek() {
                Some(c) => {
                    p.pos += 1;
                    push(&mut nodes, MathNode::Text(c.to_string()));
                }
                None => break,
            }
        }
        Self { nodes }
    }

    /// `text` as one plain run, with no markup read.
    #[must_use]
    pub fn literal(text: &str) -> Self {
        Self { nodes: if text.is_empty() { Vec::new() } else { vec![MathNode::Text(text.to_owned())] } }
    }

    /// `text` parsed when `markup` is set, else taken literally.
    #[must_use]
    pub fn new(text: &str, markup: bool) -> Self {
        if markup { Self::parse(text) } else { Self::literal(text) }
    }

    /// The text when the label is a single plain run (or empty): such labels
    /// are drawn exactly as plain text always was.
    #[must_use]
    pub fn as_plain(&self) -> Option<&str> {
        match self.nodes.as_slice() {
            [] => Some(""),
            [MathNode::Text(t)] => Some(t),
            _ => None,
        }
    }

    /// `true` when there is nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The label as plain text for places that cannot set scripts (window
    /// titles, screen readers, tab-separated tables): scripts follow `_` /
    /// `^` (in braces when longer than one character) and fractions read
    /// `a/b`.
    #[must_use]
    pub fn plain_text(&self) -> String {
        fn write(nodes: &[MathNode], out: &mut String) {
            for n in nodes {
                match n {
                    MathNode::Text(t) => out.push_str(t),
                    MathNode::Sub(a) | MathNode::Sup(a) => {
                        out.push(if matches!(n, MathNode::Sub(_)) { '_' } else { '^' });
                        let mut inner = String::new();
                        write(a, &mut inner);
                        if inner.chars().count() == 1 {
                            out.push_str(&inner);
                        } else {
                            out.push('{');
                            out.push_str(&inner);
                            out.push('}');
                        }
                    }
                    MathNode::Frac(a, b) => {
                        let (mut x, mut y) = (String::new(), String::new());
                        write(a, &mut x);
                        write(b, &mut y);
                        let wrap = |s: String| if s.chars().count() > 1 { format!("({s})") } else { s };
                        out.push_str(&format!("{}/{}", wrap(x), wrap(y)));
                    }
                    MathNode::Root(a) => {
                        let mut x = String::new();
                        write(a, &mut x);
                        if x.chars().count() > 1 {
                            out.push_str(&format!("√({x})"));
                        } else {
                            out.push('√');
                            out.push_str(&x);
                        }
                    }
                }
            }
        }
        let mut out = String::new();
        write(&self.nodes, &mut out);
        out
    }

    /// Lay the label out at `size` points, measuring text with `measure`
    /// (`measure(text, size)` is the advance width in points).
    #[must_use]
    pub fn layout(&self, size: f64, measure: &dyn Fn(&str, f64) -> f64) -> MathLayout {
        let mut out = MathLayout::default();
        let (width, ascent, descent) = lay(&self.nodes, size, 0.0, 0.0, measure, &mut out);
        out.width = width;
        out.ascent = ascent;
        out.descent = descent;
        out
    }
}

/// Lay `nodes` out from `x` with their baseline at `rise`, appending runs
/// and rules to `out`. Returns (advance, ascent above `rise` … as absolute
/// height above the label baseline, depth below it).
fn lay(
    nodes: &[MathNode],
    size: f64,
    x: f64,
    rise: f64,
    measure: &dyn Fn(&str, f64) -> f64,
    out: &mut MathLayout,
) -> (f64, f64, f64) {
    let mut cursor = x;
    let mut ascent = rise + ASCENT * size;
    let mut descent = -(rise - DESCENT * size);
    let mut i = 0;
    while i < nodes.len() {
        match &nodes[i] {
            MathNode::Text(t) => {
                if !t.is_empty() {
                    out.runs.push(MathRun { text: t.clone(), x: cursor, rise, size });
                    cursor += measure(t, size);
                }
                ascent = ascent.max(rise + ASCENT * size);
                descent = descent.max(-(rise - DESCENT * size));
            }
            MathNode::Sub(a) | MathNode::Sup(a) => {
                let small = size * SCRIPT_SCALE;
                let shift =
                    |n: &MathNode| if matches!(n, MathNode::Sub(_)) { -SUB_DROP * size } else { SUP_RISE * size };
                let (w, asc, desc) = lay(a, small, cursor, rise + shift(&nodes[i]), measure, out);
                let mut advance = w;
                // A subscript and a superscript in a row share one place.
                let pair = nodes.get(i + 1).filter(|next| {
                    matches!(
                        (&nodes[i], next),
                        (MathNode::Sub(_), MathNode::Sup(_)) | (MathNode::Sup(_), MathNode::Sub(_))
                    )
                });
                let (mut asc, mut desc) = (asc, desc);
                if let Some(next @ (MathNode::Sub(b) | MathNode::Sup(b))) = pair {
                    let (w2, a2, d2) = lay(b, small, cursor, rise + shift(next), measure, out);
                    advance = advance.max(w2);
                    asc = asc.max(a2);
                    desc = desc.max(d2);
                    i += 1;
                }
                cursor += advance;
                ascent = ascent.max(asc);
                descent = descent.max(desc);
            }
            MathNode::Frac(a, b) => {
                let small = size * FRACTION_SCALE;
                let axis = rise + MATH_AXIS * size;
                let thickness = (0.06 * size).max(0.4);
                let gap = 0.12 * size;
                // Measure both parts first (into scratch layouts).
                let mut scratch = MathLayout::default();
                let (wa, _, da) = lay(a, small, 0.0, 0.0, measure, &mut scratch);
                let mut scratch = MathLayout::default();
                let (wb, ab, _) = lay(b, small, 0.0, 0.0, measure, &mut scratch);
                let pad = 0.1 * size;
                let width = wa.max(wb) + 2.0 * pad;
                let num_base = axis + thickness / 2.0 + gap + da;
                let den_base = axis - thickness / 2.0 - gap - ab;
                let (_, asc, _) = lay(a, small, cursor + (width - wa) / 2.0, num_base, measure, out);
                let (_, _, desc) = lay(b, small, cursor + (width - wb) / 2.0, den_base, measure, out);
                out.rules.push(MathRule {
                    x0: cursor + pad * 0.5,
                    x1: cursor + width - pad * 0.5,
                    rise: axis,
                    thickness,
                });
                cursor += width;
                ascent = ascent.max(asc);
                descent = descent.max(desc);
            }
            MathNode::Root(a) => {
                // The radicand's extent first (into a scratch layout).
                let mut scratch = MathLayout::default();
                let (w, asc, desc) = lay(a, size, 0.0, 0.0, measure, &mut scratch);
                let thickness = (0.05 * size).max(0.4);
                // The bar's top edge, above the radicand.
                let top = asc + 0.08 * size;
                // The sign's foot on the baseline, lower only for a radicand
                // deeper than a descender (a fraction); its top meets the bar.
                let foot = -(desc - DESCENT * size).max(0.0);
                let sign_size = (top - foot) / RADICAL_HEIGHT;
                out.runs.push(MathRun { text: "√".into(), x: cursor, rise: rise + foot, size: sign_size });
                let sign_w = measure("√", sign_size);
                lay(a, size, cursor + sign_w, rise, measure, out);
                let end = cursor + sign_w + w + 0.05 * size;
                out.rules.push(MathRule {
                    x0: cursor + sign_w * 0.93,
                    x1: end,
                    rise: rise + top - thickness / 2.0,
                    thickness,
                });
                cursor = end + 0.05 * size;
                ascent = ascent.max(rise + top);
                descent = descent.max(desc - rise);
            }
        }
        i += 1;
    }
    (cursor - x, ascent, descent)
}

/// Drawing a [`MathText`] with egui (feature `egui`): one galley holding
/// every run and rule, so it can go anywhere egui takes text (labels, axis
/// titles of `egui_plot`, which turns it a quarter turn on a vertical axis).
#[cfg(feature = "egui")]
pub mod egui_text {
    use std::sync::Arc;

    use egui::epaint::text::{Galley, LayoutJob, Row, RowVisuals};
    use egui::{Color32, Context, FontId, Pos2, Rect, Vec2, WidgetText, pos2, vec2};

    use super::MathText;

    /// A galley showing `text` in `font` and `color`. A plain label is laid
    /// out as ordinary text.
    #[must_use]
    pub fn galley(ctx: &Context, text: &MathText, font: &FontId, color: Color32) -> Arc<Galley> {
        if let Some(plain) = text.as_plain() {
            return ctx.fonts(|f| f.layout_no_wrap(plain.to_owned(), font.clone(), color));
        }
        let size = f64::from(font.size);
        let measure = |t: &str, s: f64| {
            let f = FontId::new(s as f32, font.family.clone());
            f64::from(ctx.fonts(|fonts| fonts.layout_no_wrap(t.to_owned(), f, color)).size().x)
        };
        let layout = text.layout(size, &measure);
        let pieces: Vec<(Arc<Galley>, f32, f32)> = layout
            .runs
            .iter()
            .map(|r| {
                let f = FontId::new(r.size as f32, font.family.clone());
                (ctx.fonts(|fonts| fonts.layout_no_wrap(r.text.clone(), f, color)), r.x as f32, r.rise as f32)
            })
            .collect();
        // The baseline of each piece below its own top.
        let base =
            |g: &Galley| g.rows.first().and_then(|row| row.glyphs.first()).map_or(g.size().y * 0.8, |gl| gl.pos.y);
        // Where the label's baseline sits below the top of the galley.
        let mut baseline: f32 = 0.0;
        for (g, _, rise) in &pieces {
            baseline = baseline.max(rise + base(g));
        }
        for r in &layout.rules {
            baseline = baseline.max((r.rise + r.thickness) as f32 + 1.0);
        }
        let mut job = LayoutJob::default();
        let mut rows: Vec<Row> = Vec::new();
        let mut bottom: f32 = 0.0;
        let mut mesh_bounds = Rect::NOTHING;
        let (mut vertices, mut indices) = (0, 0);
        let mut ppp = 1.0;
        for (g, x, rise) in &pieces {
            let offset = vec2(*x, baseline - rise - base(g));
            let sections = job.sections.len() as u32;
            let text_start = job.text.len();
            job.text.push_str(&g.job.text);
            for s in &g.job.sections {
                let mut s = s.clone();
                s.byte_range = (s.byte_range.start + text_start)..(s.byte_range.end + text_start);
                job.sections.push(s);
            }
            ppp = g.pixels_per_point;
            for row in &g.rows {
                let mut row = row.clone();
                row.section_index_at_start += sections;
                row.rect = row.rect.translate(offset);
                for gl in &mut row.glyphs {
                    gl.pos += offset;
                    gl.section_index += sections;
                }
                for v in &mut row.visuals.mesh.vertices {
                    v.pos += offset;
                }
                row.visuals.mesh_bounds = row.visuals.mesh_bounds.translate(offset);
                row.ends_with_newline = false;
                mesh_bounds = mesh_bounds.union(row.visuals.mesh_bounds);
                vertices += row.visuals.mesh.vertices.len();
                indices += row.visuals.mesh.indices.len();
                bottom = bottom.max(row.rect.bottom());
                rows.push(row);
            }
        }
        for r in &layout.rules {
            let y = baseline - r.rise as f32;
            let half = (r.thickness as f32 / 2.0).max(0.5);
            let rect = Rect::from_min_max(pos2(r.x0 as f32, y - half), pos2(r.x1 as f32, y + half));
            let mut mesh = egui::Mesh::default();
            // The font atlas's top-left texel is white: a solid quad.
            mesh.add_rect_with_uv(rect, Rect::from_min_max(Pos2::ZERO, Pos2::ZERO), color);
            vertices += mesh.vertices.len();
            indices += mesh.indices.len();
            mesh_bounds = mesh_bounds.union(rect);
            let section_index_at_start = rows.last().map_or(0, |r| r.section_index_at_start);
            rows.push(Row {
                section_index_at_start,
                glyphs: Vec::new(),
                rect,
                visuals: RowVisuals { mesh, mesh_bounds: rect, glyph_index_start: 0, glyph_vertex_range: 0..0 },
                ends_with_newline: false,
            });
        }
        let height = bottom.max(baseline + (layout.descent as f32)).max(font.size);
        let width = layout.width as f32;
        Arc::new(Galley {
            job: Arc::new(job),
            rows,
            elided: false,
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height)),
            mesh_bounds,
            num_vertices: vertices,
            num_indices: indices,
            pixels_per_point: ppp,
        })
    }

    /// `text` as egui text: plain labels as `plain` styled text (exactly as
    /// before), others as a galley in `font` and `color`.
    #[must_use]
    pub fn widget_text(
        ctx: &Context,
        text: &MathText,
        plain: impl FnOnce(String) -> egui::RichText,
        font: &FontId,
        color: Color32,
    ) -> WidgetText {
        match text.as_plain() {
            Some(t) => WidgetText::RichText(plain(t.to_owned())),
            None => WidgetText::Galley(galley(ctx, text, font, color)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> MathNode {
        MathNode::Text(s.into())
    }

    #[test]
    fn plain_text_stays_one_run() {
        for s in ["Yield", "Yield (%)", "a / b", "Cost ($)", "", "x_", "50% off", "{unclosed", "two $ signs $"] {
            let m = MathText::parse(s);
            let joined = m.plain_text();
            assert!(m.as_plain().is_some() || s.contains('$'), "{s:?} → {m:?}");
            if s != "two $ signs $" {
                assert_eq!(joined, s, "{s:?}");
            }
        }
        assert_eq!(MathText::parse("Yield/hour").as_plain(), Some("Yield/hour"), "a slash outside $…$ is a slash");
        assert_eq!(MathText::literal("x_1").as_plain(), Some("x_1"));
    }

    #[test]
    fn scripts_greek_and_escapes() {
        assert_eq!(MathText::parse("x_1").nodes, vec![text("x"), MathNode::Sub(vec![text("1")])]);
        assert_eq!(MathText::parse("x^2").nodes, vec![text("x"), MathNode::Sup(vec![text("2")])]);
        assert_eq!(MathText::parse("x_{ij}").nodes, vec![text("x"), MathNode::Sub(vec![text("ij")])]);
        assert_eq!(MathText::parse("e^{-t}").nodes, vec![text("e"), MathNode::Sup(vec![text("-t")])]);
        assert_eq!(MathText::parse("\\mu \\pm \\sigma").plain_text(), "μ ± σ");
        assert_eq!(MathText::parse("\\Delta T").plain_text(), "Δ T");
        assert_eq!(MathText::parse("\\sigma^2").nodes, vec![text("σ"), MathNode::Sup(vec![text("2")])]);
        assert_eq!(MathText::parse("x_\\alpha").nodes, vec![text("x"), MathNode::Sub(vec![text("α")])]);
        assert_eq!(MathText::parse("a\\_b \\^ \\{c\\} \\$ \\\\").as_plain(), Some("a_b ^ {c} $ \\"));
        assert_eq!(MathText::parse("\\unknown").as_plain(), Some("\\unknown"));
        assert_eq!(MathText::parse("CO_2 level").plain_text(), "CO_2 level");
    }

    #[test]
    fn fractions() {
        let f = MathText::parse("\\frac{a}{b}");
        assert_eq!(f.nodes, vec![MathNode::Frac(vec![text("a")], vec![text("b")])]);
        let m = MathText::parse("Rate ($dx/dt$)");
        assert_eq!(
            m.nodes,
            vec![text("Rate ("), MathNode::Frac(vec![text("dx")], vec![text("dt")]), text(")")],
            "{m:?}"
        );
        let g = MathText::parse("$\\frac{1}{2}$ and ${a+b}/c$");
        assert_eq!(g.plain_text(), "1/2 and (a+b)/c");
        assert_eq!(MathText::parse("$/x$").plain_text(), "/x", "nothing before the slash");
    }

    #[test]
    fn roots_and_sums() {
        assert_eq!(MathText::parse("\\sqrt{x}").nodes, vec![MathNode::Root(vec![text("x")])]);
        assert_eq!(MathText::parse("\\sqrt{a+b}").plain_text(), "√(a+b)");
        assert_eq!(MathText::parse("\\sqrt x").plain_text(), "\\sqrt x", "no group: kept as typed");
        assert_eq!(MathText::parse("\\sum x_i").nodes, vec![text("Σ x"), MathNode::Sub(vec![text("i")])]);
        assert_eq!(MathText::parse("\\prod").as_plain(), Some("∏"));
        let r = MathText::parse("\\sqrt{\\frac{a}{b}}");
        assert_eq!(r.nodes, vec![MathNode::Root(vec![MathNode::Frac(vec![text("a")], vec![text("b")])])]);
    }

    /// The radical sign spans its radicand and the bar covers it.
    #[test]
    fn root_geometry() {
        let w = |t: &str, s: f64| t.chars().count() as f64 * 0.5 * s;
        let l = MathText::parse("\\sqrt{xy}").layout(10.0, &w);
        assert_eq!(l.runs.len(), 2);
        let (sign, body) = (&l.runs[0], &l.runs[1]);
        assert_eq!(sign.text, "√");
        assert!((10.0..12.0).contains(&sign.size), "a little taller than the text: {}", sign.size);
        assert!(body.x >= sign.x + 5.0 - 1e-9);
        let bar = &l.rules[0];
        assert!(bar.x0 < body.x && bar.x1 >= body.x + 10.0, "the bar covers the radicand");
        assert!(bar.rise > ASCENT * 10.0 && l.ascent >= bar.rise);
        let sign_top = sign.rise + RADICAL_HEIGHT * sign.size;
        assert!((sign_top - (bar.rise + bar.thickness / 2.0)).abs() < 1e-9, "the sign meets the bar");
        assert!(sign.rise.abs() < 1e-12, "a plain radicand: the sign stands on the baseline");
        // Over a fraction, the sign grows to span it.
        let f = MathText::parse("\\sqrt{\\frac{a}{b}}").layout(10.0, &w);
        let sign = f.runs.iter().find(|r| r.text == "√").unwrap();
        assert!(sign.size > 14.0, "{}", sign.size);
        let bar = f.rules.iter().map(|r| r.rise).fold(f64::MIN, f64::max);
        assert!(sign.rise + RADICAL_HEIGHT * sign.size > bar && sign.rise < 0.0);
        assert!((sign.rise + f.descent - DESCENT * 10.0).abs() < 1e-9, "the foot a descender above the bottom");
    }

    /// Scripts are smaller and shifted; fractions stack over a rule wide
    /// enough for both parts; the extents cover everything.
    #[test]
    fn layout_geometry() {
        let w = |t: &str, s: f64| t.chars().count() as f64 * 0.5 * s;
        let l = MathText::parse("x_i^2").layout(10.0, &w);
        assert_eq!(l.runs.len(), 3);
        let (base, sub, sup) = (&l.runs[0], &l.runs[1], &l.runs[2]);
        assert!((sub.size - 7.0).abs() < 1e-12 && (sup.size - 7.0).abs() < 1e-12);
        assert!(sub.rise < 0.0 && sup.rise > 0.0);
        assert!((sub.x - sup.x).abs() < 1e-12, "stacked at one place");
        assert!((l.width - (5.0 + 3.5)).abs() < 1e-9, "{}", l.width);
        assert!((base.x).abs() < 1e-12);
        let f = MathText::parse("\\frac{abc}{d}").layout(10.0, &w);
        assert_eq!(f.rules.len(), 1);
        let rule = &f.rules[0];
        let (num, den) = (&f.runs[0], &f.runs[1]);
        assert!(num.rise > rule.rise && den.rise < rule.rise);
        let num_w = 3.0 * 0.5 * 8.0;
        assert!(rule.x1 - rule.x0 >= num_w, "rule spans the numerator");
        assert!((den.x + 0.5 * 4.0 - (num.x + num_w / 2.0)).abs() < 1e-9, "centred");
        assert!(f.ascent > 10.0 * ASCENT && f.descent > 0.0);
    }
}
