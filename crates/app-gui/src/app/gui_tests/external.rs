//! File ▸ Import from database / the web, Data ▸ Refresh from source, and
//! Data ▸ Impute missing values.

use std::io::{BufRead, BufReader, Write};

use super::*;

/// Run frames until `done` holds (the import dialogs read on a background
/// thread); panics after about ten seconds.
fn wait_for(gui: &mut Gui, done: impl Fn(&Gui) -> bool) {
    for _ in 0..500 {
        gui.run();
        if done(gui) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("timed out; on screen: {:?}", gui.painted_text());
}

fn database(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("plant.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE runs (id INTEGER, line TEXT, yield REAL);
         INSERT INTO runs VALUES (1, 'A', 97.5), (2, 'B', NULL), (3, 'A', 91.0);
         CREATE TABLE other (x INTEGER);",
    )
    .unwrap();
    path
}

/// Import a table from an SQLite file through the table browser, then
/// refresh it after the file changes, and undo the refresh.
#[test]
fn import_from_sqlite_and_refresh() {
    const TITLE: &str = "Import from database";
    let dir = temp_dir("sqlite-import");
    let path = database(&dir);
    let mut gui = Gui::start();
    gui.menu(&["File", "Import from database…"]);
    gui.shot("dialog-import-database-first-open");
    assert!(gui.shows("⚠ Choose the database file"));
    gui.widget("File:").type_text(path.display().to_string());
    gui.run();
    assert!(gui.shows("⚠ Choose a table or write a query"));
    gui.click("List tables");
    wait_for(&mut gui, |g| g.shows("2 tables found"));
    // Let the dialog settle at its new height before clicking in the list.
    gui.run();
    gui.click("runs");
    let summary = format!("Read SQLite {} into the table “runs”", path.display());
    assert!(gui.painted_text().iter().any(|t| t.contains(&summary)), "{:?}", gui.painted_text());
    gui.shot("dialog-import-database-ready");
    gui.click("Import");
    // The import replaces the untouched Untitled table.
    wait_for(&mut gui, |g| g.app().documents.iter().any(|d| d.external.is_some()));
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.table.name(), "runs");
    assert_eq!(doc.table.n_rows(), 3);
    assert!(doc.external.is_some());
    assert!(gui.app().project_modified(doc.project), "a new import has not been saved");
    let text = app_model::project::save_project(&doc.project(&[]));
    assert!(text.contains("external_source"));

    // The file gains a row; refreshing reads it (no password needed, so at once).
    rusqlite::Connection::open(&path).unwrap().execute("INSERT INTO runs VALUES (4, 'C', 88.0)", []).unwrap();
    gui.menu(&["Data", "Refresh from source"]);
    wait_for(&mut gui, |g| g.app().documents[g.app().active].table.n_rows() == 4);
    // The message is drawn on the frame after the table opens.
    wait_for(&mut gui, |g| {
        g.painted_text().iter().any(|t| t.contains("Refreshed “runs”: 4 rows. Undo puts back the previous rows."))
    });
    gui.grid(vec![GridRequest::Undo]);
    assert_eq!(gui.app().documents[gui.app().active].table.n_rows(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A bad query leaves the dialog open with the database's message.
#[test]
fn import_errors_stay_in_the_dialog() {
    let dir = temp_dir("sqlite-error");
    let path = database(&dir);
    let mut gui = Gui::start();
    gui.menu(&["File", "Import from database…"]);
    gui.widget("File:").type_text(path.display().to_string());
    gui.harness.get_by_role(Role::MultilineTextInput).type_text("SELECT * FROM nowhere");
    gui.run();
    gui.click("Import");
    wait_for(&mut gui, |g| g.painted_text().iter().any(|t| t.starts_with("Could not read:")));
    assert!(gui.painted_text().iter().any(|t| t.contains("no such table: nowhere")), "{:?}", gui.painted_text());
    assert!(gui.app().documents.is_empty(), "nothing opened");
    gui.click("Cancel");
    let _ = std::fs::remove_dir_all(&dir);
}

/// JSON from a web address becomes a table with dotted column names.
#[test]
fn import_from_the_web() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Some(Ok(mut stream)) = listener.incoming().next() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let body =
                r#"{"results": [{"site": {"name": "North"}, "pm25": 12.5}, {"site": {"name": "South"}, "pm25": 9}]}"#;
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    let mut gui = Gui::start();
    gui.menu(&["File", "Import from the web…"]);
    gui.shot("dialog-import-web-first-open");
    assert!(gui.shows("⚠ Enter an address starting with https:// or http://"));
    gui.widget("Address (URL):").type_text(format!("http://{addr}/air/stations.json"));
    gui.run();
    assert!(gui.shows(&format!("Fetch http://{addr}/air/stations.json into the table “stations”")));
    gui.click("Import");
    // The import replaces the untouched Untitled table.
    wait_for(&mut gui, |g| g.app().documents.iter().any(|d| d.external.is_some()));
    let doc = &gui.app().documents[gui.app().active];
    let names: Vec<&str> = doc.table.columns().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["site.name", "pm25"]);
    // The message is drawn on the frame after the table opens.
    wait_for(&mut gui, |g| {
        g.painted_text().iter().any(|t| t.contains("Read 2 rows × 2 columns from http://127.0.0.1"))
    });
}

/// A web page's table, chosen by number in the dialog, becomes the table.
#[test]
fn import_a_table_from_a_web_page() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Some(Ok(mut stream)) = listener.incoming().next() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let body = "<!DOCTYPE html><html><body><table><tr><th>Lens</th><th>Diameter</th></tr>\
                        <tr><td>Alpha</td><td>14.2</td></tr></table><table><tr><th>Brand</th></tr>\
                        <tr><td>X</td></tr><tr><td>Y</td></tr><tr><td>Z</td></tr></table></body></html>";
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    let mut gui = Gui::start();
    gui.menu(&["File", "Import from the web…"]);
    gui.widget("Address (URL):").type_text(format!("http://{addr}/specs"));
    gui.run();
    gui.widget("Table on the page:").type_text("1");
    gui.run();
    gui.click("Import");
    // The page's cells come up in the Open Data dialog first.
    wait_for(&mut gui, |g| !g.app().open_data.is_empty());
    gui.run();
    assert!(gui.painted_text().iter().any(|t| t == "Read table 1 of 2 on the page."));
    gui.click("Open");
    // The import replaces the untouched Untitled table.
    wait_for(&mut gui, |g| g.app().documents.iter().any(|d| d.external.is_some()));
    let doc = &gui.app().documents[gui.app().active];
    let names: Vec<&str> = doc.table.columns().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Lens", "Diameter"], "table 1, not the larger table 2");
    // The message is drawn on the frame after the table opens.
    wait_for(&mut gui, |g| g.painted_text().iter().any(|t| t.contains("Read table 1 of 2 on the page.")));
}

/// Refresh is only offered for tables read from a database or the web.
#[test]
fn refresh_needs_an_external_table() {
    let mut gui = Gui::blank();
    gui.menu(&["Data", "Refresh from source"]);
    assert!(gui.app().external_dialog.is_none());
}

/// Data ▸ Impute missing values runs the low-rank method on the air-quality
/// sample (regularized unless unticked), and the report reopens in its dialog.
#[test]
fn imputation_dialog_runs_and_reopens() {
    const TITLE: &str = "Impute missing values";
    let mut gui = Gui::with_sample("Air quality");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Data", "Impute missing values…"]);
    gui.shot("dialog-imputation-first-open");
    assert!(gui.shows("⚠ Choose at least one column"));
    for c in ["Temperature (°C)", "Humidity (%)", "Ozone (µg/m³)"] {
        gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(c)).simulate_click();
        gui.run();
    }
    gui.click("Low-rank SVD");
    gui.click("Fixed:");
    let summary = "Low-rank SVD of 3 columns";
    assert!(gui.painted_text().iter().any(|t| t.starts_with(summary)), "{:?}", gui.painted_text());
    gui.shot("dialog-imputation-ready");
    gui.click("More options");
    assert!(gui.shows("Regularize the fit (recommended)"));
    gui.click("Regularize the fit (recommended)");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none());
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::Imputation { request } = reports[0].saved() else { panic!("not an imputation") };
    assert_eq!(
        (request.method, request.rank, request.columns.len()),
        (app_model::ImputationMethod::LowRank, Some(2), 3)
    );
    assert!(!request.regularize, "unticked in the dialog");
    let id = reports[0].id();
    gui.app_mut().edit_report(id);
    gui.run();
    assert!(gui.shows("Update"));
    gui.click("Multivariate normal (EM)");
    gui.click("Update");
    let app_model::SavedReport::Imputation { request } = gui.app().documents[active].reports[0].saved() else {
        panic!("not an imputation")
    };
    assert_eq!(request.method, app_model::ImputationMethod::NormalEm);
    assert!(
        gui.app().documents[active].reports[0]
            .title()
            .starts_with("Missing-Value Imputation (Multivariate normal (EM))")
    );
}

/// After a project is reopened, a key that was in the web address's query
/// (saved blank) is asked for when refreshing, and sent to the server.
#[test]
fn refresh_asks_again_for_a_key_in_the_address() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Some(Ok(mut stream)) = listener.incoming().next() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let ok = first.contains("/obs.csv?site=3&api_key=k3y ");
            let body = if ok { "t,ozone\n1,40\n2,41\n" } else { "bad key" };
            let status = if ok { "200 OK" } else { "403 Forbidden" };
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/csv\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    let full = app_model::external::ExternalSource::Web(app_model::external::WebRequest::new(format!(
        "http://{addr}/obs.csv?site=3&api_key=k3y"
    )));
    // What a project file keeps: the key's name, not its value.
    let saved = app_model::external::ExternalSource::from_json(&full.to_json()).unwrap();
    assert!(!full.to_json().to_compact().contains("k3y"));
    let mut gui = Gui::blank();
    let active = gui.app().active;
    let doc = &mut gui.app_mut().documents[active];
    doc.reports.clear();
    doc.external = Some(saved);
    doc.secrets = None;
    gui.menu(&["Data", "Refresh from source"]);
    gui.run();
    assert!(gui.shows("api_key (in the address):"), "{:?}", gui.painted_text());
    assert!(!gui.painted_text().iter().any(|t| t.contains("k3y")));
    gui.widget("api_key (in the address):").type_text("k3y");
    gui.run();
    gui.click("Refresh");
    wait_for(&mut gui, |g| g.app().documents[g.app().active].table.n_rows() == 2);
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.secrets.as_ref().unwrap().query, [("api_key".to_owned(), "k3y".to_owned())]);
    assert!(!app_model::project::save_project(&doc.project(&[])).contains("k3y"));
}
