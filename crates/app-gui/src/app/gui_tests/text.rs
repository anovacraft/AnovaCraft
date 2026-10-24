//! Multivariate ▸ Text exploration…, Topic model… and Sentiment scores…:
//! fill each dialog, run it, check the report, click a word of the word
//! cloud to select its documents, and reopen a report with "Edit analysis…".

use super::*;

/// Open the reviews sample with no reports.
fn reviews() -> Gui {
    let mut gui = Gui::with_sample("Espresso machine reviews");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui
}

fn reports(gui: &Gui) -> Vec<app_model::SavedReport> {
    gui.app().documents[gui.app().active].reports.iter().map(|r| r.saved()).collect()
}

/// Reopen the only report in its dialog, check its summary and Update it in place.
fn edit_and_update(gui: &mut Gui, summary: &str) {
    gui.results();
    let id = gui.app().documents[gui.app().active].reports[0].id();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
    assert!(gui.shows(summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.reports.len(), 1);
    assert_eq!(doc.reports[0].id(), id);
}

/// Press and release the primary button at `at`.
fn click_at(gui: &mut Gui, at: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(at, true));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(at, false));
    gui.run();
}

#[test]
fn text_exploration_runs_and_the_cloud_selects_documents() {
    let mut gui = reviews();
    gui.menu(&["Multivariate", "Text exploration…"]);
    gui.shot("dialog-text-exploration-first-open");
    // The review text column is chosen for you.
    let summary = "Term frequencies and word cloud · 300 rows";
    assert!(gui.shows(summary), "{:?}", gui.painted_text());
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Text exploration").is_none(), "the dialog closes");
    let saved = reports(&gui);
    let app_model::SavedReport::TextExploration { request } = &saved[0] else { panic!("not text: {saved:?}") };
    assert_eq!(request.dimensions, 2);
    gui.results();
    assert!(gui.shows("Text Exploration: Review text"), "{:?}", gui.painted_text());
    let analysis = app_model::TextExploration::run(
        &gui.app().documents[gui.app().active].table,
        &gui.app().documents[gui.app().active].rows,
        request,
    )
    .unwrap();
    let top = analysis.matrix().labels[0].clone();
    let documents = analysis.matrix().document_frequency[0];
    // Collapse the tables above the cloud so it is in view.
    for title in ["Summary", "Term Frequencies (100 most frequent of", "Word Frequencies (20 most frequent terms)"] {
        let found = gui.harness.query_all_by_label_contains(title).next().map(|h| h.simulate_click()).is_some();
        if found {
            gui.run();
        }
    }
    gui.shot("report-text-exploration");
    // The cloud's largest word is the most frequent term.
    let (_, rect, clip) = gui
        .painted_text_rects()
        .into_iter()
        .filter(|(t, _, _)| *t == top)
        .max_by(|a, b| a.1.height().total_cmp(&b.1.height()))
        .expect("the top word is drawn");
    assert!(clip.contains(rect.center()), "the cloud is in view");
    click_at(&mut gui, rect.center());
    let rows = &gui.app().documents[gui.app().active].rows;
    assert_eq!(rows.selected_rows().len(), documents, "clicking {top:?} selects the documents containing it");
    edit_and_update(&mut gui, summary);
}

#[test]
fn topic_model_dialog_runs_and_edits() {
    let mut gui = reviews();
    gui.menu(&["Multivariate", "Topic model…"]);
    gui.shot("dialog-topic-model-first-open");
    let summary = "5 topics · 300 rows";
    assert!(gui.shows(summary), "{:?}", gui.painted_text());
    gui.click("Run");
    let saved = reports(&gui);
    let app_model::SavedReport::TopicModel { request } = &saved[0] else { panic!("not topics: {saved:?}") };
    assert_eq!(request.topics, 5);
    gui.results();
    assert!(gui.shows("Topic Model (5 topics): Review text"), "{:?}", gui.painted_text());
    assert!(gui.shows("Top Terms by Topic"));
    gui.shot("report-topic-model");
    edit_and_update(&mut gui, summary);
}

#[test]
fn sentiment_dialog_checks_lexicon_entries() {
    let mut gui = reviews();
    gui.menu(&["Multivariate", "Sentiment scores…"]);
    gui.shot("dialog-sentiment-first-open");
    let summary = "Sentiment of each document · 300 rows";
    assert!(gui.shows(summary), "{:?}", gui.painted_text());
    gui.click("More options");
    gui.window("Sentiment scores").get_by_role(Role::MultilineTextInput).focus();
    gui.run();
    gui.window("Sentiment scores").get_by_role(Role::MultilineTextInput).type_text("crema = 9");
    gui.run();
    assert!(gui.shows("⚠ valences run from −4 to 4 (got 9)"), "{:?}", gui.painted_text());
    assert!(gui.widget("Run").is_disabled());
    gui.click("Reset");
    assert!(gui.shows(summary));
    gui.click("Run");
    let saved = reports(&gui);
    assert!(matches!(&saved[0], app_model::SavedReport::Sentiment { .. }), "{saved:?}");
    gui.results();
    assert!(gui.shows("Sentiment Scores: Review text"), "{:?}", gui.painted_text());
    assert!(gui.shows("Most Negative Documents"));
    gui.shot("report-sentiment");
    edit_and_update(&mut gui, summary);
}
