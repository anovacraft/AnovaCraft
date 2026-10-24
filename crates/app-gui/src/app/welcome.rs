//! The start page, shown whenever no table is open (including at startup):
//! ways to start a table beside the recent projects, a grid of common tasks,
//! and the sample data, the user manual and a tip.
//!
//! A task that needs data, chosen with no table open, brings up a small
//! "Open data for …" dialog in front ([`StatsApp::task_data_dialog`]) while
//! it waits ([`StatsApp::pending_task`]); the task's dialog opens once a table
//! is opened from there. Cancelling, or opening data some other way, drops
//! the task, so nothing waits out of sight.

use std::path::{Path, PathBuf};

use app_model::settings::{RecentContents, RecentDetails};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use eframe::egui::Color32;

use super::*;
use crate::icons::{self, Icon};

/// Widest the page's column grows, at the standard text size.
const PAGE_WIDTH: f32 = 900.0;
/// Recent files listed before "Show all recent".
const RECENT_SHOWN: usize = 4;

/// A common task the start page offers, opening an analysis or design dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Task {
    /// Describe ▸ Distribution.
    Describe,
    /// Compare ▸ Two or more groups.
    CompareGroups,
    /// Model ▸ Linear model.
    FitModel,
    /// Quality ▸ Control chart.
    ControlChart,
    /// Series and Signals ▸ Exponential smoothing.
    Forecast,
    /// A design from the Design menu (no data needed).
    Design(DesignChoice),
}

/// A design the start page's design list can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DesignChoice {
    /// Entry `n` of [`design_dialog::KIND_NAMES`].
    Kind(usize),
    /// A split-plot design.
    SplitPlot,
    /// A simplex lattice (`false`) or simplex centroid (`true`) mixture design.
    Mixture(bool),
    /// Extreme vertices of a constrained mixture region.
    ExtremeVertices,
}

impl Task {
    /// What the task is called on its card and in the note asking for data.
    fn title(self) -> &'static str {
        match self {
            Self::Describe => "Describe a column",
            Self::CompareGroups => "Compare groups",
            Self::FitModel => "Fit a model",
            Self::ControlChart => "Watch a process",
            Self::Forecast => "Forecast a series",
            Self::Design(_) => "Design an experiment",
        }
    }

    /// Whether the task needs an open table.
    pub(super) fn needs_data(self) -> bool {
        !matches!(self, Self::Design(_))
    }
}

/// The page's colours for the light or dark appearance.
struct Colors {
    card: Color32,
    stroke: Color32,
    accent: Color32,
    accent_soft: Color32,
    tip_fill: Color32,
    tip_ink: Color32,
}

impl Colors {
    fn of(ui: &egui::Ui) -> Self {
        let dark = ui.visuals().dark_mode;
        let (tip_ink, tip_fill) = theme::stale_colors(dark);
        let (card, stroke, accent_soft) = if dark {
            (Color32::from_gray(34), Color32::from_gray(60), Color32::from_rgb(30, 51, 71))
        } else {
            (Color32::WHITE, Color32::from_gray(190), Color32::from_rgb(227, 238, 248))
        };
        Self { card, stroke, accent: theme::start_accent(dark), accent_soft, tip_fill, tip_ink }
    }
}

/// Tips the page shows one at a time; Next moves to the next.
pub(super) const TIPS: [&str; 8] = [
    "Drag near either end of a plot axis to move that limit; drag its middle to shift the whole axis.",
    "Select points in any plot, or rows in the grid, and the same rows light up everywhere.",
    "Right-click column headings, row numbers, report headings and plots for what you can do with them.",
    "Side by side (⌘3 / Ctrl+3) shows the data next to the results, so you can watch both.",
    "Drag a plot's bottom edge or right edge to resize it; double-click the edge to restore its size.",
    "Each report's Options menu can filter its rows or switch it to another column without touching the table.",
    "Project notes in the sidebar are saved with the project; their first line shows here under Recent projects.",
    "File ▸ Export reports saves every report as a PDF, Word, PowerPoint, HTML, Markdown or Excel file.",
];

/// What the start page needs from the application.
pub(super) struct WelcomeData {
    /// Recent files, most recent first, with what is known about each.
    pub recent: Vec<(PathBuf, Option<RecentDetails>)>,
}

/// One clickable row or card: `icon`, a title (with an optional tag after
/// it) over a faint line, inside a frame that is highlighted while hovered.
/// Named for screen readers and tests by its title.
fn tile(
    ui: &mut egui::Ui,
    colors: &Colors,
    framed: bool,
    icon: Icon,
    title: (&str, Option<&str>),
    detail: &str,
    width: f32,
) -> egui::Response {
    tile_sized(ui, colors, framed, icon, title, detail, width, 0.0).0
}

/// [`tile`] at least `min_height` tall, also returning the height its
/// contents need, so a row of cards can be given the tallest one's height.
/// The tag stays on the title's line when both fit and moves under the
/// title otherwise; it never wraps.
#[allow(clippy::too_many_arguments)]
fn tile_sized(
    ui: &mut egui::Ui,
    colors: &Colors,
    framed: bool,
    icon: Icon,
    (title, tag): (&str, Option<&str>),
    detail: &str,
    width: f32,
    min_height: f32,
) -> (egui::Response, f32) {
    let scale = theme::text_scale(ui);
    let margin = egui::Margin::symmetric(10, 8);
    let frame = egui::Frame::NONE
        .inner_margin(margin)
        .corner_radius(4)
        .fill(if framed { colors.card } else { Color32::TRANSPARENT })
        .stroke(if framed { egui::Stroke::new(1.0_f32, colors.stroke) } else { egui::Stroke::NONE });
    let mut prepared = frame.begin(ui);
    let natural;
    {
        let ui = &mut prepared.content_ui;
        ui.style_mut().interaction.selectable_labels = false;
        ui.set_width(width - 20.0);
        let contents = ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            icons::show(ui, icon, 18.0 * scale, colors.accent);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let title_text = RichText::new(title).color(ui.visuals().strong_text_color());
                let badge = |ui: &mut egui::Ui, tag: &str| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(4, 0))
                        .corner_radius(3)
                        .stroke(egui::Stroke::new(1.0_f32, colors.accent))
                        .show(ui, |ui| {
                            let text = RichText::new(tag).size(detail_size(ui) * 0.95).color(colors.accent);
                            ui.add(egui::Label::new(text).extend())
                        });
                };
                match tag {
                    Some(tag) => {
                        let text_width = |ui: &egui::Ui, text: RichText| {
                            egui::WidgetText::from(text)
                                .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body)
                                .size()
                                .x
                        };
                        let tag_width =
                            text_width(ui, RichText::new(tag).size(detail_size(ui) * 0.95)) + 8.0 + 2.0;
                        let needed = text_width(ui, title_text.clone()) + ui.spacing().item_spacing.x + tag_width;
                        if needed <= ui.available_width() {
                            ui.horizontal(|ui| {
                                ui.label(title_text);
                                badge(ui, tag);
                            });
                        } else {
                            ui.add(egui::Label::new(title_text).wrap());
                            badge(ui, tag);
                        }
                    }
                    None => {
                        ui.add(egui::Label::new(title_text).wrap());
                    }
                }
                if !detail.is_empty() {
                    ui.add(egui::Label::new(self::detail(ui, detail)).wrap());
                }
            });
        });
        natural = contents.response.rect.height() + margin.sum().y;
        ui.set_min_height((min_height - margin.sum().y).max(0.0));
    }
    let rect = prepared.allocate_space(ui).rect;
    // The Import row only heads its sources: it is labelled text, not a button.
    let clickable = title != "Import";
    let sense = if clickable { egui::Sense::click() } else { egui::Sense::hover() };
    let response = ui.interact(rect, ui.id().with(("start_tile", title)), sense);
    if clickable && response.hovered() {
        prepared.frame.fill = colors.accent_soft;
        prepared.frame.stroke = egui::Stroke::new(1.5_f32, colors.accent);
    }
    prepared.paint(ui);
    let kind = if clickable { egui::WidgetType::Button } else { egui::WidgetType::Label };
    response.widget_info(|| egui::WidgetInfo::labeled(kind, true, title));
    (if clickable { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }, natural)
}

/// A grey button with a Lucide icon before its label (`detail`, when given,
/// follows the label in faint text), named by its label.
fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str, detail: &str, size: f32, color: Color32) -> egui::Response {
    let mut job = egui::text::LayoutJob::default();
    let font = egui::TextStyle::Button.resolve(ui.style());
    job.append(label, 0.0, egui::TextFormat::simple(font, ui.visuals().strong_text_color()));
    if !detail.is_empty() {
        let small = egui::FontId::proportional(detail_size(ui));
        job.append(detail, 8.0, egui::TextFormat::simple(small, ui.visuals().weak_text_color()));
    }
    let response = match icons::image(ui.ctx(), icon, size, color) {
        Some(image) => ui.add(egui::Button::image_and_text(image, job)),
        None => ui.add(egui::Button::new(job)),
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

/// Size of the faint second lines and notes on the page.
fn detail_size(ui: &egui::Ui) -> f32 {
    11.0 * theme::text_scale(ui)
}

/// Faint detail text.
fn detail(ui: &egui::Ui, text: impl Into<String>) -> RichText {
    RichText::new(text).size(detail_size(ui)).weak()
}

/// A section heading with an optional faint note after it.
fn heading(ui: &mut egui::Ui, text: &str, note: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(text).strong().size(14.0 * theme::text_scale(ui)));
        if !note.is_empty() {
            ui.label(detail(ui, note));
        }
    });
}

/// "Today, 14:32", "Yesterday, 09:10" or "15 Oct 2026": when a recent file
/// was last used, seen from `now` (both local times).
pub(super) fn when_label(when: NaiveDateTime, now: NaiveDateTime) -> String {
    let (day, today) = (when.date(), now.date());
    if day == today {
        format!("Today, {}", when.format("%H:%M"))
    } else if today.pred_opt() == Some(day) {
        format!("Yesterday, {}", when.format("%H:%M"))
    } else {
        when.format("%-d %b %Y").to_string()
    }
}

/// The local time of `seconds` since 1970 UTC.
fn local_time(seconds: i64) -> Option<NaiveDateTime> {
    Local.timestamp_opt(seconds, 0).single().map(|t: DateTime<Local>| t.naive_local())
}

/// The lines describing recent file `path`: what it holds, and the first
/// line of its notes (projects only; "No notes" without them).
pub(super) fn recent_lines(path: &Path, details: Option<&RecentDetails>) -> (String, Option<String>) {
    let is_project = path.extension().is_some_and(|e| e.eq_ignore_ascii_case(app_model::project::PROJECT_EXTENSION));
    match details.map(|d| &d.contents) {
        Some(RecentContents::Data { rows, columns }) => (
            format!(
                "Data file: {}, {}",
                count_grouped(*rows, "row", "rows"),
                count_grouped(*columns, "column", "columns")
            ),
            None,
        ),
        Some(RecentContents::Project { tables, notes }) => {
            let noun = if is_project { ("table", "tables") } else { ("sheet", "sheets") };
            let mut meta = format!("{}: {}", count(tables.len(), noun.0, noun.1), tables.join(", "));
            if !is_project {
                meta.insert_str(0, "Workbook, ");
            }
            let notes = is_project.then(|| if notes.is_empty() { String::new() } else { format!("“{notes}”") });
            (meta, notes)
        }
        None => (path.parent().map(|p| p.display().to_string()).unwrap_or_default(), None),
    }
}

/// `n` with thousands separated by commas and its noun ("2,400 rows").
fn count_grouped(n: usize, one: &str, many: &str) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{out} {}", if n == 1 { one } else { many })
}

/// One recent file: an icon (folder for a project, page for a data file),
/// its name and when it was used, what it holds and its notes.
fn recent_row(
    ui: &mut egui::Ui,
    colors: &Colors,
    path: &Path,
    details: Option<&RecentDetails>,
    now: NaiveDateTime,
) -> egui::Response {
    let scale = theme::text_scale(ui);
    let is_project = path.extension().is_some_and(|e| e.eq_ignore_ascii_case(app_model::project::PROJECT_EXTENSION));
    let name = if is_project {
        path.file_stem().map(|n| n.to_string_lossy().into_owned())
    } else {
        path.file_name().map(|n| n.to_string_lossy().into_owned())
    }
    .unwrap_or_else(|| path.display().to_string());
    let (meta, notes) = recent_lines(path, details);
    let when = details.and_then(|d| local_time(d.when)).map(|t| when_label(t, now));
    let mut prepared = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(8, 6)).corner_radius(4).begin(ui);
    {
        let ui = &mut prepared.content_ui;
        ui.style_mut().interaction.selectable_labels = false;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let icon = if is_project { Icon::Folder } else { Icon::FileText };
            icons::show(ui, icon, 18.0 * scale, colors.accent);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(&name).color(ui.visuals().strong_text_color())).truncate());
                    if let Some(when) = &when {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(detail(ui, when));
                        });
                    }
                });
                ui.add(egui::Label::new(RichText::new(&meta).size(detail_size(ui))).truncate());
                match notes.as_deref() {
                    Some("") => {
                        ui.label(detail(ui, "No notes").italics());
                    }
                    Some(line) => {
                        ui.add(egui::Label::new(detail(ui, line).italics()).truncate());
                    }
                    None => {}
                }
            });
        });
    }
    let rect = prepared.allocate_space(ui).rect;
    let response = ui.interact(rect, ui.id().with(("recent", path)), egui::Sense::click());
    if response.hovered() {
        prepared.frame.fill = colors.accent_soft;
    }
    prepared.paint(ui);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    response.on_hover_text(path.display().to_string()).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The Start box: new table, open, import (four sources), paste.
fn start_box(ui: &mut egui::Ui, colors: &Colors, width: f32, actions: &mut Vec<Action>) {
    let open_keys = ui.ctx().format_shortcut(&OPEN_SHORTCUT);
    heading(ui, "Start", "");
    egui::Frame::NONE
        .fill(colors.card)
        .stroke(egui::Stroke::new(1.0_f32, colors.stroke))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(4, 6))
        .show(ui, |ui| {
            ui.set_width(width - 8.0);
            let inner = width - 8.0;
            ui.spacing_mut().item_spacing.y = 2.0;
            if tile(
                ui,
                colors,
                false,
                Icon::Table,
                ("New data table", None),
                "An empty table to type or paste into",
                inner,
            )
            .clicked()
            {
                actions.push(Action::NewTable);
            }
            let open_detail = format!("Projects (.acraft), CSV, Excel and JSON files  ·  {open_keys}");
            if tile(ui, colors, false, Icon::FolderOpen, ("Open a file or project…", None), &open_detail, inner)
                .clicked()
            {
                actions.push(Action::PickAndOpen);
            }
            // A heading for the sources below it, not a button.
            tile(ui, colors, false, Icon::Download, ("Import", None), "Bring data in from another source", inner)
                .on_hover_text("Choose a source below");
            ui.horizontal(|ui| {
                ui.add_space(38.0 * theme::text_scale(ui));
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(inner - 50.0 * theme::text_scale(ui));
                    let chip = |ui: &mut egui::Ui, icon, label| {
                        let color = ui.visuals().text_color();
                        icon_button(ui, icon, label, "", 14.0 * theme::text_scale(ui), color)
                    };
                    if chip(ui, Icon::FileText, "Text (CSV, TSV)").on_hover_text("Delimited text files").clicked() {
                        actions.push(Action::OpenOfKind(file_io::FileKind::Text));
                    }
                    if chip(ui, Icon::Sheet, "Spreadsheet workbook")
                        .on_hover_text("Excel or OpenDocument workbooks; each sheet with data becomes a table")
                        .clicked()
                    {
                        actions.push(Action::OpenOfKind(file_io::FileKind::Workbook));
                    }
                    if chip(ui, Icon::Database, "Database…")
                        .on_hover_text(
                            "Rows of an SQL query: SQLite files, PostgreSQL, MySQL or MariaDB servers, ODBC data \
                             sources",
                        )
                        .clicked()
                    {
                        actions.push(Action::ImportExternal(crate::external_dialog::ExternalKind::Database));
                    }
                    if chip(ui, Icon::Globe, "Web page…")
                        .on_hover_text("JSON or CSV fetched from an address (HTTP GET), with optional headers")
                        .clicked()
                    {
                        actions.push(Action::ImportExternal(crate::external_dialog::ExternalKind::Web));
                    }
                });
            });
            ui.add_space(2.0);
            if tile(
                ui,
                colors,
                false,
                Icon::ClipboardPaste,
                ("Paste from clipboard", None),
                "Cells copied from a spreadsheet; a first line of names becomes the columns",
                inner,
            )
            .clicked()
            {
                actions.push(Action::NewTableFromClipboard);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(detail(ui, "You can also drop files onto this window."));
            });
        });
}

/// The Recent projects box: the latest few, and the rest behind "Show all recent".
fn recent_box(ui: &mut egui::Ui, colors: &Colors, width: f32, data: &WelcomeData, actions: &mut Vec<Action>) {
    heading(ui, "Recent projects", "most recent first");
    let show_all_id = egui::Id::new("start_show_all_recent");
    let show_all = ui.ctx().data(|d| d.get_temp::<bool>(show_all_id)).unwrap_or(false);
    let now = Local::now().naive_local();
    egui::Frame::NONE
        .fill(colors.card)
        .stroke(egui::Stroke::new(1.0_f32, colors.stroke))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(4, 6))
        .show(ui, |ui| {
            ui.set_width(width - 8.0);
            if data.recent.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new("Projects and files you open or save are listed here.").weak());
                ui.add_space(6.0);
                return;
            }
            let shown = if show_all { data.recent.len() } else { data.recent.len().min(RECENT_SHOWN) };
            for (k, (path, details)) in data.recent.iter().take(shown).enumerate() {
                if k > 0 {
                    ui.separator();
                }
                if recent_row(ui, colors, path, details.as_ref(), now).clicked() {
                    actions.push(Action::OpenPaths(vec![path.clone()]));
                }
            }
            if data.recent.len() > RECENT_SHOWN {
                let text = if show_all {
                    "Show fewer".to_owned()
                } else {
                    format!("Show all recent ({})…", data.recent.len())
                };
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    if ui.link(RichText::new(text).size(detail_size(ui))).clicked() {
                        ui.ctx().data_mut(|d| d.insert_temp(show_all_id, !show_all));
                    }
                });
            }
        });
}

/// The design list under "Design an experiment": the Design menu's designs
/// by group.
fn design_list(ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    ui.set_min_width(260.0);
    // Entries look like a menu's.
    ui.style_mut().visuals.button_frame = false;
    let mut pick = |ui: &mut egui::Ui, label: String, choice: DesignChoice| {
        if ui.button(label).clicked() {
            actions.push(Action::StartTask(Task::Design(choice)));
            ui.memory_mut(egui::Memory::close_popup);
        }
    };
    for (group, kinds) in DESIGN_GROUPS {
        ui.label(RichText::new(group).weak().small());
        for &i in kinds {
            pick(ui, format!("{}…", crate::design_dialog::KIND_NAMES[i]), DesignChoice::Kind(i));
        }
        if group == "Optimal" {
            pick(ui, "Split-plot (hard-to-change factors)…".to_owned(), DesignChoice::SplitPlot);
        }
    }
    ui.label(RichText::new("Mixture").weak().small());
    pick(ui, "Simplex lattice…".to_owned(), DesignChoice::Mixture(false));
    pick(ui, "Simplex centroid…".to_owned(), DesignChoice::Mixture(true));
    pick(ui, "Extreme vertices…".to_owned(), DesignChoice::ExtremeVertices);
}

/// The sample list under "Sample data": one folding group per topic, then
/// the sample projects.
fn sample_list(ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    ui.set_min_width(420.0);
    ui.style_mut().visuals.button_frame = false;
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        let catalog = app_model::sample::catalog();
        for topic in app_model::sample::SampleTopic::ALL {
            let n_topic = catalog.iter().filter(|s| s.topic == topic).count();
            egui::CollapsingHeader::new(format!("{} ({n_topic})", topic.label()))
                .id_salt(("start_samples", topic.label()))
                .show(ui, |ui| {
                    for (n, info) in catalog.iter().enumerate().filter(|(_, s)| s.topic == topic) {
                        let (dataset, uses) = info.name.split_once(" — ").unwrap_or((info.name, ""));
                        let label = if uses.is_empty() { dataset.to_owned() } else { format!("{dataset} — {uses}") };
                        if ui.button(label).on_hover_text(info.description).clicked() {
                            actions.push(Action::OpenSample(n));
                            ui.memory_mut(egui::Memory::close_popup);
                        }
                    }
                });
        }
        egui::CollapsingHeader::new("Projects").id_salt("start_sample_projects").show(ui, |ui| {
            for (n, info) in app_model::sample::project_samples().iter().enumerate() {
                if ui.button(info.name).on_hover_text(info.description).clicked() {
                    actions.push(Action::OpenProjectSample(n));
                    ui.memory_mut(egui::Memory::close_popup);
                }
            }
        });
    });
}

/// Show `contents` in a pop-up list under `response` while it is open;
/// clicking `response` opens or closes it.
fn popup_under(ui: &mut egui::Ui, response: &egui::Response, id: egui::Id, contents: impl FnOnce(&mut egui::Ui)) {
    if response.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    // Above the button when there is not room for the list below it.
    let room_below = ui.ctx().screen_rect().bottom() - response.rect.bottom();
    let side = if room_below < 320.0 { egui::AboveOrBelow::Above } else { egui::AboveOrBelow::Below };
    egui::popup::popup_above_or_below_widget(
        ui,
        id,
        response,
        side,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        contents,
    );
}

/// The "What do you want to do?" grid of six tasks.
fn task_grid(ui: &mut egui::Ui, colors: &Colors, width: f32, actions: &mut Vec<Action>) {
    heading(ui, "What do you want to do?", "Each opens its dialog; with no table open it asks for data first");
    let tasks: [(Task, Icon, &str, Option<&str>); 6] = [
        (Task::Describe, Icon::ChartColumn, "Describe ▸ Distribution", None),
        (Task::CompareGroups, Icon::ChartCandlestick, "Compare ▸ Two or more groups", None),
        (Task::FitModel, Icon::ChartScatter, "Model ▸ Linear model", None),
        (Task::ControlChart, Icon::Activity, "Quality ▸ Control chart", None),
        (
            Task::Design(DesignChoice::Kind(0)),
            Icon::FlaskConical,
            "Design ▸ Screening, factorial and more",
            Some("no data needed"),
        ),
        (Task::Forecast, Icon::ChartLine, "Series and Signals ▸ Exponential smoothing", None),
    ];
    let gap = 10.0;
    let columns = if width > 620.0 {
        3
    } else if width > 400.0 {
        2
    } else {
        1
    };
    let card = (width - gap * (columns as f32 - 1.0)) / columns as f32;
    for (n, row) in tasks.chunks(columns).enumerate() {
        // Every card in a row is as tall as the tallest one needed last frame.
        let height_id = egui::Id::new(("start_task_row_height", n));
        let height = ui.ctx().data(|d| d.get_temp::<f32>(height_id)).unwrap_or(0.0);
        let mut tallest = 0.0_f32;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &(task, icon, menu, tag) in row {
                let (response, natural) =
                    tile_sized(ui, colors, true, icon, (task.title(), tag), menu, card, height);
                tallest = tallest.max(natural);
                if let Task::Design(_) = task {
                    popup_under(ui, &response, egui::Id::new("start_design_list"), |ui| design_list(ui, actions));
                } else if response.clicked() {
                    actions.push(Action::StartTask(task));
                }
            }
        });
        if (tallest - height).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(height_id, tallest));
            ui.ctx().request_discard("start page cards resized");
        }
        ui.add_space(gap - ui.spacing().item_spacing.y);
    }
}

/// The Learn row: Sample data (a list by topic), the Quick guide, the
/// Reference manual and a tip
/// with Next.
fn learn_row(ui: &mut egui::Ui, colors: &Colors, width: f32, actions: &mut Vec<Action>) {
    heading(ui, "Learn", "");
    let scale = theme::text_scale(ui);
    let tip_id = egui::Id::new("start_tip");
    let tip = ui.ctx().data_mut(|d| {
        *d.get_temp_mut_or_insert_with(tip_id, || {
            // A different tip each time the application starts.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |t| t.subsec_nanos() as usize);
            nanos % TIPS.len()
        })
    });
    let big_button = |ui: &mut egui::Ui, icon: Icon, label: &str, detail: &str| {
        ui.scope(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(12.0, 9.0);
            ui.spacing_mut().icon_spacing = 8.0;
            icon_button(ui, icon, label, detail, 16.0 * scale, colors.accent)
        })
        .inner
    };
    let mut below = false;
    ui.horizontal_top(|ui| {
        let start = ui.cursor().left();
        let samples = app_model::sample::catalog().len();
        let response = big_button(ui, Icon::Table, "Sample data", &format!("{samples} datasets, each with a report"));
        popup_under(
            ui,
            &response.on_hover_text("Open a sample dataset by topic"),
            egui::Id::new("start_samples"),
            |ui| sample_list(ui, actions),
        );
        if big_button(ui, Icon::BookOpen, "Quick guide", "")
            .on_hover_text("Every menu item in plain language, as a PDF in your PDF viewer")
            .clicked()
        {
            actions.push(Action::OpenManual(super::manuals::Manual::QuickGuide));
        }
        if big_button(ui, Icon::BookOpen, "Reference manual", "")
            .on_hover_text("The full manual with every option and formula, as a PDF in your PDF viewer")
            .clicked()
        {
            actions.push(Action::OpenManual(super::manuals::Manual::Reference));
        }
        let used = ui.cursor().left() - start;
        // Beside the buttons when there is room for it, else on a line of its own.
        let room = width - used - 20.0;
        if room >= 280.0 {
            tip_box(ui, colors, room, tip, tip_id);
        } else {
            below = true;
        }
    });
    if below {
        ui.add_space(6.0);
        tip_box(ui, colors, width - 20.0, tip, tip_id);
    }
}

/// The tip box, `width` wide inside its margins, showing tip number `tip`
/// with Next (which moves the number kept under `tip_id`).
fn tip_box(ui: &mut egui::Ui, colors: &Colors, width: f32, tip: usize, tip_id: egui::Id) {
    let scale = theme::text_scale(ui);
    egui::Frame::NONE.fill(colors.tip_fill).corner_radius(4).inner_margin(egui::Margin::symmetric(10, 6)).show(
        ui,
        |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                icons::show(ui, Icon::Lightbulb, 18.0 * scale, colors.tip_ink);
                let next = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let next = ui.add(egui::Button::new(RichText::new("Next ›").color(colors.tip_ink)).frame(false));
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("Tip").strong().color(colors.tip_ink));
                        ui.add(egui::Label::new(RichText::new(TIPS[tip]).size(detail_size(ui) * 1.05)).wrap());
                    });
                    next
                });
                if next.inner.clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(tip_id, (tip + 1) % TIPS.len()));
                }
            });
        },
    );
}

/// The start page.
pub(super) fn welcome_page(ui: &mut egui::Ui, data: &WelcomeData, actions: &mut Vec<Action>) {
    let colors = Colors::of(ui);
    let scale = theme::text_scale(ui);
    egui::ScrollArea::vertical().id_salt("welcome_scroll").auto_shrink([false, false]).show(ui, |ui| {
        let width = (ui.available_width() - 32.0).min(PAGE_WIDTH * scale).max(280.0);
        let margin = ((ui.available_width() - width) / 2.0).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.add_space(22.0);
                ui.label(RichText::new("AnovaCraft").size(26.0 * scale).strong());
                ui.label(
                    RichText::new("Start a table, pick up where you left off, or choose what you want to do.").weak(),
                );
                ui.add_space(14.0);
                let gap = 20.0;
                if width > 640.0 {
                    let left = ((width - gap) * 0.465).floor();
                    let right = width - gap - left;
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        ui.vertical(|ui| {
                            ui.set_width(left);
                            start_box(ui, &colors, left, actions);
                        });
                        ui.vertical(|ui| {
                            ui.set_width(right);
                            recent_box(ui, &colors, right, data, actions);
                        });
                    });
                } else {
                    start_box(ui, &colors, width, actions);
                    ui.add_space(gap);
                    recent_box(ui, &colors, width, data, actions);
                }
                ui.add_space(gap);
                task_grid(ui, &colors, width, actions);
                ui.add_space(gap);
                learn_row(ui, &colors, width, actions);
                ui.add_space(24.0);
            });
        });
    });
}

impl StatsApp {
    /// Open the dialog of start-page task `task`, or, when it needs data and
    /// no table is open, ask for data first (see [`Self::task_data_dialog`]).
    pub(super) fn start_task(&mut self, task: Task) {
        let Some(doc) = self.documents.get(self.active) else {
            if task.needs_data() {
                self.pending_task = Some(task);
                return;
            }
            self.pending_task = None;
            return self.open_design_task(task);
        };
        self.pending_task = None;
        let note = task.needs_data().then(|| format!("“{}” from the start page, on {}.", task.title(), doc.name()));
        let table = &doc.table;
        match task {
            Task::Describe => {
                self.distribution_dialog = opened!(self.dialog_memory, doc, DistributionDialog::new(table));
            }
            Task::CompareGroups => {
                self.compare_dialog = opened!(self.dialog_memory, doc, CompareDialog::new(CompareKind::Groups, table));
            }
            Task::FitModel => self.dialog = opened!(self.dialog_memory, doc, ModelDialog::new(table)),
            Task::ControlChart => self.chart_dialog = opened!(self.dialog_memory, doc, ChartDialog::default()),
            Task::Forecast => {
                let fresh = crate::forecast_dialog::ForecastDialog::smoothing(table);
                self.forecast_dialog = opened!(self.dialog_memory, doc, fresh);
            }
            Task::Design(_) => self.open_design_task(task),
        }
        if let Some(note) = note {
            self.info(note);
        }
    }

    /// The small "Open data for …" dialog in front of the start page while a
    /// task waits for a table: open a file, make a new table or pick a
    /// sample, and the task's dialog opens on it; Cancel (or Escape) drops
    /// the task. A table opened any other way also drops it, so a dialog never
    /// opens later out of the blue.
    pub(super) fn task_data_dialog(&mut self, ctx: &egui::Context) {
        let Some(task) = self.pending_task else { return };
        if !self.documents.is_empty() {
            self.pending_task = None;
            return;
        }
        let mut actions = Vec::new();
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("task_data_dialog")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.heading(format!("Open data for “{}”", task.title()));
            ui.label(
                "This task works on a data table. Open one, make a new one or pick a sample; its dialog opens as \
                 soon as the table does.",
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Choose a file…").on_hover_text("A project, CSV, Excel or JSON file").clicked() {
                    actions.push(Action::PickAndOpen);
                }
                if ui.button("Start an empty table").clicked() {
                    actions.push(Action::NewTable);
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Sample data").strong());
            ui.scope(|ui| sample_list(ui, &mut actions));
            ui.add_space(8.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel || (actions.is_empty() && modal.should_close()) {
            self.pending_task = None;
            return;
        }
        for action in actions {
            self.perform(ctx, action);
        }
        if !self.documents.is_empty() {
            self.start_task(task);
        }
    }

    /// Open the design dialog of design task `task`.
    fn open_design_task(&mut self, task: Task) {
        let Task::Design(choice) = task else { return };
        let memory = &self.dialog_memory;
        match choice {
            DesignChoice::Kind(i) => self.design_dialog = Some(memory.recall(0, DesignDialog::new(i), None, &[])),
            DesignChoice::SplitPlot => {
                self.design_dialog = Some(memory.recall(0, DesignDialog::split_plot(), None, &[]));
            }
            DesignChoice::Mixture(centroid) => {
                self.mixture_design_dialog = Some(memory.recall(0, MixtureDesignDialog::new(centroid), None, &[]));
            }
            DesignChoice::ExtremeVertices => {
                self.mixture_design_dialog = Some(memory.recall(0, MixtureDesignDialog::extreme_vertices(), None, &[]));
            }
        }
    }

    /// What the start page shows, from the settings and the waiting task.
    pub(super) fn welcome_data(&self) -> WelcomeData {
        WelcomeData {
            recent: self
                .settings
                .recent_files
                .iter()
                .map(|p| (p.clone(), self.settings.recent_details(p).cloned()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()
    }

    #[test]
    fn recent_times_read_relative_to_today() {
        let now = at(2026, 10, 17, 18, 5);
        assert_eq!(when_label(at(2026, 10, 17, 14, 32), now), "Today, 14:32");
        assert_eq!(when_label(at(2026, 10, 16, 9, 10), now), "Yesterday, 09:10");
        assert_eq!(when_label(at(2026, 10, 15, 23, 59), now), "15 Oct 2026");
        // Across a month boundary, yesterday is still yesterday.
        assert_eq!(when_label(at(2026, 9, 30, 8, 0), at(2026, 10, 1, 7, 0)), "Yesterday, 08:00");
    }

    #[test]
    fn recent_files_say_what_they_hold() {
        let project = RecentDetails {
            when: 0,
            contents: RecentContents::Project { tables: vec!["Runs".into(), "Design".into()], notes: "Pump".into() },
        };
        assert_eq!(
            recent_lines(Path::new("/p/Filtration.acraft"), Some(&project)),
            ("2 tables: Runs, Design".to_owned(), Some("“Pump”".to_owned()))
        );
        let quiet = RecentDetails {
            when: 0,
            contents: RecentContents::Project { tables: vec!["Visits".into()], notes: String::new() },
        };
        assert_eq!(recent_lines(Path::new("/p/Clinic.acraft"), Some(&quiet)).1.as_deref(), Some(""));
        let data = RecentDetails { when: 0, contents: RecentContents::Data { rows: 2400, columns: 9 } };
        assert_eq!(
            recent_lines(Path::new("/p/line4.csv"), Some(&data)),
            ("Data file: 2,400 rows, 9 columns".to_owned(), None)
        );
        let workbook = RecentDetails {
            when: 0,
            contents: RecentContents::Project { tables: vec!["A".into(), "B".into()], notes: String::new() },
        };
        assert_eq!(recent_lines(Path::new("/p/book.xlsx"), Some(&workbook)).0, "Workbook, 2 sheets: A, B");
        // Files remembered before details were kept show their folder.
        assert_eq!(recent_lines(Path::new("/p/old.csv"), None), ("/p".to_owned(), None));
        assert_eq!(count_grouped(1_234_567, "row", "rows"), "1,234,567 rows");
        assert_eq!(count_grouped(1, "row", "rows"), "1 row");
    }
}
