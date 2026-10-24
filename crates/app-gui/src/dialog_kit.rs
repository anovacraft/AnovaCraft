//! Parts every dialog shares, so they look and behave alike:
//!
//! - a window that opens centred, can be moved and resized, and reopens where
//!   it was left ([`Dialog`]);
//! - the action row at its foot: **Run** for analyses or **Create** for
//!   designs and tables (**Update** and **Run as new** when editing an
//!   existing report), **Cancel**, and **Reset**, with a one-line summary of
//!   what the action will do and the rows it will use, or a note saying what
//!   is missing while the action is unavailable;
//! - keys: Enter runs (⌘/Ctrl+Enter while typing in a field), Escape cancels;
//! - a collapsed "More options" section for settings most people leave alone
//!   ([`more_options`]);
//! - column pickers with a filter box, measurement-level badges, Select all /
//!   none, Shift-click ranges and drops from the sidebar's variable list
//!   ([`column_list`], [`column_choice`]);
//! - for analyses, a **Script** toggle that shows the Rhai call the dialog
//!   would run, built live from its settings, with a button to copy it.

use eframe::egui::{self, Id, RichText};
use stat_data::{DataTable, MeasurementLevel};

use crate::report_view::{ColumnDrag, ReportRequest};
use crate::table_view;

/// The main action's verb: analyses run, designs and tables are created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Run,
    Create,
}

impl Verb {
    fn label(self) -> &'static str {
        match self {
            Self::Run => "Run",
            Self::Create => "Create",
        }
    }
}

/// Where a dialog's new report goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// A new report, above the others.
    New,
    /// In place of the report with this id (its display options kept).
    Replace(u64),
}

/// What happened in a dialog this frame.
#[derive(Debug)]
pub enum Outcome<T> {
    /// Still open.
    Open,
    /// Closed without running.
    Cancel,
    /// The user asked to clear every choice; the dialog starts again.
    Reset,
    /// The main action, with its result.
    Run(T, Placement),
}

impl<T> Outcome<T> {
    /// The same outcome with its result mapped.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Outcome<U> {
        match self {
            Self::Open => Outcome::Open,
            Self::Cancel => Outcome::Cancel,
            Self::Reset => Outcome::Reset,
            Self::Run(t, p) => Outcome::Run(f(t), p),
        }
    }
}

/// A dialog ready to run: what it would produce, and a short sentence
/// saying what that is ("Linear model of Yield on 3 effects").
pub struct Ready<T> {
    pub output: T,
    pub summary: String,
}

/// The dialog's state this frame: ready, or what is missing.
pub type Readiness<T> = Result<Ready<T>, String>;

/// Shorthand for a ready dialog.
pub fn ready<T>(output: T, summary: impl Into<String>) -> Readiness<T> {
    Ok(Ready { output, summary: summary.into() })
}

/// Rows of the active table, shown in each analysis dialog's summary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowCounts {
    pub total: usize,
    pub excluded: usize,
}

fn row_counts_id() -> Id {
    Id::new("dialog_kit_row_counts")
}

/// Where the app puts the active table's column names for this frame's
/// dialogs (the script preview names columns by name).
fn column_names_id() -> Id {
    Id::new("dialog_column_names")
}

/// Record the active table's column names for this frame's dialogs.
pub fn set_column_names(ctx: &egui::Context, names: std::sync::Arc<Vec<String>>) {
    ctx.data_mut(|d| d.insert_temp(column_names_id(), names));
}

/// Whether dialogs show the script preview (one setting for every dialog,
/// kept while the app runs).
fn script_preview_id() -> Id {
    Id::new("dialog_script_preview")
}

/// The Rhai lines that run `request` on the current table, as Copy as script
/// writes them; `None` without the table's column names.
fn script_preview(ctx: &egui::Context, request: &ReportRequest) -> Option<String> {
    let names = ctx.data(|d| d.get_temp::<std::sync::Arc<Vec<String>>>(column_names_id()))?;
    // Only names matter to the call: an empty table with the same columns.
    let columns = names.iter().map(|n| stat_data::Column::numeric(n.as_str(), Vec::new())).collect();
    let table = DataTable::new("current", columns).ok()?;
    let call = app_model::action::analysis_call("t", &request.clone().into(), &table);
    Some(format!("let t = current_table();\n{call}"))
}

/// Record the active table's row counts for this frame's dialogs.
pub fn set_row_counts(ctx: &egui::Context, counts: RowCounts) {
    ctx.data_mut(|d| d.insert_temp(row_counts_id(), counts));
}

/// Where the app puts the name of the active table's project for this
/// frame's dialogs.
fn project_name_id() -> Id {
    Id::new("dialog_kit_project_name")
}

/// Tell this frame's dialogs which project the active table belongs to.
pub fn set_project_name(ctx: &egui::Context, name: String) {
    ctx.data_mut(|d| d.insert_temp(project_name_id(), name));
}

/// The active table's project name, as the app set it this frame.
pub fn project_name(ctx: &egui::Context) -> String {
    ctx.data(|d| d.get_temp::<String>(project_name_id())).unwrap_or_default()
}

/// The "Keep in this project (name)" check box of dialogs that make a new
/// table: ticked, the table joins the project of the table it came from;
/// cleared, it opens as a separate document.
pub fn keep_in_project(ui: &mut egui::Ui, keep: &mut bool) -> egui::Response {
    let name = project_name(ui.ctx());
    let label =
        if name.is_empty() { "Keep in this project".to_owned() } else { format!("Keep in this project ({name})") };
    ui.checkbox(keep, label).on_hover_text(
        "Ticked, the new table is listed under this one in its project and saved with it; a table made by a Data \
         menu operation can then be updated when this table changes. Cleared, it opens as a separate dataset.",
    )
}

/// "48 rows" or "46 of 48 rows (2 excluded)".
pub fn rows_note(counts: RowCounts) -> String {
    let rows = |n: usize| if n == 1 { "1 row".to_owned() } else { format!("{n} rows") };
    if counts.excluded == 0 {
        rows(counts.total)
    } else {
        format!("{} of {} ({} excluded)", counts.total - counts.excluded, rows(counts.total), counts.excluded)
    }
}

/// The text height, for widths that grow with the text-size setting.
pub fn em(ui: &egui::Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Body)
}

/// A number field for a random seed. Entry is capped at
/// [`app_model::seed::MAX_SEED`] (2⁵³ − 1): seeds are saved as JSON numbers,
/// which hold larger whole numbers only approximately.
pub fn seed_value<T: egui::emath::Numeric>(seed: &mut T) -> egui::DragValue<'_> {
    egui::DragValue::new(seed).range(0.0..=app_model::seed::MAX_SEED as f64)
}

/// Width of a drop-down list of column names.
pub fn combo_width(ui: &egui::Ui) -> f32 {
    em(ui) * 15.0
}

/// A dialog window with the shared action row.
pub struct Dialog {
    title: String,
    id: Id,
    verb: Verb,
    uses_rows: bool,
    edit: Option<u64>,
    reset: bool,
    default_width: Option<f32>,
    label: Option<String>,
}

impl Dialog {
    /// A dialog titled `title`, which also names the analysis. Its position
    /// and size are remembered under the title (or [`Dialog::id_salt`]).
    pub fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        Self {
            id: Id::new(("dialog", title.clone())),
            title,
            verb: Verb::Run,
            uses_rows: true,
            edit: None,
            reset: true,
            default_width: None,
            label: None,
        }
    }

    /// Remember position and size under `salt` instead of the title (for
    /// dialogs whose title changes while open).
    pub fn id_salt(mut self, salt: impl std::hash::Hash) -> Self {
        self.id = Id::new(("dialog", salt));
        self
    }

    /// The main action creates a design or table: "Create", and no row count.
    pub fn creates(mut self) -> Self {
        self.verb = Verb::Create;
        self.uses_rows = false;
        self
    }

    /// The summary leaves out the active table's row count (dialogs that do
    /// not read its rows, such as sample-size calculators).
    pub fn without_rows(mut self) -> Self {
        self.uses_rows = false;
        self
    }

    /// Editing report `report` (if any): Update and Run as new in place of Run.
    pub fn editing(mut self, report: Option<u64>) -> Self {
        self.edit = report;
        self
    }

    /// No Reset button (dialogs with nothing worth clearing).
    pub fn without_reset(mut self) -> Self {
        self.reset = false;
        self
    }

    /// The width the window first opens at (dialogs whose contents fill
    /// the width they are given, such as side-by-side lists).
    pub fn default_width(mut self, width: f32) -> Self {
        self.default_width = Some(width);
        self
    }

    /// Label the main button `label` in place of Run or Create, for a
    /// dialog whose action has a verb of its own ("Sort", "Recode", "Apply").
    /// Editing a report still shows Update.
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Show the dialog. `contents` draws the settings and says whether the
    /// dialog is ready (with its result and summary) or what is missing.
    pub fn show<T: 'static>(
        self,
        ctx: &egui::Context,
        contents: impl FnOnce(&mut egui::Ui) -> Readiness<T>,
    ) -> Outcome<T> {
        // State before this frame's widgets see the keys: a text field with
        // focus, or an open drop-down list, keeps Enter and Escape for itself.
        // (The text field's state is read outside `memory`, which holds the
        // context's lock that `load_state` needs too.)
        let (focused, popup) = ctx.memory(|m| (m.focused(), m.any_popup_open()));
        let typing = focused.is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
        let (enter, command_enter, escape) = ctx.input(|i| {
            let enter = i.key_pressed(egui::Key::Enter);
            (enter && !i.modifiers.command, enter && i.modifiers.command, i.key_pressed(egui::Key::Escape))
        });
        let mut outcome = Outcome::Open;
        let mut open = true;
        let screen = ctx.screen_rect();
        let armed_id = self.id.with("armed");
        // A dialog that was not shown last pass has just opened: its keys
        // work until a click lands outside it, whatever happened the last
        // time it was open.
        let shown_id = self.id.with("shown_pass");
        let pass = ctx.cumulative_pass_nr();
        let opened = ctx.data(|d| d.get_temp::<u64>(shown_id)).is_none_or(|last| last + 1 < pass);
        ctx.data_mut(|d| {
            d.insert_temp(shown_id, pass);
            if opened {
                d.insert_temp(armed_id, true);
            }
        });
        // Dim the workspace behind the dialog in front (the one on top last
        // pass). Painted first in the dialog's own layer, so it lies under the
        // dialog and over everything behind it; it takes no input.
        let window_layer = egui::LayerId::new(egui::Order::Middle, self.id);
        if ctx.memory(|m| m.areas().top_layer_id(egui::Order::Middle)) == Some(window_layer) {
            let scrim = crate::theme::dialog_scrim(ctx.style().visuals.dark_mode);
            ctx.layer_painter(window_layer).rect_filled(screen, 0.0, scrim);
        }
        let mut window = egui::Window::new(self.title.as_str());
        if let Some(width) = self.default_width {
            window = window.default_width(width);
        }
        let response = window
            .id(self.id)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(screen.center())
            .constrain(true)
            .show(ctx, |ui| {
                let readiness = contents(ui);
                ui.add_space(4.0);
                ui.separator();
                let note = match &readiness {
                    Ok(r) => {
                        let rows = if self.uses_rows {
                            ctx.data(|d| d.get_temp::<RowCounts>(row_counts_id())).map(rows_note)
                        } else {
                            None
                        };
                        let text = match (r.summary.is_empty(), rows) {
                            (false, Some(rows)) => format!("{} · {rows}", r.summary),
                            (false, None) => r.summary.clone(),
                            (true, Some(rows)) => rows,
                            (true, None) => String::new(),
                        };
                        (!text.is_empty()).then(|| RichText::new(text).weak())
                    }
                    Err(why) => Some(RichText::new(format!("⚠ {why}")).strong()),
                };
                if let Some(note) = note {
                    ui.add(egui::Label::new(note).wrap());
                }
                // Analyses can show the script call their settings make.
                let scriptable = std::any::TypeId::of::<T>() == std::any::TypeId::of::<ReportRequest>();
                let show_script = scriptable && ctx.data(|d| d.get_temp::<bool>(script_preview_id())).unwrap_or(false);
                if show_script {
                    let request =
                        readiness.as_ref().ok().and_then(|r| (&r.output as &dyn std::any::Any).downcast_ref());
                    match request.and_then(|r| script_preview(ctx, r)) {
                        Some(script) => {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt(self.id.with("script"))
                                    .max_height(em(ui) * 9.0)
                                    .show(ui, |ui| {
                                        ui.add(egui::Label::new(RichText::new(&script).monospace().small()).wrap());
                                        overflow_cues(ui);
                                    });
                                if ui
                                    .small_button("Copy script")
                                    .on_hover_text("Paste it into the Script window to run this analysis from a script")
                                    .clicked()
                                {
                                    ctx.copy_text(format!("{script}\n"));
                                }
                            });
                        }
                        None => {
                            ui.label(
                                RichText::new("The script appears once the dialog is ready to run.").weak().small(),
                            );
                        }
                    }
                }
                let why = readiness.as_ref().err().cloned();
                let mut chosen: Option<Placement> = None;
                ui.horizontal(|ui| {
                    let ok = readiness.is_ok();
                    let main = match (self.edit, &self.label) {
                        (Some(_), _) => "Update",
                        (None, Some(label)) => label.as_str(),
                        (None, None) => self.verb.label(),
                    };
                    let mut button = ui.add_enabled(ok, egui::Button::new(main));
                    if let Some(why) = &why {
                        button = button.on_disabled_hover_text(why.as_str());
                    } else if self.edit.is_some() {
                        button = button.on_hover_text("Replace the report with these settings (Enter)");
                    } else {
                        button = button.on_hover_text("Enter");
                    }
                    if button.clicked() {
                        chosen = Some(self.edit.map_or(Placement::New, Placement::Replace));
                    }
                    if self.edit.is_some() {
                        let again = ui.add_enabled(ok, egui::Button::new("Run as new"));
                        let again = match &why {
                            Some(why) => again.on_disabled_hover_text(why.as_str()),
                            None => again.on_hover_text("Keep the report and add another with these settings"),
                        };
                        if again.clicked() {
                            chosen = Some(Placement::New);
                        }
                    }
                    if ui.button("Cancel").on_hover_text("Escape").clicked() {
                        outcome = Outcome::Cancel;
                    }
                    if self.reset || scriptable {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.reset
                                && ui
                                    .small_button("Reset")
                                    .on_hover_text("Clear every choice in this dialog and start again")
                                    .clicked()
                            {
                                outcome = Outcome::Reset;
                            }
                            if scriptable {
                                let mut on = show_script;
                                if ui
                                    .toggle_value(&mut on, "Script")
                                    .on_hover_text("Show the script call these settings make (for the Script window)")
                                    .clicked()
                                {
                                    ctx.data_mut(|d| d.insert_temp(script_preview_id(), on));
                                }
                            }
                        });
                    }
                });
                (readiness, chosen)
            });
        let Some(response) = response else {
            return if open { Outcome::Open } else { Outcome::Cancel };
        };
        // Keys reach only the dialog in front, and only after the last click
        // was inside it (a click in the data grid hands the keys back to it).
        let layer = response.response.layer_id;
        let rect = response.response.rect;
        if let Some(press) = ctx.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None }) {
            ctx.data_mut(|d| d.insert_temp(armed_id, rect.contains(press)));
        }
        let armed = ctx.data(|d| d.get_temp::<bool>(armed_id)).unwrap_or(true);
        let on_top = ctx.memory(|m| m.areas().top_layer_id(egui::Order::Middle)) == Some(layer);
        let keys = armed && on_top;
        if let Some((Ok(ready), chosen)) = response.inner {
            let by_key = keys && ((enter && !typing) || command_enter) && !popup;
            if let Some(place) = chosen.or(by_key.then(|| self.edit.map_or(Placement::New, Placement::Replace))) {
                return Outcome::Run(ready.output, place);
            }
        }
        if !open || (keys && escape && !typing && !popup) {
            return Outcome::Cancel;
        }
        outcome
    }
}

/// Settings most people leave alone, in a section that starts closed.
pub fn more_options<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
    egui::CollapsingHeader::new("More options")
        .id_salt(ui.id().with("more_options"))
        .default_open(false)
        .show(ui, add_contents)
        .body_returned
}

/// Cues that a scrolled list has more entries than it shows: call it last
/// inside a vertical scroll area's contents. Where entries lie below (or
/// above) the visible part, the edge fades into the background and a ▾ (▴)
/// marks it, so a list that happens to end on a whole row still reads as
/// one to scroll. Drawn on a window-coloured background (dialogs, pop-ups).
///
/// A vertical scroll area clips its contents only up and down; across, the
/// clip is whatever surrounds it (the whole screen for a drop-down list), so
/// the cues take their width from the list itself.
pub fn overflow_cues(ui: &egui::Ui) {
    let content = ui.min_rect();
    let mut clip = ui.clip_rect();
    clip.min.x = clip.min.x.max(ui.max_rect().min.x);
    clip.max.x = clip.max.x.min(ui.max_rect().max.x);
    if clip.width() <= 0.0 {
        return;
    }
    let bg = ui.visuals().window_fill();
    let band = (em(ui) * 1.6).min(clip.height() / 3.0);
    let cue = |edge_y: f32, inner_y: f32, glyph: &str| {
        let rect = egui::Rect::from_x_y_ranges(clip.x_range(), edge_y.min(inner_y)..=edge_y.max(inner_y));
        let mut mesh = egui::Mesh::default();
        let clear = bg.gamma_multiply(0.0);
        let (top, bottom) = if edge_y > inner_y { (clear, bg) } else { (bg, clear) };
        mesh.colored_vertex(rect.left_top(), top);
        mesh.colored_vertex(rect.right_top(), top);
        mesh.colored_vertex(rect.right_bottom(), bottom);
        mesh.colored_vertex(rect.left_bottom(), bottom);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        let painter = ui.painter_at(clip);
        painter.add(mesh);
        let anchor = if edge_y > inner_y { egui::Align2::CENTER_BOTTOM } else { egui::Align2::CENTER_TOP };
        painter.text(
            egui::pos2(clip.center().x, edge_y),
            anchor,
            glyph,
            egui::FontId::proportional(em(ui) * 1.4),
            ui.visuals().strong_text_color(),
        );
    };
    if content.bottom() > clip.bottom() + 1.0 {
        cue(clip.bottom(), clip.bottom() - band, "▾");
    }
    if content.top() < clip.top() - 1.0 {
        cue(clip.top(), clip.top() + band, "▴");
    }
}

/// [`egui::ComboBox::show_ui`] with [`overflow_cues`] on its list, so a
/// long drop-down list shows that it scrolls. Every drop-down list in the
/// app uses this.
pub trait ComboExt {
    /// Show the list built by `add`.
    fn show_list<R>(self, ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<Option<R>>;
}

impl ComboExt for egui::ComboBox {
    fn show_list<R>(self, ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<Option<R>> {
        self.show_ui(ui, |ui| {
            let r = add(ui);
            overflow_cues(ui);
            r
        })
    }
}

/// A faint explanatory line.
pub fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).weak().small()).wrap());
}

/// The level of column `i`, or continuous for a missing column.
pub fn level(table: &DataTable, i: usize) -> MeasurementLevel {
    table.column(i).map_or(MeasurementLevel::Continuous, |c| c.level)
}

/// Whether column `i` is numeric and continuous.
pub fn is_continuous(table: &DataTable, i: usize) -> bool {
    table.column(i).is_some_and(|c| c.as_numeric().is_some() && c.level == MeasurementLevel::Continuous)
}

/// Whether column `i` is numeric (any level).
pub fn is_numeric(table: &DataTable, i: usize) -> bool {
    table.column(i).is_some_and(|c| c.as_numeric().is_some())
}

/// Column `i`'s name ("" if there is none).
pub fn name(table: &DataTable, i: usize) -> &str {
    table.column(i).map_or("", |c| c.name.as_str())
}

/// The column dropped on `response` from the sidebar's variable list, if any.
fn dropped_column(response: &egui::Response, table: &DataTable) -> Option<usize> {
    let payload = response.dnd_release_payload::<ColumnDrag>()?;
    table.find(&payload.name)
}

/// Height of a column list (remembered per list, changed with its grip).
fn list_height_id(id: Id) -> Id {
    id.with("list_height")
}

#[derive(Clone, Default)]
struct ListState {
    filter: String,
    /// The column clicked last, where a Shift-click range starts.
    anchor: Option<usize>,
}

/// A list of check boxes, one per column `allow`ed, for choosing several
/// columns: a filter box (when the list is long), each column's level
/// badge, Select all / Select none, Shift-click to set a whole range like
/// the clicked box, and columns dragged from the sidebar's variable list
/// are added. `chosen` is indexed by column and grows with the table.
/// Returns whether the choice changed.
pub fn column_list(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    table: &DataTable,
    chosen: &mut Vec<bool>,
    allow: impl Fn(usize) -> bool,
) -> bool {
    let id = ui.id().with(("column_list", id_salt));
    if chosen.len() < table.n_cols() {
        chosen.resize(table.n_cols(), false);
    }
    let mut state: ListState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let allowed: Vec<usize> = (0..table.n_cols()).filter(|&i| allow(i)).collect();
    let mut changed = false;
    let filter = state.filter.to_lowercase();
    let shown: Vec<usize> = allowed
        .iter()
        .copied()
        .filter(|&i| filter.is_empty() || name(table, i).to_lowercase().contains(&filter))
        .collect();
    ui.horizontal(|ui| {
        if allowed.len() > 8 || !state.filter.is_empty() {
            ui.add(
                egui::TextEdit::singleline(&mut state.filter).hint_text("Filter columns").desired_width(em(ui) * 9.0),
            )
            .on_hover_text("Show only columns whose names contain this text");
        }
        let all = !shown.is_empty() && shown.iter().all(|&i| chosen[i]);
        if ui.add_enabled(!all, egui::Button::new("All").small()).on_hover_text("Select every column listed").clicked()
        {
            for &i in &shown {
                chosen[i] = true;
            }
            changed = true;
        }
        let none = shown.iter().all(|&i| !chosen[i]);
        if ui.add_enabled(!none, egui::Button::new("None").small()).on_hover_text("Clear the selection").clicked() {
            for &i in &shown {
                chosen[i] = false;
            }
            changed = true;
        }
        let n = allowed.iter().filter(|&&i| chosen[i]).count();
        ui.label(RichText::new(format!("{n} chosen")).weak().small());
    });
    let height_id = list_height_id(id);
    let default_height = em(ui) * 12.0;
    let height: f32 = ui.data(|d| d.get_temp(height_id)).unwrap_or(default_height);
    // A column dragged from the sidebar can be dropped on the list.
    let dragging = egui::DragAndDrop::has_payload_of_type::<ColumnDrag>(ui.ctx());
    let mut frame = egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(4));
    if dragging {
        frame = frame.stroke(ui.visuals().selection.stroke);
    }
    let shown_list = frame.show(ui, |ui| {
        ui.set_min_width(em(ui) * 14.0);
        egui::ScrollArea::vertical().id_salt(id.with("scroll")).max_height(height).auto_shrink([false, true]).show(
            ui,
            |ui| {
                if allowed.is_empty() {
                    ui.label(RichText::new("No suitable columns in this table").weak());
                } else if shown.is_empty() {
                    ui.label(RichText::new("No column names contain that text").weak());
                }
                for &i in &shown {
                    let mut on = chosen[i];
                    let response = ui
                        .horizontal(|ui| {
                            table_view::level_badge(ui, level(table, i));
                            ui.checkbox(&mut on, name(table, i))
                        })
                        .inner;
                    if response.changed() {
                        let shift = ui.input(|i| i.modifiers.shift);
                        match state.anchor.filter(|_| shift) {
                            Some(a) => {
                                let (lo, hi) = (a.min(i), a.max(i));
                                for &j in shown.iter().filter(|&&j| j >= lo && j <= hi) {
                                    chosen[j] = on;
                                }
                            }
                            None => chosen[i] = on,
                        }
                        state.anchor = Some(i);
                        changed = true;
                    }
                }
                overflow_cues(ui);
            },
        );
    });
    if let Some(i) = dropped_column(&shown_list.response, table).filter(|&i| allow(i)) {
        chosen[i] = true;
        changed = true;
    }
    // A grip under the list makes it taller or shorter.
    let (rect, grip) =
        ui.allocate_exact_size(egui::vec2(ui.available_width().min(em(ui) * 14.0), 6.0), egui::Sense::drag());
    let grip = grip.on_hover_cursor(egui::CursorIcon::ResizeVertical).on_hover_text("Drag to make the list taller");
    let stroke = if grip.hovered() || grip.dragged() {
        ui.visuals().widgets.active.fg_stroke
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke
    };
    let c = rect.center();
    ui.painter().line_segment([egui::pos2(c.x - 12.0, c.y), egui::pos2(c.x + 12.0, c.y)], stroke);
    if grip.dragged() {
        let new = (height + grip.drag_delta().y).clamp(em(ui) * 4.0, em(ui) * 60.0);
        ui.data_mut(|d| d.insert_temp(height_id, new));
    }
    ui.data_mut(|d| d.insert_temp(id, state));
    changed
}

/// A drop-down list for choosing one column among those `allow`ed: each
/// entry shows the column's level badge, long lists get a filter box, and
/// a column dragged from the sidebar's variable list onto it is chosen.
/// With `none` (the label of "no column"), the choice can be cleared.
/// Returns whether the choice changed.
pub fn column_choice(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    table: &DataTable,
    slot: &mut Option<usize>,
    allow: impl Fn(usize) -> bool,
    none: Option<&str>,
) -> bool {
    let id = ui.id().with(("column_choice", id_salt));
    let before = *slot;
    if slot.is_some_and(|i| i >= table.n_cols()) {
        *slot = None;
    }
    let allowed: Vec<usize> = (0..table.n_cols()).filter(|&i| allow(i)).collect();
    let selected = match *slot {
        Some(i) => name(table, i).to_owned(),
        None => none.unwrap_or("Choose a column…").to_owned(),
    };
    let filter_id = id.with("filter");
    let response = egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(combo_width(ui))
        .height(em(ui) * 24.0)
        .show_list(ui, |ui| {
            let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
            if allowed.len() > 12 {
                let edit = ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Filter columns"));
                if !edit.has_focus() && filter.is_empty() {
                    edit.request_focus();
                }
            }
            if let Some(label) = none {
                ui.selectable_value(slot, None, label);
            }
            let lower = filter.to_lowercase();
            for &i in &allowed {
                if !lower.is_empty() && !name(table, i).to_lowercase().contains(&lower) {
                    continue;
                }
                let response = ui
                    .horizontal(|ui| {
                        table_view::level_badge(ui, level(table, i));
                        ui.selectable_label(*slot == Some(i), name(table, i))
                    })
                    .inner;
                if response.clicked() {
                    *slot = Some(i);
                    ui.close_menu();
                }
            }
            if allowed.is_empty() {
                ui.label(RichText::new("No suitable columns in this table").weak());
            }
            ui.data_mut(|d| d.insert_temp(filter_id, filter));
        })
        .response;
    if let Some(i) = dropped_column(&response, table).filter(|&i| allow(i)) {
        *slot = Some(i);
    }
    *slot != before
}

/// Whether `columns` are all different (ignoring unset roles).
pub fn all_different(columns: &[Option<usize>]) -> bool {
    let set: Vec<usize> = columns.iter().flatten().copied().collect();
    (0..set.len()).all(|a| (a + 1..set.len()).all(|b| set[a] != set[b]))
}

/// The message for the first column given two roles in `roles` (role name,
/// chosen column), naming the column and both roles; `None` when every set
/// role has its own column.
pub fn duplicate_role(table: &DataTable, roles: &[(&str, Option<usize>)]) -> Option<String> {
    roles.iter().enumerate().find_map(|(a, &(first, column))| {
        let column = column?;
        roles[a + 1..].iter().find(|&&(_, other)| other == Some(column)).map(|&(second, _)| {
            format!(
                "“{}” is chosen as both {first} and {second}; each role needs a different column",
                name(table, column)
            )
        })
    })
}

/// "1 column" / "3 columns".
pub fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

/// Columns `chosen` (by index), in table order.
pub fn chosen_columns(chosen: &[bool]) -> Vec<usize> {
    chosen.iter().enumerate().filter(|(_, c)| **c).map(|(i, _)| i).collect()
}

/// A `Vec<bool>` per column with `columns` set.
pub fn mask(n_cols: usize, columns: &[usize]) -> Vec<bool> {
    let mut m = vec![false; n_cols];
    for &c in columns {
        if c < n_cols {
            m[c] = true;
        }
    }
    m
}

/// A column index from a saved request, kept only if the table has it.
pub fn valid(table: &DataTable, column: usize) -> Option<usize> {
    (column < table.n_cols()).then_some(column)
}

/// Remembered dialog settings, per document and dialog, while the app runs
/// (B2). Kept with the column names they were made for: appending columns
/// keeps them, any other change to the columns forgets them.
#[derive(Default)]
pub struct Memory {
    entries: std::collections::HashMap<MemoryKey, (Vec<String>, Box<dyn std::any::Any>)>,
}

/// Document, dialog type and mode.
type MemoryKey = (u64, std::any::TypeId, String);

/// A dialog whose settings can be remembered, restored, reset, pre-filled
/// and pointed at a report to edit.
pub trait Remember: Clone + 'static {
    /// Distinguishes modes of one dialog type (its window title).
    fn memory_key(&self) -> String;
    /// Edit report `report` (`None`: run as new).
    fn set_edit(&mut self, report: Option<u64>);
    /// Fill the dialog's main column role with the columns selected in the
    /// data grid (called on a fresh dialog only).
    fn prefill(&mut self, _columns: &[usize], _table: &DataTable) {}
}

fn column_names(table: Option<&DataTable>) -> Vec<String> {
    table.map(|t| t.columns().iter().map(|c| format!("{}\u{1f}{:?}", c.name, c.kind())).collect()).unwrap_or_default()
}

impl Memory {
    /// Keep `dialog`'s settings for document `doc` (0 for dialogs that do not
    /// read a table).
    pub fn keep<D: Remember>(&mut self, doc: u64, dialog: &D, table: Option<&DataTable>) {
        let mut d = dialog.clone();
        d.set_edit(None);
        let key = (doc, std::any::TypeId::of::<D>(), d.memory_key());
        self.entries.insert(key, (column_names(table), Box::new(d)));
    }

    /// The settings kept for a dialog like `fresh`, if its columns are still
    /// there; otherwise `fresh`, pre-filled with the grid's `selected` columns.
    pub fn recall<D: Remember>(&self, doc: u64, fresh: D, table: Option<&DataTable>, selected: &[usize]) -> D {
        let key = (doc, std::any::TypeId::of::<D>(), fresh.memory_key());
        if let Some((names, kept)) = self.entries.get(&key) {
            let now = column_names(table);
            if now.len() >= names.len() && now[..names.len()] == names[..] {
                if let Some(d) = kept.downcast_ref::<D>() {
                    return d.clone();
                }
            }
        }
        let mut fresh = fresh;
        if let Some(table) = table {
            if !selected.is_empty() {
                fresh.prefill(selected, table);
            }
        }
        fresh
    }

    /// Forget document `doc`'s settings (its columns moved or it closed).
    pub fn forget(&mut self, doc: u64) {
        self.entries.retain(|k, _| k.0 != doc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_note_counts_excluded_rows() {
        assert_eq!(rows_note(RowCounts { total: 48, excluded: 0 }), "48 rows");
        assert_eq!(rows_note(RowCounts { total: 48, excluded: 2 }), "46 of 48 rows (2 excluded)");
        assert_eq!(rows_note(RowCounts { total: 1, excluded: 0 }), "1 row");
    }

    #[test]
    fn duplicate_role_names_the_column_and_both_roles() {
        let table = DataTable::new(
            "t",
            vec![
                stat_data::Column::numeric("Width", vec![1.0, 2.0]),
                stat_data::Column::numeric("Batch", vec![1.0, 1.0]),
            ],
        )
        .unwrap();
        let roles = [("Measurement", Some(0)), ("Subgroup ID", None), ("Phase", Some(1))];
        assert_eq!(duplicate_role(&table, &roles), None);
        let roles = [("Measurement", Some(0)), ("Subgroup ID", Some(1)), ("Phase", Some(0))];
        let message = duplicate_role(&table, &roles).expect("a duplicate");
        assert!(message.contains("“Width”") && message.contains("Measurement and Phase"), "{message}");
    }

    #[test]
    fn all_different_ignores_unset_roles() {
        assert!(all_different(&[Some(1), None, Some(2), None]));
        assert!(!all_different(&[Some(1), Some(1)]));
    }

    #[derive(Clone)]
    struct Fake {
        n: usize,
        edit: Option<u64>,
        pre: Vec<usize>,
    }

    impl Remember for Fake {
        fn memory_key(&self) -> String {
            "fake".into()
        }
        fn set_edit(&mut self, report: Option<u64>) {
            self.edit = report;
        }
        fn prefill(&mut self, columns: &[usize], _: &DataTable) {
            self.pre = columns.to_vec();
        }
    }

    #[test]
    fn memory_keeps_settings_while_columns_are_only_appended() {
        let mut table = DataTable::new("t", vec![stat_data::Column::numeric("a", vec![1.0])]).unwrap();
        let mut memory = Memory::default();
        let fresh = || Fake { n: 0, edit: None, pre: Vec::new() };
        memory.keep(1, &Fake { n: 5, edit: Some(9), pre: Vec::new() }, Some(&table));
        let back = memory.recall(1, fresh(), Some(&table), &[0]);
        assert_eq!((back.n, back.edit), (5, None), "kept, but not as an edit");
        assert!(back.pre.is_empty(), "kept settings are not pre-filled");
        assert_eq!(memory.recall(2, fresh(), Some(&table), &[]).n, 0, "per document");
        table.insert_column(1, stat_data::Column::numeric("b", vec![2.0])).unwrap();
        assert_eq!(memory.recall(1, fresh(), Some(&table), &[]).n, 5, "appended column");
        let other = DataTable::new("t", vec![stat_data::Column::numeric("z", vec![1.0])]).unwrap();
        let back = memory.recall(1, fresh(), Some(&other), &[0]);
        assert_eq!((back.n, back.pre), (0, vec![0]), "different columns: fresh and pre-filled");
        memory.forget(1);
        assert_eq!(memory.recall(1, fresh(), Some(&table), &[]).n, 0);
    }
}
