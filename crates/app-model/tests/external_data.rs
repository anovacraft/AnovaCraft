//! Reading tables from databases and the web.
//!
//! SQLite and HTTP are tested against a database file and a small HTTP
//! server made by the test. PostgreSQL and MySQL need a running server:
//! set `ANOVACRAFT_TEST_POSTGRES` / `ANOVACRAFT_TEST_MYSQL` to
//! `host:port:database:user:password` to run those tests (they are skipped
//! otherwise).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use app_model::external::{
    DatabaseConnection, DatabaseKind, ExternalSource, Secrets, TlsMode, WebFormat, WebHeader, WebRequest, list_tables,
    run_query,
};
use stat_data::ColumnKind;

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_tables_and_queries() {
    let dir = std::env::temp_dir().join(format!("anovacraft-sqlite-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plant.db");
    let _ = std::fs::remove_file(&path);
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE runs (id INTEGER, line TEXT, yield REAL, made TEXT, note TEXT);
             INSERT INTO runs VALUES (1, 'A', 97.25, '2026-03-01', NULL);
             INSERT INTO runs VALUES (2, 'B', NULL, '2026-03-02', 'x');
             INSERT INTO runs VALUES (3, 'A', 0.1, '2026-03-03', NULL);
             CREATE VIEW good AS SELECT * FROM runs WHERE yield > 50;
             CREATE TABLE \"odd name\" (v INTEGER);",
        )
        .unwrap();
    }
    let c = DatabaseConnection::sqlite(path.to_string_lossy());
    assert_eq!(list_tables(&c, "").unwrap(), vec!["good", "odd name", "runs"]);
    let (t, _) = run_query(&c, "", &DatabaseKind::Sqlite.select_all("runs"), "runs").unwrap();
    assert_eq!(t.n_rows(), 3);
    let kinds: Vec<ColumnKind> = t.columns().iter().map(|c| c.kind()).collect();
    assert_eq!(kinds[0], ColumnKind::Numeric);
    assert_eq!(kinds[1], ColumnKind::Text);
    assert_eq!(kinds[2], ColumnKind::Numeric);
    assert!(matches!(kinds[3], ColumnKind::DateTime(_)), "{kinds:?}");
    let y = t.column(2).unwrap().as_numeric().unwrap();
    assert_eq!(y[0], 97.25);
    assert!(y[1].is_nan(), "NULL is missing");
    assert_eq!(y[2], 0.1, "reals keep every digit");
    assert_eq!(t.column(4).unwrap().cell_text(0), "", "NULL text is missing");
    let (v, _) = run_query(&c, "", &DatabaseKind::Sqlite.select_all("odd name"), "odd").unwrap();
    assert_eq!(v.n_rows(), 0);
    assert_eq!(v.column(0).unwrap().name, "v");
    let source = ExternalSource::Database { connection: c.clone(), query: "SELECT line, yield FROM good".into() };
    let (g, _) = source.read(&Secrets::default(), "good").unwrap();
    assert_eq!(g.n_rows(), 1);
    assert!(run_query(&c, "", "SELECT nonsense FROM nowhere", "x").unwrap_err().contains("nowhere"));
    // Read-only: a statement that writes fails.
    assert!(run_query(&c, "", "DELETE FROM runs", "x").is_err());
    assert!(list_tables(&DatabaseConnection::sqlite(dir.join("none.db").to_string_lossy()), "").is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A one-shot HTTP server answering each request with `respond(path,
/// request headers)`; returns its base URL.
fn serve(respond: fn(&str, &[String]) -> (u16, &'static str, String), requests: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(requests) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut headers = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                headers.push(line.trim().to_owned());
            }
            let path = first.split_whitespace().nth(1).unwrap_or("/").to_owned();
            let (status, content_type, body) = respond(&path, &headers);
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    format!("http://{addr}")
}

#[cfg(feature = "web")]
#[test]
fn web_json_and_csv() {
    fn respond(path: &str, headers: &[String]) -> (u16, &'static str, String) {
        let authorized = headers.iter().any(|h| h.eq_ignore_ascii_case("authorization: Bearer s3cret"));
        match path {
            "/stations.json" if authorized => (
                200,
                "application/json",
                r#"{"count": 2, "results": [{"id": "A1", "location": {"lat": 51.5, "city": "London"}, "pm25": 12.5},
                   {"id": "B7", "location": {"lat": 48.9, "city": "Paris"}, "pm25": null}]}"#
                    .to_owned(),
            ),
            "/stations.json" => (401, "text/plain", "missing token".to_owned()),
            "/daily.csv" => (200, "text/csv", "day,ozone\n2026-05-01,41.2\n2026-05-02,\n".to_owned()),
            _ => (404, "text/plain", "not found".to_owned()),
        }
    }
    let base = serve(respond, 4);
    let request = WebRequest {
        url: format!("{base}/stations.json"),
        headers: vec![WebHeader { name: "Authorization".into(), value: String::new(), secret: true }],
        ..WebRequest::default()
    };
    let source = ExternalSource::Web(request);
    let secrets = Secrets { headers: vec![("Authorization".into(), "Bearer s3cret".into())], ..Secrets::default() };
    let (t, notes) = source.read(&secrets, "stations").unwrap();
    let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "location.lat", "location.city", "pm25"]);
    assert!(t.column(3).unwrap().as_numeric().unwrap()[1].is_nan());
    assert_eq!(notes.len(), 1, "says where the rows came from");
    // Without the secret the server refuses, and the error says so.
    let err = source.read(&Secrets::default(), "stations").unwrap_err();
    assert!(err.contains("401"), "{err}");
    let csv =
        ExternalSource::Web(WebRequest { format: WebFormat::Auto, ..WebRequest::new(format!("{base}/daily.csv")) });
    let (d, _) = csv.read(&Secrets::default(), "daily").unwrap();
    assert!(matches!(d.column(0).unwrap().kind(), ColumnKind::DateTime(_)));
    assert!(d.column(1).unwrap().as_numeric().unwrap()[1].is_nan());
    let missing = ExternalSource::Web(WebRequest::new(format!("{base}/nothing")));
    assert!(missing.read(&Secrets::default(), "x").unwrap_err().contains("404"));
    assert!(ExternalSource::Web(WebRequest::new("ftp://x/y")).read(&Secrets::default(), "x").is_err());
}

/// A web page's table is read with the default browser-style `User-Agent`,
/// which gives way to one the request sets, and a server that drops the connection is reported in words.
#[cfg(feature = "web")]
#[test]
fn web_page_tables_and_refused_connections() {
    fn respond(path: &str, headers: &[String]) -> (u16, &'static str, String) {
        let browser = headers.iter().any(|h| {
            let h = h.to_ascii_lowercase();
            h.starts_with("user-agent: mozilla/5.0") && !h.contains("refused")
        });
        let agents = headers.iter().filter(|h| h.to_ascii_lowercase().starts_with("user-agent:")).count();
        assert_eq!(agents, 1, "one User-Agent: {headers:?}");
        match path {
            "/lenses" if browser => (
                200,
                "text/html; charset=utf-8",
                r#"<!DOCTYPE html><html><body><nav><table><tr><td>Home</td></tr></table></nav>
                   <h2 id="1D">Daily lenses</h2>
                   <table><tr><th>Lens</th><th>Base curve</th><th>Diameter</th></tr>
                   <tr><td>Alpha</td><td>8.6</td><td>14.2</td></tr>
                   <tr><td>Beta</td><td>8.5</td><td>14.0</td></tr></table></body></html>"#
                    .to_owned(),
            ),
            "/lenses" => (403, "text/html", "<html><body><h1>Forbidden</h1></body></html>".to_owned()),
            _ => (404, "text/plain", "not found".to_owned()),
        }
    }
    let base = serve(respond, 2);
    // The default User-Agent is a browser's, so the page is served.
    let page = ExternalSource::Web(WebRequest::new(format!("{base}/lenses#1D")));
    let (t, notes) = page.read(&Secrets::default(), "lenses").unwrap();
    let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Lens", "Base curve", "Diameter"]);
    assert_eq!(t.column(2).unwrap().as_numeric().unwrap(), &[14.2, 14.0]);
    assert_eq!(notes[0], "Read table 2 of 2 on the page.");
    // An HTML error page is not quoted back as raw markup.
    // A User-Agent given in the request replaces the default (here one the
    // test server refuses).
    let refused = vec![WebHeader { name: "User-Agent".into(), value: "Mozilla/5.0 refused".into(), secret: false }];
    let err = ExternalSource::Web(WebRequest { headers: refused, ..WebRequest::new(format!("{base}/lenses")) })
        .read(&Secrets::default(), "x")
        .unwrap_err();
    assert_eq!(err, "the server answered 403 Forbidden");

    // A server that resets the connection as soon as it is made.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        // Twice: macOS and Windows builds try again with the system's TLS.
        for stream in listener.incoming().take(2) {
            let Ok(stream) = stream else { continue };
            let socket = socket2::SockRef::from(&stream);
            socket.set_linger(Some(std::time::Duration::ZERO)).unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut &stream, &mut buf);
            drop(stream);
        }
    });
    let err =
        ExternalSource::Web(WebRequest::new(format!("http://{addr}/x"))).read(&Secrets::default(), "x").unwrap_err();
    assert!(err.starts_with(&format!("{addr} closed the connection without answering")), "{err}");
}

/// A key in the address's query is not saved, and is filled back in from the
/// session's secrets when the reopened source is read.
#[cfg(feature = "web")]
#[test]
fn web_query_keys_are_asked_for_again() {
    fn respond(path: &str, _: &[String]) -> (u16, &'static str, String) {
        match path {
            "/obs.csv?site=3&api_key=k3y" => (200, "text/csv", "t,ozone\n1,40\n".to_owned()),
            _ => (403, "text/plain", "bad key".to_owned()),
        }
    }
    let base = serve(respond, 2);
    let source = ExternalSource::Web(WebRequest::new(format!("{base}/obs.csv?site=3&api_key=k3y")));
    let secrets = Secrets::from_source(&source);
    let reopened = ExternalSource::from_json(&source.to_json()).unwrap();
    assert!(!reopened.to_json().to_compact().contains("k3y"));
    assert_eq!(reopened.secrets_needed(), ["api_key"]);
    assert!(reopened.read(&Secrets::default(), "obs").unwrap_err().contains("403"));
    let (t, _) = reopened.read(&secrets, "obs").unwrap();
    assert_eq!(t.n_rows(), 1);
}

/// `host:port:database:user:password` from an environment variable.
fn server(var: &str, kind: DatabaseKind) -> Option<(DatabaseConnection, String)> {
    let v = std::env::var(var).ok()?;
    let parts: Vec<&str> = v.splitn(5, ':').collect();
    let [host, port, database, user, password] = parts.as_slice() else {
        panic!("{var} must be host:port:database:user:password")
    };
    Some((
        DatabaseConnection {
            host: (*host).to_owned(),
            port: port.parse().unwrap(),
            database: (*database).to_owned(),
            user: (*user).to_owned(),
            tls: TlsMode::Prefer,
            ..DatabaseConnection::new(kind)
        },
        (*password).to_owned(),
    ))
}

/// The same checks on a server database: a table with integers, decimals,
/// text, a date, a timestamp, a boolean and NULLs.
fn server_round_trip(c: &DatabaseConnection, password: &str, setup: &[&str]) {
    for sql in setup {
        // Setup statements return no columns.
        assert_eq!(run_query(c, password, sql, "x").unwrap_err(), "the query returned no columns");
    }
    let tables = list_tables(c, password).unwrap();
    assert!(tables.iter().any(|t| t.ends_with("anovacraft_test")), "{tables:?}");
    let (t, _) = run_query(c, password, &c.kind.select_all("anovacraft_test"), "t").unwrap();
    assert_eq!(t.n_rows(), 3);
    let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "amount", "label", "made", "stamp"]);
    let amount = t.column(1).unwrap().as_numeric().unwrap();
    assert_eq!(amount[0], 12.5);
    assert!(amount[2].is_nan());
    assert!(matches!(t.column(3).unwrap().kind(), ColumnKind::DateTime(_)), "{:?}", t.column(3).unwrap().kind());
    assert!(matches!(t.column(4).unwrap().kind(), ColumnKind::DateTime(_)), "{:?}", t.column(4).unwrap().kind());
    assert!(run_query(c, "wrong password", "SELECT 1", "x").is_err() || password.is_empty());
}

#[cfg(feature = "postgres")]
#[test]
fn postgres_server() {
    let Some((c, password)) = server("ANOVACRAFT_TEST_POSTGRES", DatabaseKind::Postgres) else { return };
    server_round_trip(
        &c,
        &password,
        &[
            "DROP TABLE IF EXISTS anovacraft_test",
            "CREATE TABLE anovacraft_test (id integer, amount numeric(8,2), label text, made date, stamp timestamp)",
            "INSERT INTO anovacraft_test VALUES (1, 12.5, 'a', '2026-01-02', '2026-01-02 03:04:05'), \
             (2, 7, 'b', '2026-02-03', '2026-02-03 10:00:00'), (3, NULL, NULL, NULL, NULL)",
        ],
    );
}

#[cfg(feature = "mysql")]
#[test]
fn mysql_server() {
    let Some((c, password)) = server("ANOVACRAFT_TEST_MYSQL", DatabaseKind::MySql) else { return };
    server_round_trip(
        &c,
        &password,
        &[
            "DROP TABLE IF EXISTS anovacraft_test",
            "CREATE TABLE anovacraft_test (id int, amount decimal(8,2), label varchar(10), made date, stamp datetime)",
            "INSERT INTO anovacraft_test VALUES (1, 12.5, 'a', '2026-01-02', '2026-01-02 03:04:05'), \
             (2, 7, 'b', '2026-02-03', '2026-02-03 10:00:00'), (3, NULL, NULL, NULL, NULL)",
        ],
    );
}

/// Set `ANOVACRAFT_TEST_ODBC` to `connection string|password` (for example
/// `Driver={PostgreSQL Unicode};Server=127.0.0.1;Port=5432;Database=x;Uid=u|pw`)
/// and build with `--features odbc` to run this.
#[cfg(feature = "odbc")]
#[test]
fn odbc_data_source() {
    let Ok(v) = std::env::var("ANOVACRAFT_TEST_ODBC") else { return };
    let (conn, password) = v.split_once('|').unwrap_or((v.as_str(), ""));
    let c = DatabaseConnection { path: conn.to_owned(), ..DatabaseConnection::new(DatabaseKind::Odbc) };
    let _ = run_query(&c, password, "DROP TABLE IF EXISTS anovacraft_odbc", "x");
    let _ = run_query(&c, password, "CREATE TABLE anovacraft_odbc (id integer, amount numeric(8,2), made date)", "x");
    let _ = run_query(&c, password, "INSERT INTO anovacraft_odbc VALUES (1, 12.5, '2026-01-02'), (2, NULL, NULL)", "x");
    let tables = list_tables(&c, password).unwrap();
    assert!(tables.iter().any(|t| t.ends_with("anovacraft_odbc")), "{tables:?}");
    let (t, _) = run_query(&c, password, "SELECT * FROM anovacraft_odbc ORDER BY id", "t").unwrap();
    assert_eq!(t.n_rows(), 2);
    let amount = t.column(1).unwrap().as_numeric().unwrap();
    assert_eq!(amount[0], 12.5);
    assert!(amount[1].is_nan());
    assert!(matches!(t.column(2).unwrap().kind(), ColumnKind::DateTime(_)));
}
