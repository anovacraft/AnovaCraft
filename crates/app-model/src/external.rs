//! Tables read from outside the application: SQL databases (SQLite built
//! in; PostgreSQL and MySQL through pure-Rust drivers; ODBC data sources
//! with the optional `odbc` feature) and data on the web (JSON or delimited
//! text fetched by HTTP GET).
//!
//! A table read this way remembers its [`ExternalSource`] — the connection
//! settings and query, or the URL and headers — so it can be read again
//! ([`refresh_edit`]) and so project files can record where it came from.
//! **Credentials are never stored**: passwords and the values of headers
//! marked secret live only in a [`Secrets`] value for the session, and are
//! asked for again after a project is reopened. The ODBC connection string
//! is stored with any `PWD=` or `Password=` setting removed, and a web
//! address with any password in it (`https://user:pass@host`) removed.
//!
//! Values are read as text and typed exactly as delimited text files are
//! ([`stat_data::table_from_records_with_notes`]): a column of numbers is
//! numeric, one of dates or times is a date-time column, anything else is
//! text; SQL `NULL` and JSON `null` are missing values.
//!
//! Web data: a JSON document is turned into rows from an array of objects —
//! the document itself, the array at a dotted path such as `data.items`, or
//! (with no path) the only member of the top-level object that is such an
//! array. Nested objects become dotted column names (`address.city`); arrays
//! inside a row are kept as their JSON text. A web page (HTML) is read from
//! one of its `<table>` elements ([`html`]): the one named in the request,
//! else the one the address's `#section` points to, else the largest. Other
//! responses are read as delimited text with the delimiter detected.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager, flatten_object, parse_json, table_from_json_rows};

use crate::editing::Edit;

pub mod html;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Seconds allowed for connecting to a server or fetching a URL.
pub const TIMEOUT_SECONDS: u64 = 30;

/// Which kind of database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatabaseKind {
    /// An SQLite database file.
    Sqlite,
    /// A PostgreSQL server.
    Postgres,
    /// A MySQL or MariaDB server.
    MySql,
    /// An ODBC data source (needs the `odbc` feature and an ODBC driver manager).
    Odbc,
}

impl DatabaseKind {
    /// Every kind, in dialog order.
    pub const ALL: [Self; 4] = [Self::Sqlite, Self::Postgres, Self::MySql, Self::Odbc];

    /// Label for dialogs.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "SQLite file",
            Self::Postgres => "PostgreSQL",
            Self::MySql => "MySQL / MariaDB",
            Self::Odbc => "ODBC data source",
        }
    }

    /// The project-file name.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgresql",
            Self::MySql => "mysql",
            Self::Odbc => "odbc",
        }
    }

    /// The kind named `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

    /// The server's usual port (0 for kinds without one).
    #[must_use]
    pub fn default_port(self) -> u16 {
        match self {
            Self::Postgres => 5432,
            Self::MySql => 3306,
            Self::Sqlite | Self::Odbc => 0,
        }
    }

    /// Whether this build can connect to this kind of database.
    #[must_use]
    pub fn available(self) -> bool {
        match self {
            Self::Sqlite => cfg!(feature = "sqlite"),
            Self::Postgres => cfg!(feature = "postgres"),
            Self::MySql => cfg!(feature = "mysql"),
            Self::Odbc => cfg!(feature = "odbc"),
        }
    }

    /// Whether it is a network server (host, port, user and password).
    #[must_use]
    pub fn is_server(self) -> bool {
        matches!(self, Self::Postgres | Self::MySql)
    }

    /// `name` quoted as an SQL identifier (`"a b"`, or `` `a b` `` for
    /// MySQL); a dotted `schema.table` has each part quoted.
    #[must_use]
    pub fn quote(self, name: &str) -> String {
        let (open, close) = if self == Self::MySql { ('`', '`') } else { ('"', '"') };
        name.split('.')
            .map(|part| {
                let doubled = part.replace(close, &format!("{close}{close}"));
                format!("{open}{doubled}{close}")
            })
            .collect::<Vec<_>>()
            .join(".")
    }

    /// The query reading every row of `table`.
    #[must_use]
    pub fn select_all(self, table: &str) -> String {
        format!("SELECT * FROM {}", self.quote(table))
    }
}

/// Whether to encrypt the connection to a database server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsMode {
    /// Plain connection.
    Disable,
    /// Encrypt when the server offers it.
    #[default]
    Prefer,
    /// Encrypt, or fail.
    Require,
}

impl TlsMode {
    /// Every mode, in dialog order.
    pub const ALL: [Self; 3] = [Self::Prefer, Self::Require, Self::Disable];

    /// Label for dialogs.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Disable => "Off",
            Self::Prefer => "If offered",
            Self::Require => "Required",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Prefer => "prefer",
            Self::Require => "require",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// How to reach a database. Holds no password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseConnection {
    /// Which kind of database.
    pub kind: DatabaseKind,
    /// SQLite: the file. ODBC: the connection string (`DSN=…;UID=…`) without
    /// a password. Unused for servers.
    pub path: String,
    /// Server host name or address.
    pub host: String,
    /// Server port.
    pub port: u16,
    /// Database name.
    pub database: String,
    /// User name.
    pub user: String,
    /// Encryption of a server connection.
    pub tls: TlsMode,
}

impl DatabaseConnection {
    /// A connection of `kind` with its usual settings (local host, usual port).
    #[must_use]
    pub fn new(kind: DatabaseKind) -> Self {
        Self {
            kind,
            path: String::new(),
            host: if kind.is_server() { "localhost".to_owned() } else { String::new() },
            port: kind.default_port(),
            database: String::new(),
            user: String::new(),
            tls: TlsMode::default(),
        }
    }

    /// An SQLite database file.
    #[must_use]
    pub fn sqlite(path: impl Into<String>) -> Self {
        Self { path: path.into(), ..Self::new(DatabaseKind::Sqlite) }
    }

    /// A short description for menus and reports, e.g.
    /// `PostgreSQL sales on db.example.com:5432 as analyst`.
    #[must_use]
    pub fn describe(&self) -> String {
        match self.kind {
            DatabaseKind::Sqlite => format!("SQLite {}", self.path),
            DatabaseKind::Odbc => format!("ODBC {}", strip_odbc_password(&self.path)),
            _ => {
                let user = if self.user.is_empty() { String::new() } else { format!(" as {}", self.user) };
                format!("{} {} on {}:{}{user}", self.kind.label(), self.database, self.host, self.port)
            }
        }
    }

    /// Whether a password is asked for (servers, and ODBC).
    #[must_use]
    pub fn takes_password(&self) -> bool {
        self.kind != DatabaseKind::Sqlite
    }

    fn to_json(&self) -> JsonValue {
        let s = crate::project::s;
        crate::project::obj([
            ("kind", s(self.kind.key())),
            (
                "path",
                s(&if self.kind == DatabaseKind::Odbc { strip_odbc_password(&self.path) } else { self.path.clone() }),
            ),
            ("host", s(&self.host)),
            ("port", JsonValue::number(f64::from(self.port))),
            ("database", s(&self.database)),
            ("user", s(&self.user)),
            ("tls", s(self.tls.key())),
        ])
    }

    /// Read the project-file form (also the settings map scripts pass):
    /// `kind` (`"sqlite"`, `"postgresql"`, `"mysql"` or `"odbc"`), `path`,
    /// `host`, `port`, `database`, `user` and `tls` (`"disable"`,
    /// `"prefer"` or `"require"`); missing settings take their defaults.
    ///
    /// # Errors
    /// A missing or unknown `kind`.
    pub fn from_json(v: &JsonValue) -> Result<Self> {
        let kind = v
            .get("kind")
            .and_then(JsonValue::as_str)
            .and_then(DatabaseKind::from_key)
            .ok_or_else(|| invalid("external source: unknown database kind"))?;
        let text = |k: &str| v.get(k).and_then(JsonValue::as_str).unwrap_or_default().to_owned();
        let d = Self::new(kind);
        Ok(Self {
            path: text("path"),
            host: text("host"),
            port: v.get("port").and_then(JsonValue::as_usize).and_then(|p| u16::try_from(p).ok()).unwrap_or(d.port),
            database: text("database"),
            user: text("user"),
            tls: v.get("tls").and_then(JsonValue::as_str).and_then(TlsMode::from_key).unwrap_or_default(),
            kind,
        })
    }
}

/// An ODBC connection string without its `PWD=` / `Password=` settings.
/// A value in braces (`PWD={a;b}`, with `}}` standing for `}`) may hold
/// semicolons; it is removed whole.
#[must_use]
pub fn strip_odbc_password(conn: &str) -> String {
    odbc_settings(conn)
        .into_iter()
        .filter(|part| {
            let key = part.split('=').next().unwrap_or("").trim().to_ascii_lowercase();
            key != "pwd" && key != "password"
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// The `key=value` settings of an ODBC connection string, split at the
/// semicolons that are not inside a braced value.
fn odbc_settings(conn: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut in_braces) = (0, false);
    let mut chars = conn.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '{' if !in_braces => in_braces = true,
            // `}}` inside braces is an escaped brace.
            '}' if in_braces => {
                if chars.peek().is_some_and(|&(_, n)| n == '}') {
                    chars.next();
                } else {
                    in_braces = false;
                }
            }
            ';' if !in_braces => {
                parts.push(&conn[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&conn[start..]);
    parts
}

/// `PWD={…}` for an ODBC connection string, with `}` doubled as ODBC's
/// braced values require.
#[cfg_attr(not(feature = "odbc"), allow(dead_code))]
fn odbc_password_setting(password: &str) -> String {
    format!("PWD={{{}}}", password.replace('}', "}}"))
}

/// Whether a header's value is a credential that must not be saved
/// (`Authorization`, `Cookie`, and names mentioning a key, token, secret or
/// password).
#[must_use]
pub fn is_secret_header(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "authorization"
        || n == "proxy-authorization"
        || n == "cookie"
        || ["key", "token", "secret", "password", "auth"].iter().any(|w| n.contains(w))
}

/// How a web response is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WebFormat {
    /// Decide from the content type and the text.
    #[default]
    Auto,
    /// JSON.
    Json,
    /// Delimited text (delimiter detected).
    Delimited,
    /// A table on a web page (HTML).
    Html,
}

impl WebFormat {
    /// Every format, in dialog order.
    pub const ALL: [Self; 4] = [Self::Auto, Self::Json, Self::Delimited, Self::Html];

    /// Label for dialogs.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Detect",
            Self::Json => "JSON",
            Self::Delimited => "CSV or other delimited text",
            Self::Html => "Table on a web page",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Json => "json",
            Self::Delimited => "delimited",
            Self::Html => "html",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// An HTTP header sent with a web request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebHeader {
    /// Header name, e.g. `Accept`.
    pub name: String,
    /// Its value (not saved when [`secret`](Self::secret)).
    pub value: String,
    /// A credential: kept for the session only, never written to a project.
    pub secret: bool,
}

/// A web request: HTTP GET of a URL.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WebRequest {
    /// The address (`https://…` or `http://…`).
    pub url: String,
    /// Headers to send.
    pub headers: Vec<WebHeader>,
    /// How to read the response.
    pub format: WebFormat,
    /// Dotted path to the array of rows in a JSON document (`data.items`);
    /// empty to find it.
    pub json_path: String,
    /// Which table of a web page to read: its number (from 1) or `id`;
    /// empty for the one the address's `#section` points to, else the
    /// largest.
    pub html_table: String,
    /// Where the table sits in a web-page table or delimited text, as
    /// chosen in the Open Data dialog; `None` to detect it each time.
    pub open: Option<crate::open_data::OpenSettings>,
}

impl WebRequest {
    /// A request of `url` with no headers.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into(), ..Self::default() }
    }

    fn to_json(&self) -> JsonValue {
        let s = crate::project::s;
        let headers = self
            .headers
            .iter()
            .map(|h| {
                crate::project::obj([
                    ("name", s(&h.name)),
                    ("value", if h.secret { JsonValue::Null } else { s(&h.value) }),
                    ("secret", JsonValue::Bool(h.secret)),
                ])
            })
            .collect();
        crate::project::obj([
            ("url", s(&strip_url_secrets(&self.url))),
            ("headers", JsonValue::Array(headers)),
            ("format", s(self.format.key())),
            ("json_path", s(&self.json_path)),
            ("html_table", s(&self.html_table)),
            ("open", self.open.as_ref().map_or(JsonValue::Null, crate::open_data::OpenSettings::to_json)),
        ])
    }

    fn from_json(v: &JsonValue) -> Result<Self> {
        let url = v.get("url").and_then(JsonValue::as_str).ok_or_else(|| invalid("external source: no url"))?;
        let headers = v
            .get("headers")
            .and_then(JsonValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|h| WebHeader {
                name: h.get("name").and_then(JsonValue::as_str).unwrap_or_default().to_owned(),
                value: h.get("value").and_then(JsonValue::as_str).unwrap_or_default().to_owned(),
                secret: h.get("secret").and_then(JsonValue::as_bool).unwrap_or(false),
            })
            .collect();
        Ok(Self {
            url: url.to_owned(),
            headers,
            format: v.get("format").and_then(JsonValue::as_str).and_then(WebFormat::from_key).unwrap_or_default(),
            json_path: v.get("json_path").and_then(JsonValue::as_str).unwrap_or_default().to_owned(),
            html_table: v.get("html_table").and_then(JsonValue::as_str).unwrap_or_default().to_owned(),
            open: match v.get("open") {
                None | Some(JsonValue::Null) => None,
                Some(o) => {
                    Some(crate::open_data::OpenSettings::from_json(o, &crate::open_data::OpenSettings::default())?)
                }
            },
        })
    }
}

/// A URL with any password in its user information removed
/// (`https://ann:pw@host/x` → `https://ann@host/x`).
#[must_use]
pub fn strip_url_password(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else { return url.to_owned() };
    let rest = &url[scheme_end + 3..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    match authority.rfind('@') {
        Some(at) => {
            let user = authority[..at].split(':').next().unwrap_or("");
            format!("{}{user}@{}{}", &url[..scheme_end + 3], &authority[at + 1..], &rest[authority_end..])
        }
        None => url.to_owned(),
    }
}

/// The `(start, end)` byte range of a URL's query (after `?`, before `#`).
fn query_range(url: &str) -> Option<(usize, usize)> {
    let start = url.find('?')? + 1;
    let end = url[start..].find('#').map_or(url.len(), |i| start + i);
    Some((start, end))
}

/// The `name=value` parameters of a URL's query whose names mark a
/// credential ([`is_secret_header`]: `api_key`, `token`, `access_token`,
/// `password`, `secret`, `auth`…), with their values (empty when blanked).
#[must_use]
pub fn url_secret_parameters(url: &str) -> Vec<(String, String)> {
    let Some((start, end)) = query_range(url) else { return Vec::new() };
    url[start..end]
        .split('&')
        .filter_map(|p| {
            let (name, value) = p.split_once('=').unwrap_or((p, ""));
            (!name.is_empty() && is_secret_header(name)).then(|| (name.to_owned(), value.to_owned()))
        })
        .collect()
}

/// `url` with each secret query parameter's value replaced by `value(name,
/// old value)`.
fn map_secret_parameters(url: &str, value: impl Fn(&str, &str) -> String) -> String {
    let Some((start, end)) = query_range(url) else { return url.to_owned() };
    let query: Vec<String> = url[start..end]
        .split('&')
        .map(|p| match p.split_once('=') {
            Some((name, old)) if !name.is_empty() && is_secret_header(name) => format!("{name}={}", value(name, old)),
            _ => p.to_owned(),
        })
        .collect();
    format!("{}{}{}", &url[..start], query.join("&"), &url[end..])
}

/// A URL without credentials: no password in its user information
/// ([`strip_url_password`]) and the secret query parameters
/// ([`url_secret_parameters`]) kept by name with their values blanked
/// (`?city=Oslo&api_key=abc` → `?city=Oslo&api_key=`), so they can be asked
/// for again.
#[must_use]
pub fn strip_url_secrets(url: &str) -> String {
    map_secret_parameters(&strip_url_password(url), |_, _| String::new())
}

/// `url` with blank secret query parameters filled in from `values`
/// (`(name, value)` pairs).
#[must_use]
pub fn fill_url_secrets(url: &str, values: &[(String, String)]) -> String {
    map_secret_parameters(url, |name, old| {
        if old.is_empty() {
            values.iter().find(|(n, _)| n == name).map_or_else(String::new, |(_, v)| v.clone())
        } else {
            old.to_owned()
        }
    })
}

/// Where a table came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalSource {
    /// An SQL query of a database.
    Database {
        /// How to reach the database.
        connection: DatabaseConnection,
        /// The query.
        query: String,
    },
    /// Data fetched from the web.
    Web(WebRequest),
}

/// Credentials for one source, kept for the session only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Secrets {
    /// The database password (empty for none).
    pub password: String,
    /// Values of the secret headers, by header name.
    pub headers: Vec<(String, String)>,
    /// Values of the secret query parameters of a web address, by name.
    pub query: Vec<(String, String)>,
}

impl Secrets {
    /// The secrets a source holds in itself: its secret header values and
    /// the values of its address's secret query parameters (a password in
    /// the address is not kept).
    #[must_use]
    pub fn from_source(source: &ExternalSource) -> Self {
        match source {
            ExternalSource::Database { .. } => Self::default(),
            ExternalSource::Web(w) => Self {
                password: String::new(),
                headers: w.headers.iter().filter(|h| h.secret).map(|h| (h.name.clone(), h.value.clone())).collect(),
                query: url_secret_parameters(&w.url).into_iter().filter(|(_, v)| !v.is_empty()).collect(),
            },
        }
    }
}

impl ExternalSource {
    /// A short description, e.g. `SQLite /data/plant.db: SELECT * FROM "runs"`.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Database { connection, query } => format!("{}: {}", connection.describe(), one_line(query)),
            Self::Web(w) => strip_url_secrets(&w.url),
        }
    }

    /// What has to be asked for again before reading the source after a
    /// project is reopened: `"password"`, the names of secret headers and
    /// those of the address's secret query parameters.
    #[must_use]
    pub fn secrets_needed(&self) -> Vec<String> {
        match self {
            Self::Database { connection, .. } if connection.takes_password() => vec!["password".to_owned()],
            Self::Database { .. } => Vec::new(),
            Self::Web(w) => w
                .headers
                .iter()
                .filter(|h| h.secret)
                .map(|h| h.name.clone())
                .chain(url_secret_parameters(&w.url).into_iter().map(|(n, _)| n))
                .collect(),
        }
    }

    /// The project-file form, without any credential.
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        match self {
            Self::Database { connection, query } => crate::project::obj([
                ("type", crate::project::s("database")),
                ("connection", connection.to_json()),
                ("query", crate::project::s(query)),
            ]),
            Self::Web(w) => crate::project::obj([("type", crate::project::s("web")), ("request", w.to_json())]),
        }
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// An unknown source type or database kind, a missing URL.
    pub fn from_json(v: &JsonValue) -> Result<Self> {
        match v.get("type").and_then(JsonValue::as_str) {
            Some("database") => Ok(Self::Database {
                connection: DatabaseConnection::from_json(
                    v.get("connection").ok_or_else(|| invalid("external source: no connection"))?,
                )?,
                query: v.get("query").and_then(JsonValue::as_str).unwrap_or_default().to_owned(),
            }),
            Some("web") => Ok(Self::Web(WebRequest::from_json(
                v.get("request").ok_or_else(|| invalid("external source: no request"))?,
            )?)),
            _ => Err(invalid("external source: unknown type")),
        }
    }

    /// Read the source into a table named `name`, with notes on how values
    /// were read.
    ///
    /// # Errors
    /// A connection, query or HTTP error (in the driver's words), a kind of
    /// database this build cannot reach, or a response that is not a table.
    pub fn read(&self, secrets: &Secrets, name: &str) -> std::result::Result<(DataTable, Vec<String>), String> {
        match self {
            Self::Database { connection, query } => run_query(connection, &secrets.password, query, name),
            Self::Web(w) => fetch(w, secrets, name),
        }
    }

    /// Read the source for the Open Data dialog: a database result is a
    /// table at once; a web-page table or delimited text comes back as
    /// cells, with the settings to start from.
    ///
    /// # Errors
    /// As for [`read`](Self::read).
    pub fn read_cells(&self, secrets: &Secrets, name: &str) -> std::result::Result<WebRead, String> {
        match self {
            Self::Database { connection, query } => {
                run_query(connection, &secrets.password, query, name).map(|(t, n)| WebRead::Table(t, n))
            }
            Self::Web(w) => fetch_cells(w, secrets, name),
        }
    }

    /// A table name for this source: the queried table, or the last part of
    /// the URL's path.
    #[must_use]
    pub fn suggested_name(&self) -> String {
        match self {
            Self::Database { connection, query } => {
                let lower = query.to_ascii_lowercase();
                let from = lower.find(" from ").map(|i| query[i + 6..].trim_start());
                let table = from.and_then(|f| f.split_whitespace().next()).map(|t| {
                    t.trim_end_matches(';')
                        .split('.')
                        .next_back()
                        .unwrap_or(t)
                        .trim_matches(['"', '`', '[', ']'])
                        .to_owned()
                });
                match table.filter(|t| !t.is_empty()) {
                    Some(t) => t,
                    None if connection.kind == DatabaseKind::Sqlite => {
                        crate::file_formats::name_from_path(std::path::Path::new(&connection.path))
                    }
                    None => connection.database.clone(),
                }
            }
            Self::Web(w) => {
                let url = w.url.split(['?', '#']).next().unwrap_or("");
                let path = url.split_once("://").map_or("", |(_, rest)| rest.split_once('/').map_or("", |(_, p)| p));
                let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
                let stem = last.rsplit_once('.').map_or(last, |(s, _)| s);
                if stem.is_empty() { "Web data".to_owned() } else { stem.to_owned() }
            }
        }
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A result set as text cells: column names and rows (`None` for NULL).
type TextResult = (Vec<String>, Vec<Vec<Option<String>>>);

/// The table from a text result set, typed like delimited text.
fn table_from_text(name: &str, (names, rows): TextResult) -> std::result::Result<(DataTable, Vec<String>), String> {
    let names = crate::file_formats::unique_names(names);
    let rows = rows.into_iter().map(|r| r.into_iter().map(Option::unwrap_or_default).collect()).collect();
    stat_data::table_from_records_with_notes(name, names, rows).map_err(|e| e.to_string())
}

/// The tables (and views) in a database, as names [`DatabaseKind::select_all`]
/// accepts (`schema.table` outside the default schema).
///
/// # Errors
/// A connection error, or a kind this build cannot reach.
pub fn list_tables(connection: &DatabaseConnection, password: &str) -> std::result::Result<Vec<String>, String> {
    match connection.kind {
        DatabaseKind::Sqlite => sqlite::list_tables(connection),
        DatabaseKind::Postgres => postgres::list_tables(connection, password),
        DatabaseKind::MySql => mysql::list_tables(connection, password),
        DatabaseKind::Odbc => odbc::list_tables(connection, password),
    }
}

/// Run `query` and return its rows as a table named `name`. For statements
/// that return several result sets, the last one with columns is used.
///
/// # Errors
/// A connection or SQL error (in the driver's words), a query that returns
/// no columns, or a kind this build cannot reach.
pub fn run_query(
    connection: &DatabaseConnection,
    password: &str,
    query: &str,
    name: &str,
) -> std::result::Result<(DataTable, Vec<String>), String> {
    if query.trim().is_empty() {
        return Err("write a query, or choose a table".to_owned());
    }
    let result = match connection.kind {
        DatabaseKind::Sqlite => sqlite::query(connection, query)?,
        DatabaseKind::Postgres => postgres::query(connection, password, query)?,
        DatabaseKind::MySql => mysql::query(connection, password, query)?,
        DatabaseKind::Odbc => odbc::query(connection, password, query)?,
    };
    if result.0.is_empty() {
        return Err("the query returned no columns".to_owned());
    }
    table_from_text(name, result)
}

#[cfg_attr(all(feature = "sqlite", feature = "postgres", feature = "mysql", feature = "odbc"), allow(dead_code))]
fn unavailable(kind: DatabaseKind) -> String {
    format!("this build of AnovaCraft cannot connect to {} databases", kind.label())
}

/// Shortest text that reads back as the same number.
#[cfg(any(feature = "sqlite", test))]
fn number_text(x: f64) -> String {
    stat_data::format_number(x)
}

#[cfg(feature = "sqlite")]
mod sqlite {
    use super::{DatabaseConnection, TextResult, number_text};
    use rusqlite::types::ValueRef;

    fn open(c: &DatabaseConnection) -> Result<rusqlite::Connection, String> {
        if !std::path::Path::new(&c.path).is_file() {
            return Err(format!("no database file at {}", c.path));
        }
        let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
        rusqlite::Connection::open_with_flags(&c.path, flags).map_err(|e| e.to_string())
    }

    pub(super) fn list_tables(c: &DatabaseConnection) -> Result<Vec<String>, String> {
        let db = open(c)?;
        let mut st = db
            .prepare(
                "SELECT name FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
                 ORDER BY name",
            )
            .map_err(|e| e.to_string())?;
        let names = st.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        names.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub(super) fn query(c: &DatabaseConnection, sql: &str) -> Result<TextResult, String> {
        let db = open(c)?;
        let mut st = db.prepare(sql).map_err(|e| e.to_string())?;
        let names: Vec<String> = st.column_names().into_iter().map(str::to_owned).collect();
        let n = names.len();
        let mut rows = st.query([]).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let mut cells = Vec::with_capacity(n);
            for j in 0..n {
                let v = row.get_ref(j).map_err(|e| e.to_string())?;
                cells.push(match v {
                    ValueRef::Null => None,
                    ValueRef::Integer(i) => Some(i.to_string()),
                    ValueRef::Real(x) => Some(number_text(x)),
                    ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
                    ValueRef::Blob(b) => Some(format!("({} bytes)", b.len())),
                });
            }
            out.push(cells);
        }
        Ok((names, out))
    }
}

#[cfg(not(feature = "sqlite"))]
mod sqlite {
    use super::{DatabaseConnection, DatabaseKind, TextResult, unavailable};

    pub(super) fn list_tables(_: &DatabaseConnection) -> Result<Vec<String>, String> {
        Err(unavailable(DatabaseKind::Sqlite))
    }

    pub(super) fn query(_: &DatabaseConnection, _: &str) -> Result<TextResult, String> {
        Err(unavailable(DatabaseKind::Sqlite))
    }
}

/// A TLS client configuration trusting the Mozilla root certificates.
#[cfg(feature = "postgres")]
fn tls_config() -> rustls::ClientConfig {
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("the ring provider supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

#[cfg(feature = "postgres")]
mod postgres {
    use super::{DatabaseConnection, TIMEOUT_SECONDS, TextResult, TlsMode};
    use ::postgres::{Client, Config, SimpleQueryMessage, config::SslMode};

    /// The server's message for a database error, else the error and its causes.
    fn describe(e: &::postgres::Error) -> String {
        if let Some(db) = e.as_db_error() {
            return db.message().to_owned();
        }
        let mut text = e.to_string();
        let mut source = std::error::Error::source(e);
        while let Some(cause) = source {
            text.push_str(": ");
            text.push_str(&cause.to_string());
            source = cause.source();
        }
        text
    }

    fn connect(c: &DatabaseConnection, password: &str) -> Result<Client, String> {
        let mut config = Config::new();
        config
            .host(&c.host)
            .port(c.port)
            .dbname(&c.database)
            .user(&c.user)
            .connect_timeout(std::time::Duration::from_secs(TIMEOUT_SECONDS))
            .application_name("AnovaCraft")
            .ssl_mode(match c.tls {
                TlsMode::Disable => SslMode::Disable,
                TlsMode::Prefer => SslMode::Prefer,
                TlsMode::Require => SslMode::Require,
            });
        if !password.is_empty() {
            config.password(password);
        }
        if c.tls == TlsMode::Disable {
            config.connect(::postgres::NoTls)
        } else {
            config.connect(tokio_postgres_rustls::MakeRustlsConnect::new(super::tls_config()))
        }
        .map_err(|e| describe(&e))
    }

    pub(super) fn list_tables(c: &DatabaseConnection, password: &str) -> Result<Vec<String>, String> {
        let (_, rows) = query(
            c,
            password,
            "SELECT table_schema, table_name FROM information_schema.tables \
             WHERE table_schema NOT IN ('pg_catalog', 'information_schema') ORDER BY 1, 2",
        )?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let schema = r[0].clone().unwrap_or_default();
                let table = r[1].clone().unwrap_or_default();
                if schema == "public" { table } else { format!("{schema}.{table}") }
            })
            .collect())
    }

    pub(super) fn query(c: &DatabaseConnection, password: &str, sql: &str) -> Result<TextResult, String> {
        let mut client = connect(c, password)?;
        let messages = client.simple_query(sql).map_err(|e| describe(&e))?;
        // The last result set that has columns.
        let mut names: Vec<String> = Vec::new();
        let mut rows = Vec::new();
        let mut current: Vec<Vec<Option<String>>> = Vec::new();
        let mut current_names: Vec<String> = Vec::new();
        for m in messages {
            match m {
                SimpleQueryMessage::RowDescription(cols) => {
                    current_names = cols.iter().map(|c| c.name().to_owned()).collect();
                    current.clear();
                }
                SimpleQueryMessage::Row(row) => {
                    if current_names.is_empty() {
                        current_names = row.columns().iter().map(|c| c.name().to_owned()).collect();
                    }
                    current.push((0..row.len()).map(|j| row.get(j).map(str::to_owned)).collect());
                }
                SimpleQueryMessage::CommandComplete(_) if !current_names.is_empty() => {
                    names = std::mem::take(&mut current_names);
                    rows = std::mem::take(&mut current);
                }
                _ => {}
            }
        }
        Ok((names, rows))
    }
}

#[cfg(not(feature = "postgres"))]
mod postgres {
    use super::{DatabaseConnection, DatabaseKind, TextResult, unavailable};

    pub(super) fn list_tables(_: &DatabaseConnection, _: &str) -> Result<Vec<String>, String> {
        Err(unavailable(DatabaseKind::Postgres))
    }

    pub(super) fn query(_: &DatabaseConnection, _: &str, _: &str) -> Result<TextResult, String> {
        Err(unavailable(DatabaseKind::Postgres))
    }
}

#[cfg(feature = "mysql")]
mod mysql {
    use super::{DatabaseConnection, TIMEOUT_SECONDS, TextResult, TlsMode};
    use ::mysql::prelude::Queryable;
    use ::mysql::{Conn, OptsBuilder, SslOpts, Value};

    /// The server's message for a server error, else the error.
    fn describe(e: &::mysql::Error) -> String {
        match e {
            ::mysql::Error::MySqlError(e) => e.message.clone(),
            e => e.to_string(),
        }
    }

    fn connect(c: &DatabaseConnection, password: &str, tls: bool) -> Result<Conn, ::mysql::Error> {
        let mut opts = OptsBuilder::new()
            .ip_or_hostname(Some(c.host.clone()))
            .tcp_port(c.port)
            .user(Some(c.user.clone()).filter(|u| !u.is_empty()))
            .pass(Some(password.to_owned()).filter(|p| !p.is_empty()))
            .db_name(Some(c.database.clone()).filter(|d| !d.is_empty()))
            .prefer_socket(false)
            .tcp_connect_timeout(Some(std::time::Duration::from_secs(TIMEOUT_SECONDS)));
        if tls {
            opts = opts.ssl_opts(Some(SslOpts::default()));
        }
        Conn::new(opts)
    }

    /// Whether a failed encrypted connection failed in setting up TLS (the
    /// server offers none, or the handshake or certificate check failed), so
    /// that "if offered" may connect plainly. A refused login, a server
    /// error or an unreachable host is not retried without encryption.
    pub(super) fn tls_negotiation_failed(e: &::mysql::Error) -> bool {
        match e {
            ::mysql::Error::DriverError(::mysql::DriverError::TlsNotSupported) | ::mysql::Error::TlsError(_) => true,
            // rustls reports handshake and certificate failures as invalid data.
            ::mysql::Error::IoError(io) => io.kind() == std::io::ErrorKind::InvalidData,
            _ => false,
        }
    }

    fn connect_any(c: &DatabaseConnection, password: &str) -> Result<Conn, String> {
        match c.tls {
            TlsMode::Disable => connect(c, password, false),
            TlsMode::Require => connect(c, password, true),
            // Encrypt if the server can; connect plainly only when TLS
            // itself could not be set up.
            TlsMode::Prefer => match connect(c, password, true) {
                Err(e) if tls_negotiation_failed(&e) => connect(c, password, false),
                other => other,
            },
        }
        .map_err(|e| describe(&e))
    }

    fn text(v: Value) -> Option<String> {
        match v {
            Value::NULL => None,
            Value::Bytes(b) => Some(String::from_utf8_lossy(&b).into_owned()),
            Value::Int(i) => Some(i.to_string()),
            Value::UInt(u) => Some(u.to_string()),
            Value::Float(f) => Some(stat_data::format_number(f64::from(f))),
            Value::Double(d) => Some(stat_data::format_number(d)),
            Value::Date(y, mo, d, h, mi, s, us) => Some(if (h, mi, s, us) == (0, 0, 0, 0) {
                format!("{y:04}-{mo:02}-{d:02}")
            } else {
                format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}")
            }),
            Value::Time(neg, d, h, mi, s, _) => {
                Some(format!("{}{:02}:{mi:02}:{s:02}", if neg { "-" } else { "" }, u32::from(h) + 24 * d))
            }
        }
    }

    pub(super) fn list_tables(c: &DatabaseConnection, password: &str) -> Result<Vec<String>, String> {
        let mut conn = connect_any(c, password)?;
        let sql = if c.database.is_empty() {
            "SELECT CONCAT(table_schema, '.', table_name) FROM information_schema.tables \
             WHERE table_schema NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys') ORDER BY 1"
        } else {
            "SELECT table_name FROM information_schema.tables WHERE table_schema = DATABASE() ORDER BY 1"
        };
        conn.query_map(sql, |name: String| name).map_err(|e| describe(&e))
    }

    pub(super) fn query(c: &DatabaseConnection, password: &str, sql: &str) -> Result<TextResult, String> {
        let mut conn = connect_any(c, password)?;
        let mut result = conn.query_iter(sql).map_err(|e| describe(&e))?;
        let (mut names, mut rows) = (Vec::new(), Vec::new());
        while let Some(set) = result.iter() {
            let set_names: Vec<String> = set.columns().as_ref().iter().map(|c| c.name_str().into_owned()).collect();
            let mut set_rows = Vec::new();
            for row in set {
                let row = row.map_err(|e| describe(&e))?;
                set_rows.push(row.unwrap().into_iter().map(text).collect());
            }
            if !set_names.is_empty() {
                names = set_names;
                rows = set_rows;
            }
        }
        Ok((names, rows))
    }
}

#[cfg(not(feature = "mysql"))]
mod mysql {
    use super::{DatabaseConnection, DatabaseKind, TextResult, unavailable};

    pub(super) fn list_tables(_: &DatabaseConnection, _: &str) -> Result<Vec<String>, String> {
        Err(unavailable(DatabaseKind::MySql))
    }

    pub(super) fn query(_: &DatabaseConnection, _: &str, _: &str) -> Result<TextResult, String> {
        Err(unavailable(DatabaseKind::MySql))
    }
}

#[cfg(feature = "odbc")]
mod odbc {
    use super::{DatabaseConnection, TextResult};
    use odbc_api::buffers::TextRowSet;
    use odbc_api::{ConnectionOptions, Cursor, Environment};

    /// Longest text read from one cell (longer values are cut).
    const MAX_TEXT: usize = 4096;
    const BATCH: usize = 1000;

    fn connection_string(c: &DatabaseConnection, password: &str) -> String {
        let base = super::strip_odbc_password(&c.path);
        if password.is_empty() {
            base
        } else {
            format!("{};{}", base.trim_end_matches(';'), super::odbc_password_setting(password))
        }
    }

    fn read<C: Cursor>(mut cursor: C) -> Result<TextResult, String> {
        let n = cursor.num_result_cols().map_err(|e| e.to_string())?;
        let names =
            (1..=n).map(|i| cursor.col_name(i as u16).map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
        let buffer = TextRowSet::for_cursor(BATCH, &mut cursor, Some(MAX_TEXT)).map_err(|e| e.to_string())?;
        let mut bound = cursor.bind_buffer(buffer).map_err(|e| e.to_string())?;
        let mut rows = Vec::new();
        while let Some(batch) = bound.fetch().map_err(|e| e.to_string())? {
            for r in 0..batch.num_rows() {
                rows.push(
                    (0..batch.num_cols())
                        .map(|j| batch.at(j, r).map(|b| String::from_utf8_lossy(b).into_owned()))
                        .collect(),
                );
            }
        }
        Ok((names, rows))
    }

    pub(super) fn list_tables(c: &DatabaseConnection, password: &str) -> Result<Vec<String>, String> {
        let env = Environment::new().map_err(|e| e.to_string())?;
        let conn = env
            .connect_with_connection_string(&connection_string(c, password), ConnectionOptions::default())
            .map_err(|e| e.to_string())?;
        let text = |v: &odbc_api::parameter::VarCharArray<128>| {
            v.as_bytes().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default()
        };
        let mut names = Vec::new();
        for row in conn.tables("", "", "", "TABLE,VIEW").map_err(|e| e.to_string())? {
            let row = row.map_err(|e| e.to_string())?;
            let table = row.table.as_bytes().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
            let schema = text(&row.schema);
            if !table.is_empty() {
                names.push(if schema.is_empty() { table } else { format!("{schema}.{table}") });
            }
        }
        Ok(names)
    }

    pub(super) fn query(c: &DatabaseConnection, password: &str, sql: &str) -> Result<TextResult, String> {
        let env = Environment::new().map_err(|e| e.to_string())?;
        let conn = env
            .connect_with_connection_string(&connection_string(c, password), ConnectionOptions::default())
            .map_err(|e| e.to_string())?;
        match conn.execute(sql, (), None).map_err(|e| e.to_string())? {
            Some(cursor) => read(cursor),
            None => Ok((Vec::new(), Vec::new())),
        }
    }
}

#[cfg(not(feature = "odbc"))]
mod odbc {
    use super::{DatabaseConnection, DatabaseKind, TextResult, unavailable};

    pub(super) fn list_tables(_: &DatabaseConnection, _: &str) -> Result<Vec<String>, String> {
        Err(unavailable(DatabaseKind::Odbc))
    }

    pub(super) fn query(_: &DatabaseConnection, _: &str, _: &str) -> Result<TextResult, String> {
        Err(unavailable(DatabaseKind::Odbc))
    }
}

/// Rows of a JSON document as a table: see the [module documentation](self).
///
/// # Errors
/// Invalid JSON, a path that does not lead to an array, or no array of
/// objects to read.
pub fn json_document_table(
    name: &str,
    text: &str,
    path: &str,
) -> std::result::Result<(DataTable, Vec<String>), String> {
    let doc = parse_json(text).map_err(|e| e.to_string())?;
    let mut notes = Vec::new();
    let mut node = &doc;
    let path = path.trim();
    if !path.is_empty() {
        for part in path.split('.') {
            node = match node {
                JsonValue::Object(_) => node.get(part),
                JsonValue::Array(items) => part.parse::<usize>().ok().and_then(|i| items.get(i)),
                _ => None,
            }
            .ok_or_else(|| format!("the JSON document has nothing at {path:?}"))?;
        }
    } else if let JsonValue::Object(members) = &doc {
        let arrays: Vec<&(String, JsonValue)> = members
            .iter()
            .filter(|(_, v)| v.as_array().is_some_and(|a| !a.is_empty() && a.iter().all(|x| x.as_object().is_some())))
            .collect();
        if let [(key, v)] = arrays.as_slice() {
            node = v;
            notes.push(format!("Rows read from the document's {key:?} member."));
        }
    }
    let rows: Vec<JsonValue> = match node {
        JsonValue::Array(items) => items
            .iter()
            .map(|x| match x {
                JsonValue::Object(_) => flatten_object(x),
                other => JsonValue::Object(vec![("value".to_owned(), other.clone())]),
            })
            .collect(),
        JsonValue::Object(_) => vec![flatten_object(node)],
        _ => return Err("the JSON document holds no rows (an array of objects)".to_owned()),
    };
    if rows.is_empty() {
        return Err("the JSON array is empty".to_owned());
    }
    let table = table_from_json_rows(name, &rows).map_err(|e| e.to_string())?;
    Ok((table, notes))
}

/// The response to `request` (`text`, with its content type) as a table:
/// JSON, a table on a web page, or delimited text, as
/// [`WebRequest::format`] says; [`WebFormat::Auto`] decides from the content
/// type and the text.
///
/// # Errors
/// A response that cannot be read as asked: invalid JSON, a web page with
/// no table (or not the table asked for), or unreadable delimited text.
pub fn response_table(
    name: &str,
    text: &str,
    content_type: &str,
    request: &WebRequest,
) -> std::result::Result<(DataTable, Vec<String>), String> {
    let format = match request.format {
        WebFormat::Auto if content_type.to_ascii_lowercase().contains("json") => WebFormat::Json,
        WebFormat::Auto if html::looks_like_html(text, content_type) => WebFormat::Html,
        WebFormat::Auto if text.trim_start().starts_with(['[', '{']) => WebFormat::Json,
        WebFormat::Auto => WebFormat::Delimited,
        f => f,
    };
    match format {
        WebFormat::Json => json_document_table(name, text, &request.json_path),
        _ => response_cells(name, text, content_type, request)?.into_table(name, request.open.as_ref()),
    }
}

/// A response read for the Open Data dialog.
#[derive(Debug, Clone)]
pub enum WebRead {
    /// A table already (JSON, or a database result), with notes.
    Table(DataTable, Vec<String>),
    /// Cells to pick the table from: a web-page table or delimited text.
    Cells {
        /// The cells.
        sheet: crate::open_data::RawSheet,
        /// The delimiter used to split text (`None` for a web-page table).
        delimiter: Option<char>,
        /// The settings to start from: the request's own, else detected.
        settings: crate::open_data::OpenSettings,
        /// Notes on how the cells were read (which table of the page).
        notes: Vec<String>,
    },
}

impl WebRead {
    /// The table, reading cells with `settings` (else the ones proposed).
    ///
    /// # Errors
    /// A block that holds nothing.
    pub fn into_table(
        self,
        name: &str,
        settings: Option<&crate::open_data::OpenSettings>,
    ) -> std::result::Result<(DataTable, Vec<String>), String> {
        match self {
            Self::Table(t, notes) => Ok((t, notes)),
            Self::Cells { sheet, settings: proposed, notes: mut before, .. } => {
                let settings = settings.unwrap_or(&proposed);
                let (t, notes) =
                    crate::open_data::table_from_sheet(name, &sheet, settings).map_err(|e| e.to_string())?;
                before.extend(notes);
                Ok((t, before))
            }
        }
    }
}

/// The response to `request` as cells (a web-page table or delimited text),
/// or as a table when it is JSON; see [`response_table`].
///
/// # Errors
/// As for [`response_table`].
pub fn response_read(
    name: &str,
    text: &str,
    content_type: &str,
    request: &WebRequest,
) -> std::result::Result<WebRead, String> {
    let format = match request.format {
        WebFormat::Auto if content_type.to_ascii_lowercase().contains("json") => WebFormat::Json,
        WebFormat::Auto if html::looks_like_html(text, content_type) => WebFormat::Html,
        WebFormat::Auto if text.trim_start().starts_with(['[', '{']) => WebFormat::Json,
        f => f,
    };
    if format == WebFormat::Json {
        return json_document_table(name, text, &request.json_path).map(|(t, n)| WebRead::Table(t, n));
    }
    response_cells(name, text, content_type, &WebRequest { format, ..request.clone() })
}

fn response_cells(
    name: &str,
    text: &str,
    content_type: &str,
    request: &WebRequest,
) -> std::result::Result<WebRead, String> {
    let is_html = request.format == WebFormat::Html
        || request.format == WebFormat::Auto && html::looks_like_html(text, content_type);
    let (sheet, delimiter, proposed, notes) = if is_html {
        let (sheet, header, notes) = page_cells(name, text, request)?;
        let block = if header {
            crate::open_data::Block::default()
        } else {
            crate::open_data::Block { header_row: None, data_start: 1, ..crate::open_data::Block::default() }
        };
        (sheet, None, crate::open_data::OpenSettings { block, ..Default::default() }, notes)
    } else {
        let delimiter = if content_type.to_ascii_lowercase().contains("tab-separated") { Some('\t') } else { None };
        let delimiter = request.open.as_ref().and_then(|o| o.delimiter).or(delimiter);
        let (sheet, used) = crate::open_data::split_text(name, text, delimiter).map_err(|e| e.to_string())?;
        let proposed = crate::open_data::detect_settings(&sheet, Some(used));
        (sheet, Some(used), proposed, Vec::new())
    };
    let settings = request.open.clone().unwrap_or(proposed);
    Ok(WebRead::Cells { sheet, delimiter, settings, notes })
}

/// The `#section` part of an address, if any.
#[cfg_attr(not(feature = "web"), allow(dead_code))]
fn url_fragment(url: &str) -> Option<&str> {
    url.split_once('#').map(|(_, f)| f.trim()).filter(|f| !f.is_empty())
}

/// One table of a web page (see [`WebRequest::html_table`]) as cells: its
/// heading row first when it has one (the flag says so), then its rows.
#[cfg(feature = "web")]
fn page_cells(
    name: &str,
    text: &str,
    request: &WebRequest,
) -> std::result::Result<(crate::open_data::RawSheet, bool, Vec<String>), String> {
    use html::TableChoice;
    let wanted = request.html_table.trim();
    let choice = match (wanted.parse::<usize>(), wanted.is_empty(), url_fragment(&request.url)) {
        (Ok(n), _, _) => TableChoice::Number(n),
        (_, false, _) => TableChoice::Anchor(wanted),
        (_, true, Some(f)) => TableChoice::Anchor(f),
        (_, true, None) => TableChoice::Largest,
    };
    let (tables, picked) = html::page_tables(text, &choice);
    let Some(i) = picked else {
        return Err(if tables.iter().all(|t| t.rows.is_empty()) {
            "the address returned a web page (HTML) with no table of data in it. Use the address of a JSON or CSV \
             file, or of a page that shows its data in a table. Pages that fill in their tables with JavaScript \
             after opening cannot be read; look on the page for a download link instead"
                .to_owned()
        } else {
            format!(
                "the page has {} table{} but none \u{201c}{wanted}\u{201d}: give a table's number, from 1, or its id",
                tables.len(),
                if tables.len() == 1 { "" } else { "s" }
            )
        });
    };
    let t = &tables[i];
    let width = t.rows.first().map_or(0, Vec::len);
    let has_header = t.header.iter().any(|h| !h.trim().is_empty());
    let mut cells = Vec::with_capacity(t.rows.len() + 1);
    if has_header {
        let mut header = t.header.clone();
        header.resize(width.max(header.len()), String::new());
        cells.push(header);
    }
    cells.extend(t.rows.iter().cloned());
    let which = t.id.as_ref().map_or_else(String::new, |id| format!(" (id \u{201c}{id}\u{201d})"));
    let note = format!("Read table {} of {} on the page{which}.", i + 1, tables.len());
    Ok((crate::open_data::RawSheet::new(name, cells), has_header, vec![note]))
}

#[cfg(not(feature = "web"))]
fn page_cells(
    _: &str,
    _: &str,
    _: &WebRequest,
) -> std::result::Result<(crate::open_data::RawSheet, bool, Vec<String>), String> {
    Err("this build of AnovaCraft cannot read web pages".to_owned())
}

/// Fetch a web request and read the response as a table.
///
/// # Errors
/// An address that is not `http://` or `https://`, a network or HTTP error,
/// or a response that cannot be read as a table.
pub fn fetch(
    request: &WebRequest,
    secrets: &Secrets,
    name: &str,
) -> std::result::Result<(DataTable, Vec<String>), String> {
    let url = request.url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("the address must start with http:// or https://".to_owned());
    }
    let url = fill_url_secrets(url, &secrets.query);
    let (content_type, body) = http_get(&url, &headers_with_secrets(request, secrets))?;
    response_table(name, &body, &content_type, request)
}

/// Fetch a web request and read the response for the Open Data dialog (see
/// [`response_read`]).
///
/// # Errors
/// As for [`fetch`].
pub fn fetch_cells(request: &WebRequest, secrets: &Secrets, name: &str) -> std::result::Result<WebRead, String> {
    let url = request.url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("the address must start with http:// or https://".to_owned());
    }
    let url = fill_url_secrets(url, &secrets.query);
    let (content_type, body) = http_get(&url, &headers_with_secrets(request, secrets))?;
    response_read(name, &body, &content_type, request)
}

/// The request's headers, secret values taken from `secrets`.
fn headers_with_secrets(request: &WebRequest, secrets: &Secrets) -> Vec<(String, String)> {
    request
        .headers
        .iter()
        .filter(|h| !h.name.trim().is_empty())
        .map(|h| {
            let value = if h.secret {
                secrets.headers.iter().find(|(n, _)| n == &h.name).map_or_else(|| h.value.clone(), |(_, v)| v.clone())
            } else {
                h.value.clone()
            };
            (h.name.trim().to_owned(), value)
        })
        .collect()
}

/// The `User-Agent` sent when the request gives none: the form current
/// desktop browsers send (many sites close the connection on anything
/// else), with AnovaCraft's own name and version at the end.
#[cfg(target_os = "macos")]
#[cfg_attr(not(feature = "web"), allow(dead_code))]
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 \
     Safari/537.36 AnovaCraft/",
    env!("CARGO_PKG_VERSION")
);
#[cfg(target_os = "windows")]
#[cfg_attr(not(feature = "web"), allow(dead_code))]
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 \
     Safari/537.36 AnovaCraft/",
    env!("CARGO_PKG_VERSION")
);
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[cfg_attr(not(feature = "web"), allow(dead_code))]
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 \
     AnovaCraft/",
    env!("CARGO_PKG_VERSION")
);

/// The `Accept-Language` sent when the request gives none.
#[cfg_attr(not(feature = "web"), allow(dead_code))]
const ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";

/// The `Accept` sent when the request gives none: data first, then pages.
#[cfg_attr(not(feature = "web"), allow(dead_code))]
const ACCEPT: &str =
    "application/json, text/csv, text/tab-separated-values;q=0.9, text/plain;q=0.8, text/html;q=0.7, */*;q=0.5";

#[cfg(feature = "web")]
fn http_get(url: &str, headers: &[(String, String)]) -> std::result::Result<(String, String), String> {
    let config = || {
        ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(TIMEOUT_SECONDS)))
            .http_status_as_error(false)
    };
    let first = fetch_once(&config().build().into(), url, headers);
    // Some servers drop the secure connection AnovaCraft's own TLS library
    // (rustls) offers but accept the one the operating system's offers, as a
    // browser on that system does; try that before giving up.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let first = match first {
        Err(e) if worth_system_tls(&e) => {
            use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
            let tls =
                TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
            // When that fails too, the first failure is the one to report.
            fetch_once(&config().tls_config(tls).build().into(), url, headers).or(Err(e))
        }
        other => other,
    };
    let (status, content_type, body) = first.map_err(|e| web_error(url, &e))?;
    if !status.is_success() {
        let detail: String =
            if html::looks_like_html(&body, &content_type) { String::new() } else { body.chars().take(200).collect() };
        return Err(format!(
            "the server answered {status}{}",
            if detail.trim().is_empty() { String::new() } else { format!(": {}", detail.trim()) }
        ));
    }
    Ok((content_type, body))
}

/// One GET with `agent`: the status, content type and body text. Browser-like
/// `User-Agent`, `Accept` and `Accept-Language` headers are sent unless
/// `headers` gives its own.
#[cfg(feature = "web")]
fn fetch_once(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(String, String)],
) -> std::result::Result<(ureq::http::StatusCode, String, String), ureq::Error> {
    let given = |name: &str| headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
    let mut req = agent.get(url);
    if !given("User-Agent") {
        req = req.header("User-Agent", USER_AGENT);
    }
    if !given("Accept") {
        req = req.header("Accept", ACCEPT);
    }
    if !given("Accept-Language") {
        req = req.header("Accept-Language", ACCEPT_LANGUAGE);
    }
    for (name, value) in headers {
        req = req.header(name.as_str(), value.as_str());
    }
    let mut response = req.call()?;
    let status = response.status();
    let content_type =
        response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    let body = response.body_mut().with_config().limit(512 * 1024 * 1024).read_to_string()?;
    Ok((status, content_type, body))
}

/// Whether a failure could come from the server rejecting the secure
/// connection itself (a reset, an abrupt close or a TLS error), so that the
/// operating system's TLS is worth a second try.
#[cfg(all(feature = "web", any(target_os = "macos", target_os = "windows", test)))]
fn worth_system_tls(e: &ureq::Error) -> bool {
    use std::io::ErrorKind;
    match e {
        ureq::Error::Io(io) => matches!(
            io.kind(),
            ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::BrokenPipe
                | ErrorKind::UnexpectedEof
        ),
        ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Timeout(_)
        | ureq::Error::BodyExceedsLimit(_)
        | ureq::Error::StatusCode(_) => false,
        other => {
            let text = other.to_string().to_ascii_lowercase();
            text.contains("tls") || text.contains("certificate") || text.contains("handshake")
        }
    }
}

/// A network failure in words: what happened at which server, and what to
/// try.
#[cfg(feature = "web")]
fn web_error(url: &str, e: &ureq::Error) -> String {
    use std::io::ErrorKind;
    let host = url_host(url);
    match e {
        ureq::Error::Io(io) | ureq::Error::Decompress(_, io)
            if matches!(
                io.kind(),
                ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe
            ) =>
        {
            format!(
                "{host} closed the connection without answering ({io}). Some sites turn away programs that are not \
                 web browsers even when the request looks like one. Check that the address opens in your browser; if \
                 it does, look on the site for a download link or a data address"
            )
        }
        ureq::Error::HostNotFound => format!("could not find the server {host}: check the address and your connection"),
        ureq::Error::ConnectionFailed => format!("could not connect to {host}: check the address and your connection"),
        ureq::Error::Timeout(_) => format!("{host} did not answer within {TIMEOUT_SECONDS} seconds"),
        ureq::Error::Io(io) if io.kind() == ErrorKind::TimedOut => {
            format!("{host} did not answer within {TIMEOUT_SECONDS} seconds")
        }
        ureq::Error::BodyExceedsLimit(_) => "the response is larger than 512 MB".to_owned(),
        other => {
            let text = other.to_string();
            if text.to_ascii_lowercase().contains("tls") || text.to_ascii_lowercase().contains("certificate") {
                format!("could not make a secure connection to {host}: {text}")
            } else {
                text
            }
        }
    }
}

/// The server part of an address (`www.example.org`).
#[cfg_attr(not(feature = "web"), allow(dead_code))]
fn url_host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    authority.rsplit_once('@').map_or(authority, |(_, h)| h)
}

#[cfg(not(feature = "web"))]
fn http_get(_: &str, _: &[(String, String)]) -> std::result::Result<(String, String), String> {
    Err("this build of AnovaCraft cannot fetch web data".to_owned())
}

/// The edit that replaces `table`'s rows with `fresh`'s, as one undoable
/// step: every row is replaced (row states are reset), columns with the same
/// name are refilled in place (keeping their measurement level when their
/// type is unchanged), new columns are added at the end, and columns the
/// source no longer has are removed. Formula columns are kept and recomputed.
///
/// # Errors
/// A column the source no longer has that a formula uses, or one listed in
/// `in_use` (columns that open reports read).
pub fn refresh_edit(table: &DataTable, fresh: &DataTable, in_use: &[usize]) -> Result<Edit> {
    let mut edits = Vec::new();
    let n_old = table.n_rows();
    let n_new = fresh.n_rows();
    // Remove columns first (from the end, so indices stay valid), then rows.
    let mut kept: Vec<usize> = Vec::new();
    for j in (0..table.n_cols()).rev() {
        let c = table.column(j).expect("index in range");
        if c.formula.is_some() || fresh.find(&c.name).is_some() {
            kept.push(j);
            continue;
        }
        if in_use.contains(&j) {
            return Err(invalid(format!(
                "the source no longer has column {:?}, which an open report uses; close that report first",
                c.name
            )));
        }
        edits.push(Edit::RemoveColumn { index: j });
    }
    kept.reverse();
    if n_old > 0 {
        edits.push(Edit::RemoveRows { rows: (0..n_old).collect() });
    }
    // The table now has the kept columns and no rows.
    let mut probe = table.clone();
    let mut rows = RowStateManager::new(n_old);
    for e in &edits {
        e.clone().apply(&mut probe, &mut rows)?;
    }
    if n_new > 0 {
        edits.push(Edit::insert_empty_rows(&probe, 0, n_new));
    }
    for (k, &j) in kept.iter().enumerate() {
        let old = table.column(j).expect("index in range");
        if old.formula.is_some() {
            continue;
        }
        let mut column =
            fresh.column(fresh.find(&old.name).expect("kept columns are in the fresh table")).expect("found").clone();
        if column.kind() == old.kind() {
            column.level = old.level;
        }
        edits.push(Edit::ReplaceColumn { index: k, column });
    }
    let mut at = kept.len();
    for c in fresh.columns() {
        if table.find(&c.name).is_none() {
            edits.push(Edit::InsertColumn { index: at, column: c.clone() });
            at += 1;
        }
    }
    Ok(Edit::Batch { label: "refresh from source".to_owned(), edits })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    #[test]
    fn credentials_are_not_saved() {
        let web = ExternalSource::Web(WebRequest {
            url: "https://ann:hunter2@example.com/data.json?x=1".to_owned(),
            headers: vec![
                WebHeader { name: "Accept".into(), value: "application/json".into(), secret: false },
                WebHeader { name: "Authorization".into(), value: "Bearer abc".into(), secret: true },
            ],
            ..WebRequest::default()
        });
        let text = web.to_json().to_compact();
        assert!(!text.contains("hunter2") && !text.contains("Bearer abc"), "{text}");
        assert!(text.contains("application/json"));
        let back = ExternalSource::from_json(&web.to_json()).unwrap();
        assert_eq!(back.secrets_needed(), vec!["Authorization".to_owned()]);
        let odbc = ExternalSource::Database {
            connection: DatabaseConnection {
                path: "DSN=plant;UID=ann;PWD=hunter2;".into(),
                ..DatabaseConnection::new(DatabaseKind::Odbc)
            },
            query: "SELECT 1".into(),
        };
        assert!(!odbc.to_json().to_compact().contains("hunter2"));
        let pg = ExternalSource::Database {
            connection: DatabaseConnection {
                database: "sales".into(),
                user: "ann".into(),
                ..DatabaseConnection::new(DatabaseKind::Postgres)
            },
            query: "SELECT * FROM t".into(),
        };
        assert_eq!(ExternalSource::from_json(&pg.to_json()).unwrap(), pg);
        assert_eq!(pg.secrets_needed(), vec!["password".to_owned()]);
    }

    #[test]
    fn url_passwords_and_secret_headers() {
        assert_eq!(strip_url_password("https://ann:pw@host/x?a=b"), "https://ann@host/x?a=b");
        assert_eq!(strip_url_password("https://host/x@y"), "https://host/x@y");
        assert!(is_secret_header("X-Api-Key") && is_secret_header("authorization") && !is_secret_header("Accept"));
        assert_eq!(strip_odbc_password("DSN=a;Pwd=x;UID=b"), "DSN=a;UID=b");
    }

    #[test]
    fn braced_odbc_passwords_are_removed_whole() {
        // A braced value may hold semicolons and doubled closing braces.
        let conn = "DSN=a;PWD={hun;ter}}2};UID=b;Description={x;y}";
        let stripped = strip_odbc_password(conn);
        assert_eq!(stripped, "DSN=a;UID=b;Description={x;y}");
        let odbc = ExternalSource::Database {
            connection: DatabaseConnection { path: conn.into(), ..DatabaseConnection::new(DatabaseKind::Odbc) },
            query: "SELECT 1".into(),
        };
        let saved = odbc.to_json().to_compact();
        assert!(!saved.contains("ter") && !saved.contains("hun"), "{saved}");
        assert!(!odbc.describe().contains("ter"));
        // The password sent is braced with its closing braces doubled.
        assert_eq!(odbc_password_setting("p}w;x"), "PWD={p}}w;x}");
        assert_eq!(strip_odbc_password(&format!("DSN=a;{}", odbc_password_setting("p}w;x"))), "DSN=a");
    }

    #[test]
    fn secret_query_parameters_are_not_saved() {
        let url = "https://ann:pw@api.example.com/v1/obs.json?city=Oslo&api_key=abc123&access_token=t0k#top";
        assert_eq!(
            strip_url_secrets(url),
            "https://ann@api.example.com/v1/obs.json?city=Oslo&api_key=&access_token=#top"
        );
        let web = ExternalSource::Web(WebRequest::new(url));
        let saved = web.to_json().to_compact();
        assert!(!saved.contains("abc123") && !saved.contains("t0k"), "{saved}");
        assert!(!web.describe().contains("abc123"));
        // Kept for the session, and asked for again after reopening.
        let secrets = Secrets::from_source(&web);
        assert_eq!(secrets.query, [("api_key".to_owned(), "abc123".to_owned()), ("access_token".into(), "t0k".into())]);
        let back = ExternalSource::from_json(&web.to_json()).unwrap();
        assert_eq!(back.secrets_needed(), ["api_key", "access_token"]);
        let ExternalSource::Web(w) = &back else { panic!() };
        assert_eq!(
            fill_url_secrets(&w.url, &secrets.query),
            "https://ann@api.example.com/v1/obs.json?city=Oslo&api_key=abc123&access_token=t0k#top"
        );
        // Ordinary parameters are kept; values already present are not replaced.
        assert_eq!(strip_url_secrets("https://e.com/x?a=1&b=2"), "https://e.com/x?a=1&b=2");
        assert_eq!(
            fill_url_secrets("https://e.com/x?key=old", &[("key".into(), "new".into())]),
            "https://e.com/x?key=old"
        );
        assert!(ExternalSource::Web(WebRequest::new("https://e.com/x?a=1")).secrets_needed().is_empty());
    }

    /// "If offered" falls back to a plain connection only when TLS could
    /// not be set up, never after a refused login or other failure.
    #[cfg(feature = "mysql")]
    #[test]
    fn mysql_falls_back_to_plain_only_on_tls_failures() {
        use ::mysql::{DriverError, Error, MySqlError};
        let io = |kind| Error::IoError(std::io::Error::new(kind, "x"));
        assert!(mysql::tls_negotiation_failed(&Error::DriverError(DriverError::TlsNotSupported)));
        assert!(mysql::tls_negotiation_failed(&io(std::io::ErrorKind::InvalidData)));
        let denied = MySqlError { state: "28000".into(), message: "Access denied".into(), code: 1045 };
        assert!(!mysql::tls_negotiation_failed(&Error::MySqlError(denied)));
        assert!(!mysql::tls_negotiation_failed(&io(std::io::ErrorKind::ConnectionRefused)));
        assert!(!mysql::tls_negotiation_failed(&io(std::io::ErrorKind::TimedOut)));
        assert!(!mysql::tls_negotiation_failed(&Error::DriverError(DriverError::CleartextPluginDisabled)));
    }

    #[test]
    fn identifiers_are_quoted() {
        assert_eq!(DatabaseKind::Postgres.select_all("sales.my \"t\""), r#"SELECT * FROM "sales"."my ""t""""#);
        assert_eq!(DatabaseKind::MySql.select_all("a b"), "SELECT * FROM `a b`");
    }

    #[test]
    fn suggested_names() {
        let q = |s: &str| ExternalSource::Database {
            connection: DatabaseConnection::sqlite("/x/plant.db"),
            query: s.into(),
        };
        assert_eq!(q("SELECT * FROM \"runs\"").suggested_name(), "runs");
        assert_eq!(q("select a from public.batches where x").suggested_name(), "batches");
        assert_eq!(q("PRAGMA table_info(x)").suggested_name(), "plant");
        assert_eq!(
            ExternalSource::Web(WebRequest::new("https://e.com/api/v1/stations.json?k=1")).suggested_name(),
            "stations"
        );
        assert_eq!(ExternalSource::Web(WebRequest::new("https://e.com/")).suggested_name(), "Web data");
    }

    #[test]
    fn json_documents_become_rows() {
        let doc = r#"{"meta": {"n": 2}, "data": [{"id": 1, "site": {"name": "A", "lat": 51.5}}, {"id": 2, "site": {"name": "B"}, "tags": ["x"]}]}"#;
        let (t, notes) = json_document_table("t", doc, "").unwrap();
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["id", "site.name", "site.lat", "tags"]);
        assert!(t.column(2).unwrap().as_numeric().unwrap()[1].is_nan());
        assert_eq!(notes.len(), 1);
        let (t, _) = json_document_table("t", doc, "data.1.site").unwrap();
        assert_eq!(t.n_rows(), 1);
        assert!(json_document_table("t", doc, "nope").is_err());
        let (t, _) = json_document_table("t", "[1, 2, 3]", "").unwrap();
        assert_eq!(t.column(0).unwrap().name, "value");
    }

    #[test]
    fn responses_are_typed_like_files() {
        let (t, _) =
            response_table("t", "a,b\n1,x\n2,y\n", "text/csv", &WebRequest::new("https://e.org/a.csv")).unwrap();
        assert!(t.column(0).unwrap().as_numeric().is_some());
        let (t, _) = response_table("t", "[{\"a\": 1}]", "text/plain", &WebRequest::new("https://e.org/a")).unwrap();
        assert_eq!(t.n_rows(), 1);
        assert!(
            response_table(
                "t",
                "[{\"a\": 1}]",
                "",
                &WebRequest { format: WebFormat::Delimited, ..WebRequest::new("x") }
            )
            .is_ok()
        );
    }

    #[cfg(feature = "web")]
    #[test]
    fn web_pages_are_read_from_their_tables() {
        let page = "<!doctype html><html><body><table id=\"menu\"><tr><td>Home</td></tr></table>\
                    <h2 id=\"1D\">Daily</h2><table><tr><th>Lens</th><th>Price</th></tr>\
                    <tr><td>A</td><td>12.5</td></tr><tr><td>B</td><td>NA</td></tr></table></body></html>";
        let request = WebRequest::new("https://e.org/#1D");
        let (t, notes) = response_table("t", page, "text/html", &request).unwrap();
        assert_eq!(t.n_rows(), 2);
        assert_eq!(t.columns()[1].name, "Price");
        assert!(t.columns()[1].as_numeric().is_some(), "numbers in cells are typed as numbers");
        assert_eq!(notes[0], "Read table 2 of 2 on the page.");
        // A table named in the request wins over the address's #section.
        let menu = WebRequest { html_table: "menu".into(), ..request.clone() };
        assert_eq!(response_table("t", page, "", &menu).unwrap().0.n_rows(), 1);
        let missing = WebRequest { html_table: "7".into(), ..request };
        let err = response_table("t", page, "text/html", &missing).unwrap_err();
        assert!(err.contains("has 2 tables but none \u{201c}7\u{201d}"), "{err}");
        let err = response_table("t", "<html><body><p>Hello</p></body></html>", "text/html", &WebRequest::new("x"))
            .unwrap_err();
        assert!(err.starts_with("the address returned a web page (HTML) with no table"), "{err}");
    }

    #[cfg(feature = "web")]
    #[test]
    fn default_user_agent_looks_like_a_browser() {
        assert!(USER_AGENT.starts_with("Mozilla/5.0 ("), "{USER_AGENT}");
        assert!(USER_AGENT.ends_with(concat!("AnovaCraft/", env!("CARGO_PKG_VERSION"))));
        assert!(!USER_AGENT.contains("  "), "no doubled spaces from the line continuation: {USER_AGENT}");
    }

    #[test]
    fn network_errors_are_in_words() {
        let reset = ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionReset));
        let text = web_error("https://www.example.org/#1D", &reset);
        assert!(text.starts_with("www.example.org closed the connection without answering"), "{text}");
        assert!(web_error("https://ann@h.org:8443/x", &ureq::Error::HostNotFound).contains("server h.org:8443"));
        assert_eq!(url_fragment("https://e.org/a#1D"), Some("1D"));
        assert_eq!(url_fragment("https://e.org/a#"), None);
    }

    #[test]
    fn dropped_connections_get_a_second_try_with_system_tls() {
        let io = |k| ureq::Error::Io(std::io::Error::from(k));
        assert!(worth_system_tls(&io(std::io::ErrorKind::ConnectionReset)));
        assert!(worth_system_tls(&io(std::io::ErrorKind::UnexpectedEof)));
        assert!(!worth_system_tls(&io(std::io::ErrorKind::TimedOut)));
        assert!(!worth_system_tls(&ureq::Error::HostNotFound));
        assert!(!worth_system_tls(&ureq::Error::ConnectionFailed));
    }

    #[test]
    fn html_table_choice_is_saved() {
        let source = ExternalSource::Web(WebRequest {
            html_table: "2".into(),
            format: WebFormat::Html,
            ..WebRequest::new("https://e.org/p")
        });
        let back = ExternalSource::from_json(&source.to_json()).unwrap();
        assert_eq!(back, source);
    }

    #[test]
    fn refresh_keeps_levels_and_formulas() {
        let mut old = DataTable::new(
            "t",
            vec![
                Column::numeric("a", vec![1.0, 2.0]),
                Column::text("gone", vec!["x".into(), "y".into()]),
                Column::text("g", vec!["p".into(), "q".into()]),
            ],
        )
        .unwrap();
        old.set_level(2, stat_data::MeasurementLevel::Ordinal).unwrap();
        let mut rows = RowStateManager::new(2);
        Edit::formula_column(&old, None, "twice", 3, "2 * col(\"a\")", None)
            .unwrap()
            .apply(&mut old, &mut rows)
            .unwrap();
        let fresh = DataTable::new(
            "t",
            vec![
                Column::text("g", vec!["r".into(), "s".into(), "t".into()]),
                Column::numeric("a", vec![5.0, 6.0, 7.0]),
                Column::numeric("new", vec![0.0, 0.0, 1.0]),
            ],
        )
        .unwrap();
        assert!(refresh_edit(&old, &fresh, &[1]).is_err(), "a report uses the dropped column");
        let edit = refresh_edit(&old, &fresh, &[0]).unwrap();
        let mut t = old.clone();
        edit.apply(&mut t, &mut rows).unwrap();
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "g", "twice", "new"]);
        assert_eq!(t.n_rows(), 3);
        assert_eq!(t.column(1).unwrap().level, stat_data::MeasurementLevel::Ordinal);
        assert_eq!(t.column(2).unwrap().as_numeric().unwrap(), &[10.0, 12.0, 14.0]);
        assert_eq!(rows.len(), 3);
    }
}
