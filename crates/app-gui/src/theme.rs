//! The application's own color tokens. Chosen independently; colors are never
//! the only cue (markers, labels and text carry the same information).

use eframe::egui::{self, Color32};
use egui_plot::MarkerShape;
use stat_data::MeasurementLevel;

/// Selected rows: points, brush outline.
pub const SELECTED: Color32 = Color32::from_rgb(232, 118, 20);
/// Side-by-side divider at rest.
pub const DIVIDER: Color32 = Color32::from_rgb(112, 128, 156);
/// Side-by-side divider while hovered or dragged.
pub const DIVIDER_ACTIVE: Color32 = Color32::from_rgb(52, 120, 190);
/// Error text.
pub const ERROR: Color32 = Color32::from_rgb(200, 50, 50);

/// "Source changed" marks and banner: amber text and a pale amber fill for
/// the light appearance, or their counterparts for the dark one. Always
/// shown with the ⚠ sign and words, never by colour alone.
pub fn stale_colors(dark: bool) -> (Color32, Color32) {
    if dark {
        (Color32::from_rgb(240, 190, 90), Color32::from_rgb(66, 52, 22))
    } else {
        (Color32::from_rgb(150, 92, 0), Color32::from_rgb(253, 244, 218))
    }
}

/// Accent of the start page's icons, links and highlighted cards, for the
/// light or dark appearance (always beside text).
pub fn start_accent(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(100, 166, 230) } else { DIVIDER_ACTIVE }
}

/// Colors of plots, for the light or the dark appearance. Every plot reads
/// its colors from here, so plots follow the application's mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotPalette {
    /// The appearance these colors are for.
    pub dark: bool,
    /// Plot canvas.
    pub plot_bg: Color32,
    /// One-point frame around the canvas.
    pub container_border: Color32,
    /// Horizontal rules at major ticks of the vertical axis.
    pub grid_major: Color32,
    /// Vertical rules (only when turned on in the axis settings).
    pub grid_vertical: Color32,
    /// Axis baselines and tick marks.
    pub axis_spine: Color32,
    /// Tick labels.
    pub tick_text: Color32,
    /// Axis titles.
    pub axis_label: Color32,
    /// Data points.
    pub point: Color32,
    /// Soft halo behind points in the dark appearance (transparent in light).
    pub point_glow: Color32,
    /// Fitted lines.
    pub fit_line: Color32,
    /// Confidence band outline.
    pub band: Color32,
    /// Zero line of residual plots.
    pub zero_baseline: Color32,
    /// Selected rows (rings, brush outline).
    pub selected: Color32,
    /// Translucent fill of the selection box.
    pub brush_fill: Color32,
    /// Fill of the selected part of a bar.
    pub selected_fill: Color32,
    /// Excluded rows.
    pub excluded: Color32,
    /// Pane dividers and headings.
    pub divider: Color32,
    /// Control chart center line.
    pub center_line: Color32,
    /// Control limits.
    pub limit: Color32,
    /// Run-rule violations.
    pub violation: Color32,
    /// 1σ / 2σ zone lines.
    pub zone: Color32,
    /// Lines joining points in order, contours, structural line work.
    pub chart_trace: Color32,
    /// Histogram bars and boxes.
    pub bar_fill: Color32,
    /// Shading behind every other phase of a control chart.
    pub phase_shade: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// `rgba(r, g, b, a)` as CSS composites it: premultiplied in gamma space
/// (egui's `from_rgba_unmultiplied` premultiplies in linear space, which makes
/// faint light colors much stronger than intended).
fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    let m = |c: u8| (f32::from(c) * a).round() as u8;
    Color32::from_rgba_premultiplied(m(r), m(g), m(b), (a * 255.0).round() as u8)
}

impl PlotPalette {
    /// The light appearance.
    #[must_use]
    pub fn light() -> Self {
        Self {
            dark: false,
            plot_bg: rgb(0xFF_FFFF),
            container_border: rgb(0xE2_E8F0),
            grid_major: rgba(15, 23, 42, 0.06),
            grid_vertical: rgba(15, 23, 42, 0.03),
            axis_spine: rgb(0xCB_D5E1),
            tick_text: rgb(0x64_748B),
            axis_label: rgb(0x1E_293B),
            point: rgb(0x02_84C7),
            point_glow: Color32::TRANSPARENT,
            fit_line: rgb(0xE1_1D48),
            band: rgba(225, 29, 72, 0.45),
            zero_baseline: rgba(100, 116, 139, 0.5),
            selected: rgb(0xEA_580C),
            brush_fill: rgba(234, 88, 12, 0.10),
            selected_fill: rgba(234, 88, 12, 0.70),
            excluded: rgb(0x94_A3B8),
            divider: rgb(0x64_748B),
            center_line: rgb(0x05_9669),
            limit: rgb(0xE1_1D48),
            violation: rgb(0xB9_1C1C),
            zone: rgb(0x94_A3B8),
            chart_trace: rgb(0x94_A3B8),
            bar_fill: rgba(2, 132, 199, 0.22),
            phase_shade: rgba(100, 116, 139, 0.08),
        }
    }

    /// The dark appearance.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            dark: true,
            plot_bg: rgb(0x16_171B),
            container_border: rgb(0x2B_2D33),
            grid_major: rgba(255, 255, 255, 0.05),
            grid_vertical: rgba(255, 255, 255, 0.03),
            axis_spine: rgb(0x2B_2D33),
            tick_text: rgb(0x9C_A3AF),
            axis_label: rgb(0xE5_E7EB),
            point: rgb(0x38_BDF8),
            point_glow: rgba(56, 189, 248, 0.25),
            fit_line: rgb(0xFF_4566),
            band: rgba(255, 69, 102, 0.5),
            zero_baseline: rgba(156, 163, 175, 0.55),
            selected: rgb(0xFB_923C),
            brush_fill: rgba(251, 146, 60, 0.12),
            selected_fill: rgba(251, 146, 60, 0.70),
            excluded: rgb(0x6B_7280),
            divider: rgb(0x9C_A3AF),
            center_line: rgb(0x34_D399),
            limit: rgb(0xFF_4566),
            violation: rgb(0xF8_7171),
            zone: rgb(0x4B_5563),
            chart_trace: rgb(0x6B_7280),
            bar_fill: rgba(56, 189, 248, 0.25),
            phase_shade: rgba(156, 163, 175, 0.07),
        }
    }
}

/// The plot colors for the appearance `ui` is drawn in.
#[must_use]
pub fn plot_palette(ui: &egui::Ui) -> PlotPalette {
    if ui.visuals().dark_mode { PlotPalette::dark() } else { PlotPalette::light() }
}

impl PlotPalette {
    /// The diverging ramp of heat maps at `t ∈ [−1, 1]`: blue below zero,
    /// orange above, fading to a neutral that suits the appearance at zero
    /// ([`stat_viz::palette::diverging`]).
    #[must_use]
    pub fn diverging(&self, t: f64) -> Color32 {
        let ends = if self.dark { stat_viz::palette::DIVERGING_DARK } else { stat_viz::palette::DIVERGING_LIGHT };
        let [r, g, b, _] = stat_viz::palette::diverging(t, ends).0;
        Color32::from_rgb(r, g, b)
    }

    /// Text colour that stays legible on `fill`: dark ink on light fills,
    /// white on dark ones.
    #[must_use]
    pub fn ink_on(&self, fill: Color32) -> Color32 {
        let lum = (0.2126 * f32::from(fill.r()) + 0.7152 * f32::from(fill.g()) + 0.0722 * f32::from(fill.b())) / 255.0;
        if lum > 0.5 { rgb(0x1E_293B) } else { Color32::WHITE }
    }
}

/// Colors of the script editor, for the light or the dark appearance.
/// Highlighting only eases reading: every token is also plain text, and the
/// line of an error is marked in the gutter with a glyph as well as a color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptPalette {
    /// Reserved words.
    pub keyword: Color32,
    /// String literals.
    pub string: Color32,
    /// Numbers.
    pub number: Color32,
    /// Comments.
    pub comment: Color32,
    /// Function names.
    pub function: Color32,
    /// Line numbers in the gutter.
    pub gutter: Color32,
    /// Background of the line an error points at.
    pub error_line: Color32,
    /// Error text in the output log, and the gutter mark.
    pub error: Color32,
    /// Notes from the application (not the script) in the output log.
    pub note: Color32,
    /// Background of a bracket and its partner beside the cursor (also
    /// drawn underlined, so colour is not the only cue).
    pub bracket: Color32,
    /// Background of find matches.
    pub found: Color32,
}

impl ScriptPalette {
    /// The light appearance.
    #[must_use]
    pub fn light() -> Self {
        Self {
            keyword: rgb(0x7C_3AED),
            string: rgb(0x04_7857),
            number: rgb(0xB4_5309),
            comment: rgb(0x64_748B),
            function: rgb(0x02_69A1),
            gutter: rgb(0x94_A3B8),
            error_line: rgba(225, 29, 72, 0.12),
            error: rgb(0xB9_1C1C),
            note: rgb(0x47_5569),
            bracket: rgba(2, 105, 161, 0.18),
            found: rgba(234, 179, 8, 0.30),
        }
    }

    /// The dark appearance.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            keyword: rgb(0xC4_B5FD),
            string: rgb(0x6E_E7B7),
            number: rgb(0xFC_D34D),
            comment: rgb(0x8B_93A1),
            function: rgb(0x7D_D3FC),
            gutter: rgb(0x6B_7280),
            error_line: rgba(255, 69, 102, 0.18),
            error: rgb(0xF8_7171),
            note: rgb(0x9C_A3AF),
            bracket: rgba(125, 211, 252, 0.25),
            found: rgba(250, 204, 21, 0.28),
        }
    }

    /// The script colors for the appearance `ui` is drawn in.
    #[must_use]
    pub fn of(ui: &egui::Ui) -> Self {
        if ui.visuals().dark_mode { Self::dark() } else { Self::light() }
    }
}

/// Size of tick labels at the standard text size, in points.
pub const TICK_LABEL_SIZE: f32 = 10.0;
/// Size of axis titles at the standard text size, in points.
pub const AXIS_TITLE_SIZE: f32 = 11.5;

/// Style a plot draws with: a transparent canvas (the canvas and grid are
/// painted behind the plot by `plot_axes::PlotChrome`), the frame color, and
/// tabular (monospaced) tick labels at text scale `scale`. Returns the style
/// to restore afterwards (`ui.set_style(saved)`).
#[must_use]
pub fn begin_plot(ui: &mut egui::Ui, pal: &PlotPalette, scale: f32) -> std::sync::Arc<egui::Style> {
    let saved = ui.style().clone();
    let style = ui.style_mut();
    style.visuals.extreme_bg_color = Color32::TRANSPARENT;
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, pal.container_border);
    style.visuals.override_text_color = Some(pal.tick_text);
    style.text_styles.insert(egui::TextStyle::Body, egui::FontId::monospace(TICK_LABEL_SIZE * scale));
    saved
}

/// An axis title in the plot style at text scale `scale` in `family`
/// (egui_plot draws titles in the body style, which [`begin_plot`] sets to
/// the tick-label face, so the title carries its own font and color).
#[must_use]
pub fn axis_title(
    text: impl Into<String>,
    scale: f32,
    family: &egui::FontFamily,
    pal: &PlotPalette,
) -> egui::RichText {
    egui::RichText::new(text).font(egui::FontId::new(AXIS_TITLE_SIZE * scale, family.clone())).color(pal.axis_label)
}

/// Font family of serif plot titles (Liberation Serif, see
/// [`install_fonts`]).
#[must_use]
pub fn plot_serif() -> egui::FontFamily {
    egui::FontFamily::Name(PLOT_SERIF.into())
}

/// Name of the [`plot_serif`] family.
const PLOT_SERIF: &str = "plot-serif";

/// An axis title in math markup: plain text exactly as [`axis_title`],
/// anything else set with scripts and fractions in the same font and color.
pub fn axis_title_math(
    ctx: &egui::Context,
    text: &stat_viz::MathText,
    scale: f32,
    family: &egui::FontFamily,
    pal: &PlotPalette,
) -> egui::WidgetText {
    let font = egui::FontId::new(AXIS_TITLE_SIZE * scale, family.clone());
    stat_viz::math_text::egui_text::widget_text(ctx, text, |t| axis_title(t, scale, family, pal), &font, pal.axis_label)
}

/// Badge colour of a measurement level (its icon carries the level too).
pub fn level_color(level: MeasurementLevel) -> Color32 {
    match level {
        MeasurementLevel::Continuous => Color32::from_rgb(38, 140, 128),
        MeasurementLevel::Nominal => Color32::from_rgb(138, 92, 170),
        MeasurementLevel::Ordinal => Color32::from_rgb(196, 138, 28),
    }
}

/// Group colors, from the Okabe–Ito color-blind-safe palette (Okabe & Ito 2008,
/// "Color Universal Design"), ordered for contrast on light and dark backgrounds.
/// Orange is left out because it marks selection.
const GROUP_COLORS: [Color32; 6] = [
    Color32::from_rgb(0, 114, 178),   // blue
    Color32::from_rgb(0, 158, 115),   // bluish green
    Color32::from_rgb(204, 121, 167), // reddish purple
    Color32::from_rgb(86, 180, 233),  // sky blue
    Color32::from_rgb(213, 94, 0),    // vermillion
    Color32::from_rgb(120, 120, 120), // gray
];

/// Marker shapes paired with the group colors, so groups stay distinguishable
/// without color.
const GROUP_SHAPES: [MarkerShape; 6] = [
    MarkerShape::Circle,
    MarkerShape::Square,
    MarkerShape::Diamond,
    MarkerShape::Up,
    MarkerShape::Down,
    MarkerShape::Asterisk,
];

/// The group colors on the dark canvas: the first (blue) and last (gray) are
/// lightened, which otherwise sit too close to the canvas; the rest keep
/// their light-appearance values.
const GROUP_COLORS_DARK: [Color32; 6] = [
    Color32::from_rgb(64, 140, 230),  // blue, lightened
    Color32::from_rgb(0, 158, 115),   // bluish green
    Color32::from_rgb(204, 121, 167), // reddish purple
    Color32::from_rgb(86, 180, 233),  // sky blue
    Color32::from_rgb(213, 94, 0),    // vermillion
    Color32::from_rgb(160, 160, 160), // gray, lightened
];

/// A row's own colour as plots draw it: the stored sRGB colour, with the
/// palette's dark counterpart on a dark canvas.
#[must_use]
pub fn row_color(c: stat_data::RowColor, dark: bool) -> Color32 {
    let rgba = stat_viz::row_styles::rgba(c);
    let [r, g, b, _] = if dark { stat_viz::palette::for_dark(rgba).0 } else { rgba.0 };
    Color32::from_rgb(r, g, b)
}

/// The plot marker of a row marker.
#[must_use]
pub fn row_marker(m: stat_data::RowMarker) -> MarkerShape {
    use stat_data::RowMarker as M;
    match m {
        M::Circle => MarkerShape::Circle,
        M::Square => MarkerShape::Square,
        M::Diamond => MarkerShape::Diamond,
        M::Up => MarkerShape::Up,
        M::Down => MarkerShape::Down,
        M::Plus => MarkerShape::Plus,
        M::Cross => MarkerShape::Cross,
        M::Asterisk => MarkerShape::Asterisk,
    }
}

impl PlotPalette {
    /// Color and marker shape for group `index` (cycling after six groups, with
    /// the shape sequence offset so color/shape pairs stay distinct for 36
    /// groups).
    #[must_use]
    pub fn group_style(&self, index: usize) -> (Color32, MarkerShape) {
        let colors = if self.dark { &GROUP_COLORS_DARK } else { &GROUP_COLORS };
        let n = colors.len();
        (colors[index % n], GROUP_SHAPES[(index + index / n) % n])
    }
}

/// Interface typeface: Inter (Rasmus Andersson; SIL Open Font License 1.1),
/// designed for legibility on screens at small sizes.
const INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.otf");
/// Typeface for numbers in tables and code-like text: JetBrains Mono (SIL
/// Open Font License 1.1), with distinct 0/O and 1/l/I.
const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
/// Symbol fallback: DejaVu Sans (Bitstream Vera licence), for mathematical
/// and geometric symbols the two text fonts lack (∈, ⋯, ▸, ▾, ◌, ᵀ, ↺ …).
const DEJAVU_SANS: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
/// Combining macron and circumflex placed over the preceding capital without
/// mark positioning, so X̄ and λ̂ draw with their bar and hat (built from
/// Inter's marks by `assets/fonts/build_marks.py`; SIL Open Font License 1.1).
const MARKS: &[u8] = include_bytes!("../assets/fonts/AnovaCraftMarks.ttf");

/// Use Inter for interface and report text and JetBrains Mono for monospaced
/// text, with DejaVu Sans and then egui's built-in fonts behind them for
/// symbols those fonts lack (so no character is drawn as an empty box), and
/// Liberation Serif for plots whose plot font is serif ([`plot_serif`]).
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("inter".to_owned(), std::sync::Arc::new(egui::FontData::from_static(INTER)));
    fonts
        .font_data
        .insert("jetbrains-mono".to_owned(), std::sync::Arc::new(egui::FontData::from_static(JETBRAINS_MONO)));
    fonts.font_data.insert("dejavu-sans".to_owned(), std::sync::Arc::new(egui::FontData::from_static(DEJAVU_SANS)));
    fonts.font_data.insert("marks".to_owned(), std::sync::Arc::new(egui::FontData::from_static(MARKS)));
    // The marks font has only combining marks, so it goes first and every
    // other character falls through to the text fonts.
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        list.splice(0..0, ["marks".to_owned(), "inter".to_owned(), "dejavu-sans".to_owned()]);
    }
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        // Symbols missing from the monospace font come from Inter, then DejaVu.
        list.splice(
            0..0,
            ["marks".to_owned(), "jetbrains-mono".to_owned(), "inter".to_owned(), "dejavu-sans".to_owned()],
        );
    }
    // Serif plot titles (the plot font), with the interface fonts behind it.
    fonts.font_data.insert(
        "liberation-serif".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(stat_viz::raster::LIBERATION_SERIF)),
    );
    let mut serif = vec!["marks".to_owned(), "liberation-serif".to_owned(), "dejavu-sans".to_owned()];
    serif.extend(fonts.families.get(&egui::FontFamily::Proportional).into_iter().flatten().cloned());
    fonts.families.insert(plot_serif(), serif);
    ctx.set_fonts(fonts);
}

/// Windows (dialogs, Settings) stand out from the reports and grid behind
/// them: in the light theme the default window has the panels' own fill and a
/// faint shadow, so it gets a white fill, a darker edge and a deeper shadow;
/// the dark theme gets a slightly lighter edge.
///
/// Scroll bars are always drawn where content overflows (egui's default
/// hides them until the pointer is over the area), so a list, report or
/// grid with more to see says so before anyone hovers it.
pub fn apply_window_style(ctx: &egui::Context) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(theme, |s| {
            s.spacing.scroll = egui::style::ScrollStyle {
                bar_width: 8.0,
                dormant_background_opacity: 0.5,
                dormant_handle_opacity: 0.7,
                active_background_opacity: 0.6,
                active_handle_opacity: 0.85,
                ..egui::style::ScrollStyle::solid()
            };
        });
    }
    ctx.style_mut_of(egui::Theme::Light, |s| {
        s.visuals.window_fill = Color32::WHITE;
        s.visuals.window_stroke = egui::Stroke::new(1.0_f32, Color32::from_gray(150));
        s.visuals.window_shadow =
            egui::epaint::Shadow { offset: [0, 10], blur: 28, spread: 2, color: Color32::from_black_alpha(70) };
    });
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.visuals.window_stroke = egui::Stroke::new(1.0_f32, Color32::from_gray(80));
    });
}

/// Colour laid over the workspace behind the dialog in front, so the eye goes
/// to the dialog (drawn only: clicks and drops still reach the workspace).
pub fn dialog_scrim(dark: bool) -> Color32 {
    if dark { Color32::from_black_alpha(70) } else { Color32::from_black_alpha(26) }
}

/// Body text size of egui's default style, before the user's text-size setting.
fn standard_body_size() -> f32 {
    egui::style::default_text_styles().get(&egui::TextStyle::Body).map_or(12.5, |f| f.size)
}

/// Set every text style to its standard size times `scale` (the user's text-size
/// setting), in both the light and dark styles.
pub fn apply_text_scale(ctx: &egui::Context, scale: f32) {
    let styles: std::collections::BTreeMap<_, _> = egui::style::default_text_styles()
        .into_iter()
        .map(|(style, mut font)| {
            font.size *= scale;
            (style, font)
        })
        .collect();
    ctx.all_styles_mut(|s| s.text_styles = styles.clone());
}

/// Wait `seconds` of stillness before showing a control's tooltip, in both
/// the light and dark styles. Tooltips still appear only while the pointer
/// is still, and a click hides them (egui's own behaviour). Moving on to
/// another control waits the full delay again: egui's grace time, which shows
/// the next tooltip at once after one was open, is turned off.
pub fn apply_tooltip_delay(ctx: &egui::Context, seconds: f32) {
    ctx.all_styles_mut(|s| {
        s.interaction.tooltip_delay = seconds;
        s.interaction.tooltip_grace_time = 0.0;
        s.interaction.show_tooltips_only_when_still = true;
    });
}

/// How much larger than standard the text currently is (1 at the standard
/// size), for sizes and spacings that must grow with the text.
pub fn text_scale(ui: &egui::Ui) -> f32 {
    ui.style().text_styles.get(&egui::TextStyle::Body).map_or(1.0, |f| f.size / standard_body_size())
}

/// A small window glyph (an outlined window with a title bar, after the
/// open-licensed Lucide `app-window` icon), drawn with shapes at the text
/// size, in `color`. Marks a view shown in a window of its own; callers add
/// a tooltip or label saying which view, so the mark never relies on colour.
pub fn window_glyph(ui: &mut egui::Ui, color: Color32) -> egui::Response {
    let size = 13.0 * text_scale(ui);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_window_glyph(ui.painter(), rect, color);
    }
    response
}

/// Paint the window glyph of [`window_glyph`] centred in `rect` (a square).
pub fn paint_window_glyph(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let size = rect.width().min(rect.height());
    let r = egui::Rect::from_center_size(rect.center(), egui::vec2(size * 0.92, size * 0.74));
    let stroke = egui::Stroke::new(1.2_f32, color);
    painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
    let bar = r.top() + r.height() * 0.32;
    painter.line_segment([egui::pos2(r.left(), bar), egui::pos2(r.right(), bar)], stroke);
    for i in 0..2 {
        let x = r.left() + r.width() * (0.16 + 0.14 * i as f32);
        painter.circle_filled(egui::pos2(x, (r.top() + bar) / 2.0), 0.9, color);
    }
}

/// A small dashboard glyph (an outlined square split into a wide top panel
/// and two panels below, after the open-licensed Lucide `layout-dashboard`
/// idea, drawn with shapes), at the text size, in `color`. Marks dashboards
/// in the sidebar, their tabs and toolbars, always beside the dashboard's
/// name, so it never carries meaning alone.
pub fn dashboard_glyph(ui: &mut egui::Ui, color: Color32) -> egui::Response {
    let size = 13.0 * text_scale(ui);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_dashboard_glyph(ui.painter(), rect, color);
    }
    response
}

/// Paint the glyph of [`dashboard_glyph`] centred in `rect` (a square).
pub fn paint_dashboard_glyph(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let size = rect.width().min(rect.height()) * 0.86;
    let r = egui::Rect::from_center_size(rect.center(), egui::vec2(size, size));
    let stroke = egui::Stroke::new(1.2_f32, color);
    let gap = (size * 0.14).max(1.5);
    let top = egui::Rect::from_min_max(r.min, egui::pos2(r.right(), r.top() + size * 0.38));
    let left = egui::Rect::from_min_max(
        egui::pos2(r.left(), top.bottom() + gap),
        egui::pos2(r.center().x - gap / 2.0, r.bottom()),
    );
    let right = egui::Rect::from_min_max(egui::pos2(r.center().x + gap / 2.0, top.bottom() + gap), r.max);
    for panel in [top, left, right] {
        painter.rect_stroke(panel, 1.5, stroke, egui::StrokeKind::Inside);
    }
}

/// Font size of example text for `scale`, independent of the current setting.
pub fn example_text_size(scale: f32) -> f32 {
    standard_body_size() * scale
}

#[cfg(test)]
mod font_tests {
    use super::*;

    /// Every character in the interface's text appears in one of the fonts.
    #[test]
    fn fonts_load_and_cover_interface_symbols() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            ctx.fonts(|f| {
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    let id = egui::FontId::new(13.0, family.clone());
                    for c in "X̄ λ̂ σ χ² R² ≤ ≥ ± × − … √ ∞ ∈ ⋯ ▸ ▾ ◌ ᵀ ↺ λ μ θ ₁₀ ⁻¹ → ✔ ■".chars()
                    {
                        assert!(f.has_glyph(&id, c), "{family:?} lacks {c:?}");
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plot_palettes_carry_the_design_tokens() {
        let (l, d) = (PlotPalette::light(), PlotPalette::dark());
        assert!(!l.dark && d.dark);
        assert_eq!((l.plot_bg, d.plot_bg), (rgb(0xFF_FFFF), rgb(0x16_171B)));
        assert_eq!((l.point, d.point), (rgb(0x02_84C7), rgb(0x38_BDF8)));
        assert_eq!((l.fit_line, d.fit_line), (rgb(0xE1_1D48), rgb(0xFF_4566)));
        assert_eq!((l.axis_label, d.axis_label), (rgb(0x1E_293B), rgb(0xE5_E7EB)));
        // Faint colors are premultiplied as CSS composites them.
        assert_eq!(d.grid_major.to_array(), [13, 13, 13, 13]);
        assert_eq!(l.grid_major.a(), 15);
        assert!(d.grid_vertical.a() < d.grid_major.a());
        assert_eq!(l.point_glow, Color32::TRANSPARENT);
        assert_eq!(d.point_glow.a(), 64);
    }
}
