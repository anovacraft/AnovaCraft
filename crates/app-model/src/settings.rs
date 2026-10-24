//! Application preferences, kept in a small JSON file in the user's
//! configuration folder so they survive restarts:
//!
//! | System | File |
//! |---|---|
//! | macOS | `~/Library/Application Support/Statistical Engine/settings.json` |
//! | Windows | `%APPDATA%\Statistical Engine\settings.json` |
//! | Linux and others | `$XDG_CONFIG_HOME/statistical-engine/settings.json` (default `~/.config/…`) |
//!
//! ```text
//! { "format": "statistical-engine-settings",
//!   "color_scheme": "system" | "light" | "dark",
//!   "text_size": "standard" | "large" | "extra-large",
//!   "tooltip_delay": "short" | "medium" | "long",
//!   "recent_files": ["/path/to/project.acraft", …],
//!   "recent_details": [{ "path": "/path/to/project.acraft", "when": 1792240320,
//!                        "tables": ["Runs", …], "notes": "first line" } |
//!                      { "path": "/path/to/data.csv", "when": …, "rows": 2400, "columns": 9 }, …],
//!   "scripts_folder": "/path/to/scripts" }
//! ```
//!
//! Unknown or missing values fall back to the defaults, so a damaged or older
//! file never stops the application from starting.

use std::path::{Path, PathBuf};

use stat_data::{JsonValue, parse_json};

use crate::report_export::{PaperSize, ReportFormat};

/// Value of the `format` member of a settings file.
pub const SETTINGS_FORMAT: &str = "statistical-engine-settings";

/// Light or dark appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ColorScheme {
    /// Follow the operating system's light/dark setting.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

impl ColorScheme {
    /// Every scheme, in menu order.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Match system",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// Size of all text in the application, as a whole-interface setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum TextSize {
    /// The standard size.
    #[default]
    Standard,
    /// 20 % larger than standard.
    Large,
    /// 40 % larger than standard.
    ExtraLarge,
}

impl TextSize {
    /// Every size, smallest first.
    pub const ALL: [Self; 3] = [Self::Standard, Self::Large, Self::ExtraLarge];

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Large => "Large",
            Self::ExtraLarge => "Extra large",
        }
    }

    /// Factor applied to every font size.
    #[must_use]
    pub fn scale(self) -> f32 {
        match self {
            Self::Standard => 1.0,
            Self::Large => 1.2,
            Self::ExtraLarge => 1.4,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Large => "large",
            Self::ExtraLarge => "extra-large",
        }
    }
}

/// How long the pointer must rest on a control before its explanatory
/// tooltip appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum TooltipDelay {
    /// Half a second.
    Short,
    /// One second (the default), so tooltips stay out of the way while the
    /// pointer passes over controls on its way somewhere else.
    #[default]
    Medium,
    /// Two seconds.
    Long,
}

impl TooltipDelay {
    /// Every delay, shortest first.
    pub const ALL: [Self; 3] = [Self::Short, Self::Medium, Self::Long];

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Short => "Short (½ second)",
            Self::Medium => "Medium (1 second)",
            Self::Long => "Long (2 seconds)",
        }
    }

    /// Seconds the pointer must rest before a tooltip appears.
    #[must_use]
    pub fn seconds(self) -> f32 {
        match self {
            Self::Short => 0.5,
            Self::Medium => 1.0,
            Self::Long => 2.0,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Short => "short",
            Self::Medium => "medium",
            Self::Long => "long",
        }
    }
}

/// How many files File ▸ Open recent remembers.
pub const RECENT_FILES: usize = 10;

/// What the start page says about a recent file: when it was last opened or
/// saved, and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentDetails {
    /// When the file was last opened or saved, in seconds since 1970-01-01 UTC.
    pub when: i64,
    /// What the file holds.
    pub contents: RecentContents,
}

/// What a recent file holds, as the start page describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecentContents {
    /// A project: its tables' names and the first line of its notes (empty
    /// without notes).
    Project {
        /// Table names, in tab order.
        tables: Vec<String>,
        /// The first non-blank line of the project's notes.
        notes: String,
    },
    /// A plain data file (text, JSON or one sheet of a workbook).
    Data {
        /// Number of rows.
        rows: usize,
        /// Number of columns.
        columns: usize,
    },
}

impl RecentDetails {
    fn to_json(&self, path: &Path) -> JsonValue {
        let mut members = vec![
            ("path".to_owned(), JsonValue::String(path.to_string_lossy().into_owned())),
            ("when".to_owned(), JsonValue::Number(self.when as f64)),
        ];
        match &self.contents {
            RecentContents::Project { tables, notes } => {
                let names = tables.iter().map(|t| JsonValue::String(t.clone())).collect();
                members.push(("tables".to_owned(), JsonValue::Array(names)));
                members.push(("notes".to_owned(), JsonValue::String(notes.clone())));
            }
            RecentContents::Data { rows, columns } => {
                members.push(("rows".to_owned(), JsonValue::Number(*rows as f64)));
                members.push(("columns".to_owned(), JsonValue::Number(*columns as f64)));
            }
        }
        JsonValue::Object(members)
    }

    /// Read one entry of `recent_details`; `None` if it is malformed.
    fn from_json(v: &JsonValue) -> Option<(PathBuf, Self)> {
        let path = v.get("path").and_then(JsonValue::as_str).filter(|p| !p.is_empty())?;
        let when = v.get("when").and_then(JsonValue::as_f64).filter(|w| w.is_finite())? as i64;
        let contents = match v.get("tables").and_then(JsonValue::as_array) {
            Some(tables) => RecentContents::Project {
                tables: tables.iter().filter_map(JsonValue::as_str).map(str::to_owned).collect(),
                notes: v.get("notes").and_then(JsonValue::as_str).unwrap_or_default().to_owned(),
            },
            None => RecentContents::Data {
                rows: v.get("rows").and_then(JsonValue::as_usize)?,
                columns: v.get("columns").and_then(JsonValue::as_usize)?,
            },
        };
        Some((PathBuf::from(path), Self { when, contents }))
    }
}

/// The first non-blank line of `notes`, trimmed (empty when there is none).
#[must_use]
pub fn first_line(notes: &str) -> &str {
    notes.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default()
}

/// User preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    /// Light or dark appearance.
    pub color_scheme: ColorScheme,
    /// Size of all text.
    pub text_size: TextSize,
    /// How long the pointer rests on a control before its tooltip appears.
    pub tooltip_delay: TooltipDelay,
    /// Files opened or saved lately, most recent first, at most
    /// [`RECENT_FILES`].
    pub recent_files: Vec<PathBuf>,
    /// What the start page shows for recent files, for those it knows about.
    pub recent_details: Vec<(PathBuf, RecentDetails)>,
    /// Mark p-values below 0.05 in report tables (with a ★ and strong text).
    pub mark_significant: bool,
    /// The first-use tips have been dismissed (this also hides the plot
    /// hint line above the reports).
    pub tips_dismissed: bool,
    /// Report tables separate thousands with commas (184,150,146).
    pub group_thousands: bool,
    /// Report tables line up the decimal points of each numeric column.
    pub align_decimals: bool,
    /// Text files and workbooks whose table starts in the top-left cell
    /// open without the Open Data dialog.
    pub open_plain_directly: bool,
    /// The status bar shows how long the last frame took to draw (for
    /// tracing slowdowns to a report or plot).
    pub show_frame_time: bool,
    /// Folder whose `.rhai` scripts the sidebar's script library lists.
    pub scripts_folder: Option<PathBuf>,
    /// The format the report export dialog chose last.
    pub report_format: crate::report_export::ReportFormat,
    /// The paper size of PDF and Word report exports.
    pub paper: crate::report_export::PaperSize,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            color_scheme: ColorScheme::default(),
            text_size: TextSize::default(),
            tooltip_delay: TooltipDelay::default(),
            recent_files: Vec::new(),
            recent_details: Vec::new(),
            mark_significant: true,
            tips_dismissed: false,
            group_thousands: false,
            align_decimals: false,
            open_plain_directly: false,
            show_frame_time: false,
            scripts_folder: None,
            report_format: ReportFormat::default(),
            paper: PaperSize::default(),
        }
    }
}

impl AppSettings {
    /// Put `path` first in the recent files (moving it up if it is already
    /// listed), keeping at most [`RECENT_FILES`].
    pub fn remember_file(&mut self, path: &Path) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(RECENT_FILES);
        self.recent_details.retain(|(p, _)| self.recent_files.contains(p));
    }

    /// Put `path` first in the recent files, with what the start page
    /// should say about it.
    pub fn remember_file_with(&mut self, path: &Path, details: RecentDetails) {
        self.remember_file(path);
        self.recent_details.retain(|(p, _)| p != path);
        self.recent_details.push((path.to_path_buf(), details));
    }

    /// What is known about recent file `path`, if anything.
    #[must_use]
    pub fn recent_details(&self, path: &Path) -> Option<&RecentDetails> {
        self.recent_details.iter().find(|(p, _)| p == path).map(|(_, d)| d)
    }

    /// Drop `path` from the recent files (it could not be opened).
    pub fn forget_file(&mut self, path: &Path) {
        self.recent_files.retain(|p| p != path);
        self.recent_details.retain(|(p, _)| p != path);
    }

    /// Serialize to JSON text.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut members = vec![
            ("format".to_owned(), JsonValue::String(SETTINGS_FORMAT.to_owned())),
            ("color_scheme".to_owned(), JsonValue::String(self.color_scheme.key().to_owned())),
            ("text_size".to_owned(), JsonValue::String(self.text_size.key().to_owned())),
            ("tooltip_delay".to_owned(), JsonValue::String(self.tooltip_delay.key().to_owned())),
            ("mark_significant".to_owned(), JsonValue::Bool(self.mark_significant)),
            ("tips_dismissed".to_owned(), JsonValue::Bool(self.tips_dismissed)),
            ("group_thousands".to_owned(), JsonValue::Bool(self.group_thousands)),
            ("align_decimals".to_owned(), JsonValue::Bool(self.align_decimals)),
            ("open_plain_directly".to_owned(), JsonValue::Bool(self.open_plain_directly)),
            ("show_frame_time".to_owned(), JsonValue::Bool(self.show_frame_time)),
            ("report_format".to_owned(), JsonValue::String(self.report_format.key().to_owned())),
            ("paper".to_owned(), JsonValue::String(self.paper.key().to_owned())),
            (
                "recent_files".to_owned(),
                JsonValue::Array(
                    self.recent_files.iter().map(|p| JsonValue::String(p.to_string_lossy().into_owned())).collect(),
                ),
            ),
        ];
        if !self.recent_details.is_empty() {
            let details = self.recent_details.iter().map(|(p, d)| d.to_json(p)).collect();
            members.push(("recent_details".to_owned(), JsonValue::Array(details)));
        }
        if let Some(folder) = &self.scripts_folder {
            members.push(("scripts_folder".to_owned(), JsonValue::String(folder.to_string_lossy().into_owned())));
        }
        JsonValue::Object(members).to_pretty()
    }

    /// Parse JSON text leniently: anything missing or unrecognised keeps its default.
    #[must_use]
    pub fn from_json(text: &str) -> Self {
        let mut s = Self::default();
        if let Ok(v) = parse_json(text) {
            if let Some(name) = v.get("color_scheme").and_then(JsonValue::as_str) {
                if let Some(c) = ColorScheme::ALL.into_iter().find(|c| c.key() == name) {
                    s.color_scheme = c;
                }
            }
            if let Some(name) = v.get("text_size").and_then(JsonValue::as_str) {
                if let Some(t) = TextSize::ALL.into_iter().find(|t| t.key() == name) {
                    s.text_size = t;
                }
            }
            if let Some(name) = v.get("tooltip_delay").and_then(JsonValue::as_str) {
                if let Some(d) = TooltipDelay::ALL.into_iter().find(|d| d.key() == name) {
                    s.tooltip_delay = d;
                }
            }
            if let Some(on) = v.get("mark_significant").and_then(JsonValue::as_bool) {
                s.mark_significant = on;
            }
            if let Some(on) = v.get("tips_dismissed").and_then(JsonValue::as_bool) {
                s.tips_dismissed = on;
            }
            if let Some(on) = v.get("group_thousands").and_then(JsonValue::as_bool) {
                s.group_thousands = on;
            }
            if let Some(on) = v.get("align_decimals").and_then(JsonValue::as_bool) {
                s.align_decimals = on;
            }
            if let Some(on) = v.get("open_plain_directly").and_then(JsonValue::as_bool) {
                s.open_plain_directly = on;
            }
            if let Some(on) = v.get("show_frame_time").and_then(JsonValue::as_bool) {
                s.show_frame_time = on;
            }
            if let Some(f) = v.get("report_format").and_then(JsonValue::as_str).and_then(ReportFormat::from_key) {
                s.report_format = f;
            }
            if let Some(p) = v.get("paper").and_then(JsonValue::as_str).and_then(PaperSize::from_key) {
                s.paper = p;
            }
            if let Some(files) = v.get("recent_files").and_then(JsonValue::as_array) {
                s.recent_files = files
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .filter(|p| !p.is_empty())
                    .take(RECENT_FILES)
                    .map(PathBuf::from)
                    .collect();
            }
            if let Some(details) = v.get("recent_details").and_then(JsonValue::as_array) {
                s.recent_details = details
                    .iter()
                    .filter_map(RecentDetails::from_json)
                    .filter(|(p, _)| s.recent_files.contains(p))
                    .collect();
            }
            if let Some(folder) = v.get("scripts_folder").and_then(JsonValue::as_str).filter(|p| !p.is_empty()) {
                s.scripts_folder = Some(PathBuf::from(folder));
            }
        }
        s
    }

    /// Read the settings file, or the defaults if it is missing or unreadable.
    #[must_use]
    pub fn load() -> Self {
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map_or_else(Self::default, |t| Self::from_json(&t))
    }

    /// Write the settings file, creating its folder if needed.
    ///
    /// # Errors
    /// No configuration folder could be determined, or the file could not be written.
    pub fn save(&self) -> std::io::Result<()> {
        let path = settings_path()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no configuration folder found"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_json())
    }
}

/// The settings file for this computer.
#[must_use]
pub fn settings_path() -> Option<PathBuf> {
    settings_path_for(std::env::consts::OS, |k| std::env::var_os(k).map(PathBuf::from))
}

/// The settings file for operating system `os` (as in `std::env::consts::OS`)
/// with environment lookup `env`.
pub fn settings_path_for(os: &str, env: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let nonempty = |k: &str| env(k).filter(|p| !p.as_os_str().is_empty());
    let dir = match os {
        "macos" => nonempty("HOME")?.join("Library").join("Application Support").join("Statistical Engine"),
        "windows" => nonempty("APPDATA")?.join("Statistical Engine"),
        _ => nonempty("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| nonempty("HOME").map(|h| h.join(".config")))?
            .join("statistical-engine"),
    };
    Some(dir.join(Path::new("settings.json")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_lenient_parsing() {
        for c in ColorScheme::ALL {
            for t in TextSize::ALL {
                let s = AppSettings { color_scheme: c, text_size: t, ..AppSettings::default() };
                assert_eq!(AppSettings::from_json(&s.to_json()), s);
            }
        }
        assert_eq!(AppSettings::from_json(r#"{"color_scheme": "dark"}"#).text_size, TextSize::Standard);
        assert_eq!(AppSettings::from_json(r#"{"text_size": "huge"}"#).text_size, TextSize::Standard);
        assert!(TextSize::ALL.windows(2).all(|w| w[0].scale() < w[1].scale()));
        assert_eq!(AppSettings::from_json("not json"), AppSettings::default());
        assert_eq!(AppSettings::from_json(r#"{"color_scheme": "purple"}"#).color_scheme, ColorScheme::System);
        assert_eq!(AppSettings::from_json(r#"{"color_scheme": "dark", "future": 1}"#).color_scheme, ColorScheme::Dark);
        assert_eq!(ColorScheme::default(), ColorScheme::System);
    }

    #[test]
    fn tooltip_delay_round_trips_and_defaults_to_one_second() {
        for d in TooltipDelay::ALL {
            let s = AppSettings { tooltip_delay: d, ..AppSettings::default() };
            assert_eq!(AppSettings::from_json(&s.to_json()).tooltip_delay, d);
        }
        assert!(TooltipDelay::ALL.windows(2).all(|w| w[0].seconds() < w[1].seconds()));
        assert!((TooltipDelay::default().seconds() - 1.0).abs() < f32::EPSILON);
        assert_eq!(AppSettings::from_json(r#"{"text_size": "large"}"#).tooltip_delay, TooltipDelay::Medium);
        assert_eq!(AppSettings::from_json(r#"{"tooltip_delay": "never"}"#).tooltip_delay, TooltipDelay::Medium);
    }

    #[test]
    fn display_flags_round_trip_and_default() {
        let d = AppSettings::default();
        assert!(d.mark_significant && !d.tips_dismissed && !d.group_thousands && !d.align_decimals);
        assert!(!d.open_plain_directly, "the Open Data dialog shows by default");
        assert!(!d.show_frame_time);
        let s = AppSettings {
            open_plain_directly: true,
            show_frame_time: true,
            mark_significant: false,
            tips_dismissed: true,
            group_thousands: true,
            align_decimals: true,
            report_format: crate::report_export::ReportFormat::Word,
            paper: crate::report_export::PaperSize::A4,
            ..AppSettings::default()
        };
        assert_eq!(AppSettings::from_json(&s.to_json()), s);
        // Older files without the keys keep the defaults.
        assert_eq!(AppSettings::from_json(r#"{"format":"x"}"#), d);
    }

    #[test]
    fn recent_details_round_trip_and_follow_the_list() {
        let mut s = AppSettings::default();
        let project = RecentDetails {
            when: 1_792_240_320,
            contents: RecentContents::Project {
                tables: vec!["Runs".into(), "Screening design".into()],
                notes: "Second batch".into(),
            },
        };
        let data = RecentDetails { when: 1_792_000_000, contents: RecentContents::Data { rows: 2400, columns: 9 } };
        s.remember_file_with(Path::new("/p/filtration.acraft"), project.clone());
        s.remember_file_with(Path::new("/p/line4.csv"), data.clone());
        s.remember_file(Path::new("/p/plain.csv"));
        let back = AppSettings::from_json(&s.to_json());
        assert_eq!(back, s);
        assert_eq!(back.recent_details(Path::new("/p/filtration.acraft")), Some(&project));
        assert_eq!(back.recent_details(Path::new("/p/line4.csv")), Some(&data));
        assert_eq!(back.recent_details(Path::new("/p/plain.csv")), None);
        // Remembering again replaces the details; forgetting drops them.
        let later = RecentDetails { when: 1_792_300_000, ..data };
        s.remember_file_with(Path::new("/p/line4.csv"), later.clone());
        assert_eq!(s.recent_details(Path::new("/p/line4.csv")), Some(&later));
        assert_eq!(s.recent_details.len(), 2);
        s.forget_file(Path::new("/p/line4.csv"));
        assert_eq!(s.recent_details(Path::new("/p/line4.csv")), None);
        // Details of files that fell off the list go too.
        for i in 0..RECENT_FILES {
            s.remember_file(Path::new(&format!("/p/f{i}.csv")));
        }
        assert!(s.recent_details.is_empty());
        // Malformed entries are skipped.
        let text = r#"{"recent_files": ["/a.csv", "/b.csv"], "recent_details": [{"path": "/a.csv"}, 3,
            {"path": "/b.csv", "when": 5, "rows": 2, "columns": 1}, {"path": "/c.csv", "when": 5, "rows": 1, "columns": 1}]}"#;
        let read = AppSettings::from_json(text);
        assert_eq!(read.recent_details.len(), 1);
        assert_eq!(read.recent_details(Path::new("/b.csv")).map(|d| d.when), Some(5));
    }

    #[test]
    fn first_line_of_notes() {
        assert_eq!(first_line("\n  \n  Pump replaced.  \nCheck run 14"), "Pump replaced.");
        assert_eq!(first_line("   "), "");
    }

    #[test]
    fn recent_files_most_recent_first() {
        let mut s = AppSettings::default();
        for i in 0..12 {
            s.remember_file(Path::new(&format!("/data/f{i}.csv")));
        }
        assert_eq!(s.recent_files.len(), RECENT_FILES);
        assert_eq!(s.recent_files[0], Path::new("/data/f11.csv"));
        s.remember_file(Path::new("/data/f5.csv"));
        assert_eq!(s.recent_files[0], Path::new("/data/f5.csv"));
        assert_eq!(s.recent_files.iter().filter(|p| p.ends_with("f5.csv")).count(), 1);
        s.forget_file(Path::new("/data/f5.csv"));
        assert!(!s.recent_files.iter().any(|p| p.ends_with("f5.csv")));
        assert_eq!(AppSettings::from_json(&s.to_json()), s);
        assert!(
            AppSettings::from_json(r#"{"recent_files": [3, "", "/a.csv"]}"#).recent_files == [PathBuf::from("/a.csv")]
        );
    }

    #[test]
    fn scripts_folder_round_trips() {
        let s = AppSettings { scripts_folder: Some(PathBuf::from("/home/a/scripts")), ..AppSettings::default() };
        assert_eq!(AppSettings::from_json(&s.to_json()), s);
        assert_eq!(AppSettings::from_json(r#"{"scripts_folder": ""}"#).scripts_folder, None);
        assert_eq!(AppSettings::from_json(&AppSettings::default().to_json()).scripts_folder, None);
    }

    #[test]
    fn per_platform_locations() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |k: &str| vars.iter().find(|v| v.0 == k).map(|v| PathBuf::from(v.1))
        };
        assert_eq!(
            settings_path_for("macos", env(&[("HOME", "/Users/a")])),
            Some(PathBuf::from("/Users/a/Library/Application Support/Statistical Engine/settings.json"))
        );
        assert_eq!(
            settings_path_for("windows", env(&[("APPDATA", "C:/Users/a/AppData/Roaming")])),
            Some(PathBuf::from("C:/Users/a/AppData/Roaming/Statistical Engine/settings.json"))
        );
        assert_eq!(
            settings_path_for("linux", env(&[("HOME", "/home/a")])),
            Some(PathBuf::from("/home/a/.config/statistical-engine/settings.json"))
        );
        // XDG_CONFIG_HOME must be absolute on the host running the test, which
        // on Windows needs a drive.
        let cfg = if cfg!(windows) { "C:/cfg" } else { "/cfg" };
        let xdg: &'static [(&str, &str)] = if cfg!(windows) {
            &[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "C:/cfg")]
        } else {
            &[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "/cfg")]
        };
        assert_eq!(
            settings_path_for("linux", env(xdg)),
            Some(PathBuf::from(cfg).join("statistical-engine").join("settings.json"))
        );
        assert_eq!(settings_path_for("windows", env(&[])), None);
    }
}
