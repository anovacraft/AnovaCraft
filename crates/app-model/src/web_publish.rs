//! Publishing a project's dashboards as a static web app: the folder that
//! "Publish dashboard as web app…" writes, which any web server can serve.
//!
//! The folder holds
//!
//! - `index.html` ([`index_html`]): a full-window canvas, a status line and
//!   the script that starts the viewer;
//! - [`PROJECT_FILE`]: the project ([`published_project`]), which the viewer
//!   fetches, reads and recomputes in the browser;
//! - [`VIEWER_SCRIPT`] and [`VIEWER_WASM`]: the viewer itself, the app-gui
//!   crate built for `wasm32-unknown-unknown` and run through
//!   `wasm-bindgen --target web` (`scripts/build-web-viewer.sh`);
//! - [`NOTICES_FILE`]: the copyright notices and licence texts of the
//!   open-source fonts, data and libraries the viewer is built with, which
//!   must travel with it (written by the desktop app, which embeds them).
//!
//! Browsers do not let a page fetch files from a `file://` address, so the
//! folder must be opened through a web server (any static host, or
//! `python3 -m http.server` run in the folder).

use crate::dashboard::DashboardId;
use crate::html_export::escape;
use crate::project::ProjectFile;

/// The project file the viewer fetches when the page names none.
pub const PROJECT_FILE: &str = "project.acraft";
/// The viewer's JavaScript glue, written by `wasm-bindgen`.
pub const VIEWER_SCRIPT: &str = "anovacraft_viewer.js";
/// The viewer's WebAssembly module, written by `wasm-bindgen`.
pub const VIEWER_WASM: &str = "anovacraft_viewer_bg.wasm";
/// The third-party notices written beside the viewer (the page links to it).
pub const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES.txt";
/// The viewer files a published folder needs besides the page and project.
pub const VIEWER_FILES: [&str; 2] = [VIEWER_SCRIPT, VIEWER_WASM];

/// The page of a published dashboard titled `title` (the browser tab's
/// title, and the name the viewer shows in its top bar).
#[must_use]
pub fn index_html(title: &str) -> String {
    let title = escape(title);
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="AnovaCraft">
<title>{title}</title>
<!-- Built with open-source software: notices and licences in {NOTICES_FILE} -->
<link rel="license" href="{NOTICES_FILE}">
<style>
  :root {{ color-scheme: light dark; }}
  html, body {{ margin: 0; padding: 0; height: 100%; overflow: hidden; background: #f7f7f5; }}
  @media (prefers-color-scheme: dark) {{ html, body {{ background: #1b1c1f; }} }}
  #anovacraft_canvas {{ position: absolute; inset: 0; width: 100%; height: 100%; display: block; }}
  #anovacraft_status {{
    position: absolute; top: 40%; left: 0; right: 0; text-align: center;
    font: 15px system-ui, sans-serif; color: #666; padding: 0 16px;
  }}
  #anovacraft_status.error {{ color: #b3261e; }}
</style>
</head>
<body>
<canvas id="anovacraft_canvas"></canvas>
<p id="anovacraft_status">Loading the dashboard…</p>
<noscript><p>This dashboard needs JavaScript and WebAssembly.</p></noscript>
<script type="module">
  import init from "./{VIEWER_SCRIPT}";
  const status = document.getElementById("anovacraft_status");
  if (location.protocol === "file:") {{
    status.className = "error";
    status.textContent = "Browsers do not load a dashboard from a file on disk: open this folder through a web " +
      "server (for example, run python3 -m http.server in it and open http://localhost:8000).";
  }} else {{
    init().catch((e) => {{
      status.className = "error";
      status.textContent = "Could not load the viewer. {VIEWER_SCRIPT} and {VIEWER_WASM} must be in the " +
        "same folder as this page (publish again from AnovaCraft to write them): " + e;
    }});
  }}
</script>
</body>
</html>
"#
    )
}

/// `file` as it is published with dashboard `first` shown first: the
/// dashboards reordered so it leads (the viewer opens on the first), the
/// window layout and scripts left out (the viewer has one window and runs
/// no scripts), and row selections cleared. The tables, row exclusions,
/// reports, dashboards and notes are kept.
#[must_use]
pub fn published_project(mut file: ProjectFile, first: &DashboardId) -> ProjectFile {
    if let Some(i) = file.dashboards.iter().position(|d| &d.id == first) {
        let lead = file.dashboards.remove(i);
        file.dashboards.insert(0, lead);
    }
    file.layout = Default::default();
    file.scripts.clear();
    for t in &mut file.tables {
        t.rows.clear_selection();
    }
    file
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{load_project_file, save_project_file};

    #[test]
    fn page_loads_the_viewer_script_and_escapes_the_title() {
        let html = index_html("Yield & <growth>");
        assert!(html.contains("<title>Yield &amp; &lt;growth&gt;</title>"));
        assert!(html.contains(r#"import init from "./anovacraft_viewer.js";"#));
        assert!(html.contains(r#"<canvas id="anovacraft_canvas">"#));
        assert!(html.contains(r#"id="anovacraft_status""#));
        // A file:// page explains that it needs a server instead of failing to fetch.
        assert!(html.contains("location.protocol === \"file:\""));
    }

    #[test]
    fn published_project_leads_with_the_chosen_dashboard() {
        let mut file = crate::sample::greenhouse_project();
        let mut second = file.dashboards[0].clone();
        second.id = DashboardId("second".to_owned());
        second.name = "Second".to_owned();
        file.dashboards.push(second);
        file.tables[0].rows.select(vec![0, 1], stat_data::SelectionMode::Replace);
        let published = published_project(file.clone(), &DashboardId("second".to_owned()));
        assert_eq!(published.dashboards.len(), 2);
        assert_eq!(published.dashboards[0].name, "Second");
        assert_eq!(published.dashboards[1].id, file.dashboards[0].id);
        assert!(published.scripts.is_empty());
        assert_eq!(published.tables.len(), file.tables.len());
        assert!(published.tables.iter().all(|t| t.rows.selected_rows().is_empty()));
        // It reads back as the same project.
        let back = load_project_file(&save_project_file(&published)).unwrap();
        assert_eq!(back.dashboards[0].name, "Second");
        assert_eq!(back.tables.len(), file.tables.len());
        // An unknown dashboard leaves the order alone.
        let same = published_project(file.clone(), &DashboardId("none".to_owned()));
        assert_eq!(same.dashboards[0].id, file.dashboards[0].id);
    }
}
