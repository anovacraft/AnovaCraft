//! Marker settings: the Markers menu (size, color and shape for the whole
//! graph, and for each level of the columns on the Color, Marker shape and
//! Subgroups shelves) and the menu opened by right-clicking a marker, which
//! sets that marker's group in every panel or in its panel only. Fit lines,
//! bands and labels follow their subgroup's marker color.

use eframe::egui::{self, Color32, RichText};
use stat_data::DataTable;
use stat_viz::{ColorKey, GraphSpec, Legend, MarkShape, MarkerStyle, Panel, Rgba, is_continuous, palette};

/// `Rgba` as an egui color.
fn to_color32(c: Rgba) -> Color32 {
    let [r, g, b, a] = c.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// An egui color as `Rgba`.
fn from_color32(c: Color32) -> Rgba {
    Rgba(c.to_srgba_unmultiplied())
}

// Pickers open as submenus of the Markers menu rather than as separate
// pop-ups: a click in a pop-up lands outside the menu and closes it.

/// A swatch with its hex code (so color is not the only cue) that opens a
/// color picker; `true` when changed.
fn color_button(ui: &mut egui::Ui, color: &mut Rgba) -> bool {
    let mut c = to_color32(*color);
    let font = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append("■ ", 0.0, egui::TextFormat::simple(font.clone(), c));
    job.append(&color.hex(), 0.0, egui::TextFormat::simple(font, ui.visuals().text_color()));
    let mut changed = false;
    ui.menu_button(job, |ui| {
        changed = egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque);
    })
    .response
    .on_hover_text("Choose a color");
    if changed {
        *color = from_color32(c);
    }
    changed
}

/// A shape chooser; `true` when changed.
fn shape_choice(ui: &mut egui::Ui, shape: &mut MarkShape) -> bool {
    let before = *shape;
    ui.menu_button(shape.label(), |ui| {
        for s in MarkShape::ALL {
            ui.selectable_value(shape, s, s.label());
        }
    })
    .response
    .on_hover_text("Choose a shape");
    *shape != before
}

/// One editable level: its label, a color (when `color` is given) and a shape.
fn level_row(
    ui: &mut egui::Ui,
    style: &mut MarkerStyle,
    column: &str,
    level: &str,
    color: Option<Rgba>,
    shape: Option<MarkShape>,
) {
    let key = MarkerStyle::key(column, level);
    ui.horizontal(|ui| {
        let name = if level.is_empty() { "(blank)" } else { level };
        ui.add_sized([110.0, 18.0], egui::Label::new(name).truncate());
        if let Some(mut c) = color {
            if color_button(ui, &mut c) {
                style.level_colors.insert(key.clone(), c);
            }
        }
        if let Some(mut s) = shape {
            if shape_choice(ui, &mut s) {
                style.level_shapes.insert(key.clone(), s);
            }
        }
        let custom = style.level_colors.contains_key(&key) || style.level_shapes.contains_key(&key);
        if ui.add_enabled(custom, egui::Button::new("↺").small()).on_hover_text("Back to the default").clicked() {
            style.level_colors.remove(&key);
            style.level_shapes.remove(&key);
        }
    });
}

/// The Markers menu button. `legend` is the key of the graph as last drawn
/// (it lists the levels to edit).
pub(super) fn markers_menu(ui: &mut egui::Ui, spec: &mut GraphSpec, legend: Option<&Legend>) {
    crate::menus::menu_button(ui, "Markers…", |ui| {
        let style = &mut spec.options.style;
        ui.label(RichText::new("All markers, every plot").strong());
        ui.horizontal(|ui| {
            ui.label("Size:");
            let mut r = style.radius();
            if ui.add(egui::Slider::new(&mut r, 1.0..=20.0).step_by(0.5)).changed() {
                style.size = Some(r);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Color:");
            let mut c = style.color.unwrap_or(palette::DEFAULT_POINT);
            if color_button(ui, &mut c) {
                style.color = Some(c);
            }
            ui.label(RichText::new("(used when no column is on Color)").weak().small());
        });
        ui.horizontal(|ui| {
            ui.label("Shape:");
            let mut s = style.shape.unwrap_or(MarkShape::Circle);
            if shape_choice(ui, &mut s) {
                style.shape = Some(s);
            }
        });
        let Some(legend) = legend else { return };
        let mut any = false;
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            if let Some(ColorKey::Levels { title, entries }) = &legend.color {
                any = true;
                ui.separator();
                ui.label(RichText::new(format!("Levels of {title}")).strong());
                let shapes = legend.shape.is_none();
                for (level, color, shape) in entries {
                    level_row(ui, style, title, level, Some(*color), shapes.then_some(*shape));
                }
            }
            if let Some((title, levels)) = &legend.shape {
                any = true;
                ui.separator();
                ui.label(RichText::new(format!("Shapes of {title}")).strong());
                for (level, shape) in levels {
                    level_row(ui, style, title, level, None, Some(*shape));
                }
            }
            if let Some((title, groups)) = &legend.groups {
                any = true;
                ui.separator();
                ui.label(RichText::new(format!("Subgroups of {title}")).strong());
                for g in groups {
                    level_row(ui, style, title, &g.label, Some(g.color), Some(g.shape));
                }
            }
        });
        if !any {
            ui.label(
                RichText::new("Put a column on Color, Marker shape or Subgroups to style each level.").weak().small(),
            );
        }
        if !style.panels.is_empty() {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(format!("{} plot(s) have their own marker settings.", style.panels.len()));
                if ui.button("Clear them").on_hover_text("Every plot follows the settings above again").clicked() {
                    style.panels.clear();
                }
            });
        }
        ui.label(
            RichText::new("These apply to every plot. Right-click a marker to style just its plot or group.")
                .weak()
                .small(),
        );
        ui.separator();
        if ui
            .add_enabled(*style != MarkerStyle::default(), egui::Button::new("Reset marker style"))
            .on_hover_text("Default size, color and shape for every marker, level and plot")
            .clicked()
        {
            *style = MarkerStyle::default();
        }
    });
}

/// The marker a right-click landed on (`row: None`: on the plot, away from
/// any marker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MarkerTarget {
    /// Panel index.
    pub panel: usize,
    /// The row under the pointer.
    pub row: Option<usize>,
}

/// Which settings a marker's appearance comes from: the level keys that
/// decide its color, shape and size (`None`: the graph's own setting).
struct Keys {
    /// Group shown to the user, e.g. `Fertilizer = B`.
    name: Option<String>,
    /// Level key of its color (`None` with a continuous color column too).
    color: Option<String>,
    /// Color comes from a continuous ramp and cannot be set here.
    ramp: bool,
    shape: Option<String>,
    size: Option<String>,
}

fn keys(spec: &GraphSpec, table: &DataTable, row: Option<usize>) -> Keys {
    let e = &spec.encoding;
    let cell = |col: &str| -> Option<String> {
        let r = row?;
        table.find(col).and_then(|i| table.column(i)).map(|c| c.level_text(r)).filter(|t| !t.trim().is_empty())
    };
    let ramp = e.color.as_deref().is_some_and(|c| is_continuous(table, c));
    // The group: a categorical color level, else the subgroup level, else the shape level.
    let group = e
        .color
        .as_deref()
        .filter(|_| !ramp)
        .or(e.overlay.as_deref())
        .or(e.shape.as_deref())
        .and_then(|c| cell(c).map(|l| (c.to_owned(), l)));
    let shape_key = match e.shape.as_deref() {
        Some(c) => cell(c).map(|l| MarkerStyle::key(c, &l)),
        None => group.as_ref().map(|(c, l)| MarkerStyle::key(c, l)),
    };
    let group_key = group.as_ref().map(|(c, l)| MarkerStyle::key(c, l));
    Keys {
        name: group.as_ref().map(|(c, l)| format!("{c} = {l}")),
        color: group_key.clone(),
        ramp,
        shape: shape_key,
        size: group_key,
    }
}

/// The style a change applies to: the graph's, or one panel's own.
fn scoped(style: &mut MarkerStyle, panel: Option<usize>) -> &mut MarkerStyle {
    match panel {
        Some(i) => style.panels.entry(i).or_default(),
        None => style,
    }
}

/// The menu opened by right-clicking a marker (or the plot) of panel
/// `target.panel`. `all_panels` remembers whether changes apply to every panel
/// (by default they apply to the clicked panel only)
/// only. Changes go into `style`.
#[allow(clippy::too_many_arguments)]
pub(super) fn marker_context_menu(
    ui: &mut egui::Ui,
    spec: &GraphSpec,
    table: &DataTable,
    panel: &Panel,
    panel_name: &str,
    n_panels: usize,
    target: MarkerTarget,
    all_panels: &mut bool,
    style: &mut MarkerStyle,
) {
    let point = target.row.and_then(|r| panel.points.iter().find(|p| p.row == r));
    let k = keys(spec, table, point.map(|p| p.row));
    ui.label(
        RichText::new(match &k.name {
            Some(n) => format!("Markers: {n}"),
            None if point.is_some() => "Markers".to_owned(),
            None => "Markers (right-click one to style its group)".to_owned(),
        })
        .strong(),
    );
    if n_panels > 1 {
        ui.horizontal(|ui| {
            ui.label("Apply to:");
            ui.radio_value(all_panels, false, format!("this plot only ({panel_name})"));
            ui.radio_value(all_panels, true, "every plot");
        });
    }
    let scope = (!*all_panels && n_panels > 1).then_some(target.panel);
    // What this scope currently shows.
    let shown = match scope {
        Some(i) => style.for_panel(i),
        None => MarkerStyle { panels: Default::default(), ..style.clone() },
    };
    let fallback_color = point.map_or(shown.color.unwrap_or(palette::DEFAULT_POINT), |p| p.color);
    let fallback_shape = point.map_or(shown.shape.unwrap_or(MarkShape::Circle), |p| p.shape);
    ui.horizontal(|ui| {
        ui.label("Color:");
        if k.ramp {
            ui.label(RichText::new("set by the color column's gradient").weak());
        } else {
            let mut c = match &k.color {
                Some(key) => shown.level_colors.get(key).copied().unwrap_or(fallback_color),
                None => shown.color.unwrap_or(fallback_color),
            };
            if color_button(ui, &mut c) {
                let s = scoped(style, scope);
                match &k.color {
                    Some(key) => {
                        s.level_colors.insert(key.clone(), c);
                    }
                    None => s.color = Some(c),
                }
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Shape:");
        let mut sh = match &k.shape {
            Some(key) => shown.level_shapes.get(key).copied().unwrap_or(fallback_shape),
            None => shown.shape.unwrap_or(fallback_shape),
        };
        if shape_choice(ui, &mut sh) {
            let s = scoped(style, scope);
            match &k.shape {
                Some(key) => {
                    s.level_shapes.insert(key.clone(), sh);
                }
                None => s.shape = Some(sh),
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Size:");
        let mut r = match &k.size {
            Some(key) => shown.level_sizes.get(key).copied().unwrap_or_else(|| shown.radius()),
            None => shown.radius(),
        };
        if ui.add(egui::Slider::new(&mut r, 1.0..=20.0).step_by(0.5)).changed() {
            let s = scoped(style, scope);
            match &k.size {
                Some(key) => {
                    s.level_sizes.insert(key.clone(), r);
                }
                None => s.size = Some(r),
            }
        }
    });
    if spec.encoding.size.is_some() {
        ui.label(RichText::new("The Marker size column scales markers around this size.").weak().small());
    }
    if scope.is_none() && style.panels.contains_key(&target.panel) {
        ui.label(RichText::new("This plot has its own settings, which take precedence here.").weak().small());
    }
    ui.separator();
    let what = if k.name.is_some() { "this group" } else { "these markers" };
    let where_ = if scope.is_some() { " in this plot" } else { "" };
    if ui.button(format!("Back to the default for {what}{where_}")).clicked() {
        let s = scoped(style, scope);
        let any_level = k.color.is_some() || k.shape.is_some() || k.size.is_some();
        for key in [&k.color, &k.shape, &k.size].into_iter().flatten() {
            s.level_colors.remove(key);
            s.level_shapes.remove(key);
            s.level_sizes.remove(key);
        }
        if !any_level {
            s.color = None;
            s.shape = None;
            s.size = None;
        }
    }
    if scope.is_some() && ui.button("Make this plot follow the graph's settings").clicked() {
        style.panels.remove(&target.panel);
    }
    // Panels left without choices are dropped.
    style.panels.retain(|_, p| !p.is_default());
}
