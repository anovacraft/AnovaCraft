//! Settings of the clustered heat map, estimation plot, Sankey, chord and
//! network chart forms, shown in a row under the view bar while one of
//! those forms is chosen.

use crate::dialog_kit::ComboExt as _;
use eframe::egui::{self, RichText};
use stat_data::DataTable;
use stat_viz::cluster_map::LINKAGES;
use stat_viz::filter::column_levels;
use stat_viz::{EstimationInterval, GraphForm, GraphSpec, JitterKind, PointsElement};

/// The chart forms this module adds to the Chart chooser, with their hints.
pub(super) fn choices() -> [(GraphForm, &'static str); 5] {
    [
        (
            GraphForm::ClusterMap(stat_viz::ClusterMapSettings::default()),
            "The numeric horizontal-axis columns as a matrix of shaded cells, one row per table row, with rows \
             and columns reordered by hierarchical clustering and their dendrograms beside it",
        ),
        (
            GraphForm::Estimation,
            "Every value of each group (horizontal column) and each group's mean difference from a reference \
             group with its confidence interval (bootstrap or Welch)",
        ),
        (
            GraphForm::Sankey(stat_viz::FlowSettings::default()),
            "Two or more horizontal-axis columns as stages, joined by flows sized by rows or by the vertical \
             column's sum",
        ),
        (
            GraphForm::Chord(stat_viz::FlowSettings::default()),
            "A \"from\" and a \"to\" column (horizontal axis): arcs around a circle joined by ribbons sized by \
             the flow between them",
        ),
        (
            GraphForm::Network(stat_viz::NetworkSettings::default()),
            "The two ends of each edge (horizontal axis), with an optional numeric weight (vertical axis), laid \
             out by a force-directed algorithm",
        ),
    ]
}

/// What to change when one of these forms is chosen: an estimation plot
/// lays its points out as a swarm.
pub(super) fn on_chosen(spec: &mut GraphSpec) {
    if spec.form == GraphForm::Estimation {
        let p = spec.points.get_or_insert_with(PointsElement::default);
        p.jitter = JitterKind::Swarm;
        p.jitter_width = p.jitter_width.max(0.45);
    }
}

/// The chosen form's settings, if it is one of these.
pub(super) fn settings_bar(ui: &mut egui::Ui, spec: &mut GraphSpec, table: &DataTable) {
    match &mut spec.form {
        GraphForm::ClusterMap(c) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Heat map:").strong());
                ui.checkbox(&mut c.standardize, "standardize columns")
                    .on_hover_text("Shade each column's values as z-scores (mean 0, standard deviation 1)");
                ui.checkbox(&mut c.cluster_rows, "cluster rows");
                ui.checkbox(&mut c.cluster_columns, "cluster columns");
                ui.label("linkage");
                egui::ComboBox::from_id_salt("graph_cluster_linkage").selected_text(c.linkage.label()).show_list(
                    ui,
                    |ui| {
                        for l in LINKAGES {
                            ui.selectable_value(&mut c.linkage, l, l.label());
                        }
                    },
                );
                ui.checkbox(&mut c.values, "values in cells");
            });
        }
        GraphForm::Estimation => {
            let levels = spec.encoding.x.first().map(|c| column_levels(table, c)).unwrap_or_default();
            let e = &mut spec.estimation;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Estimation:").strong());
                ui.label("interval");
                for m in EstimationInterval::ALL {
                    ui.radio_value(&mut e.interval, m, m.label());
                }
                if e.interval == EstimationInterval::Bca {
                    ui.add(egui::DragValue::new(&mut e.resamples).range(200..=100_000).speed(50).suffix(" resamples"));
                    ui.add(egui::DragValue::new(&mut e.seed).prefix("seed "))
                        .on_hover_text("Seed of the resampling: the same seed gives the same interval");
                }
                let mut pct = e.level * 100.0;
                if ui.add(egui::DragValue::new(&mut pct).range(50.0..=99.9).speed(0.5).suffix("% level")).changed() {
                    e.level = pct / 100.0;
                }
                ui.label("reference");
                let shown = e.reference.clone().filter(|r| levels.contains(r));
                let text = shown.clone().or_else(|| levels.first().cloned()).unwrap_or_else(|| "—".to_owned());
                egui::ComboBox::from_id_salt("graph_estimation_reference").selected_text(text).show_list(ui, |ui| {
                    for (k, l) in levels.iter().enumerate() {
                        let on = shown.as_ref().map_or(k == 0, |r| r == l);
                        if ui.selectable_label(on, l.as_str()).clicked() {
                            e.reference = Some(l.clone());
                        }
                    }
                });
            });
        }
        GraphForm::Sankey(f) | GraphForm::Chord(f) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Flows:").strong());
                ui.checkbox(&mut f.size_by_sum, "size by the vertical column's sum")
                    .on_hover_text("Otherwise each flow's width is its number of rows");
                ui.checkbox(&mut f.labels, "labels");
            });
        }
        GraphForm::Network(n) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Network:").strong());
                ui.add(egui::DragValue::new(&mut n.iterations).range(1..=5000).suffix(" iterations"));
                ui.add(egui::DragValue::new(&mut n.seed).prefix("seed "))
                    .on_hover_text("Seed of the starting positions: the same seed gives the same layout");
                ui.checkbox(&mut n.labels, "labels");
            });
        }
        _ => {}
    }
}
