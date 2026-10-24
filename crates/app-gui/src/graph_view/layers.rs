//! The layer bar: a toggle per drawing element, each with a settings menu
//! ("…") and, for the most adjusted settings, a control right beside it.

use crate::dialog_kit::ComboExt as _;
use eframe::egui::{self, RichText};
use stat_viz::expr::Expr;
use stat_viz::summary::{ErrorBarDirection, ErrorBarLook, ErrorBars, SummaryStat};
use stat_viz::{
    BarElement, BarLayout, BinElement, BoxPlotElement, ContourElement, DotPlotElement, EllipseElement, FitLineElement,
    FunctionElement, GraphForm, GraphSpec, HeatmapElement, HistogramBins, HistogramElement, JitterKind, LineConnect,
    LineElement, PointsElement, ProbabilityPlotElement, SmoothMethod, SmootherElement, SummaryElement, ViolinElement,
};
use stat_viz::element::{STAR_THRESHOLDS, star_key};
use stat_viz::{BracketElement, BracketLabel, BracketTest, ElementKind, HullElement};

/// A toggle for an optional layer; turning it on uses `make`. Returns `true`
/// while the layer is on.
/// Right-clicking a layer that is on offers to move it in the drawing order.
fn toggle<T>(ui: &mut egui::Ui, slot: &mut Option<T>, label: &str, hint: &str, make: impl FnOnce() -> T) -> bool {
    let mut on = slot.is_some();
    let response = ui.toggle_value(&mut on, label).on_hover_text(hint);
    if response.changed() {
        *slot = if on { Some(make()) } else { None };
    }
    let kind = ElementKind::PANEL_ORDER.into_iter().find(|k| k.label() == label);
    if let (Some(kind), true) = (kind, slot.is_some()) {
        response.context_menu(|ui| {
            for (forward, text) in [(true, "Bring forward"), (false, "Send backward")] {
                if ui.button(text).clicked() {
                    request_move(ui, kind, forward);
                    ui.close_menu();
                }
            }
        });
    }
    slot.is_some()
}

/// Where a request to move a layer in the drawing order waits for the end
/// of the layer bar (the toggles borrow the graph).
fn order_request_id() -> egui::Id {
    egui::Id::new("graph_layer_order_request")
}

fn request_move(ui: &egui::Ui, kind: ElementKind, forward: bool) {
    ui.ctx().data_mut(|d| d.insert_temp(order_request_id(), (kind, forward)));
}

/// The active layers front to back, each with buttons moving it one place
/// nearer the front or the back.
fn drawing_order(ui: &mut egui::Ui, spec: &GraphSpec) {
    let kinds = spec.layer_kinds();
    if kinds.len() < 2 {
        return;
    }
    crate::menus::menu_button(ui, "Drawing order", |ui| {
        ui.label(RichText::new("Front").weak().small());
        for (i, kind) in kinds.iter().enumerate().rev() {
            ui.horizontal(|ui| {
                let front = ui.add_enabled(i + 1 < kinds.len(), egui::Button::new("▲").small());
                front.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Bring {} forward", kind.label()))
                });
                if front.on_hover_text("Bring forward").clicked() {
                    request_move(ui, *kind, true);
                }
                let back = ui.add_enabled(i > 0, egui::Button::new("▼").small());
                back.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Send {} backward", kind.label()))
                });
                if back.on_hover_text("Send backward").clicked() {
                    request_move(ui, *kind, false);
                }
                ui.label(kind.label());
            });
        }
        ui.label(RichText::new("Back").weak().small());
    })
    .response
    .on_hover_text("Which layers are drawn on top of which");
}

/// The settings menu of a layer.
fn settings(ui: &mut egui::Ui, what: &str, add: impl FnOnce(&mut egui::Ui)) {
    crate::menus::menu_button(ui, "…", add).response.on_hover_text(format!("Settings for {what}"));
}

/// A confidence level as a percentage field.
fn level_field(ui: &mut egui::Ui, level: &mut f64, label: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut pct = *level * 100.0;
        if ui.add(egui::DragValue::new(&mut pct).range(50.0..=99.9).speed(0.5).suffix("%")).changed() {
            *level = pct / 100.0;
        }
    });
}

/// Checkboxes choosing a set of probabilities from `choices`.
fn probability_set(ui: &mut egui::Ui, values: &mut Vec<f64>, choices: &[f64]) {
    ui.horizontal_wrapped(|ui| {
        for &c in choices {
            let mut on = values.iter().any(|v| (v - c).abs() < 1e-9);
            if ui.checkbox(&mut on, format!("{}%", c * 100.0)).changed() {
                if on {
                    values.push(c);
                    values.sort_by(f64::total_cmp);
                } else {
                    values.retain(|v| (v - c).abs() >= 1e-9);
                }
            }
        }
    });
}

/// How error bars are drawn: colour, width, caps and direction.
fn error_look_settings(ui: &mut egui::Ui, look: &mut ErrorBarLook) {
    ui.horizontal(|ui| {
        let mut own = look.color.is_some();
        if ui.checkbox(&mut own, "Own colour").on_hover_text("Otherwise each subgroup's colour").changed() {
            look.color = own.then_some(stat_viz::palette::LABEL);
        }
        if let Some(c) = look.color.as_mut() {
            let [r, g, b, a] = c.0;
            let mut c32 = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
            if ui.color_edit_button_srgba(&mut c32).changed() {
                *c = stat_viz::Rgba(c32.to_srgba_unmultiplied());
            }
        }
        ui.add(egui::DragValue::new(&mut look.width).range(0.25..=12.0).speed(0.1).prefix("width "));
        ui.add(egui::DragValue::new(&mut look.cap).range(0.0..=5.0).speed(0.05).prefix("caps ×"))
            .on_hover_text("Cap width as a multiple of the usual (0: no caps)");
    });
    ui.horizontal(|ui| {
        for d in ErrorBarDirection::ALL {
            ui.radio_value(&mut look.direction, d, d.label());
        }
    });
}

/// Bracket settings beyond the procedure and label: which pairs, star
/// thresholds, "ns" and the gap above the data. `levels` are the categories
/// across.
fn bracket_settings(ui: &mut egui::Ui, k: &mut BracketElement, levels: &[String]) {
    ui.separator();
    ui.label("Pairs to draw:");
    // "Only the pairs ticked" with none ticked yet is remembered for the
    // menu's life, so the pair list stays open while the first is ticked.
    let picking_id = ui.id().with("bracket_picking");
    let picking = ui.data(|d| d.get_temp::<bool>(picking_id)).unwrap_or(false);
    let mut every = k.pairs.is_empty() && !picking;
    ui.horizontal(|ui| {
        if ui.radio_value(&mut every, true, "Every pair compared").clicked() {
            k.pairs.clear();
        }
        ui.radio_value(&mut every, false, "Only the pairs ticked");
    });
    ui.data_mut(|d| d.insert_temp(picking_id, !every));
    if !every {
        egui::ScrollArea::vertical().id_salt("bracket_pairs").max_height(180.0).show(ui, |ui| {
            for (i, a) in levels.iter().enumerate() {
                for b in &levels[i + 1..] {
                    let at = k.pairs.iter().position(|(p, q)| (p == a && q == b) || (p == b && q == a));
                    let mut on = at.is_some();
                    if ui.checkbox(&mut on, format!("{a} – {b}")).changed() {
                        match at {
                            Some(i) => {
                                k.pairs.remove(i);
                            }
                            None => k.pairs.push((a.clone(), b.clone())),
                        }
                    }
                }
            }
            crate::dialog_kit::overflow_cues(ui);
        });
        if levels.len() < 2 {
            ui.label(RichText::new("Put categories on the horizontal axis to pick pairs.").weak().small());
        }
    }
    ui.separator();
    ui.checkbox(&mut k.ns_label, "Write \"ns\" on pairs that are not significant");
    ui.horizontal(|ui| {
        ui.label("Stars below p =");
        let mut t = k.thresholds;
        for (i, v) in t.iter_mut().enumerate() {
            ui.add(egui::DragValue::new(v).range(1e-6..=0.5).speed(0.0005).max_decimals(5))
                .on_hover_text(format!("{} below this p-value", "*".repeat(i + 1)));
        }
        // Each threshold below the one before.
        if t.windows(2).all(|w| w[1] < w[0]) {
            k.thresholds = t;
        }
        if ui.small_button("Usual").on_hover_text("0.05, 0.01, 0.001, 0.0001").clicked() {
            k.thresholds = STAR_THRESHOLDS;
        }
    });
    ui.add(egui::Slider::new(&mut k.gap, 0.0..=0.3).text("gap above the data"))
        .on_hover_text("Space between the tallest point under a bracket and the bracket, as a share of the panel's spread");
}

/// The layer bar. `levels` are the categories across (for picking bracket
/// pairs).
pub(super) fn layer_bar(ui: &mut egui::Ui, spec: &mut GraphSpec, legend: Option<&stat_viz::Legend>, levels: &[String]) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Layers:").strong());

        if toggle(ui, &mut spec.points, "Points", "One marker per row", PointsElement::default) {
            let p = spec.points.as_mut().expect("on");
            settings(ui, "points", |ui| {
                ui.label("Jitter on category axes:");
                for k in JitterKind::ALL {
                    ui.radio_value(&mut p.jitter, k, k.label());
                }
                ui.add(egui::Slider::new(&mut p.jitter_width, 0.0..=1.0).text("jitter width"));
                ui.checkbox(&mut p.fade_unselected, "Fade unselected rows while some are selected");
            });
        }
        super::markers::markers_menu(ui, spec, legend);
        if spec.form != GraphForm::Panels {
            ui.label(
                RichText::new(format!(
                    "The other layers draw on panels with axes, not on a {}.",
                    spec.form.label().to_lowercase()
                ))
                .weak(),
            );
            return;
        }
        ui.separator();

        if toggle(
            ui,
            &mut spec.smoother,
            "Smoother",
            "A smooth curve through each subgroup (dashed): smoothing spline or LOESS",
            SmootherElement::default,
        ) {
            let s = spec.smoother.as_mut().expect("on");
            match s.method {
                SmoothMethod::Spline => {
                    ui.add(
                        egui::Slider::new(&mut s.lambda, 1e-6..=10.0)
                            .logarithmic(true)
                            .text("λ")
                            .custom_formatter(|v, _| format!("{v:.1e}")),
                    )
                    .on_hover_text("Small values follow the data closely; large ones approach a straight line");
                }
                SmoothMethod::Loess => {
                    ui.add(egui::Slider::new(&mut s.span, 0.1..=1.5).text("span"))
                        .on_hover_text("Fraction of the rows used in each local fit; larger is smoother");
                }
            }
            settings(ui, "the smoother", |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.method, SmoothMethod::Spline, SmoothMethod::Spline.label());
                    ui.radio_value(&mut s.method, SmoothMethod::Loess, SmoothMethod::Loess.label());
                });
                if s.method == SmoothMethod::Loess {
                    ui.horizontal(|ui| {
                        ui.label("Local polynomial:");
                        ui.radio_value(&mut s.local_degree, 1, "line");
                        ui.radio_value(&mut s.local_degree, 2, "quadratic");
                    });
                }
                ui.checkbox(&mut s.band, "Confidence band");
                if s.band {
                    level_field(ui, &mut s.level, "Level");
                }
            });
        }
        ui.separator();

        if toggle(
            ui,
            &mut spec.fit,
            "Fit line",
            "Least-squares polynomial per subgroup with bands and fit statistics",
            FitLineElement::default,
        ) {
            let f = spec.fit.as_mut().expect("on");
            ui.add(egui::DragValue::new(&mut f.degree).range(1..=5).prefix("degree "));
            settings(ui, "the fit line", |ui| {
                ui.checkbox(&mut f.band, "Confidence band for the mean (shaded)");
                ui.checkbox(&mut f.prediction, "Prediction limits for a new row (dashed)");
                level_field(ui, &mut f.level, "Level");
                ui.separator();
                ui.label("Write on the graph:");
                ui.checkbox(&mut f.labels.equation, "Equation");
                ui.checkbox(&mut f.labels.r_squared, "R²");
                ui.checkbox(&mut f.labels.p_value, "p-value (F test of the slope terms)");
            });
        }
        ui.separator();

        if toggle(
            ui,
            &mut spec.box_plot,
            "Box plot",
            "Median, quartiles and 1.5 × IQR whiskers per category (Tukey)",
            BoxPlotElement::default,
        ) {
            let b = spec.box_plot.as_mut().expect("on");
            settings(ui, "box plots", |ui| {
                ui.checkbox(&mut b.outliers, "Mark rows beyond the whiskers");
                ui.checkbox(&mut b.notches, "Notches (≈95% interval for the median)");
            });
        }
        if toggle(ui, &mut spec.violin, "Violin", "Density of values per category, mirrored", ViolinElement::default) {
            let v = spec.violin.as_mut().expect("on");
            settings(ui, "violins", |ui| {
                ui.add(egui::Slider::new(&mut v.bandwidth_scale, 0.2..=5.0).logarithmic(true).text("bandwidth ×"))
                    .on_hover_text("Multiplier of the default bandwidth; larger is smoother");
                ui.checkbox(&mut v.quartiles, "Quartile and median lines");
            });
        }
        if toggle(
            ui,
            &mut spec.summary,
            "Summary",
            "A statistic per category or x (mean, median, …) with error bars",
            SummaryElement::default,
        ) {
            let s = spec.summary.as_mut().expect("on");
            settings(ui, "summary points", |ui| summary_settings(ui, s));
        }
        if toggle(
            ui,
            &mut spec.brackets,
            "Brackets",
            "Brackets above the data between compared categories, with adjusted p-values or stars \
             (Tukey–Kramer, Games–Howell, Holm-adjusted t or Dunnett)",
            BracketElement::default,
        ) {
            let k = spec.brackets.as_mut().expect("on");
            settings(ui, "significance brackets", |ui| {
                ui.label("Comparisons:");
                for t in BracketTest::ALL {
                    ui.radio_value(&mut k.test, t, t.label());
                }
                ui.separator();
                ui.label("Write on each bracket:");
                for b in BracketLabel::ALL {
                    ui.radio_value(&mut k.label, b, b.label());
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut k.only_significant, "Only pairs with p below");
                    ui.add(egui::DragValue::new(&mut k.alpha).range(0.0001..=0.5).speed(0.001));
                });
                ui.label(RichText::new(star_key(k.thresholds)).weak().small());
                bracket_settings(ui, k, levels);
            });
        }
        ui.separator();

        if toggle(
            ui,
            &mut spec.bars,
            "Bars",
            "Bars of a statistic per category (counts with no vertical column), side by side or stacked",
            BarElement::default,
        ) {
            let b = spec.bars.as_mut().expect("on");
            settings(ui, "bars", |ui| {
                stat_choice(ui, "graph_bar_stat", &mut b.stat);
                ui.label("Subgroups:");
                for l in BarLayout::ALL {
                    ui.radio_value(&mut b.layout, l, l.label());
                }
                ui.separator();
                ui.label("Error bars (side-by-side means):");
                for e in ErrorBars::ALL {
                    ui.radio_value(&mut b.error_bars, e, e.label());
                }
                if b.error_bars == ErrorBars::Confidence {
                    level_field(ui, &mut b.level, "Level");
                }
                if b.error_bars != ErrorBars::None {
                    error_look_settings(ui, &mut b.error_look);
                }
                ui.checkbox(&mut b.labels, "Write values on the bars");
            });
        }
        if toggle(
            ui,
            &mut spec.line,
            "Line",
            "Join rows in table order (a run chart), in horizontal order, or a statistic per position",
            LineElement::default,
        ) {
            let l = spec.line.as_mut().expect("on");
            settings(ui, "the line", |ui| {
                for c in LineConnect::ALL {
                    ui.radio_value(&mut l.connect, c, c.label());
                }
                if l.connect == LineConnect::Summary {
                    stat_choice(ui, "graph_line_stat", &mut l.stat);
                }
            });
        }
        if toggle(
            ui,
            &mut spec.heatmap,
            "Heat map",
            "Cells for each pair of categories across and up, shaded by count (or by a continuous color column)",
            HeatmapElement::default,
        ) {
            let h = spec.heatmap.as_mut().expect("on");
            settings(ui, "the heat map", |ui| {
                ui.label("With a continuous color column, shade by:");
                stat_choice(ui, "graph_heat_stat", &mut h.stat);
                ui.checkbox(&mut h.labels, "Write values in the cells");
            });
        }
        ui.separator();

        distribution_layers(ui, spec);
        ui.separator();

        if toggle(
            ui,
            &mut spec.ellipse,
            "Ellipse",
            "Bivariate normal density ellipse per subgroup",
            EllipseElement::default,
        ) {
            let e = spec.ellipse.as_mut().expect("on");
            settings(ui, "density ellipses", |ui| {
                ui.label("Probability inside:");
                probability_set(ui, &mut e.levels, &[0.5, 0.9, 0.95, 0.99]);
                ui.checkbox(&mut e.correlation, "Write the correlation");
            });
        }
        if toggle(
            ui,
            &mut spec.hull,
            "Hulls",
            "Convex hull around each subgroup's points, outlined in its color and line pattern",
            HullElement::default,
        ) {
            let h = spec.hull.as_mut().expect("on");
            settings(ui, "convex hulls", |ui| {
                ui.checkbox(&mut h.fill, "Shade inside each hull");
            });
        }
        if toggle(
            ui,
            &mut spec.contour,
            "Contours",
            "Kernel density contours holding given fractions of the rows",
            ContourElement::default,
        ) {
            let c = spec.contour.as_mut().expect("on");
            settings(ui, "density contours", |ui| {
                ui.label("Fractions of rows enclosed:");
                probability_set(ui, &mut c.fractions, &[0.1, 0.25, 0.5, 0.75, 0.9, 0.95]);
                ui.add(egui::Slider::new(&mut c.bandwidth_scale, 0.2..=5.0).logarithmic(true).text("bandwidth ×"));
                ui.checkbox(&mut c.filled, "Shade the regions")
                    .on_hover_text("A translucent wash inside each contour; the densest region is darkest");
            });
        }
        if toggle(
            ui,
            &mut spec.bins,
            "Bins",
            "Counts of rows in hexagonal or square cells, shaded light to dark",
            BinElement::default,
        ) {
            let b = spec.bins.as_mut().expect("on");
            settings(ui, "binned counts", |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut b.hexagonal, true, "hexagons");
                    ui.radio_value(&mut b.hexagonal, false, "squares");
                });
                ui.add(egui::Slider::new(&mut b.bins, 4..=80).text("cells across"));
            });
        }
        ui.separator();

        if toggle(ui, &mut spec.function, "Formula", "Curves y = f(x) typed as formulas, drawn in data units", || {
            FunctionElement { formulas: vec![String::new()] }
        }) {
            let f = spec.function.as_mut().expect("on");
            settings(ui, "formula curves", |ui| formula_settings(ui, f));
        }
        ui.separator();
        drawing_order(ui, spec);
    });
    let request = ui.ctx().data_mut(|d| {
        let r = d.get_temp::<(ElementKind, bool)>(order_request_id());
        d.remove::<(ElementKind, bool)>(order_request_id());
        r
    });
    if let Some((kind, forward)) = request {
        spec.move_layer(kind, forward);
    }
}

/// Layers for one continuous column across: histogram, dot plot and
/// probability plot. The dot and probability plots use the vertical axis for
/// their own scale, so turning one on turns the points layer off.
fn distribution_layers(ui: &mut egui::Ui, spec: &mut GraphSpec) {
    if toggle(
        ui,
        &mut spec.histogram,
        "Histogram",
        "Counts (or density) of the horizontal column in equal-width bins; needs no vertical column",
        HistogramElement::default,
    ) {
        let h = spec.histogram.as_mut().expect("on");
        settings(ui, "the histogram", |ui| {
            ui.label("Bins:");
            let auto = matches!(h.bins, HistogramBins::Auto);
            if ui.radio(auto, "Automatic (Sturges' rule, round width)").clicked() {
                h.bins = HistogramBins::Auto;
            }
            let mut count = if let HistogramBins::Count(k) = h.bins { Some(k) } else { None };
            ui.horizontal(|ui| {
                if ui.radio(count.is_some(), "About").clicked() && count.is_none() {
                    count = Some(10);
                }
                let mut k = count.unwrap_or(10);
                if ui.add(egui::DragValue::new(&mut k).range(1..=200)).changed() {
                    count = Some(k);
                }
                ui.label("bins");
            });
            if let Some(k) = count {
                h.bins = HistogramBins::Count(k);
            }
            let mut width = if let HistogramBins::Width(w) = h.bins { Some(w) } else { None };
            ui.horizontal(|ui| {
                if ui.radio(width.is_some(), "Width").clicked() && width.is_none() {
                    width = Some(1.0);
                }
                let mut w = width.unwrap_or(1.0);
                if ui.add(egui::DragValue::new(&mut w).range(1e-9..=1e12).speed(0.05)).changed() {
                    width = Some(w);
                }
            });
            if let Some(w) = width {
                h.bins = HistogramBins::Width(w);
            }
            ui.separator();
            ui.checkbox(&mut h.density, "Density (each subgroup's area is 1)");
            ui.checkbox(&mut h.stacked, "Stack subgroups (otherwise overlaid)");
        });
    }
    if toggle(
        ui,
        &mut spec.dot_plot,
        "Dot plot",
        "One dot per row of the horizontal column, stacked where values are close (Wilkinson)",
        || {
            spec.points = None;
            DotPlotElement::default()
        },
    ) {
        let d = spec.dot_plot.as_mut().expect("on");
        settings(ui, "the dot plot", |ui| {
            let mut fixed = d.width.is_some();
            if ui.checkbox(&mut fixed, "Fixed stack width").on_hover_text("Otherwise 1/30 of the range").changed() {
                d.width = fixed.then_some(1.0);
            }
            if let Some(w) = d.width.as_mut() {
                ui.add(egui::DragValue::new(w).range(1e-9..=1e12).speed(0.05).prefix("width "));
            }
        });
    }
    if toggle(
        ui,
        &mut spec.probability_plot,
        "Probability plot",
        "Ordered values of the horizontal column against a distribution fitted to them; straight means a good fit",
        || {
            spec.points = None;
            ProbabilityPlotElement::default()
        },
    ) {
        let p = spec.probability_plot.as_mut().expect("on");
        egui::ComboBox::from_id_salt("graph_probability_family").selected_text(p.family.label()).show_list(ui, |ui| {
            for f in stat_core::distribution_fit::FitFamily::ALL {
                ui.selectable_value(&mut p.family, f, f.label());
            }
        });
        settings(ui, "the probability plot", |ui| {
            ui.checkbox(&mut p.band, "Band for each ordered value under the fit");
            if p.band {
                level_field(ui, &mut p.level, "Level");
            }
        });
    }
}

/// A statistic chooser (percentiles with a slider for p).
fn stat_choice(ui: &mut egui::Ui, id: &str, stat: &mut SummaryStat) {
    egui::ComboBox::from_id_salt(id).selected_text(stat.label()).show_list(ui, |ui| {
        for choice in SummaryStat::CHOICES {
            let chosen = stat.same_kind(choice);
            let label = if let SummaryStat::Percentile(_) = choice { "Percentile".to_owned() } else { choice.label() };
            if ui.selectable_label(chosen, label).clicked() && !chosen {
                *stat = choice;
            }
        }
    });
    if let SummaryStat::Percentile(p) = stat {
        let mut pct = *p * 100.0;
        if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).text("percentile")).changed() {
            *p = pct / 100.0;
        }
    }
}

fn summary_settings(ui: &mut egui::Ui, s: &mut SummaryElement) {
    stat_choice(ui, "graph_summary_stat", &mut s.stat);
    ui.separator();
    ui.label("Error bars (around means):");
    for e in ErrorBars::ALL {
        ui.radio_value(&mut s.error_bars, e, e.label());
    }
    if s.error_bars == ErrorBars::Confidence {
        level_field(ui, &mut s.level, "Level");
    }
    if s.error_bars != ErrorBars::None {
        error_look_settings(ui, &mut s.error_look);
    }
}

fn formula_settings(ui: &mut egui::Ui, f: &mut FunctionElement) {
    ui.label("y = f(x), e.g. 2 + 0.5*x^2 or 100*exp(-x/4)");
    let mut remove = None;
    for (i, text) in f.formulas.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label("y =");
            ui.add(egui::TextEdit::singleline(text).desired_width(220.0).font(egui::TextStyle::Monospace));
            if ui.small_button("×").on_hover_text("Remove this formula").clicked() {
                remove = Some(i);
            }
        });
        if !text.trim().is_empty() {
            if let Err(e) = Expr::parse(text) {
                ui.colored_label(crate::theme::ERROR, e.to_string());
            }
        }
    }
    if let Some(i) = remove {
        f.formulas.remove(i);
    }
    if ui.button("+ Add a formula").clicked() {
        f.formulas.push(String::new());
    }
    ui.label(
        RichText::new("Functions: sqrt abs exp ln log10 log(v, base) sin cos tan min max pow; constants pi, e")
            .weak()
            .small(),
    );
}
