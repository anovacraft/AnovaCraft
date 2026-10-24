//! First-impression polish: the grid's column headings, the starting
//! Untitled table, menu headings, the report outline and plot hint, number
//! formats, the GLM's response and event, and the dialogs' script preview.

use super::*;

/// How many times `text` was painted in the last frame.
fn painted(gui: &Gui, text: &str) -> usize {
    gui.painted_text().iter().filter(|t| t.as_str() == text).count()
}

/// Tick the first check box labelled `label`.
fn tick(gui: &mut Gui, label: &str) {
    let tick = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
    gui.harness.get_all_by(tick).next().expect("a check box").simulate_click();
    gui.run();
}

/// Whether the radio button labelled `label` is chosen.
fn radio_on(gui: &Gui, label: &str) -> bool {
    let radio = |n: &Node<'_>| n.role() == Role::RadioButton && n.label().as_deref() == Some(label);
    gui.harness.get_by(radio).toggled() == Some(egui::accesskit::Toggled::True)
}

/// The app starts on the start page with no table open; a new blank table
/// left untouched closes once a sample opens, but a table with any edit stays.
#[test]
fn an_untouched_untitled_table_closes_when_something_opens() {
    let mut gui = Gui::start();
    assert!(gui.app().documents.is_empty(), "no table at startup");
    gui.perform(Action::NewTable);
    assert_eq!(gui.app().documents.len(), 1);
    gui.perform(Action::OpenSample(0));
    assert_eq!(gui.app().documents.len(), 1, "the Untitled table made way for the sample");
    assert!(gui.app().project(gui.app().documents[0].project).unwrap().sample);

    gui.perform(Action::NewTable);
    assert_eq!(gui.app().documents.len(), 2, "a new table opens beside the sample");
    gui.perform(Action::OpenSample(0));
    assert_eq!(gui.app().documents.len(), 2, "the blank new table closed for the second sample");

    gui.perform(Action::NewTable);
    gui.grid(vec![GridRequest::InsertRows { at: 0, count: 1 }]);
    gui.perform(Action::OpenSample(0));
    assert_eq!(gui.app().documents.len(), 4, "a table with an edit is kept");
}

/// Column headings in the grid carry the whole name (no "GNP …or") and a
/// level icon in place of the old letter and type tags.
#[test]
fn grid_headings_fit_whole_names() {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].workspace = Workspace::Data;
    gui.run();
    gui.shot("grid-fitted-headings");
    for name in ["GNP deflator", "Population 14+", "Armed forces", "Unemployed"] {
        assert_eq!(painted(&gui, name), 2, "{name} in the sidebar and whole in its heading");
    }
    assert_eq!(painted(&gui, "123"), 0, "no number-type tag beside the names");
}

/// The line on how to work plots shows above the reports only while the
/// getting-started tips are on.
#[test]
fn plot_hint_follows_the_tips_setting() {
    let mut gui = Gui::with_sample("Longley");
    gui.results();
    let hint = |gui: &Gui| gui.painted_text().iter().any(|t| t.starts_with("Plots: drag an axis end"));
    gui.app_mut().settings.tips_dismissed = false;
    gui.run();
    assert!(hint(&gui));
    gui.app_mut().settings.tips_dismissed = true;
    gui.run();
    assert!(!hint(&gui), "hidden once the tips are dismissed");
}

/// The Model menu groups its items under headings.
#[test]
fn model_menu_has_headings() {
    let mut gui = Gui::with_sample("Longley");
    let is_menu = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Model");
    gui.harness.get_all_by(is_menu).next().expect("the Model menu").simulate_click();
    gui.run();
    gui.shot("model-menu-headings");
    for heading in [
        "Linear models",
        "Generalized, nonlinear and mixed",
        "Method comparison and stability",
        "Predictive models",
        "Compare and optimize",
    ] {
        assert!(gui.shows(heading), "{heading}");
    }
}

/// With more than four reports the outline becomes a "Go to" list, whose
/// entries jump to their report.
#[test]
fn go_to_list_with_many_reports() {
    let mut gui = Gui::with_sample("Longley");
    gui.results();
    assert!(gui.shows("Go to"), "three reports: links");
    let active = gui.app().active;
    let saved: Vec<_> = gui.app().documents[active].reports.iter().map(|r| r.saved()).collect();
    for s in saved.into_iter().take(2) {
        let id = gui.app().next_report_id;
        gui.app_mut().next_report_id += 1;
        let doc = &mut gui.app_mut().documents[active];
        let report = Report::new(id, s, &doc.table, &doc.rows);
        doc.reports.push(report);
    }
    gui.run();
    assert_eq!(gui.app().documents[active].reports.len(), 5);
    assert!(!gui.shows("Go to"), "no line of links");
    gui.click("Go to ▾");
    gui.shot("go-to-list");
    let titles: Vec<String> = gui.app().documents[active].reports.iter().map(|r| r.title()).collect();
    let entries = gui
        .harness
        .query_all_by(|n| {
            n.role() == Role::Button
                && n.label().is_some_and(|l| titles.iter().any(|t| t.starts_with(l.trim_end_matches('…'))))
        })
        .count();
    assert!(entries >= 5, "an entry per report ({entries})");
}

/// Report tables can separate thousands and line up decimal points.
#[test]
fn report_numbers_follow_the_number_format() {
    let mut gui = Gui::with_sample("Longley");
    gui.results();
    assert!(gui.painted_text().iter().any(|t| t == "184150146"));
    gui.app_mut().settings.group_thousands = true;
    gui.app_mut().settings.align_decimals = true;
    gui.run();
    gui.shot("report-number-format");
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t == "184,150,146"), "thousands separated");
    assert!(text.iter().any(|t| t == "-3,598,729"), "signed numbers too");
    assert!(text.iter().any(|t| t == "0.995359"), "short numbers unchanged");
}

/// The GLM dialog asks for the response first, suggests a distribution from
/// its values, and lets the user pick which value is the event.
#[test]
fn glm_suggests_a_distribution_and_takes_an_event() {
    const T: &str = "Generalized linear model";
    let mut gui = Gui::with_sample("Customer churn");
    gui.menu(&["Model", "Generalized linear model…"]);
    gui.choose(T, 0, "Support calls");
    assert!(radio_on(&gui, "Poisson (counts)"), "whole numbers suggest counts");
    gui.choose(T, 0, "Churned");
    assert!(radio_on(&gui, "Binomial (yes/no)"), "two values suggest binomial");
    assert!(gui.painted_text().iter().any(|t| t == "models P(= Yes) against No"));
    gui.choose(T, 1, "No");
    assert!(gui.painted_text().iter().any(|t| t == "models P(= No) against Yes"));
    tick(&mut gui, "Tenure (months)");
    gui.shot("dialog-glm-event");
    gui.click("Run");
    let active = gui.app().active;
    let title = gui.app().documents[active].reports[0].title();
    assert!(title.starts_with("Logistic Regression: P(Churned = No)"), "{title}");
}

/// Analysis dialogs show the script call their settings make, live.
#[test]
fn dialog_script_preview() {
    const T: &str = "Linear model";
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Model", "Linear model…"]);
    gui.click("Script");
    assert!(gui.shows("The script appears once the dialog is ready to run."));
    gui.choose(T, 0, "Employed");
    tick(&mut gui, "GNP");
    gui.shot("dialog-script-preview");
    let script = gui
        .painted_text()
        .into_iter()
        .find(|t| t.starts_with("let t = current_table();"))
        .expect("the script is shown");
    assert!(script.contains("t.analyze(") && script.contains("\"Employed\"") && script.contains("\"GNP\""), "{script}");
    assert!(gui.shows("Copy script"));
    // The setting holds for the next dialog too, and turns off again.
    gui.click("Script");
    assert!(!gui.shows("Copy script"));
}

/// Tooltips wait one second of stillness by default, also when the pointer
/// moves on from one control's open tooltip to the next (no grace time), and
/// the Settings window's tooltip delay changes that at once for the light and
/// dark styles.
#[test]
fn tooltip_delay_defaults_to_one_second_and_follows_settings() {
    let mut gui = Gui::start();
    let delays = |gui: &Gui| {
        [egui::Theme::Light, egui::Theme::Dark].map(|t| {
            let style = gui.harness.ctx.style_of(t);
            let i = &style.interaction;
            (i.tooltip_delay, i.tooltip_grace_time, i.show_tooltips_only_when_still)
        })
    };
    assert_eq!(delays(&gui), [(1.0, 0.0, true); 2]);
    gui.menu(&["File", "Settings…"]);
    assert!(radio_on(&gui, "Medium (1 second)"));
    gui.click("Long (2 seconds)");
    assert_eq!(gui.app().settings.tooltip_delay, TooltipDelay::Long);
    assert_eq!(delays(&gui), [(2.0, 0.0, true); 2]);
    gui.click("Short (½ second)");
    assert_eq!(delays(&gui), [(0.5, 0.0, true); 2]);
}

/// Control charts by phase: a solid boundary keyed as such, every other
/// phase shaded, and each phase named in a label along the top of the chart
/// (they were small text after a faint dotted line, hard to see).
#[test]
fn control_chart_phases_are_easy_to_read() {
    let mut gui = Gui::with_sample("Fill weights");
    gui.harness.set_size(egui::vec2(1500.0, 2600.0));
    gui.results();
    gui.shot("control-chart-phases");
    assert!(painted(&gui, "Phase boundary") >= 1, "the boundary is in the key");
    let rects = gui.painted_text_rects();
    for phase in ["A", "B"] {
        assert!(rects.iter().any(|(t, ..)| t == phase), "phase {phase} is named: {:?}", gui.painted_text());
    }
    assert_eq!(painted(&gui, " A") + painted(&gui, " B"), 0, "the old small labels are gone");
    // The names sit at the top edge of their chart, inside it.
    for (_, rect, clip) in rects.iter().filter(|(t, ..)| t == "B") {
        assert!(clip.contains_rect(*rect), "{rect:?} inside {clip:?}");
    }
}
