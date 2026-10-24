//! A Rhai code editor: highlighting, line numbers, folding of `{ … }`
//! blocks from the margin, the bracket beside the cursor and its partner
//! marked, and completion of script functions, names the script declares
//! and (inside a string) column names. Used by the Script window and by
//! script notebook cells. The text logic is in [`crate::script_edit`].

use std::ops::Range;
use std::sync::OnceLock;

use eframe::egui::text::{CCursor, CCursorRange, LayoutJob};
use eframe::egui::{self, Color32, FontId, Key, Modifiers, RichText, TextFormat};
use stat_script::ApiFunction;
use stat_script::syntax::{TokenKind, tokens};

use crate::script_edit::{self, Candidate, Folding, Typed};
use crate::theme::ScriptPalette;

/// Completions shown at once.
const MAX_CANDIDATES: usize = 8;

/// The functions scripts can call (worked out once).
pub fn api() -> &'static [ApiFunction] {
    static API: OnceLock<Vec<ApiFunction>> = OnceLock::new();
    API.get_or_init(stat_script::api_functions)
}

/// The completion list while it is open.
#[derive(Debug, Clone)]
struct Completion {
    typed: Typed,
    items: Vec<Candidate>,
    selected: usize,
    /// Where the list was drawn last frame (a click on it moves focus away
    /// from the editor, which must not close it).
    rect: Option<egui::Rect>,
}

/// What the editor needs from its owner each frame.
pub struct EditorSettings<'a> {
    /// Rows the editor is at least.
    pub rows: usize,
    /// 1-based line of an error, marked in the margin and tinted.
    pub error_line: Option<usize>,
    /// Column names offered inside strings.
    pub columns: &'a [String],
    /// Byte ranges of the full text to tint (find matches).
    pub found: &'a [Range<usize>],
    /// Show line numbers and fold markers.
    pub gutter: bool,
}

/// What happened in the editor this frame.
#[derive(Debug, Clone, Default)]
pub struct EditorOutput {
    /// The text changed.
    pub changed: bool,
    /// Selected characters of the full text (character indices).
    pub selection: Option<Range<usize>>,
}

/// One code editor's state (the text itself belongs to its owner).
#[derive(Debug, Clone)]
pub struct CodeEditor {
    id: egui::Id,
    /// Folded blocks, while any are folded.
    folding: Option<Folding>,
    /// Cursor (character index in the shown text) as of the last frame.
    cursor: Option<usize>,
    completion: Option<Completion>,
    /// Move the cursor to this 1-based line on the next frame.
    goto: Option<usize>,
    /// Select these characters of the full text on the next frame.
    select: Option<Range<usize>>,
}

impl CodeEditor {
    /// An editor whose widget has id `id`.
    #[must_use]
    pub fn new(id: egui::Id) -> Self {
        Self { id, folding: None, cursor: None, completion: None, goto: None, select: None }
    }

    /// The editor widget's id.
    #[must_use]
    pub fn id(&self) -> egui::Id {
        self.id
    }

    /// The owner replaced the text: forget folds and positions.
    pub fn reset(&mut self) {
        self.folding = None;
        self.cursor = None;
        self.completion = None;
    }

    /// Some blocks are folded.
    #[must_use]
    pub fn is_folded(&self) -> bool {
        self.folding.is_some()
    }

    /// Open every folded block.
    pub fn unfold_all(&mut self) {
        self.folding = None;
    }

    /// Fold every outermost multi-line block of `text`.
    pub fn fold_all(&mut self, text: &str) {
        let mut folding = self.folding.take().unwrap_or_else(|| Folding::new(text));
        loop {
            let open: Vec<usize> = folding.fold_points().into_iter().filter(|p| !p.1).map(|p| p.0).collect();
            let Some(&line) = open.first() else { break };
            folding.fold_line(line);
        }
        self.folding = folding.is_folded().then_some(folding);
    }

    /// Put the cursor on 1-based line `line` (opening folds).
    pub fn go_to_line(&mut self, line: usize) {
        self.folding = None;
        self.goto = Some(line);
    }

    /// Select characters `chars` of the full text (opening folds).
    pub fn select(&mut self, chars: Range<usize>) {
        self.folding = None;
        self.select = Some(chars);
    }

    /// The completion list is open.
    #[must_use]
    pub fn completing(&self) -> bool {
        self.completion.is_some()
    }

    /// Draw the editor for `text`.
    pub fn show(&mut self, ui: &mut egui::Ui, text: &mut String, settings: &EditorSettings<'_>) -> EditorOutput {
        let pal = ScriptPalette::of(ui);
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        if self.folding.as_ref().is_some_and(|f| f.full() != *text) {
            // Changed from outside (replace all, a new script).
            self.folding = None;
        }
        let focused = ui.ctx().memory(|m| m.has_focus(self.id));
        let keys = if focused { self.completion_keys(ui.ctx()) } else { CompletionKeys::default() };
        let mut out = EditorOutput::default();
        let mut toggle_fold = None;

        ui.horizontal_top(|ui| {
            let shown: String = self.folding.as_ref().map_or_else(|| text.clone(), |f| f.shown().to_owned());
            if settings.gutter {
                toggle_fold = self.gutter(ui, &shown, &font, &pal, settings.error_line);
            }
            // What to tint: the bracket pair at the cursor, find matches.
            let mut marks: Vec<(Range<usize>, Color32, bool)> = Vec::new();
            if let Some(c) = self.cursor {
                let at = script_edit::byte_index(&shown, c);
                if let Some((a, b)) = script_edit::matching_bracket(&shown, at) {
                    marks.push((a..a + 1, pal.bracket, true));
                    marks.push((b..b + 1, pal.bracket, true));
                }
            }
            if self.folding.is_none() {
                marks.extend(settings.found.iter().map(|r| (r.clone(), pal.found, false)));
            }
            let error_line = if self.folding.is_none() { settings.error_line } else { None };
            let text_color = ui.visuals().text_color();
            let mut layouter = |ui: &egui::Ui, t: &str, _wrap: f32| {
                let job = highlight(t, &font, text_color, &pal, error_line, &marks);
                ui.fonts(|f| f.layout_job(job))
            };
            let before = shown.clone();
            let target: &mut String = match &mut self.folding {
                Some(f) => f.shown_mut(),
                None => text,
            };
            let output = egui::TextEdit::multiline(target)
                .id(self.id)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(settings.rows)
                .lock_focus(true)
                .layouter(&mut layouter)
                .show(ui);
            let changed = output.response.changed();
            if changed {
                if let Some(f) = &mut self.folding {
                    f.edited(&before);
                    *text = f.full();
                    if !f.is_folded() {
                        self.folding = None;
                    }
                }
                out.changed = true;
            }
            let shown_now: String = self.folding.as_ref().map_or_else(|| text.clone(), |f| f.shown().to_owned());
            let range = output.cursor_range.map(|c| c.as_sorted_char_range());
            let over_list = self.completion.as_ref().and_then(|c| c.rect).is_some_and(|r| {
                ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())).is_some_and(|p| r.contains(p))
            });
            // A click on the completion list takes the cursor away; keep where it was.
            if range.is_some() || !over_list {
                self.cursor = range.as_ref().map(|r| r.end);
            }
            out.selection = range.filter(|r| !r.is_empty()).map(|r| match &self.folding {
                Some(f) => {
                    let start = f.full_offset(script_edit::byte_index(&shown_now, r.start));
                    let end = f.full_offset(script_edit::byte_index(&shown_now, r.end));
                    script_edit::char_index(text, start)..script_edit::char_index(text, end)
                }
                None => r,
            });

            // Completion: follow typing, then the keys pressed.
            if changed || keys.open {
                self.update_completion(&shown_now, settings.columns, keys.open);
            } else if self
                .completion
                .as_ref()
                .is_some_and(|c| self.cursor.map(|k| script_edit::byte_index(&shown_now, k)) != Some(c.typed.range.end))
            {
                self.completion = None;
            }
            let mut accept = None;
            if let Some(c) = &mut self.completion {
                let n = c.items.len();
                if keys.down {
                    c.selected = (c.selected + 1) % n;
                }
                if keys.up {
                    c.selected = (c.selected + n - 1) % n;
                }
                if keys.accept {
                    accept = Some(c.selected);
                }
                if keys.cancel {
                    self.completion = None;
                }
            }
            if let Some(c) = &mut self.completion {
                let at = output.galley.from_ccursor(CCursor::new(self.cursor.unwrap_or(0)));
                let rect = output.galley.pos_from_cursor(&at).translate(output.galley_pos.to_vec2());
                let (clicked, drawn) = completion_popup(ui, self.id, rect.left_bottom(), c);
                c.rect = Some(drawn);
                if let Some(clicked) = clicked {
                    accept = Some(clicked);
                }
            }
            if let Some(i) = accept {
                self.accept(i, text, ui.ctx());
                out.changed = true;
            }

            if let Some(line) = self.goto.take() {
                let at = line_start(text, line);
                self.place_cursor(ui.ctx(), at..at);
                let row = output.galley.pos_from_cursor(&output.galley.from_ccursor(CCursor::new(at)));
                ui.scroll_to_rect(row.translate(output.galley_pos.to_vec2()), Some(egui::Align::Center));
            }
            if let Some(chars) = self.select.take() {
                self.place_cursor(ui.ctx(), chars.clone());
                let row = output.galley.pos_from_cursor(&output.galley.from_ccursor(CCursor::new(chars.start)));
                ui.scroll_to_rect(row.translate(output.galley_pos.to_vec2()), Some(egui::Align::Center));
            }
        });

        if let Some(line) = toggle_fold {
            let mut folding = self.folding.take().unwrap_or_else(|| Folding::new(text));
            if !folding.unfold_line(line) {
                folding.fold_line(line);
            }
            self.folding = folding.is_folded().then_some(folding);
            self.completion = None;
        }
        out
    }

    /// The margin: line numbers of the full text, the error line marked by
    /// a glyph, and ▾ / ▸ on lines whose block can fold / is folded. Returns
    /// the shown line whose marker was clicked.
    fn gutter(
        &self,
        ui: &mut egui::Ui,
        shown: &str,
        font: &FontId,
        pal: &ScriptPalette,
        error_line: Option<usize>,
    ) -> Option<usize> {
        let lines = match &self.folding {
            Some(f) => f.full_lines(),
            None => (0..shown.split('\n').count()).collect(),
        };
        let points = match &self.folding {
            Some(f) => f.fold_points(),
            None => script_edit::foldable_blocks(shown).into_iter().map(|b| (b.line, false)).collect(),
        };
        let last = lines.last().copied().unwrap_or(0) + 1;
        let width = last.to_string().len();
        let mut gutter = LayoutJob::default();
        for (i, &full) in lines.iter().enumerate() {
            let number = full + 1;
            let error = self.folding.is_none() && Some(number) == error_line;
            let mark = if error { "▸" } else { " " };
            let fold = match points.iter().find(|p| p.0 == i) {
                Some((_, true)) => "⊞",
                Some((_, false)) => "⊟",
                None => " ",
            };
            let sep = if i + 1 < lines.len() { "\n" } else { "" };
            let color = if error { pal.error } else { pal.gutter };
            gutter.append(&format!("{mark}{number:>width$}{fold}{sep}"), 0.0, TextFormat::simple(font.clone(), color));
        }
        let response = ui
            .add(egui::Label::new(gutter).selectable(false).sense(egui::Sense::click()))
            .on_hover_text("⊟ folds a block, ⊞ shows it again");
        let pos = response.interact_pointer_pos().filter(|_| response.clicked())?;
        let row_height = ui.fonts(|f| f.row_height(font));
        let row = ((pos.y - response.rect.top()) / row_height).floor().max(0.0) as usize;
        points.iter().any(|p| p.0 == row).then_some(row)
    }

    /// Keys for the completion list, taken before the text editor sees
    /// them (only those the list uses while it is open; Ctrl + Space opens it).
    fn completion_keys(&self, ctx: &egui::Context) -> CompletionKeys {
        ctx.input_mut(|i| {
            let open = i.consume_key(Modifiers::CTRL, Key::Space);
            if self.completion.is_none() {
                return CompletionKeys { open, ..CompletionKeys::default() };
            }
            CompletionKeys {
                open,
                down: i.consume_key(Modifiers::NONE, Key::ArrowDown),
                up: i.consume_key(Modifiers::NONE, Key::ArrowUp),
                accept: i.consume_key(Modifiers::NONE, Key::Tab) || i.consume_key(Modifiers::NONE, Key::Enter),
                cancel: i.consume_key(Modifiers::NONE, Key::Escape),
            }
        })
    }

    /// Work out what is being typed and what completes it. Without `forced`
    /// (Ctrl + Space) code completions need two characters typed.
    fn update_completion(&mut self, shown: &str, columns: &[String], forced: bool) {
        self.completion = None;
        let Some(cursor) = self.cursor else { return };
        let at = script_edit::byte_index(shown, cursor);
        let Some(typed) = script_edit::typed_at(shown, at) else { return };
        if !forced && !typed.in_string && typed.prefix.chars().count() < 2 {
            return;
        }
        if !forced && typed.in_string && typed.prefix.is_empty() {
            return;
        }
        let declared = script_edit::declared_names(shown);
        let items = script_edit::candidates(&typed, api(), &declared, columns, MAX_CANDIDATES);
        if !items.is_empty() {
            self.completion = Some(Completion { typed, items, selected: 0, rect: None });
        }
    }

    /// Put completion `index` in place of what was typed.
    fn accept(&mut self, index: usize, text: &mut String, ctx: &egui::Context) {
        let Some(c) = self.completion.take() else { return };
        let Some(item) = c.items.get(index) else { return };
        let mut insert = item.text.clone();
        let is_api = !c.typed.in_string && api().iter().any(|f| f.name == item.text);
        let target: &mut String = match &mut self.folding {
            Some(f) => f.shown_mut(),
            None => text,
        };
        if is_api && !target[c.typed.range.end..].starts_with('(') {
            insert.push('(');
        }
        let before = target.clone();
        target.replace_range(c.typed.range.clone(), &insert);
        let cursor = script_edit::char_index(target, c.typed.range.start + insert.len());
        if let Some(f) = &mut self.folding {
            f.edited(&before);
            *text = f.full();
        }
        self.cursor = Some(cursor);
        self.place_cursor(ctx, cursor..cursor);
    }

    /// Set the editor's selection (character indices of the text it shows).
    fn place_cursor(&self, ctx: &egui::Context, chars: Range<usize>) {
        if let Some(mut state) = egui::TextEdit::load_state(ctx, self.id) {
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(chars.start), CCursor::new(chars.end))));
            state.store(ctx, self.id);
        } else {
            let mut state = egui::text_edit::TextEditState::default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(chars.start), CCursor::new(chars.end))));
            state.store(ctx, self.id);
        }
        ctx.memory_mut(|m| m.request_focus(self.id));
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CompletionKeys {
    open: bool,
    down: bool,
    up: bool,
    accept: bool,
    cancel: bool,
}

/// The completion list under the cursor. Returns the item clicked and
/// where the list was drawn.
fn completion_popup(ui: &egui::Ui, id: egui::Id, at: egui::Pos2, c: &Completion) -> (Option<usize>, egui::Rect) {
    let mut clicked = None;
    let response =
        egui::Area::new(id.with("completion")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(520.0);
                for (i, item) in c.items.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let label = RichText::new(&item.text).monospace();
                        if ui.selectable_label(i == c.selected, label).clicked() {
                            clicked = Some(i);
                        }
                        ui.label(RichText::new(&item.detail).weak().small());
                    });
                }
                ui.label(RichText::new("Tab or Enter inserts · Esc closes").weak().small());
            });
        });
    (clicked, response.response.rect)
}

/// Colour Rhai source: tokens, the error line's background, and `marks`
/// (byte ranges with a background, underlined when the flag is set).
pub fn highlight(
    text: &str,
    font: &FontId,
    plain: Color32,
    pal: &ScriptPalette,
    error_line: Option<usize>,
    marks: &[(Range<usize>, Color32, bool)],
) -> LayoutJob {
    let mut job = LayoutJob::default();
    // Byte range of the error line, painted with a background.
    let error_range = error_line.map(|l| {
        let start = text.split('\n').take(l.saturating_sub(1)).map(|s| s.len() + 1).sum::<usize>().min(text.len());
        let end = text[start..].find('\n').map_or(text.len(), |k| start + k);
        start..end
    });
    let marks: Vec<&(Range<usize>, Color32, bool)> = marks
        .iter()
        .filter(|(r, _, _)| r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end))
        .collect();
    for (range, kind) in tokens(text) {
        let color = match kind {
            TokenKind::Keyword => pal.keyword,
            TokenKind::String => pal.string,
            TokenKind::Number => pal.number,
            TokenKind::Comment => pal.comment,
            TokenKind::Function => pal.function,
            TokenKind::Plain => plain,
        };
        // Split where the error line or a mark starts or ends.
        let mut cuts = vec![range.start, range.end];
        if let Some(e) = &error_range {
            cuts.extend([e.start, e.end].into_iter().filter(|c| range.contains(c)));
        }
        for (m, _, _) in &marks {
            cuts.extend([m.start, m.end].into_iter().filter(|c| range.contains(c)));
        }
        cuts.sort_unstable();
        cuts.dedup();
        for w in cuts.windows(2) {
            let mut format = TextFormat::simple(font.clone(), color);
            if kind == TokenKind::Comment {
                format.italics = true;
            }
            if error_range.as_ref().is_some_and(|e| w[0] >= e.start && w[1] <= e.end && e.start < e.end) {
                format.background = pal.error_line;
            }
            if let Some((_, bg, underline)) = marks.iter().find(|(m, _, _)| w[0] >= m.start && w[1] <= m.end) {
                format.background = *bg;
                if *underline {
                    format.underline = egui::Stroke::new(1.0_f32, color);
                }
            }
            job.append(&text[w[0]..w[1]], 0.0, format);
        }
    }
    job
}

/// Character index where 1-based `line` starts (the end of the text if there
/// are fewer lines).
pub fn line_start(text: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    let mut seen = 1;
    for (i, c) in text.chars().enumerate() {
        if c == '\n' {
            seen += 1;
            if seen == line {
                return i + 1;
            }
        }
    }
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_starts_are_character_indices() {
        let text = "αβ\ngamma\n\nδ";
        assert_eq!(line_start(text, 1), 0);
        assert_eq!(line_start(text, 2), 3);
        assert_eq!(line_start(text, 3), 9);
        assert_eq!(line_start(text, 4), 10);
        assert_eq!(line_start(text, 9), text.chars().count());
    }

    #[test]
    fn highlighting_keeps_the_text_and_marks_the_error_line_and_brackets() {
        let text = "let x = 1;\nprint(\"y\"); // z\n";
        let pal = ScriptPalette::light();
        let font = FontId::monospace(12.0);
        let open = text.find('(').unwrap();
        let close = text.find(')').unwrap();
        let marks = [(open..open + 1, pal.bracket, true), (close..close + 1, pal.bracket, true)];
        let job = highlight(text, &font, Color32::BLACK, &pal, Some(2), &marks);
        assert_eq!(job.text, text);
        let marked: String = job
            .sections
            .iter()
            .filter(|s| s.format.background == pal.error_line)
            .map(|s| &job.text[s.byte_range.clone()])
            .collect();
        assert_eq!(marked, "print\"y\"; // z");
        let brackets: String = job
            .sections
            .iter()
            .filter(|s| s.format.background == pal.bracket && s.format.underline.width > 0.0)
            .map(|s| &job.text[s.byte_range.clone()])
            .collect();
        assert_eq!(brackets, "()");
        let keyword = job.sections.iter().find(|s| &job.text[s.byte_range.clone()] == "let").unwrap();
        assert_eq!(keyword.format.color, pal.keyword);
    }

    #[test]
    fn the_api_list_is_available_for_completion() {
        assert!(api().iter().any(|f| f.name == "current_table"));
    }
}
