//! Views in windows of their own.
//!
//! A document's data grid, its results pane, or a single report can be
//! shown in a native window of its own (an egui *immediate* viewport, drawn
//! from the same application state during the main window's frame, so a
//! selection made in one window shows in every other window on the same
//! frame). A view lives in exactly one place: while it has a window, the
//! main window shows a panel saying so, with buttons to bring the window to
//! the front or the view back.
//!
//! Closing such a window (its close button, ⌘W / Ctrl+W in it, or its "Back
//! to main window" button) puts the view back in the main window; nothing
//! is closed. Closing the document closes its windows.
//!
//! The document a window belongs to becomes the command target (the one the
//! menus, dialogs and the sidebar act on) when the window is used, without
//! changing what the main window shows. Shortcuts pressed in a window act
//! on that window's document.
//!
//! When egui draws viewports inside the main window instead (as the headless
//! tests do), each view's window is an ordinary egui window drawn by the
//! same code.

use super::*;

/// A view of a document that can be shown in a window of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum View {
    /// The data grid.
    Data,
    /// The results pane: every report, with the outline.
    Results,
    /// One report (by id).
    Report(u64),
    /// A dashboard of the document's project (by its GUI id). Its window
    /// belongs to the project's first table, which can change.
    Dashboard(u64),
}

/// A view shown in a window of its own.
pub(super) struct ViewWindow {
    /// The document (by id).
    pub document: u64,
    pub view: View,
    /// Bring the window to the front on the next frame.
    pub raise: bool,
    /// Its native window had keyboard focus last frame.
    pub focused: bool,
    /// Where the window was last frame (outer position and size, in
    /// points), saved with the project's layout.
    pub rect: Option<egui::Rect>,
    /// Where to open it (from a saved layout, already placed on screen).
    pub place: Option<egui::Rect>,
}

impl ViewWindow {
    /// A new window for `view` of document `document`, brought to the front.
    pub fn new(document: u64, view: View) -> Self {
        Self { document, view, raise: true, focused: false, rect: None, place: None }
    }
}

/// Fit a window saved at `rect` onto a screen of size `screen` (points,
/// origin at the top left): no larger than the screen, and moved so that
/// all of it is visible. A window saved on a second monitor (beyond the
/// screen's edge, or at negative coordinates) comes back onto this one.
pub(super) fn clamp_onto_screen(rect: egui::Rect, screen: egui::Vec2) -> egui::Rect {
    let size =
        egui::vec2(rect.width().clamp(240.0, screen.x.max(240.0)), rect.height().clamp(160.0, screen.y.max(160.0)));
    let max = (screen - size).max(egui::Vec2::ZERO);
    let min = egui::pos2(rect.min.x.clamp(0.0, max.x), rect.min.y.clamp(0.0, max.y));
    egui::Rect::from_min_size(min, size)
}

/// What happened in a view's window during one frame.
#[derive(Default)]
struct WindowOutcome {
    /// Actions asked for there, carried out on its document.
    actions: Vec<Action>,
    /// Close the window and show the view in the main window again.
    back: bool,
    /// The window was clicked in or gained focus: its document becomes the
    /// command target.
    used: bool,
    /// Its native window has keyboard focus.
    focused: bool,
}

/// The egui viewport of view `view` of document `doc`.
fn viewport_id(doc: u64, view: View) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("anovacraft_view_window", window_key(doc, view)))
}

/// What identifies a view's window across frames: a dashboard's window
/// keeps its identity when its project's first table changes.
fn window_key(doc: u64, view: View) -> (u64, View) {
    match view {
        View::Dashboard(_) => (0, view),
        _ => (doc, view),
    }
}

/// A window's first size, in points.
fn default_size(view: View) -> [f32; 2] {
    match view {
        View::Data => [900.0, 600.0],
        View::Results => [780.0, 760.0],
        View::Report(_) => [760.0, 700.0],
        View::Dashboard(_) => [1100.0, 760.0],
    }
}

/// Where a press started this frame, if one did.
fn press_position(ctx: &egui::Context) -> Option<egui::Pos2> {
    ctx.input(|i| if i.pointer.any_pressed() { i.pointer.press_origin() } else { None })
}

/// `true` if a command-key shortcut, or copy, cut or paste, was pressed this frame.
pub(super) fn shortcut_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        i.events.iter().any(|e| match e {
            egui::Event::Key { pressed: true, modifiers, .. } => modifiers.command,
            egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) => true,
            _ => false,
        })
    })
}

/// The "New window" button beside the main window's view tabs: the window
/// glyph and its label.
pub(super) fn new_window_button(ui: &mut egui::Ui) -> egui::Response {
    // Room for the glyph before the text (an em space), painted over below.
    let response = ui.add(egui::Button::new("\u{2003} New window"));
    let size = 13.0 * theme::text_scale(ui);
    let padding = ui.spacing().button_padding.x;
    let glyph = egui::Rect::from_min_size(
        egui::pos2(response.rect.left() + padding, response.rect.center().y - size / 2.0),
        egui::vec2(size, size),
    );
    let color = ui.style().interact(&response).text_color();
    theme::paint_window_glyph(ui.painter(), glyph, color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "New window"));
    response
}

/// Shown in the main window where a view in its own window would be: what
/// it is, and buttons to bring its window to the front or the view back.
/// `reports` is how many reports the document has.
pub(super) fn away_panel(ui: &mut egui::Ui, doc: u64, view: View, reports: usize, actions: &mut Vec<Action>) {
    let (title, detail) = match view {
        View::Data => (
            "This data grid is in its own window",
            format!(
                "Edits, selections and undo work there as usual; the reports under Results ({reports}) and Side by \
                 side still open here."
            ),
        ),
        View::Results | View::Report(_) => (
            "The results are in their own window",
            format!("{} there; analyses you run are added to it.", count(reports, "report", "reports")),
        ),
        View::Dashboard(_) => (
            "This dashboard is in its own window",
            "Its tiles, filters and undo work there; selections made in its tiles show in their tables here."
                .to_owned(),
        ),
    };
    ui.add_space((ui.available_height() * 0.3).min(220.0));
    ui.vertical_centered(|ui| {
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
            ui.set_max_width(440.0);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.horizontal(|ui| {
                    theme::window_glyph(ui, ui.visuals().text_color());
                    ui.label(RichText::new(title).size(15.0 * theme::text_scale(ui)));
                });
                ui.label(RichText::new(detail).weak().small());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Show window").on_hover_text("Bring the window to the front").clicked() {
                        actions.push(Action::ShowWindow { doc, view });
                    }
                    if ui.button("Bring back here").on_hover_text("Close the window and show this view here").clicked()
                    {
                        actions.push(Action::BringBack { doc, view });
                    }
                });
            });
        });
    });
}

/// One line above the view Side by side still shows here when the other
/// view (`view`) is in its own window: what is away, and buttons to bring
/// its window to the front or the view back.
pub(super) fn away_note(ui: &mut egui::Ui, doc: u64, view: View, reports: usize, actions: &mut Vec<Action>) {
    let (text, detail) = match view {
        View::Data => ("The data grid is in its own window", String::new()),
        _ => ("The results are in their own window", format!("({})", count(reports, "report", "reports"))),
    };
    ui.horizontal_wrapped(|ui| {
        theme::window_glyph(ui, ui.visuals().weak_text_color());
        ui.label(RichText::new(text).weak());
        if !detail.is_empty() {
            ui.label(RichText::new(detail).weak());
        }
        if ui.small_button("Show window").on_hover_text("Bring the window to the front").clicked() {
            actions.push(Action::ShowWindow { doc, view });
        }
        if ui.small_button("Bring back here").on_hover_text("Close the window and show this view here").clicked() {
            actions.push(Action::BringBack { doc, view });
        }
    });
    ui.separator();
}

/// The columns of `table` as a list to drag onto a graph canvas's shelves
/// (drag and drop does not cross windows, so a canvas in its own window
/// cannot take columns from the main window's sidebar).
fn column_list(ui: &mut egui::Ui, table: &DataTable, doc: u64) {
    ui.add_space(4.0);
    ui.label(RichText::new("Columns").strong());
    ui.label(RichText::new("Drag onto a shelf").weak().small());
    egui::ScrollArea::vertical().id_salt(("window_columns", doc)).auto_shrink([false, false]).show(ui, |ui| {
        for col in table.columns() {
            let payload = ColumnDrag { name: col.name.clone(), from: None };
            ui.dnd_drag_source(egui::Id::new(("window_column", doc, col.name.as_str())), payload, |ui| {
                ui.horizontal(|ui| {
                    table_view::level_badge(ui, col.level);
                    ui.label(&col.name);
                })
            })
            .response
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text(format!(
                "{} — {} {}\nDrag onto a graph canvas shelf",
                col.name,
                col.level.label(),
                table_view::kind_description(col.kind())
            ));
        }
    });
}

impl StatsApp {
    /// `true` if view `view` of document `doc` (an id) has a window of its own.
    pub(super) fn in_window(&self, doc: u64, view: View) -> bool {
        self.windows.iter().any(|w| w.document == doc && w.view == view)
    }

    /// Show view `view` of document `doc` in a window of its own, or bring
    /// its window to the front if it has one.
    pub(super) fn pop_out(&mut self, doc: u64, view: View) {
        if !self.documents.iter().any(|d| d.id == doc) {
            return;
        }
        match self.windows.iter_mut().find(|w| w.document == doc && w.view == view) {
            Some(w) => w.raise = true,
            None => self.windows.push(ViewWindow::new(doc, view)),
        }
    }

    /// Close the window of view `view` of document `doc`, showing the view
    /// in the main window again (which switches to that document).
    pub(super) fn bring_back(&mut self, doc: u64, view: View) {
        let Some(i) = self.windows.iter().position(|w| w.document == doc && w.view == view) else { return };
        self.windows.remove(i);
        if self.last_window == Some((doc, view)) {
            self.last_window = None;
        }
        if let Some(index) = self.documents.iter().position(|d| d.id == doc) {
            self.reveal(index, view);
        }
    }

    /// Bring the window of view `view` of document `doc` to the front.
    pub(super) fn raise_window(&mut self, doc: u64, view: View) {
        for w in self.windows.iter_mut().filter(|w| w.document == doc && w.view == view) {
            w.raise = true;
        }
    }

    /// Bring every window of document `doc` to the front.
    pub(super) fn raise_windows_of(&mut self, doc: u64) {
        for w in self.windows.iter_mut().filter(|w| w.document == doc) {
            w.raise = true;
        }
    }

    /// Make view `view` of document `index` visible: bring its window (or
    /// the results window holding a report) to the front, or else show the
    /// document in the main window with the grid or the results in view.
    pub(super) fn reveal(&mut self, index: usize, view: View) {
        let Some(id) = self.documents.get(index).map(|d| d.id) else { return };
        let holder = match view {
            View::Report(_) if self.in_window(id, view) => Some(view),
            View::Report(_) | View::Results if self.in_window(id, View::Results) => Some(View::Results),
            View::Data if self.in_window(id, View::Data) => Some(View::Data),
            _ => None,
        };
        if let Some(holder) = holder {
            self.raise_window(id, holder);
            return;
        }
        self.shown = index;
        let doc = &mut self.documents[index];
        doc.workspace = match (view, doc.workspace) {
            (View::Data, Workspace::Results) => Workspace::SideBySide,
            (View::Results | View::Report(_), Workspace::Data) => Workspace::SideBySide,
            (_, current) => current,
        };
    }

    /// `true` if the command target's data grid is on screen (in its own
    /// window, or in the main window showing that document).
    pub(super) fn grid_on_screen(&self) -> bool {
        let Some(doc) = self.documents.get(self.active) else { return false };
        self.in_window(doc.id, View::Data) || (self.active == self.shown && doc.workspace != Workspace::Results)
    }

    /// The documents (by id) with a view on screen: the one the main window
    /// shows, the command target, and those with windows of their own.
    pub(super) fn on_screen_documents(&self) -> Vec<u64> {
        let mut ids: Vec<u64> =
            [self.shown, self.active].iter().filter_map(|&i| self.documents.get(i)).map(|d| d.id).collect();
        ids.extend(self.windows.iter().map(|w| w.document));
        let mut seen = Vec::new();
        ids.retain(|id| {
            let new = !seen.contains(id);
            seen.push(*id);
            new
        });
        ids
    }

    /// Keep document `id` up to date: refit analyses whose rows were
    /// excluded or re-included, remove reports closed with their ×, and
    /// carry out what its reports asked for (edit, new columns, reports and
    /// tables).
    /// Returns `true` if it queued a report to compute in the background.
    pub(super) fn service_document(&mut self, ctx: &egui::Context, id: u64) -> bool {
        let Some(index) = self.documents.iter().position(|d| d.id == id) else { return false };
        let queued = self.refresh_reports(index);
        let doc = &mut self.documents[index];
        if let Some(i) = doc.reports.iter().position(|r| r.remove_requested) {
            let report = &doc.reports[i];
            self.removed_report = Some(RemovedReport {
                document: doc.id,
                index: i,
                title: report.title(),
                saved: report.saved(),
                at: ctx.input(|i| i.time),
            });
        }
        doc.reports.retain(|r| !r.remove_requested);
        let edit = doc.reports.iter_mut().find(|r| r.edit_requested).map(|r| {
            r.edit_requested = false;
            r.id()
        });
        if let Some(report) = edit {
            // Its dialog opens in the main window, for this document.
            self.set_target(index);
            self.edit_report(report);
        }
        self.add_report_columns(index);
        let Some(doc) = self.documents.get_mut(index) else { return queued };
        let tiles: Vec<(app_model::project::SavedReport, String, Option<u64>)> = doc
            .reports
            .iter_mut()
            .filter_map(|r| r.dashboard_requested.take().map(|target| (r.saved(), r.title(), target)))
            .collect();
        for (saved, title, target) in tiles {
            self.add_report_tile(index, saved, &title, target);
        }
        let Some(doc) = self.documents.get_mut(index) else { return queued };
        let styles: Vec<(u64, crate::plot_style_menu::StyleRequest)> =
            doc.reports.iter_mut().filter_map(|r| r.style_request.take().map(|q| (r.id(), q))).collect();
        for (report, request) in styles {
            self.carry_out_style(ctx, index, report, request);
        }
        let Some(doc) = self.documents.get_mut(index) else { return queued };
        let duplicate =
            doc.reports.iter_mut().filter_map(|r| std::mem::take(&mut r.duplicate_requested).then(|| r.id())).last();
        if let Some(report) = duplicate {
            self.open_duplicate_dialog(index, report);
        }
        let Some(doc) = self.documents.get_mut(index) else { return queued };
        let export =
            doc.reports.iter_mut().filter_map(|r| std::mem::take(&mut r.export_requested).then(|| r.id())).last();
        if let Some(report) = export {
            self.open_export_dialog(index, Some(report));
        }
        let Some(doc) = self.documents.get_mut(index) else { return queued };
        let requested: Vec<ReportRequest> =
            doc.reports.iter_mut().flat_map(|r| std::mem::take(&mut r.new_reports)).collect();
        for request in requested {
            self.add_report_to(index, request);
        }
        let Some(doc) = self.documents.get_mut(index) else { return queued };
        let tables: Vec<(DataTable, bool)> =
            doc.reports.iter_mut().flat_map(|r| std::mem::take(&mut r.new_tables)).collect();
        for (table, keep) in tables {
            self.open_report_table(id, table, keep);
        }
        let Some(doc) = self.documents.get_mut(index) else { return queued };
        let derived: Vec<app_model::table_ops::TableOperation> =
            doc.reports.iter_mut().flat_map(|r| std::mem::take(&mut r.new_derived)).collect();
        for op in derived {
            self.make_derived(index, &op);
        }
        queued
    }

    /// Bring document `index`'s reports up to date with its data: computed
    /// in the background when workers run (returns `true` if any was
    /// queued), otherwise here and now.
    fn refresh_reports(&mut self, index: usize) -> bool {
        let doc = &mut self.documents[index];
        let Some(jobs) = &mut self.report_jobs else {
            for report in &mut doc.reports {
                report.refresh_if_stale(&doc.table, &doc.rows);
            }
            return false;
        };
        // One snapshot of the row states serves every report queued now.
        let mut rows: Option<Arc<RowStateManager>> = None;
        for report in &mut doc.reports {
            if report.wants_refresh(&doc.table, &doc.rows) {
                let rows = rows.get_or_insert_with(|| Arc::new(doc.rows.clone()));
                report.queue_refresh(jobs, Arc::clone(&doc.table), Arc::clone(rows));
            }
        }
        rows.is_some()
    }

    /// Install the reports computed in the background, waiting up to `wait`
    /// for those still running.
    /// Returns `true` if a result was dropped because its report changed
    /// meanwhile (it is queued again on the next frame).
    pub(super) fn install_report_jobs(&mut self, wait: Option<std::time::Duration>) -> bool {
        let Some(jobs) = &mut self.report_jobs else { return false };
        if !jobs.busy() {
            return false;
        }
        let mut dropped = false;
        for done in jobs.take(wait) {
            let report = done.report();
            let target = self.documents.iter_mut().find_map(|d| {
                let i = d.reports.iter().position(|r| r.id() == report)?;
                Some((d, i))
            });
            if let Some((doc, i)) = target {
                dropped |= !doc.reports[i].install(done, &doc.table, &doc.rows);
            }
        }
        dropped
    }

    /// Take the reports' requests to open or leave a window of their own,
    /// drop windows whose document or report has gone, and tell each report
    /// whether it is in its own window.
    pub(super) fn sync_report_windows(&mut self) {
        self.sync_dashboard_windows();
        let mut moves = Vec::new();
        let mut raise = Vec::new();
        for doc in &mut self.documents {
            for report in &mut doc.reports {
                let view = View::Report(report.id());
                if std::mem::take(&mut report.window_requested) {
                    moves.push((doc.id, view, report.in_own_window));
                }
                if std::mem::take(&mut report.show_window_requested) {
                    raise.push((doc.id, view));
                }
            }
        }
        for (doc, view, inside) in moves {
            if inside {
                self.bring_back(doc, view);
            } else {
                self.pop_out(doc, view);
            }
        }
        for (doc, view) in raise {
            self.raise_window(doc, view);
        }
        let documents = &self.documents;
        self.windows.retain(|w| {
            documents.iter().find(|d| d.id == w.document).is_some_and(|d| match w.view {
                View::Report(r) => d.reports.iter().any(|x| x.id() == r),
                View::Data | View::Results | View::Dashboard(_) => true,
            })
        });
        let windows = &self.windows;
        for doc in &mut self.documents {
            for report in &mut doc.reports {
                let view = View::Report(report.id());
                report.in_own_window = windows.iter().any(|w| w.document == doc.id && w.view == view);
            }
        }
    }

    /// Follow the main window's focus and presses: a press in its workspace
    /// (view tabs, grid, results) makes the document it shows the command
    /// target. Presses in the menu bar or sidebar leave the target alone,
    /// so the menus act on the document of the window used before.
    pub(super) fn track_main_window(&mut self, ctx: &egui::Context) {
        if !ctx.embed_viewports() {
            let focused = ctx.input(|i| i.viewport().focused).unwrap_or(true);
            if focused && !self.main_focused {
                self.main_focus_pending = true;
                self.last_window = None;
            }
            self.main_focused = focused;
        }
        let Some(pos) = press_position(ctx) else { return };
        // Presses on dialogs, menus and (drawn inside the main window) other windows do not count.
        if ctx.layer_id_at(pos).is_some_and(|layer| layer.order != egui::Order::Background) {
            return;
        }
        self.main_focus_pending = false;
        self.last_window = None;
        if self.workspace_rect.is_some_and(|r| r.contains(pos))
            && self.shown_dashboard().is_none()
            && self.active != self.shown
            && self.shown < self.documents.len()
        {
            self.set_target(self.shown);
        }
    }

    /// Bring the main window to the front when a dialog opens (dialogs open
    /// there, even when asked for from another window).
    pub(super) fn raise_main_for_new_dialog(&mut self, ctx: &egui::Context) {
        let n = self.dialog_count();
        if n > self.open_dialogs && !ctx.embed_viewports() && !self.windows.is_empty() {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        }
        self.open_dialogs = n;
    }

    /// How many dialogs are open.
    fn dialog_count(&self) -> usize {
        macro_rules! open {
            ($($slot:ident),* $(,)?) => {
                0 $(+ usize::from(self.$slot.is_some()))*
            };
        }
        open!(
            table_op_dialog,
            formula_dialog,
            recode_dialog,
            external_dialog,
            design_dialog,
            sheet_dialog,
            mixture_design_dialog,
            phase2_dialog,
            method_dialog,
            factor_dialog,
            discriminant_dialog,
            map_dialog,
            text_dialog,
            imputation_dialog,
            mixed_dialog,
            nonlinear_dialog,
            curve_template_dialog,
            tree_dialog,
            forest_dialog,
            boost_dialog,
            pls_dialog,
            svm_dialog,
            mars_dialog,
            neural_dialog,
            gaussian_process_dialog,
            search_dialog,
            functional_dialog,
            model_comparison_dialog,
            survival_dialog,
            reliability_dialog,
            forecast_dialog,
            compare_dialog,
            dialog,
            chart_dialog,
            capability_dialog,
            quality_tool_dialog,
            study_dialog,
            agreement_dialog,
            bayes_dialog,
            treatment_dialog,
            evaluation_dialog,
            tabulation_dialog,
            factorial_dialog,
            sequential_dialog,
            plot3d_dialog,
            permutation_dialog,
            bootstrap_dialog,
            mixture_dialog,
            augment_dialog,
            simulate_dialog,
            attribute_dialog,
            pareto_dialog,
            cause_effect_dialog,
            multi_vari_dialog,
            multivariate_dialog,
            anom_dialog,
            regression_dialog,
            time_weighted_dialog,
            gauge_dialog,
            distribution_dialog,
            distribution_fit_dialog,
            bivariate_dialog,
        )
    }

    /// The title of view `view` of document `index`'s window.
    fn window_title(&self, index: usize, view: View) -> String {
        let doc = &self.documents[index];
        let what = match view {
            View::Data => "Data".to_owned(),
            View::Results => "Results".to_owned(),
            View::Report(id) => {
                doc.reports.iter().find(|r| r.id() == id).map_or_else(|| "Report".to_owned(), Report::title)
            }
            View::Dashboard(id) => {
                let name = self.dashboard(id).map_or("Dashboard", |d| d.dashboard.name.as_str());
                return format!("{name} · Dashboard · AnovaCraft");
            }
        };
        format!("{} · {what} · AnovaCraft", doc.name())
    }

    /// A small window mark after document `doc`'s name (sidebar list and
    /// tabs) when any of its views is in a window of its own, with a
    /// tooltip naming them.
    pub(super) fn window_mark(&self, ui: &mut egui::Ui, doc: &Document) {
        let views: Vec<String> = self
            .windows
            .iter()
            .filter(|w| w.document == doc.id && !matches!(w.view, View::Dashboard(_)))
            .map(|w| match w.view {
                View::Data => "the data grid".to_owned(),
                View::Results => "the results".to_owned(),
                View::Report(id) => doc
                    .reports
                    .iter()
                    .find(|r| r.id() == id)
                    .map_or_else(|| "a report".to_owned(), |r| format!("the report “{}”", r.title())),
                View::Dashboard(_) => "a dashboard".to_owned(),
            })
            .collect();
        if views.is_empty() {
            return;
        }
        let tip = format!(
            "In a window of its own: {}. Click the dataset when it is already selected to bring its windows to the front.",
            views.join(", ")
        );
        let color = ui.visuals().text_color();
        theme::window_glyph(ui, color)
            .on_hover_text(tip.clone())
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "In its own window"));
    }

    /// Draw each view's window, and carry out what was asked for there on
    /// its document.
    pub(super) fn show_windows(&mut self, ctx: &egui::Context) {
        let keys: Vec<(u64, View)> = self.windows.iter().map(|w| (w.document, w.view)).collect();
        for (doc_id, view) in keys {
            let Some(w) = self.windows.iter().position(|w| w.document == doc_id && w.view == view) else { continue };
            let Some(index) = self.documents.iter().position(|d| d.id == doc_id) else { continue };
            if matches!(view, View::Dashboard(d) if self.drawn_dashboard == Some(d)) {
                // Popped out after the main window drew it this frame: drawn from the next one.
                ctx.request_repaint();
                continue;
            }
            let title = self.window_title(index, view);
            let viewport = viewport_id(doc_id, view);
            let raise = std::mem::take(&mut self.windows[w].raise);
            let was_focused = self.windows[w].focused;
            let embedded = ctx.embed_viewports();
            if raise && !embedded {
                ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Focus);
            }
            // A saved place, fitted onto this screen.
            let place = Self::fitted_place(ctx, self.windows[w].place);
            let mut builder = egui::ViewportBuilder::default()
                .with_title(&title)
                .with_inner_size(place.map_or(default_size(view), |r| [r.width(), r.height()]))
                .with_min_inner_size([360.0, 240.0]);
            if let Some(place) = place {
                builder = builder.with_position(place.min);
            }
            let mut rect = None;
            let mut out = WindowOutcome::default();
            ctx.show_viewport_immediate(viewport, builder, |ctx, class| {
                if class == egui::ViewportClass::Embedded {
                    let mut open = true;
                    let id = egui::Id::new(("view_window", window_key(doc_id, view)));
                    let size = place.map_or(default_size(view), |r| [r.width(), r.height()]);
                    // Drawn inside the main window: each a little below and right of the last.
                    let pos = place.map_or(egui::pos2(250.0 + 70.0 * w as f32, 110.0 + 50.0 * w as f32), |r| r.min);
                    let shown = egui::Window::new(&title)
                        .id(id)
                        .open(&mut open)
                        .default_size(size)
                        .default_pos(pos)
                        .collapsible(false)
                        .resizable(true)
                        .show(ctx, |ui| self.window_contents(ui, index, view, &mut out));
                    rect = shown.as_ref().map(|s| s.response.rect);
                    self.export_dialog_window(ctx, Some((doc_id, view)));
                    if raise {
                        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
                    }
                    if !open {
                        out.back = true;
                    }
                    if let (Some(shown), Some(pos)) = (shown, press_position(ctx)) {
                        let layer = shown.response.layer_id;
                        out.used = shown.response.rect.contains(pos) && ctx.layer_id_at(pos) == Some(layer);
                    }
                } else {
                    // A native window of its own, with its own keys.
                    rect = ctx.input(|i| i.viewport().outer_rect);
                    let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
                    out.focused = focused;
                    out.used = (focused && !was_focused) || ctx.input(|i| i.pointer.any_pressed());
                    let mut keys = Vec::new();
                    self.keyboard_shortcuts(ctx, &mut keys, index, Some((doc_id, view)));
                    let frame = egui::Frame::central_panel(&ctx.style()).inner_margin(0);
                    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
                        self.window_contents(ui, index, view, &mut out);
                    });
                    self.export_dialog_window(ctx, Some((doc_id, view)));
                    out.actions.splice(0..0, keys);
                    if ctx.input(|i| i.viewport().close_requested()) {
                        out.back = true;
                    }
                }
            });
            if let Some(w) = self.windows.iter_mut().find(|w| w.document == doc_id && w.view == view) {
                w.focused = out.focused;
                w.rect = rect.or(w.rect);
            }
            // A dashboard's window keeps the target on the table of its project last used.
            let keep = matches!(view, View::Dashboard(_))
                && self.documents.get(self.active).map(|d| d.project) == Some(self.documents[index].project);
            if out.used {
                self.last_window = Some((doc_id, view));
                if !keep {
                    self.set_target(index);
                }
            }
            if !out.actions.is_empty() {
                if !keep {
                    self.set_target(index);
                }
                self.acting_window = Some((doc_id, view));
                for action in out.actions {
                    self.perform(ctx, action);
                }
                self.acting_window = None;
            }
            if out.back {
                match view {
                    View::Dashboard(d) => self.bring_back_dashboard(d),
                    _ => self.bring_back(doc_id, view),
                }
            }
        }
    }

    /// The inside of view `view` of document `index`'s window: a bar with
    /// "Back to main window", the view, a column list beside a graph
    /// canvas, and the row counts along the bottom.
    fn window_contents(&mut self, ui: &mut egui::Ui, index: usize, view: View, out: &mut WindowOutcome) {
        if let View::Dashboard(d) = view {
            return self.dashboard_window_contents(ui, d, &mut out.back, &mut out.actions);
        }
        let choices = std::sync::Arc::new(self.dashboard_choices(index));
        ui.ctx().data_mut(|d| d.insert_temp(report_view::dashboard_choices_id(), choices));
        let stale = self.documents.get(index).and_then(|d| self.stale.get(&d.id)).cloned();
        let Some(doc) = self.documents.get_mut(index) else { return };
        let id = doc.id;
        egui::TopBottomPanel::top(egui::Id::new(("view_window_bar", id, view))).show_inside(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Back to main window")
                    .on_hover_text("Close this window and show its contents in the main window again")
                    .clicked()
                {
                    out.back = true;
                }
            });
        });
        egui::TopBottomPanel::bottom(egui::Id::new(("view_window_status", id, view))).show_inside(ui, |ui| {
            let c = doc.rows.counts();
            let mut parts = vec![format!("{} selected", c.selected)];
            if c.excluded > 0 {
                parts.push(format!("{} excluded", c.excluded));
            }
            if c.hidden > 0 {
                parts.push(format!("{} hidden", c.hidden));
            }
            ui.label(
                RichText::new(format!("{} · {} · {}", doc.name(), count(c.total, "row", "rows"), parts.join(" · ")))
                    .weak()
                    .small(),
            );
        });
        let graph = match view {
            View::Data | View::Dashboard(_) => false,
            View::Results => doc.reports.iter().any(|r| r.is_graph() && !r.in_own_window),
            View::Report(r) => doc.reports.iter().any(|x| x.id() == r && x.is_graph()),
        };
        if graph {
            egui::SidePanel::left(egui::Id::new(("view_window_columns", id, view)))
                .resizable(true)
                .default_width(170.0)
                .show_inside(ui, |ui| column_list(ui, &doc.table, id));
        }
        if view == View::Data {
            if let Some(check) = stale.filter(|s| !s.changed.is_empty()) {
                egui::TopBottomPanel::top(egui::Id::new(("view_window_banner", id))).show_inside(ui, |ui| {
                    super::projects::banner_contents(ui, id, &check, &mut out.actions);
                });
            }
        }
        let undo = doc.history.next_undo().map(|(l, _)| l.to_owned());
        let redo = doc.history.next_redo().map(|(l, _)| l.to_owned());
        let (table, rows, reports, grid) = (&doc.table, &mut doc.rows, &mut doc.reports, &mut doc.grid);
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.push_id(("view_window", id, view), |ui| match view {
                View::Data => {
                    let requests = table_view::show(ui, table, rows, grid, undo.as_deref(), redo.as_deref());
                    if !requests.is_empty() {
                        out.actions.push(Action::Grid(requests));
                    }
                }
                View::Results => {
                    if report_view::show_reports(ui, reports, table, rows) {
                        out.actions.push(Action::ExportReports(index));
                    }
                }
                View::Report(r) => match reports.iter_mut().find(|x| x.id() == r) {
                    Some(report) => report_view::show_single_report(ui, report, table, rows),
                    None => {
                        ui.label(RichText::new("This report was removed.").weak());
                    }
                },
                View::Dashboard(_) => {}
            });
        });
    }
}
