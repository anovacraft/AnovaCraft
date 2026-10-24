//! File ▸ Import from database / Import from the web, and Data ▸ Refresh
//! from source: settings for an [`ExternalSource`], read on a background
//! thread so the window stays responsive while a server answers.

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use app_model::external::{
    DatabaseConnection, DatabaseKind, ExternalSource, Secrets, TlsMode, WebFormat, WebHeader, WebRead, WebRequest,
    fill_url_secrets, is_secret_header, list_tables, strip_url_secrets, url_secret_parameters,
};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome};

/// Which kind of source the dialog sets up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalKind {
    Database,
    Web,
}

/// What the dialog produced this frame.
pub enum ExternalOutcome {
    Open,
    Cancel,
    /// A table was read: open it (or, when refreshing, replace document
    /// `refresh`'s rows with it).
    Read {
        table: DataTable,
        notes: Vec<String>,
        source: Box<ExternalSource>,
        secrets: Secrets,
        refresh: Option<u64>,
    },
    /// A web-page table or delimited text was fetched: pick the table from
    /// its cells in the Open Data dialog.
    Cells {
        read: WebRead,
        source: Box<ExternalSource>,
        secrets: Secrets,
        name: String,
    },
}

enum JobResult {
    Tables(Vec<String>),
    Table(DataTable, Vec<String>),
    Cells(WebRead),
}

struct Job {
    /// Listing tables, or reading the table.
    importing: bool,
    rx: Receiver<Result<JobResult, String>>,
}

pub struct ExternalDialog {
    kind: ExternalKind,
    connection: DatabaseConnection,
    password: String,
    tables: Vec<String>,
    query: String,
    web: WebRequest,
    /// Name of the new table (empty: suggested from the source).
    name: String,
    job: Option<Job>,
    error: Option<String>,
    /// Refreshing document `id` from `source` (only the credentials are asked for).
    refresh: Option<(u64, ExternalSource)>,
    /// Start reading as soon as the dialog shows (a refresh that needs no credentials).
    start: bool,
    /// Refreshing: values typed for the address's blanked secret query
    /// parameters (`api_key=` after a project is reopened), by name.
    query_secrets: Vec<(String, String)>,
}

impl ExternalDialog {
    pub fn new(kind: ExternalKind) -> Self {
        Self {
            kind,
            connection: DatabaseConnection::new(DatabaseKind::Sqlite),
            password: String::new(),
            tables: Vec::new(),
            query: String::new(),
            web: WebRequest::default(),
            name: String::new(),
            job: None,
            error: None,
            refresh: None,
            start: false,
            query_secrets: Vec::new(),
        }
    }

    /// Refresh document `doc` from `source`. With the credentials known
    /// (`secrets`), or none needed, reading starts at once.
    pub fn refresh(doc: u64, source: ExternalSource, secrets: Option<Secrets>) -> Self {
        let kind = match source {
            ExternalSource::Database { .. } => ExternalKind::Database,
            ExternalSource::Web(_) => ExternalKind::Web,
        };
        let start = secrets.is_some() || source.secrets_needed().is_empty();
        let secrets = secrets.unwrap_or_default();
        let mut d =
            Self { refresh: Some((doc, source.clone())), start, password: secrets.password.clone(), ..Self::new(kind) };
        match source {
            ExternalSource::Database { connection, query } => {
                d.connection = connection;
                d.query = query;
            }
            ExternalSource::Web(mut web) => {
                for h in web.headers.iter_mut().filter(|h| h.secret) {
                    h.value =
                        secrets.headers.iter().find(|(n, _)| n == &h.name).map(|(_, v)| v.clone()).unwrap_or_default();
                }
                web.url = fill_url_secrets(&web.url, &secrets.query);
                d.query_secrets = url_secret_parameters(&web.url).into_iter().filter(|(_, v)| v.is_empty()).collect();
                d.web = web;
            }
        }
        d
    }

    fn title(&self) -> &'static str {
        match (self.refresh.is_some(), self.kind) {
            (true, _) => "Refresh from source",
            (false, ExternalKind::Database) => "Import from database",
            (false, ExternalKind::Web) => "Import from the web",
        }
    }

    fn source(&self) -> ExternalSource {
        match self.kind {
            ExternalKind::Database => {
                ExternalSource::Database { connection: self.connection.clone(), query: self.query.trim().to_owned() }
            }
            ExternalKind::Web => {
                let mut web = self.web.clone();
                web.url = web.url.trim().to_owned();
                web.headers.retain(|h| !h.name.trim().is_empty());
                ExternalSource::Web(web)
            }
        }
    }

    fn secrets(&self) -> Secrets {
        let mut s = Secrets::from_source(&self.source());
        s.password.clone_from(&self.password);
        s.query.extend(self.query_secrets.iter().filter(|(_, v)| !v.is_empty()).cloned());
        s
    }

    fn table_name(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() { self.source().suggested_name() } else { name.to_owned() }
    }

    fn start_import(&mut self, ctx: &egui::Context) {
        let (source, secrets, name) = (self.source(), self.secrets(), self.table_name());
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        // A refresh reads the table as it was chosen; a new import of a web
        // page or text goes through the Open Data dialog first.
        let refresh = self.refresh.is_some();
        spawn(move || {
            let result = if refresh {
                source.read(&secrets, &name).map(|(t, notes)| JobResult::Table(t, notes))
            } else {
                source.read_cells(&secrets, &name).map(|read| match read {
                    WebRead::Table(t, notes) => JobResult::Table(t, notes),
                    cells => JobResult::Cells(cells),
                })
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.job = Some(Job { importing: true, rx });
        self.error = None;
    }

    fn start_listing(&mut self, ctx: &egui::Context) {
        let (connection, password) = (self.connection.clone(), self.password.clone());
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        spawn(move || {
            let _ = tx.send(list_tables(&connection, &password).map(JobResult::Tables));
            ctx.request_repaint();
        });
        self.job = Some(Job { importing: false, rx });
        self.error = None;
    }

    pub fn show(&mut self, ctx: &egui::Context) -> ExternalOutcome {
        if std::mem::take(&mut self.start) {
            self.start_import(ctx);
        }
        // A finished background job.
        if let Some(job) = &self.job {
            match job.rx.try_recv() {
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.error = Some("the connection stopped unexpectedly".to_owned());
                }
                Ok(result) => {
                    self.job = None;
                    match result {
                        Ok(JobResult::Tables(tables)) => {
                            if tables.is_empty() {
                                self.error = Some("the database has no tables".to_owned());
                            }
                            self.tables = tables;
                        }
                        Ok(JobResult::Table(table, notes)) => {
                            return ExternalOutcome::Read {
                                table,
                                notes,
                                source: Box::new(self.source()),
                                secrets: self.secrets(),
                                refresh: self.refresh.as_ref().map(|(id, _)| *id),
                            };
                        }
                        Ok(JobResult::Cells(read)) => {
                            return ExternalOutcome::Cells {
                                read,
                                source: Box::new(self.source()),
                                secrets: self.secrets(),
                                name: self.table_name(),
                            };
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            }
        }
        let action = if self.refresh.is_some() { "Refresh" } else { "Import" };
        let dialog = Dialog::new(self.title()).creates().without_reset().action(action);
        let busy = self.job.is_some();
        let outcome = dialog.show(ctx, |ui| {
            ui.set_max_width(kit::em(ui) * 32.0);
            if let Some((_, source)) = &self.refresh {
                kit::note(ui, &format!("Read the table again from {}.", source.describe()));
                ui.add_space(4.0);
                self.credentials(ui);
            } else {
                match self.kind {
                    ExternalKind::Database => self.database(ui),
                    ExternalKind::Web => self.web(ui),
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let label = ui.label("Table name:");
                    let hint = self.source().suggested_name();
                    ui.add(egui::TextEdit::singleline(&mut self.name).hint_text(hint).desired_width(kit::em(ui) * 14.0))
                        .labelled_by(label.id);
                });
                kit::note(ui, "Passwords, secret header values and keys in the address (api_key=, token=…) are kept only while AnovaCraft is open; projects never store them.");
            }
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(if self.job.as_ref().is_some_and(|j| j.importing) { "Reading…" } else { "Listing tables…" });
                });
            }
            if let Some(e) = &self.error {
                ui.add(egui::Label::new(egui::RichText::new(format!("Could not read: {e}")).color(ui.visuals().error_fg_color)).wrap());
            }
            self.readiness()
        });
        match outcome {
            Outcome::Cancel => ExternalOutcome::Cancel,
            Outcome::Run(..) if !busy => {
                self.start_import(ctx);
                ExternalOutcome::Open
            }
            _ => ExternalOutcome::Open,
        }
    }

    /// Ready when the settings can be read, with a one-line summary.
    fn readiness(&self) -> kit::Readiness<()> {
        match self.source() {
            ExternalSource::Database { connection, query } => {
                if connection.kind == DatabaseKind::Sqlite && connection.path.trim().is_empty() {
                    return Err("Choose the database file".into());
                }
                if connection.kind == DatabaseKind::Odbc && connection.path.trim().is_empty() {
                    return Err("Enter the data source (DSN=…)".into());
                }
                if connection.kind.is_server()
                    && (connection.host.trim().is_empty() || connection.database.trim().is_empty())
                {
                    return Err("Enter the server and database".into());
                }
                if query.is_empty() {
                    return Err("Choose a table or write a query".into());
                }
                if !connection.kind.available() {
                    return Err(format!("This build cannot connect to {} databases", connection.kind.label()));
                }
                kit::ready((), format!("Read {} into the table “{}”", connection.describe(), self.table_name()))
            }
            ExternalSource::Web(web) => {
                if !(web.url.starts_with("http://") || web.url.starts_with("https://")) {
                    return Err("Enter an address starting with https:// or http://".into());
                }
                kit::ready((), format!("Fetch {} into the table “{}”", strip_url_secrets(&web.url), self.table_name()))
            }
        }
    }

    fn credentials(&mut self, ui: &mut egui::Ui) {
        match self.kind {
            ExternalKind::Database if self.connection.takes_password() => {
                ui.horizontal(|ui| {
                    let label = ui.label("Password:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.password).password(true).desired_width(kit::em(ui) * 12.0),
                    )
                    .labelled_by(label.id);
                });
            }
            ExternalKind::Database => {}
            ExternalKind::Web => {
                for h in self.web.headers.iter_mut().filter(|h| h.secret) {
                    ui.horizontal(|ui| {
                        let label = ui.label(format!("{}:", h.name));
                        ui.add(
                            egui::TextEdit::singleline(&mut h.value).password(true).desired_width(kit::em(ui) * 14.0),
                        )
                        .labelled_by(label.id);
                    });
                }
                for (name, value) in &mut self.query_secrets {
                    ui.horizontal(|ui| {
                        let label = ui.label(format!("{name} (in the address):"));
                        ui.add(egui::TextEdit::singleline(value).password(true).desired_width(kit::em(ui) * 14.0))
                            .labelled_by(label.id);
                    });
                }
            }
        }
    }

    fn database(&mut self, ui: &mut egui::Ui) {
        let before = self.connection.kind;
        ui.horizontal_wrapped(|ui| {
            ui.label("Database:");
            for kind in DatabaseKind::ALL.into_iter().filter(|k| k.available()) {
                ui.radio_value(&mut self.connection.kind, kind, kind.label());
            }
        });
        if self.connection.kind != before {
            let kind = self.connection.kind;
            self.connection = DatabaseConnection { path: String::new(), ..DatabaseConnection::new(kind) };
            self.tables.clear();
            self.query.clear();
            self.error = None;
        }
        let width = kit::em(ui) * 16.0;
        egui::Grid::new("external_connection").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            match self.connection.kind {
                DatabaseKind::Sqlite => {
                    let label = ui.label("File:");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.connection.path).desired_width(width))
                            .labelled_by(label.id);
                        if ui.button("Browse…").clicked() {
                            if let Some(path) = crate::file_io::pick_open_file(
                                "Open SQLite database",
                                "SQLite database",
                                &["db", "sqlite", "sqlite3", "db3"],
                            ) {
                                self.connection.path = path.display().to_string();
                            }
                        }
                    });
                    ui.end_row();
                }
                DatabaseKind::Odbc => {
                    let label = ui.label("Data source:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.connection.path)
                            .hint_text("DSN=name;UID=user")
                            .desired_width(width),
                    )
                    .labelled_by(label.id)
                    .on_hover_text(
                        "An ODBC data source name, or a connection string with Driver=…; give the password below",
                    );
                    ui.end_row();
                    let label = ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut self.password).password(true).desired_width(width))
                        .labelled_by(label.id);
                    ui.end_row();
                }
                DatabaseKind::Postgres | DatabaseKind::MySql => {
                    let label = ui.label("Server:");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.connection.host).desired_width(width * 0.7))
                            .labelled_by(label.id);
                        ui.label("Port:");
                        ui.add(egui::DragValue::new(&mut self.connection.port).range(1..=65535));
                    });
                    ui.end_row();
                    let label = ui.label("Database name:");
                    ui.add(egui::TextEdit::singleline(&mut self.connection.database).desired_width(width))
                        .labelled_by(label.id);
                    ui.end_row();
                    let label = ui.label("User:");
                    ui.add(egui::TextEdit::singleline(&mut self.connection.user).desired_width(width))
                        .labelled_by(label.id);
                    ui.end_row();
                    let label = ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut self.password).password(true).desired_width(width))
                        .labelled_by(label.id);
                    ui.end_row();
                    ui.label("Encryption:");
                    ui.horizontal(|ui| {
                        for mode in TlsMode::ALL {
                            ui.radio_value(&mut self.connection.tls, mode, mode.label());
                        }
                    });
                    ui.end_row();
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let can_list = self.job.is_none() && self.connection.kind.available();
            if ui
                .add_enabled(can_list, egui::Button::new("List tables"))
                .on_hover_text("Connect and show the tables and views")
                .clicked()
            {
                self.start_listing(ui.ctx());
            }
            if !self.tables.is_empty() {
                ui.label(format!("{} found", kit::count(self.tables.len(), "table", "tables")));
            }
        });
        if !self.tables.is_empty() {
            egui::ScrollArea::vertical().id_salt("external_tables").max_height(kit::em(ui) * 8.0).show(ui, |ui| {
                for t in &self.tables {
                    let query = self.connection.kind.select_all(t);
                    if ui.selectable_label(self.query == query, t).clicked() {
                        self.query = query;
                        self.name.clear();
                    }
                }
            });
        }
        let label = ui.label("Query (SQL):");
        ui.add(
            egui::TextEdit::multiline(&mut self.query)
                .code_editor()
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("SELECT * FROM measurements WHERE …"),
        )
        .labelled_by(label.id);
        kit::note(ui, "SQLite files are opened read-only. Every row the query returns becomes a row of the new table.");
    }

    fn web(&mut self, ui: &mut egui::Ui) {
        let label = ui.label("Address (URL):");
        ui.add(
            egui::TextEdit::singleline(&mut self.web.url)
                .hint_text("https://example.org/data.json")
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id);
        ui.horizontal(|ui| {
            ui.label("Read as:");
            for f in WebFormat::ALL {
                ui.radio_value(&mut self.web.format, f, f.label());
            }
        });
        if matches!(self.web.format, WebFormat::Auto | WebFormat::Html) {
            ui.horizontal(|ui| {
                let label = ui.label("Table on the page:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.web.html_table)
                        .hint_text("found automatically")
                        .desired_width(kit::em(ui) * 12.0),
                )
                .labelled_by(label.id)
                .on_hover_text(
                    "For a web page: the table's number (1 for the first) or its id. Empty: the table the \
                     address's #section points to, else the largest table",
                );
            });
        }
        if matches!(self.web.format, WebFormat::Auto | WebFormat::Json) {
            ui.horizontal(|ui| {
                let label = ui.label("Rows at (JSON):");
                ui.add(
                    egui::TextEdit::singleline(&mut self.web.json_path)
                        .hint_text("found automatically")
                        .desired_width(kit::em(ui) * 12.0),
                )
                .labelled_by(label.id)
                .on_hover_text("Dotted path to the array of rows, such as data.items; leave empty to find it");
            });
        }
        ui.label("Headers:");
        let mut remove = None;
        egui::Grid::new("external_headers").num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
            for (i, h) in self.web.headers.iter_mut().enumerate() {
                let before = h.name.clone();
                ui.add(egui::TextEdit::singleline(&mut h.name).hint_text("Name").desired_width(kit::em(ui) * 8.0));
                if h.name != before && is_secret_header(&h.name) {
                    h.secret = true;
                }
                ui.add(
                    egui::TextEdit::singleline(&mut h.value)
                        .hint_text("Value")
                        .password(h.secret)
                        .desired_width(kit::em(ui) * 12.0),
                );
                ui.checkbox(&mut h.secret, "Secret").on_hover_text("A credential: never saved in projects");
                if ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.web.headers.remove(i);
        }
        if ui.small_button("Add header").clicked() {
            self.web.headers.push(WebHeader { name: String::new(), value: String::new(), secret: false });
        }
        kit::note(
            ui,
            "JSON: an array of objects becomes rows, nested objects become dotted column names. A web page: one of \
             its tables becomes the new table. Other responses are read as delimited text.",
        );
    }
}

fn spawn(f: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().name("external-data".into()).spawn(f);
}
