//! The web build's entry point: the read-only dashboard viewer.
//!
//! The page (`index.html`, written by "Publish dashboard as web app…")
//! holds a canvas with id `anovacraft_canvas` and an element with id
//! `anovacraft_status`. The viewer fetches the project file next to the
//! page (`project.acraft`, or the one named by a `?project=` query
//! parameter), recomputes its reports in the browser and draws its
//! dashboards on the canvas. The page's title names the project.

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

/// The project file fetched when the page names none.
const DEFAULT_PROJECT: &str = "project.acraft";

/// Start the viewer once the page has loaded.
pub fn start() {
    // A panic shows on the page instead of leaving a blank canvas.
    std::panic::set_hook(Box::new(|info| {
        show_status(&format!("The viewer stopped: {info}"), true);
    }));
    wasm_bindgen_futures::spawn_local(async {
        if let Err(e) = run().await {
            show_status(&e, true);
        }
    });
}

async fn run() -> Result<(), String> {
    let window = web_sys::window().ok_or("no browser window")?;
    let document = window.document().ok_or("no document")?;
    let search = window.location().search().unwrap_or_default();
    let name = web_sys::UrlSearchParams::new_with_str(&search)
        .ok()
        .and_then(|p| p.get("project"))
        .unwrap_or_else(|| DEFAULT_PROJECT.to_owned());
    show_status(&format!("Loading {name}…"), false);
    let text = fetch_text(&window, &name).await?;
    let file = app_model::project::load_project_file(&text).map_err(|e| format!("Could not read {name}: {e}"))?;
    let title = match document.title() {
        t if t.trim().is_empty() => name.trim_end_matches(".acraft").to_owned(),
        t => t,
    };
    let canvas = document
        .get_element_by_id("anovacraft_canvas")
        .ok_or("the page has no canvas with id anovacraft_canvas")?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| "anovacraft_canvas is not a canvas")?;
    show_status("Computing the dashboard…", false);
    let options = eframe::WebOptions::default();
    eframe::WebRunner::new()
        .start(canvas, options, Box::new(move |cc| Ok(Box::new(crate::app::StatsApp::new_viewer(cc, file, title)))))
        .await
        .map_err(|e| format!("Could not start the viewer: {}", describe(&e)))?;
    hide_status();
    Ok(())
}

/// The text of `url`, relative to the page.
async fn fetch_text(window: &web_sys::Window, url: &str) -> Result<String, String> {
    let response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| format!("Could not fetch {url}: {}", describe(&e)))?;
    let response: web_sys::Response = response.dyn_into().map_err(|_| "fetch returned no response")?;
    if !response.ok() {
        return Err(format!("Could not fetch {url}: HTTP {} {}", response.status(), response.status_text()));
    }
    let text = response.text().map_err(|e| describe(&e))?;
    let text = JsFuture::from(text).await.map_err(|e| format!("Could not read {url}: {}", describe(&e)))?;
    text.as_string().ok_or_else(|| format!("{url} is not text"))
}

fn describe(value: &JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}

/// Show `text` in the page's status line (as an error when `error`).
fn show_status(text: &str, error: bool) {
    let Some(element) =
        web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("anovacraft_status"))
    else {
        return;
    };
    element.set_text_content(Some(text));
    let _ = element.set_attribute("class", if error { "error" } else { "" });
    let _ = element.set_attribute("style", "");
}

fn hide_status() {
    if let Some(element) =
        web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("anovacraft_status"))
    {
        let _ = element.set_attribute("style", "display: none");
    }
}
