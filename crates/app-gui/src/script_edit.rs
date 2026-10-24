//! Editing aids for the Script window and script notebook cells: folding of
//! `{ … }` blocks, bracket matching, completion of script functions and
//! column names, and find and replace. Plain text logic, so it is tested
//! without a window.
//!
//! Positions are byte offsets into the text (always on character
//! boundaries) unless a name says otherwise.

use std::ops::Range;

use stat_script::syntax::{TokenKind, tokens};

/// What a folded block shows in its place.
pub const FOLD_MARK: &str = "⋯";

// ---------------------------------------------------------------- folding

/// A `{ … }` block spanning several lines: `open` and `close` are the byte
/// offsets of its braces, `line` the 0-based line of the opening brace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    pub open: usize,
    pub close: usize,
    pub line: usize,
}

/// Blocks of `text` that span more than one line, by their opening brace's
/// position. Braces in strings and comments do not count.
#[must_use]
pub fn foldable_blocks(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    for (open, close) in bracket_pairs(text) {
        if text.as_bytes()[open] == b'{' && text[open..close].contains('\n') {
            out.push(Block { open, close, line: text[..open].matches('\n').count() });
        }
    }
    out.sort_by_key(|b| b.open);
    out
}

/// One folded block: the mark at `at` in the shown text stands for `hidden`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fold {
    at: usize,
    hidden: String,
}

/// A text with some blocks folded: the editor shows and edits [`Self::shown`],
/// in which each folded block's inside is replaced by [`FOLD_MARK`]; the full
/// text is [`Self::full`]. Typing anywhere outside the marks keeps the folds;
/// deleting or overwriting a mark deletes the block's inside with it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folding {
    shown: String,
    folds: Vec<Fold>,
}

impl Folding {
    /// `text` with nothing folded.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self { shown: text.to_owned(), folds: Vec::new() }
    }

    /// The text as shown, with folded blocks as marks.
    #[must_use]
    pub fn shown(&self) -> &str {
        &self.shown
    }

    /// The text being edited, for the editor widget.
    pub fn shown_mut(&mut self) -> &mut String {
        &mut self.shown
    }

    /// `true` if any block is folded.
    #[must_use]
    pub fn is_folded(&self) -> bool {
        !self.folds.is_empty()
    }

    /// The full text, with every fold opened.
    #[must_use]
    pub fn full(&self) -> String {
        let mut out =
            String::with_capacity(self.shown.len() + self.folds.iter().map(|f| f.hidden.len()).sum::<usize>());
        let mut from = 0;
        for f in &self.folds {
            out.push_str(&self.shown[from..f.at]);
            out.push_str(&f.hidden);
            from = f.at + FOLD_MARK.len();
        }
        out.push_str(&self.shown[from..]);
        out
    }

    /// Positions of the marks in the shown text.
    #[cfg(test)]
    #[must_use]
    pub fn marks(&self) -> Vec<usize> {
        self.folds.iter().map(|f| f.at).collect()
    }

    /// Blocks of the shown text that can be folded, and folded marks, each
    /// by its 0-based shown line: `(line, folded)`, in line order, one per
    /// line (the outermost block when several start on one line).
    #[must_use]
    pub fn fold_points(&self) -> Vec<(usize, bool)> {
        let line_of = |at: usize| self.shown[..at].matches('\n').count();
        let mut points: Vec<(usize, bool)> = self.folds.iter().map(|f| (line_of(f.at), true)).collect();
        for b in foldable_blocks(&self.shown) {
            points.push((b.line, false));
        }
        points.sort_by_key(|&(line, folded)| (line, !folded));
        points.dedup_by_key(|p| p.0);
        points
    }

    /// Fold the outermost multi-line block opening on shown line `line`.
    /// Folds inside it become part of it. Returns `false` if there is none.
    pub fn fold_line(&mut self, line: usize) -> bool {
        let Some(block) = foldable_blocks(&self.shown).into_iter().find(|b| b.line == line) else { return false };
        let inside = block.open + 1..block.close;
        // The full text of the inside, opening the folds within it.
        let mut hidden = String::new();
        let mut from = inside.start;
        let mut kept = Vec::new();
        let removed = inside.len() as isize - FOLD_MARK.len() as isize;
        for f in self.folds.drain(..) {
            if inside.contains(&f.at) {
                hidden.push_str(&self.shown[from..f.at]);
                hidden.push_str(&f.hidden);
                from = f.at + FOLD_MARK.len();
            } else {
                kept.push(f);
            }
        }
        hidden.push_str(&self.shown[from..inside.end]);
        self.shown.replace_range(inside.clone(), FOLD_MARK);
        for f in &mut kept {
            if f.at >= inside.end {
                f.at = (f.at as isize - removed) as usize;
            }
        }
        kept.push(Fold { at: inside.start, hidden });
        kept.sort_by_key(|f| f.at);
        self.folds = kept;
        true
    }

    /// Open the fold whose mark is on shown line `line`. Returns `false` if
    /// there is none.
    pub fn unfold_line(&mut self, line: usize) -> bool {
        let Some(i) = self.folds.iter().position(|f| self.shown[..f.at].matches('\n').count() == line) else {
            return false;
        };
        let f = self.folds.remove(i);
        self.shown.replace_range(f.at..f.at + FOLD_MARK.len(), &f.hidden);
        let grown = f.hidden.len() - FOLD_MARK.len();
        for g in &mut self.folds {
            if g.at > f.at {
                g.at += grown;
            }
        }
        true
    }

    /// Follow an edit of the shown text from `before` to the current
    /// [`Self::shown`] (as an editor widget made it): folds before and after
    /// the changed stretch keep their contents; a fold whose mark was in the
    /// changed stretch is gone with its contents.
    pub fn edited(&mut self, before: &str) {
        let after = &self.shown;
        let prefix = common_prefix(before, after);
        let suffix = common_suffix(&before[prefix..], &after[prefix..]);
        let changed = prefix..before.len() - suffix;
        let delta = after.len() as isize - before.len() as isize;
        self.folds.retain_mut(|f| {
            let mark = f.at..f.at + FOLD_MARK.len();
            if mark.start < changed.end && changed.start < mark.end {
                return false;
            }
            if mark.start >= changed.end {
                f.at = (f.at as isize + delta) as usize;
            }
            true
        });
        // A safety net: a fold whose mark is not where it should be is dropped.
        self.folds.retain(|f| after.get(f.at..f.at + FOLD_MARK.len()) == Some(FOLD_MARK));
    }

    /// Byte offset in the full text of byte offset `shown` of the shown text
    /// (a position inside a mark counts as its start).
    #[must_use]
    pub fn full_offset(&self, shown: usize) -> usize {
        let mut extra = 0isize;
        for f in &self.folds {
            if f.at < shown {
                extra += f.hidden.len() as isize - FOLD_MARK.len() as isize;
                if shown < f.at + FOLD_MARK.len() {
                    return (f.at as isize + extra - (f.hidden.len() as isize - FOLD_MARK.len() as isize)) as usize;
                }
            }
        }
        (shown as isize + extra) as usize
    }

    /// The full-text line (0-based) of each shown line.
    #[must_use]
    pub fn full_lines(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut full = 0;
        let mut at = 0;
        let mut folds = self.folds.iter().peekable();
        for line in self.shown.split('\n') {
            out.push(full);
            let end = at + line.len();
            while let Some(f) = folds.peek() {
                if f.at < end {
                    full += f.hidden.matches('\n').count();
                    folds.next();
                } else {
                    break;
                }
            }
            full += 1;
            at = end + 1;
        }
        out
    }
}

/// Length of the longest common prefix, on a character boundary.
fn common_prefix(a: &str, b: &str) -> usize {
    let mut n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    while !a.is_char_boundary(n) || !b.is_char_boundary(n) {
        n -= 1;
    }
    n
}

/// Length of the longest common suffix, on a character boundary.
fn common_suffix(a: &str, b: &str) -> usize {
    let mut n = a.bytes().rev().zip(b.bytes().rev()).take_while(|(x, y)| x == y).count();
    while !a.is_char_boundary(a.len() - n) || !b.is_char_boundary(b.len() - n) {
        n -= 1;
    }
    n
}

// ------------------------------------------------------- bracket matching

/// Pairs of matching brackets (`()`, `[]`, `{}`) outside strings and
/// comments, as byte offsets of the opening and closing bracket. Unmatched
/// brackets are left out.
#[must_use]
pub fn bracket_pairs(text: &str) -> Vec<(usize, usize)> {
    let mut stack: Vec<(u8, usize)> = Vec::new();
    let mut out = Vec::new();
    for (range, kind) in tokens(text) {
        if kind != TokenKind::Plain {
            continue;
        }
        for (i, b) in text.as_bytes()[range.clone()].iter().enumerate() {
            let at = range.start + i;
            match b {
                b'(' | b'[' | b'{' => stack.push((*b, at)),
                b')' | b']' | b'}' => {
                    let want = match b {
                        b')' => b'(',
                        b']' => b'[',
                        _ => b'{',
                    };
                    // Skip unmatched openers of another kind.
                    if let Some(k) = stack.iter().rposition(|(o, _)| *o == want) {
                        let (_, open) = stack[k];
                        stack.truncate(k);
                        out.push((open, at));
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// The bracket next to the cursor (at byte `cursor`: the one just before it,
/// else the one just after) and its partner, as byte offsets; `None` when
/// the cursor is not beside a matched bracket.
#[must_use]
pub fn matching_bracket(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let pairs = bracket_pairs(text);
    let find = |at: usize| {
        pairs.iter().find_map(|&(o, c)| {
            if o == at {
                Some((o, c))
            } else if c == at {
                Some((c, o))
            } else {
                None
            }
        })
    };
    let before = text[..cursor.min(text.len())].char_indices().next_back().map(|(i, _)| i);
    before.and_then(find).or_else(|| find(cursor))
}

// ------------------------------------------------------------- completion

/// What is being typed at the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typed {
    /// The bytes the chosen completion replaces.
    pub range: Range<usize>,
    /// The text typed so far.
    pub prefix: String,
    /// Inside a string literal (complete column names) rather than code.
    pub in_string: bool,
}

/// The word, or the start of a string literal's text, ending at byte
/// `cursor`. `None` inside a comment or a number.
#[must_use]
pub fn typed_at(text: &str, cursor: usize) -> Option<Typed> {
    let cursor = cursor.min(text.len());
    for (range, kind) in tokens(text) {
        // The token the cursor is in (or just after, for a name).
        let inside = range.start < cursor && cursor <= range.end;
        if !inside {
            continue;
        }
        match kind {
            TokenKind::String => {
                let closed = range.len() >= 2 && text[range.clone()].ends_with(&text[range.start..=range.start]);
                if closed && cursor == range.end {
                    return None;
                }
                let start = range.start + 1;
                return Some(Typed { range: start..cursor, prefix: text[start..cursor].to_owned(), in_string: true });
            }
            TokenKind::Comment | TokenKind::Number => return None,
            _ => {}
        }
    }
    let start = text[..cursor]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
        .last()
        .map_or(cursor, |(i, _)| i);
    let word = &text[start..cursor];
    if word.is_empty() || word.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(Typed { range: start..cursor, prefix: word.to_owned(), in_string: false })
}

/// Names declared in the script with `let`, `const` or `fn`.
#[must_use]
pub fn declared_names(text: &str) -> Vec<String> {
    let toks = tokens(text);
    let mut out = Vec::new();
    let mut next_is_name = false;
    for (range, kind) in toks {
        let word = &text[range];
        if kind == TokenKind::Keyword && matches!(word, "let" | "const" | "fn") {
            next_is_name = true;
            continue;
        }
        if next_is_name && !word.trim().is_empty() {
            let name = word.trim().split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or("");
            if !name.is_empty() && !out.iter().any(|n| n == name) {
                out.push(name.to_owned());
            }
            next_is_name = false;
        }
    }
    out
}

/// One completion offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Inserted in place of what was typed.
    pub text: String,
    /// Shown beside it: a signature, "column" or "variable".
    pub detail: String,
}

/// Completions of `typed`: column names inside a string, otherwise script
/// functions (with their shortest signature) and names the script declares.
/// Names starting with the prefix come first (case-insensitively), then
/// names containing it; the exact prefix alone is not offered. At most
/// `limit`.
#[must_use]
pub fn candidates(
    typed: &Typed,
    functions: &[stat_script::ApiFunction],
    declared: &[String],
    columns: &[String],
    limit: usize,
) -> Vec<Candidate> {
    let prefix = typed.prefix.to_lowercase();
    let mut pool: Vec<Candidate> =
        if typed.in_string {
            columns.iter().map(|c| Candidate { text: c.clone(), detail: "column".to_owned() }).collect()
        } else {
            let mut v: Vec<Candidate> =
                declared.iter().map(|d| Candidate { text: d.clone(), detail: "in this script".to_owned() }).collect();
            v.extend(functions.iter().filter(|f| !declared.contains(&f.name)).map(|f| Candidate {
                text: f.name.clone(),
                detail: f.signatures.first().cloned().unwrap_or_default(),
            }));
            v
        };
    pool.retain(|c| c.text != typed.prefix && c.text.to_lowercase().contains(&prefix));
    pool.sort_by_key(|c| !c.text.to_lowercase().starts_with(&prefix));
    pool.truncate(limit);
    pool
}

// --------------------------------------------------------- find / replace

/// How to match in find and replace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindOptions {
    /// Upper and lower case differ.
    pub match_case: bool,
    /// Only whole names (not inside a longer name).
    pub whole_names: bool,
}

/// Byte ranges of the matches of `needle` in `text`, without overlaps.
#[must_use]
pub fn find_all(text: &str, needle: &str, options: FindOptions) -> Vec<Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    let name_char = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let found = if options.match_case {
            text[at..].find(needle).map(|i| (at + i, at + i + needle.len()))
        } else {
            find_caseless(&text[at..], needle).map(|(s, e)| (at + s, at + e))
        };
        let Some((start, end)) = found else { break };
        let whole = !options.whole_names
            || (!name_char(text[..start].chars().next_back()) && !name_char(text[end..].chars().next()));
        if whole {
            out.push(start..end);
            at = end;
        } else {
            at = start + text[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

/// First case-insensitive match of `needle` in `hay` (byte range in `hay`).
fn find_caseless(hay: &str, needle: &str) -> Option<(usize, usize)> {
    let lower: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    for (start, _) in hay.char_indices() {
        let mut want = lower.iter();
        let mut end = start;
        let mut matched = true;
        'chars: for (i, c) in hay[start..].char_indices() {
            for lc in c.to_lowercase() {
                match want.next() {
                    Some(w) if *w == lc => {}
                    Some(_) => {
                        matched = false;
                        break 'chars;
                    }
                    None => break 'chars,
                }
            }
            end = start + i + c.len_utf8();
            if want.len() == 0 {
                break;
            }
        }
        if matched && want.next().is_none() && end > start {
            return Some((start, end));
        }
    }
    None
}

/// `text` with every match replaced, and how many there were.
#[must_use]
pub fn replace_all(text: &str, needle: &str, with: &str, options: FindOptions) -> (String, usize) {
    let matches = find_all(text, needle, options);
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for m in &matches {
        out.push_str(&text[from..m.start]);
        out.push_str(with);
        from = m.end;
    }
    out.push_str(&text[from..]);
    (out, matches.len())
}

/// Character index of byte offset `byte` in `text`.
#[must_use]
pub fn char_index(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

/// Byte offset of character index `chars` in `text` (the end if beyond).
#[must_use]
pub fn byte_index(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "let t = current_table();\nfor r in 0..3 {\n    if r > 1 {\n        print(r);\n    }\n}\nprint(\"{ not a block\");\n";

    #[test]
    fn blocks_spanning_lines_can_fold() {
        let blocks = foldable_blocks(SCRIPT);
        assert_eq!(blocks.iter().map(|b| b.line).collect::<Vec<_>>(), [1, 2]);
        let f = Folding::new(SCRIPT);
        assert_eq!(f.fold_points(), [(1, false), (2, false)]);
    }

    #[test]
    fn folding_hides_and_restores_the_text() {
        let mut f = Folding::new(SCRIPT);
        assert!(f.fold_line(2));
        assert_eq!(f.shown().lines().nth(2), Some("    if r > 1 {⋯}"));
        assert_eq!(f.full(), SCRIPT);
        // Folding the outer block takes the inner fold with it.
        assert!(f.fold_line(1));
        assert_eq!(f.shown(), "let t = current_table();\nfor r in 0..3 {⋯}\nprint(\"{ not a block\");\n");
        assert_eq!(f.full(), SCRIPT);
        assert_eq!(f.full_lines(), [0, 1, 6, 7]);
        assert_eq!(f.fold_points(), [(1, true)]);
        // Offsets after the fold map past its hidden text.
        let p = f.shown().find("print(\"{").unwrap();
        assert_eq!(f.full_offset(p), SCRIPT.find("print(\"{").unwrap());
        assert_eq!(f.full_offset(3), 3);
        assert!(f.unfold_line(1));
        assert_eq!(f.shown(), SCRIPT);
        assert!(!f.is_folded());
        assert!(!f.fold_line(0), "line 0 opens no block");
    }

    #[test]
    fn edits_around_a_fold_keep_it_and_deleting_the_mark_deletes_the_block() {
        let mut f = Folding::new(SCRIPT);
        f.fold_line(2);
        // Type at the start of the text: the fold moves along.
        let before = f.shown().to_owned();
        f.shown_mut().insert_str(0, "// a\n");
        f.edited(&before);
        assert_eq!(f.full(), format!("// a\n{SCRIPT}"));
        // Type after the fold.
        let before = f.shown().to_owned();
        let end = f.shown().len();
        f.shown_mut().insert_str(end, "x;");
        f.edited(&before);
        assert_eq!(f.full(), format!("// a\n{SCRIPT}x;"));
        // Delete the mark: the block's inside goes with it.
        let before = f.shown().to_owned();
        let at = f.marks()[0];
        f.shown_mut().replace_range(at..at + FOLD_MARK.len(), "");
        f.edited(&before);
        assert!(!f.is_folded());
        assert!(f.full().contains("    if r > 1 {}\n"), "{}", f.full());
        assert!(!f.full().contains("print(r)"));
    }

    #[test]
    fn brackets_match_outside_strings_and_comments() {
        let text = "f(a[1], \"(\") // )\n{ g() }";
        let open = text.find('(').unwrap();
        let close = text.find("\")").unwrap() + 1;
        assert_eq!(matching_bracket(text, open + 1), Some((open, close)));
        assert_eq!(matching_bracket(text, close), Some((close, open)), "cursor before the bracket");
        assert_eq!(matching_bracket(text, close + 1), Some((close, open)));
        let brace = text.find('{').unwrap();
        assert_eq!(matching_bracket(text, brace), Some((brace, text.len() - 1)));
        assert_eq!(matching_bracket(text, 1 + text.find(',').unwrap()), None);
    }

    #[test]
    fn completion_offers_functions_variables_and_columns() {
        let functions = vec![
            stat_script::ApiFunction {
                name: "open_sample".into(),
                signatures: vec!["open_sample(name: String)".into()],
            },
            stat_script::ApiFunction { name: "open_file".into(), signatures: vec!["open_file(path: String)".into()] },
            stat_script::ApiFunction { name: "print_table".into(), signatures: vec![] },
        ];
        let text = "let opening = 1;\nop";
        let typed = typed_at(text, text.len()).unwrap();
        assert_eq!((typed.prefix.as_str(), typed.in_string, typed.range.clone()), ("op", false, 17..19));
        let declared = declared_names(text);
        assert_eq!(declared, ["opening"]);
        let names: Vec<String> =
            candidates(&typed, &functions, &declared, &[], 10).into_iter().map(|c| c.text).collect();
        assert_eq!(names, ["opening", "open_sample", "open_file"]);
        // Inside a string: column names, matching anywhere in the name.
        let text = "t.analyze(\"linear_model\", #{ response: \"Rea";
        let typed = typed_at(text, text.len()).unwrap();
        assert!(typed.in_string);
        assert_eq!(typed.prefix, "Rea");
        let columns = vec!["Area".to_owned(), "Reading".to_owned(), "Reference".to_owned()];
        let names: Vec<String> = candidates(&typed, &[], &[], &columns, 10).into_iter().map(|c| c.text).collect();
        assert_eq!(names, ["Reading", "Area"], "names starting with it first; Reference does not match");
        // Nothing in comments, numbers or after a closed string.
        assert!(typed_at("// op", 5).is_none());
        assert!(typed_at("x = 12", 6).is_none());
        assert!(typed_at("\"ab\"", 4).is_none());
    }

    #[test]
    fn find_and_replace_whole_names_and_case() {
        let text = "select(\"Weight\"); weight_kg = Weight * 2; // WEIGHT";
        let exact = FindOptions { match_case: true, whole_names: true };
        assert_eq!(find_all(text, "Weight", exact).len(), 2);
        let any_case = FindOptions { match_case: false, whole_names: true };
        assert_eq!(find_all(text, "weight", any_case).len(), 3);
        let loose = FindOptions::default();
        assert_eq!(find_all(text, "weight", loose).len(), 4);
        let (out, n) = replace_all(text, "Weight", "Mass", exact);
        assert_eq!(n, 2);
        assert_eq!(out, "select(\"Mass\"); weight_kg = Mass * 2; // WEIGHT");
        assert!(find_all(text, "", loose).is_empty());
        // Case-insensitive matching keeps byte ranges right around wide characters.
        let wide = "Größe GRÖSSE größe";
        let found = find_all(wide, "größe", loose);
        assert_eq!(found.iter().map(|r| &wide[r.clone()]).collect::<Vec<_>>(), ["Größe", "größe"]);
    }

    #[test]
    fn char_and_byte_indices_convert() {
        let t = "aé b";
        assert_eq!(char_index(t, 3), 2);
        assert_eq!(byte_index(t, 2), 3);
        assert_eq!(byte_index(t, 99), t.len());
    }
}
