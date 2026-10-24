//! Behaviour every dialog shares (`dialog_kit`): why Run is unavailable, the
//! summary line, keys, remembered settings, Reset and "Edit analysis…".

use super::*;

const LINEAR: &str = "Linear model";

fn reports(gui: &Gui) -> usize {
    gui.app().documents[gui.app().active].reports.len()
}

fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

/// The ⋯ Options menu of the top report.
fn first_options(gui: &mut Gui) {
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
}

fn key(gui: &mut Gui, key: egui::Key, modifiers: egui::Modifiers) {
    gui.harness.input_mut().events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    gui.run();
}

/// The note under the buttons says what is missing, then what will run.
#[test]
fn run_button_says_why_it_is_unavailable() {
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Model", "Linear model…"]);
    assert!(gui.shows("⚠ Choose a response"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(LINEAR, 0, "Employed");
    assert!(gui.shows("⚠ Choose at least one predictor"));
    tick(&mut gui, "GNP");
    tick(&mut gui, "Population 14+");
    assert!(!gui.widget("Run").is_disabled());
    assert!(gui.shows("Linear model of Employed on 2 effects · 16 rows"), "{:?}", gui.painted_text());
    gui.shot("dialog-run-ready");
    let before = reports(&gui);
    gui.click("Run");
    assert_eq!(reports(&gui), before + 1);
    assert!(gui.harness.query_by_role_and_label(Role::Window, LINEAR).is_none());
}

/// Enter runs and Escape cancels; settings come back on reopening, and
/// Reset clears them.
#[test]
fn keys_memory_and_reset() {
    let mut gui = Gui::with_sample("Longley");
    let before = reports(&gui);
    gui.menu(&["Model", "Linear model…"]);
    gui.choose(LINEAR, 0, "Employed");
    tick(&mut gui, "GNP");
    key(&mut gui, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(gui.harness.query_by_role_and_label(Role::Window, LINEAR).is_none(), "Escape cancels");
    assert_eq!(reports(&gui), before);
    gui.menu(&["Model", "Linear model…"]);
    assert!(gui.shows("Linear model of Employed on 1 effect · 16 rows"), "settings remembered");
    key(&mut gui, egui::Key::Enter, egui::Modifiers::NONE);
    assert_eq!(reports(&gui), before + 1, "Enter runs");
    gui.menu(&["Model", "Linear model…"]);
    gui.click("Reset");
    assert!(gui.shows("⚠ Choose a response"), "Reset clears the dialog");
}

/// A dialog that closed while keys were handed back to the data grid (the
/// last click was outside it) answers Enter and Escape again when it is
/// next opened (it used to ignore them until clicked inside).
#[test]
fn keys_work_in_a_reopened_dialog() {
    let mut gui = Gui::with_sample("Longley");
    let longley = gui.app().active;
    let other = DataTable::new("Other", vec![stat_data::Column::numeric("x", vec![1.0, 2.0])]).unwrap();
    gui.app_mut().open_table(other, None);
    let other = gui.app().active;
    gui.perform(Action::Activate(longley));
    gui.menu(&["Model", "Linear model…"]);
    // A click outside the dialog hands the keys to the grid.
    let outside = egui::pos2(1350.0, 600.0);
    let b = gui.window(LINEAR).raw_bounds().expect("dialog bounds");
    let (x, y) = (f64::from(outside.x), f64::from(outside.y));
    assert!(!(x >= b.x0 && x <= b.x1 && y >= b.y0 && y <= b.y1), "the click is outside the dialog");
    for pressed in [true, false] {
        gui.harness.input_mut().events.push(egui::Event::PointerButton {
            pos: outside,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        gui.run();
    }
    key(&mut gui, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(gui.app().dialog.is_some(), "keys belong to the grid now");
    // Switching documents closes the dialog; back on Longley it opens afresh.
    gui.perform(Action::Activate(other));
    assert!(gui.app().dialog.is_none());
    gui.perform(Action::Activate(longley));
    gui.menu(&["Model", "Linear model…"]);
    key(&mut gui, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(gui.app().dialog.is_none(), "Escape cancels the newly opened dialog");
}

/// The dialog opens with the columns selected in the grid.
#[test]
fn dialog_starts_with_selected_columns() {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].grid.select_columns(&[1, 2]);
    gui.menu(&["Describe", "Distribution…"]);
    assert!(gui.shows("2 distribution reports · 16 rows"), "{:?}", gui.painted_text());
}

/// "Edit analysis…" reopens the dialog filled in; Update replaces the
/// report in place, Run as new keeps both.
#[test]
fn edit_analysis_updates_the_report_in_place() {
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Model", "Linear model…"]);
    gui.choose(LINEAR, 0, "Employed");
    tick(&mut gui, "GNP");
    let before = reports(&gui);
    gui.click("Run");
    gui.results();
    let id = gui.app().documents[gui.app().active].reports[0].id();
    first_options(&mut gui);
    gui.click("Edit analysis…");
    assert!(gui.shows("Linear model of Employed on 1 effect · 16 rows"), "filled from the report");
    tick(&mut gui, "Population 14+");
    gui.shot("dialog-edit-analysis");
    gui.click("Update");
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.reports.len(), before + 1);
    assert_eq!(doc.reports[0].id(), id, "same report");
    let app_model::SavedReport::Linear { spec, .. } = doc.reports[0].saved() else { panic!("a linear model") };
    assert_eq!(spec.predictors.len(), 2);
    first_options(&mut gui);
    gui.click("Edit analysis…");
    gui.click("Run as new");
    assert_eq!(reports(&gui), before + 2);
}

/// Switching to another document closes the open dialog but keeps its
/// settings for the document it was open on: reopening it there restores
/// them (they used to be lost), while the other document's dialog starts
/// afresh.
#[test]
fn switching_documents_keeps_open_dialog_settings() {
    let mut gui = Gui::with_sample("Longley");
    let longley = gui.app().active;
    let other = DataTable::new("Other", vec![stat_data::Column::numeric("x", vec![1.0, 2.0])]).unwrap();
    gui.app_mut().open_table(other, None);
    let other = gui.app().active;
    gui.perform(Action::Activate(longley));
    gui.menu(&["Model", "Linear model…"]);
    gui.choose(LINEAR, 0, "Employed");
    tick(&mut gui, "GNP");
    assert!(gui.shows("Linear model of Employed on 1 effect · 16 rows"));
    gui.perform(Action::Activate(other));
    assert!(gui.app().dialog.is_none(), "the dialog closes");
    gui.menu(&["Model", "Linear model…"]);
    assert!(gui.shows("⚠ Choose a response"), "the other document's dialog starts afresh");
    gui.click("Cancel");
    gui.perform(Action::Activate(longley));
    gui.menu(&["Model", "Linear model…"]);
    assert!(gui.shows("Linear model of Employed on 1 effect · 16 rows"), "settings restored: {:?}", gui.painted_text());
}

/// Tick the last check box labelled `label` (the model-effect list comes
/// after the predictor list, whose boxes have the same names).
fn tick_last(gui: &mut Gui, label: &str) {
    let found = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
    gui.harness.get_all_by(found).last().expect("a check box").simulate_click();
    gui.run();
}

/// A main effect unticked in the effect list stays out while its predictor
/// is still chosen (it used to come straight back), and heredity still
/// holds: an interaction brings its main effects, and removing a main
/// effect removes the interaction built on it.
#[test]
fn unticked_main_effect_stays_out_of_the_model() {
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Model", "Linear model…"]);
    gui.choose(LINEAR, 0, "Employed");
    tick(&mut gui, "GNP");
    tick(&mut gui, "Population 14+");
    assert!(gui.shows("Linear model of Employed on 2 effects · 16 rows"));
    tick_last(&mut gui, "GNP");
    assert!(gui.shows("Linear model of Employed on 1 effect · 16 rows"), "{:?}", gui.painted_text());
    tick_last(&mut gui, "GNP × Population 14+");
    assert!(gui.shows("Linear model of Employed on 3 effects (1 interaction) · 16 rows"), "{:?}", gui.painted_text());
    tick_last(&mut gui, "GNP");
    assert!(gui.shows("Linear model of Employed on 1 effect · 16 rows"), "{:?}", gui.painted_text());
    let before = reports(&gui);
    gui.click("Run");
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.reports.len(), before + 1);
    let app_model::SavedReport::Linear { spec, .. } = doc.reports[0].saved() else { panic!("a linear model") };
    assert_eq!(spec.predictors.len(), 1, "GNP, with no effect left, is not in the model");
}

/// A response surface whose contour plot showed its third factor, edited
/// down to two factors, goes back to the first two on the contour axes (it
/// kept an axis beyond the factors, so its drop-down showed nothing).
#[test]
fn surface_edited_to_fewer_factors_resets_contour_axes() {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    let app = gui.app_mut();
    let id = app.next_report_id;
    app.next_report_id += 1;
    let doc = &mut app.documents[active];
    let col = |name: &str| doc.table.find(name).expect("a Longley column");
    let saved = app_model::SavedReport::Surface {
        request: app_model::SurfaceRequest {
            response: col("Employed"),
            factors: vec![col("GNP"), col("Population 14+"), col("Year")],
        },
        options: app_model::SurfaceOptions { contour_x: 2, contour_y: 0, ..app_model::SurfaceOptions::default() },
    };
    let report = Report::new(id, saved, &doc.table, &doc.rows);
    doc.reports.insert(0, report);
    let axes = |gui: &Gui| match gui.app().documents[active].reports[0].saved() {
        app_model::SavedReport::Surface { request, options } => {
            (request.factors.len(), options.contour_x, options.contour_y)
        }
        _ => panic!("a response surface"),
    };
    assert_eq!(axes(&gui), (3, 2, 0));
    gui.results();
    first_options(&mut gui);
    gui.click("Edit analysis…");
    tick(&mut gui, "Year");
    gui.click("Update");
    assert_eq!(axes(&gui), (2, 0, 1), "the contour plot is of the two factors left");
}

/// A drop-down list longer than its pop-up says so at the cut-off edge
/// (a ▾ over a fade); a short list does not.
#[test]
fn long_drop_down_lists_show_there_is_more() {
    const NONLINEAR: &str = "Nonlinear regression";
    let mut gui = Gui::with_sample("Fuel spectra");
    gui.menu(&["Model", "Nonlinear regression…"]);
    gui.choose(NONLINEAR, 0, "Octane");
    let marks = |gui: &Gui| gui.painted_text().iter().filter(|t| t.as_str() == "▾" || t.as_str() == "▴").count();
    let combo = |gui: &mut Gui, n: usize| {
        gui.window(NONLINEAR).get_all_by_role(Role::ComboBox).nth(n).expect("a drop-down list").simulate_click();
        gui.run();
    };
    // The predictor list holds every numeric column: hundreds of wavelengths.
    let before = marks(&gui);
    combo(&mut gui, 2);
    let texts = gui.painted_text();
    assert!(texts.iter().any(|t| t == "▾"), "no cue below a long list");
    assert!(!texts.iter().any(|t| t == "▴"), "nothing above the top of the list yet");
    assert_eq!(marks(&gui), before + 1);
    gui.shot("long-drop-down-list");
    combo(&mut gui, 2);
    // The weights list is short enough to show whole.
    let before = marks(&gui);
    combo(&mut gui, 4);
    assert!(gui.shows("1/Y² (relative error)") || gui.painted_text().iter().any(|t| t.starts_with("1/Y")));
    assert_eq!(marks(&gui), before, "a list that fits has no cue");
}

/// A long drop-down list fades at its cut-off edge across the list only: the
/// cue once took the screen's width (a scroll area clips only up and down),
/// drawing a faint band across the whole window.
#[test]
fn drop_down_overflow_cue_stays_inside_the_list() {
    let mut gui = Gui::with_sample("Fuel spectra");
    gui.menu(&["Model", "Compare models…"]);
    gui.window("Compare models").get_all_by_role(Role::ComboBox).next().expect("the response list").simulate_click();
    gui.run();
    let screen = gui.harness.ctx.screen_rect();
    // The pop-up is painted last, after any other ▾ in the window.
    let cue = gui.painted_text_rects().into_iter().rfind(|t| t.0 == "▾").expect("the list scrolls");
    assert!(cue.2.width() < screen.width() / 2.0, "the cue is clipped to the list, not the screen: {:?}", cue.2);
    // The fade itself: a four-cornered mesh ending at the list's lower edge.
    fn meshes(shape: &egui::Shape, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Mesh(m) if m.vertices.len() == 4 => out.push(m.calc_bounds()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| meshes(s, out)),
            _ => {}
        }
    }
    let mut fades = Vec::new();
    for clipped in &gui.harness.output().shapes {
        meshes(&clipped.shape, &mut fades);
    }
    let fade = fades.iter().find(|r| (r.bottom() - cue.1.bottom()).abs() < 12.0).expect("a fade above the ▾");
    assert!(fade.width() < screen.width() / 2.0, "the fade spans the list, not the window: {fade:?}");
}
