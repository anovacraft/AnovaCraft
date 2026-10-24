//! Top-level application state and frame layout.
//!
//! The application holds any number of open documents — a dataset with its row
//! states and reports — one of which is active. Open documents are listed as
//! tabs along the bottom of the window and in the left sidebar, where
//! right-clicking an entry offers save, export and close. Opening a file or a
//! sample while a document is open adds a new document rather than replacing it.
//!
//! A document's data grid, its results pane or a single report can also be
//! shown in a native window of its own ([`windows`]). The main window then
//! shows one document (`shown`) while menus, dialogs and the sidebar act on
//! the document of the window used last (`active`, the command target).
//!
//! A project can also hold dashboards ([`dashboards`]): tiles of reports and
//! notes from its tables on a grid, with shared filters. The main window
//! shows either a table or one of its project's dashboards
//! (`shown_dashboard`), and a dashboard can have a window of its own.

#[cfg(target_arch = "wasm32")]
use crate::web_shims::{arboard, rfd};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agreement_dialog::{AgreementDialog, AgreementTool};
use crate::anom_dialog::AnomDialog;
use crate::attribute_dialog::AttributeDialog;
use crate::augment_dialog::AugmentDialog;
use crate::batch_window::BatchWindow;
use crate::capability_dialog::CapabilityDialog;
use crate::chart_dialog::ChartDialog;
use crate::compare_ui::{CompareDialog, CompareKind, CompareOutput};
use crate::describe_dialogs::{BivariateDialog, DistributionDialog, DistributionFitDialog};
use crate::design_dialog::{self, DesignDialog};
use crate::diagram_dialog::{CauseEffectDialog, MultiVariDialog, ParetoDialog};
use crate::dialog_kit::{self, Outcome, Placement, Remember};
use crate::figure_page_window::FigurePageWindow;
use crate::file_io;
use crate::formula_dialog::FormulaDialog;
use crate::gauge_dialog::GaugeDialog;
use crate::mixture_design_dialog::MixtureDesignDialog;
use crate::mixture_dialog::MixtureDialog;
use crate::model_dialog::ModelDialog;
use crate::multivariate_dialog::MultivariateDialog;
use crate::phase2_dialogs::{Phase2Dialog, Phase2Kind};
use crate::quality_tools_dialog::{QualityTool, QualityToolDialog};
use crate::regression_dialogs::RegressionDialog;
use crate::reliability_dialog::ReliabilityDialog;
use crate::report_view::{self, ColumnDrag, Report, ReportRequest};
use crate::script_library::{LibraryOp, LibraryRequest, ProjectRequest, ScriptLibrary};
use crate::script_window::{ScriptEvent, ScriptWindow};
use crate::study_dialogs::{SheetDialog, SheetKind, StudyDialog, StudyTool};
use crate::survival_dialog::SurvivalDialog;
use crate::table_ops_dialog::{TableOpAction, TableOpDialog, TableOpKind};
use crate::table_view::{self, GridRequest, GridState};
use crate::theme;
use crate::time_weighted_dialog::TimeWeightedDialog;
use app_model::editing::{ColumnShift, Edit, EditHistory};
use app_model::file_formats::{ExportFormat, Opened};
use app_model::project::{Project, ProjectFile, ProjectScript, ProjectTable, TableId, TableLineage};
use app_model::recording::{OpenedFrom, Recorder};
use app_model::settings::{AppSettings, ColorScheme, TextSize, TooltipDelay};
use app_model::table_ops::{DerivedDocument, Source, TableOrigin};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};
use projects::{NewProject, OpenProject, ReadItem, StaleCheck};
use stat_data::{DataTable, RowStateManager, SelectionMode};
use stat_script::{DocumentChange, OpenDocument, Workspace as ScriptWorkspace, WorkspaceRun};
use windows::{View, ViewWindow};

/// A dialog opened from a menu: the settings it was last left with for this
/// document, or `fresh` pre-filled with the columns selected in the grid.
macro_rules! opened {
    ($memory:expr, $doc:expr, $fresh:expr) => {
        Some($memory.recall($doc.id, $fresh, Some(&$doc.table), $doc.grid.selected_columns()))
    };
}

/// Narrowest the data grid or results pane may be dragged in side-by-side view.
const MIN_PANE_WIDTH: f32 = 320.0;
/// Shown in Help ▸ About AnovaCraft.
const INDEPENDENCE_NOTICE: &str = "AnovaCraft is an independent implementation built from published statistical \
                                   literature and is not affiliated with any commercial statistics package.";

const OPEN_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SAVE_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const CLOSE_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
const QUIT_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
const SETTINGS_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma);
const UNDO_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO_ALT_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
/// Shown in the Edit menu; the keys themselves arrive as copy and paste events.
const COPY_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);
const PASTE_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::V);
const CUT_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::X);
const CLEAR_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Delete);
const FILL_DOWN_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
const SELECT_ALL_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::A);
const NEW_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const FIND_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
const EXPORT_REPORTS_SHORTCUT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::E);
/// Data, Results and Side by side.
const WORKSPACE_SHORTCUTS: [KeyboardShortcut; 3] = [
    KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1),
    KeyboardShortcut::new(Modifiers::COMMAND, Key::Num2),
    KeyboardShortcut::new(Modifiers::COMMAND, Key::Num3),
];

fn save_as_shortcut() -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)
}

/// A report the user removed, kept so that it can be put back.
struct RemovedReport {
    /// The document it was in.
    document: u64,
    /// Its place in the document's report list.
    index: usize,
    /// Its title, for the status bar.
    title: String,
    /// The analysis and display options, to compute it again.
    saved: app_model::project::SavedReport,
    /// When it was removed (seconds, egui's clock).
    at: f64,
}

/// How long the status bar offers to put a removed report back, in seconds.
const UNDO_REMOVE_SECONDS: f64 = 30.0;

/// A window opened from the Help menu.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HelpWindow {
    Shortcuts,
    About,
    /// The third-party notices (`THIRD-PARTY-NOTICES.txt`), opened from About.
    Licences,
}


/// Whether a project had unsaved changes when last checked, and what its
/// tables looked like then (checked again when one of them changes, and
/// every two seconds for the parts that leave no other trace).
struct ModifiedMark {
    /// Each table's revisions and reports.
    key: Vec<ModifiedKey>,
    checked_at: f64,
    modified: bool,
}

/// What a table looked like at a modified check: its edit, data and saved
/// row-state revisions, and each report's id and options revision.
#[derive(PartialEq, Eq)]
struct ModifiedKey {
    edits: u64,
    data: u64,
    rows: u64,
    reports: Vec<(u64, u64)>,
}

fn redo_shortcut() -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)
}

/// What the central workspace shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Workspace {
    Data,
    Results,
    SideBySide,
}

/// One open table with its row states and reports. Every document belongs
/// to an open project ([`OpenProject`]), which may hold other tables too.
struct Document {
    /// Unique for the life of the application (used for egui ids).
    id: u64,
    /// The open project it belongs to (by id).
    project: u64,
    /// Its id within the project's file.
    table_id: TableId,
    /// The data, shared with analyses computing in the background (an edit
    /// copies it only while one of them still holds the old version).
    table: Arc<DataTable>,
    rows: RowStateManager,
    reports: Vec<Report>,
    workspace: Workspace,
    /// The data file it was imported from, if any.
    source_path: Option<PathBuf>,
    /// Undo / redo of data edits.
    history: EditHistory,
    /// Cell cursor and in-progress edits in the data grid.
    grid: GridState,
    /// How the table was made, when a table operation made it, with its
    /// sources' ids and fingerprints when they are in the same project.
    lineage: Option<TableLineage>,
    /// Lineages replaced by "Update", most recent last: undoing an update
    /// puts the lineage back with the data.
    lineage_undo: Vec<TableLineage>,
    /// Lineages of undone updates, for redo.
    lineage_redo: Vec<TableLineage>,
    /// Incremented by every data edit, undo and redo (so a script's changes
    /// are only merged into data it saw).
    revision: u64,
    /// The database or web source the table was read from (saved in projects).
    external: Option<app_model::external::ExternalSource>,
    /// Its password and secret header values, for this session only.
    secrets: Option<app_model::external::Secrets>,
}

impl Document {
    /// A new document for `table` (its project and id are set when it is added).
    fn new(table: DataTable, rows: RowStateManager, reports: Vec<Report>, workspace: Workspace) -> Self {
        Self {
            id: 0,
            project: 0,
            table_id: TableId::from("table-1"),
            table: Arc::new(table),
            rows,
            reports,
            workspace,
            source_path: None,
            history: EditHistory::default(),
            grid: GridState::default(),
            lineage: None,
            lineage_undo: Vec::new(),
            lineage_redo: Vec::new(),
            revision: 0,
            external: None,
            secrets: None,
        }
    }

    fn name(&self) -> &str {
        self.table.name()
    }

    /// The operation that made the table, if one did.
    fn origin(&self) -> Option<&TableOrigin> {
        self.lineage.as_ref().map(|l| &l.origin)
    }

    /// The table alone as a one-table project carrying `scripts`.
    fn project(&self, scripts: &[ProjectScript]) -> Project {
        Project {
            table: (*self.table).clone(),
            rows: self.rows.clone(),
            reports: self.reports.iter().map(Report::saved).collect(),
            origin: self.origin().cloned(),
            external: self.external.clone(),
            scripts: scripts.to_vec(),
        }
    }

    /// The table as it is written in its project's file.
    fn saved(&self) -> ProjectTable {
        ProjectTable {
            id: self.table_id.clone(),
            table: (*self.table).clone(),
            rows: self.rows.clone(),
            reports: self.reports.iter().map(Report::saved).collect(),
            external: self.external.clone(),
            lineage: self.lineage.clone(),
        }
    }

    /// The view a table opens in when its project records none.
    fn default_workspace(&self) -> Workspace {
        if self.reports.is_empty() { Workspace::Data } else { Workspace::SideBySide }
    }

    fn tooltip(&self, project: Option<&OpenProject>) -> String {
        let c = self.rows.counts();
        let mut text = format!(
            "{} × {} · {} reports",
            count(c.total, "row", "rows"),
            count(self.table.n_cols(), "column", "columns"),
            self.reports.len()
        );
        if let Some(origin) = self.origin() {
            text.push_str(&format!("\n{}", origin.describe()));
        }
        if let Some(source) = &self.external {
            text.push_str(&format!("\nRead from {}", source.describe()));
        }
        match project {
            Some(p) if p.sample => {
                text.push_str("\nBuilt-in sample: saving creates a new project file; the sample is never changed");
            }
            Some(OpenProject { path: Some(path), .. }) => text.push_str(&format!("\nProject: {}", path.display())),
            _ => {
                if let Some(p) = &self.source_path {
                    text.push_str(&format!("\nImported from: {}", p.display()));
                }
            }
        }
        text
    }
}

/// Something the user asked for this frame, carried out once the UI pass is done.
/// Where a table added to an open project comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AddSource {
    /// An empty table.
    Blank,
    /// The text on the clipboard (through the Open Data dialog).
    Clipboard,
    /// A data file the user picks.
    File,
}

enum Action {
    PickAndOpen,
    /// Give a table or project the name typed in place.
    Rename(renaming::RenameTarget, String),
    OpenPaths(Vec<PathBuf>),
    /// Open one of the built-in manuals in the system's PDF viewer.
    OpenManual(manuals::Manual),
    /// Open entry `n` of the sample catalog with its reports.
    OpenSample(usize),
    /// Open an empty data table.
    NewTable,
    /// Make a new table from the text on the system clipboard.
    NewTableFromClipboard,
    /// Add a table to project `0` from `1` (File ▸ Add to this project).
    AddToProject(u64, AddSource),
    /// Copy from the active grid; `rows_only`: the selected rows with column names.
    Copy {
        rows_only: bool,
    },
    /// Paste the system clipboard into the active grid.
    Paste,
    /// Copy the grid's selection, then clear it.
    Cut,
    /// Clear the grid's selected cells.
    ClearCells,
    /// Fill the grid's block down from its first row.
    FillDown,
    /// Select every cell of the active grid.
    SelectAllCells,
    /// Open the active grid's Find bar.
    Find,
    /// Put back the report removed last.
    UndoRemoveReport,
    Activate(usize),
    Save(usize),
    SaveAs(usize),
    /// File ▸ Save project as template… for document `n`'s project.
    SaveTemplate(usize),
    /// File ▸ New project from template….
    NewFromTemplate,
    /// Export a table's data (`true`: labelled columns written as their
    /// value labels).
    Export(usize, ExportFormat, bool),
    /// File ▸ Export data as ▸ Image… for document `n`.
    ExportImage(usize),
    /// Save every report of document `n` on one HTML page.
    ExportReports(usize),
    /// Close document `n`: the whole project when it is the project's only
    /// table, otherwise just that table (removed from its project).
    Close(usize),
    /// Close every table of an open project (by id), asking first if it
    /// has unsaved changes.
    CloseProject(u64),
    /// Change the folders of open project `.0` (by id).
    Folder(u64, folders::FolderOp),
    /// Show report `report` of document `doc` (both by id).
    ShowReport {
        doc: u64,
        report: u64,
    },
    /// Open notebook `.1` of project `.0` (by id) in its window.
    OpenNotebook(u64, String),
    /// Make derived table `doc` (by id) again from its sources' current
    /// data, as one undoable edit.
    UpdateFromSource(u64),
    /// Edits (or undo / redo) for the active document's data.
    Grid(Vec<GridRequest>),
    /// Edits made in the grid of document `.0` as drawn in the main window:
    /// that document becomes the command target first, so keyboard edits
    /// never land in another table that was the target (a pop-out window
    /// used last).
    GridIn(usize, Vec<GridRequest>),
    /// Add an empty graph canvas to the active document.
    NewGraph,
    /// Add a graph read from a pipeline file to the active document.
    GraphFromPipeline,
    /// Ask for a script file and show it in the Script window.
    OpenScript,
    /// Do something with script `script` kept in document `doc`'s project.
    ProjectScript {
        doc: usize,
        script: usize,
        op: ProjectScriptOp,
    },
    /// Something asked for in the sidebar's script library.
    Library(LibraryRequest),
    /// Show view `view` of document `doc` in a window of its own (or bring
    /// that window to the front if it already has one).
    PopOut {
        doc: u64,
        view: View,
    },
    /// Close the window of view `view` of document `doc` and show the view
    /// in the main window again.
    BringBack {
        doc: u64,
        view: View,
    },
    /// Bring the window of view `view` of document `doc` to the front.
    ShowWindow {
        doc: u64,
        view: View,
    },
    /// Open entry `n` of the sample projects (several tables, a dashboard).
    OpenProjectSample(usize),
    /// Add an empty dashboard to open project `n` (by id) and show it.
    NewDashboard(u64),
    /// Show dashboard `n` (by id) in the main window, or bring its window forward.
    ShowDashboard(u64),
    /// Show dashboard `n` in a window of its own.
    DashboardWindow(u64),
    /// Undo the latest edit of dashboard `n`.
    DashboardUndo(u64),
    /// Redo the latest undone edit of dashboard `n`.
    DashboardRedo(u64),
    /// Save dashboard `n` as one HTML page.
    ExportDashboard(u64),
    /// Take dashboard `n` out of its project.
    RemoveDashboard(u64),
    /// Open the dialog of a start-page task (or ask for data first).
    StartTask(welcome::Task),
    /// Ask for data files of one kind and open them.
    OpenOfKind(file_io::FileKind),
    /// Open the database or web import dialog.
    ImportExternal(crate::external_dialog::ExternalKind),
    /// Data ▸ Rows ▸ Use row as column names: the cursor's row (else the one
    /// selected row) becomes the column names.
    UseRowAsNames,
}

/// Each report's id and saved form, for the recorder.
/// A new report on a stacked imputation table pools across the imputations
/// when it can (turned off in its options menu); others are left as they are.
fn pooled_on_imputations(saved: app_model::SavedReport, table: &DataTable) -> app_model::SavedReport {
    let Some(column) = app_model::multiple_imputation::imputation_column(table) else { return saved };
    if !app_model::pooling::can_pool(&saved) || saved.scope().is_some_and(|s| s.pool.is_some()) {
        return saved;
    }
    let (inner, mut scope) = saved.split_scope();
    scope.pool = table.column(column).map(|c| c.name.clone());
    inner.with_scope(scope)
}

fn report_ids(reports: &[Report]) -> Vec<(u64, app_model::SavedReport)> {
    reports.iter().map(|r| (r.id(), r.saved())).collect()
}

/// Row states a script set on an open document (each `None` if untouched).
struct RowChanges {
    selected: Option<Vec<usize>>,
    excluded: Option<Vec<usize>>,
    hidden: Option<Vec<usize>>,
    colors: Option<app_model::action::RowStyles>,
    subsets: Option<app_model::action::RowSubsets>,
}

/// What to do with a script kept in a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectScriptOp {
    Open,
    Run,
    ToggleRunOnOpen,
    Remove,
}

/// What happens after the "unsaved changes" question for a closing document.
enum CloseChoice {
    Save,
    SaveAs,
    Discard,
    Cancel,
}

pub struct StatsApp {
    /// Open tables. The tables of one project are kept next to each other,
    /// in the project's order.
    documents: Vec<Document>,
    /// Open projects: each holds one or more of the documents.
    projects: Vec<OpenProject>,
    next_project_id: u64,
    /// For each derived table (by document id) whose sources are in its
    /// project: which sources changed since it was made.
    stale: HashMap<u64, StaleCheck>,
    /// Table (by document id) waiting for confirmation to be removed from
    /// its project, because other tables were made from it.
    confirm_remove: Option<u64>,
    /// The project that tables being opened now join (File ▸ Add to this
    /// project, or the Open Data dialog's box), instead of each starting a
    /// project of its own.
    into_project: Option<u64>,
    /// Why the columns asked to be deleted were kept (they are used by
    /// reports or dashboard tiles), shown in a box until dismissed.
    column_refusal: Option<String>,
    /// Dashboard (by id) waiting for confirmation to be removed from its project.
    confirm_dashboard_removal: Option<u64>,
    /// The dashboard removed last, which Edit ▸ Undo puts back until the
    /// next edit of a table.
    removed_dashboard: Option<dashboards::RemovedDashboard>,
    /// The open "Export reports" dialog.
    export_dialog: Option<crate::report_export_ui::ExportDialog>,
    /// The window the export dialog shows in: `None` for the main window.
    export_window: Option<(u64, View)>,
    /// The window whose keys or buttons asked for the action being carried
    /// out: `None` for the main window.
    acting_window: Option<(u64, View)>,
    /// The command target: the document menus, dialogs, the sidebar and the
    /// status bar act on (the document of the window used last).
    active: usize,
    /// The document the main window shows (its tabs, grid and results).
    shown: usize,
    /// The dashboard the main window shows in place of document `shown`
    /// (by id), if one is shown.
    shown_dashboard: Option<u64>,
    /// The dashboard the main window drew this frame: its window (if it
    /// was just popped out) waits a frame, so no widget is drawn twice.
    drawn_dashboard: Option<u64>,
    next_dashboard_id: u64,
    /// Views shown in windows of their own.
    windows: Vec<ViewWindow>,
    /// The window used last: `None` for the main window.
    last_window: Option<(u64, View)>,
    /// The main window had keyboard focus last frame (native windows only).
    main_focused: bool,
    /// The main window got focus and has not been clicked since: a shortcut
    /// pressed now acts on the document it shows.
    main_focus_pending: bool,
    /// Where the main window's workspace (view tabs, grid and results) was
    /// last frame: a press there makes its document the command target.
    workspace_rect: Option<egui::Rect>,
    /// How many dialogs were open last frame (a new one brings the main
    /// window, where dialogs open, to the front).
    open_dialogs: usize,
    next_document_id: u64,
    /// Report ids are unique across documents so egui state never leaks between tabs.
    next_report_id: u64,
    dialog: Option<ModelDialog>,
    chart_dialog: Option<ChartDialog>,
    /// Attribute control chart dialog.
    attribute_dialog: Option<AttributeDialog>,
    /// Pareto chart dialog.
    pareto_dialog: Option<ParetoDialog>,
    /// Cause-and-effect diagram dialog.
    cause_effect_dialog: Option<CauseEffectDialog>,
    /// Multi-vari chart dialog.
    multi_vari_dialog: Option<MultiVariDialog>,
    /// Multivariate control chart dialog.
    multivariate_dialog: Option<MultivariateDialog>,
    /// Analysis of means dialog.
    anom_dialog: Option<AnomDialog>,
    regression_dialog: Option<RegressionDialog>,
    /// Mixture model dialog.
    mixture_dialog: Option<MixtureDialog>,
    /// Mixed model dialog.
    mixed_dialog: Option<crate::mixed_dialog::MixedDialog>,
    /// Nonlinear regression dialog.
    nonlinear_dialog: Option<crate::nonlinear_dialog::NonlinearDialog>,
    /// Assay curve template dialog.
    curve_template_dialog: Option<crate::curve_template_dialog::CurveTemplateDialog>,
    tree_dialog: Option<crate::tree_dialog::TreeDialog>,
    forest_dialog: Option<crate::forest_dialog::ForestDialog>,
    boost_dialog: Option<crate::boost_dialog::BoostDialog>,
    pls_dialog: Option<crate::pls_dialog::PlsDialog>,
    svm_dialog: Option<crate::svm_dialog::SvmDialog>,
    mars_dialog: Option<crate::mars_dialog::MarsDialog>,
    neural_dialog: Option<crate::neural_dialog::NeuralDialog>,
    gaussian_process_dialog: Option<crate::gaussian_process_dialog::GaussianProcessDialog>,
    search_dialog: Option<crate::search_dialog::SearchDialog>,
    /// Functional data dialog.
    functional_dialog: Option<crate::functional_dialog::FunctionalDialog>,
    model_comparison_dialog: Option<crate::comparison_dialog::ComparisonDialog>,
    mixture_design_dialog: Option<MixtureDesignDialog>,
    /// Process capability dialog.
    capability_dialog: Option<CapabilityDialog>,
    /// Attribute capability, between/within capability or acceptance sampling.
    quality_tool_dialog: Option<QualityToolDialog>,
    /// Principal-components control chart or monitoring limits.
    monitoring_dialog: Option<crate::monitoring_dialog::MonitoringDialog>,
    /// Gauge linearity, repeatability, intraclass correlation, signal-to-noise or design declaration.
    study_dialog: Option<StudyDialog>,
    /// Attribute agreement or go/no-go gauge study.
    agreement_dialog: Option<AgreementDialog>,
    /// Bayesian estimation, Bayes factor, linear model or regression dialog.
    bayes_dialog: Option<crate::bayes_dialog::BayesDialog>,
    /// Treatment effects from observational data.
    treatment_dialog: Option<crate::treatment_dialog::TreatmentDialog>,
    signal_dialog: Option<crate::signal_dialog::SignalDialog>,
    /// Design evaluation (Design ▸ This table).
    evaluation_dialog: Option<crate::evaluation_dialog::EvaluationDialog>,
    permutation_dialog: Option<crate::resampling_dialogs::PermutationDialog>,
    bootstrap_dialog: Option<crate::resampling_dialogs::BootstrapDialog>,
    /// Gauge study worksheet or robust (inner/outer array) design.
    sheet_dialog: Option<SheetDialog>,
    /// Multi-way table or stratified contingency analysis.
    tabulation_dialog: Option<crate::tabulation_dialog::TabulationDialog>,
    /// Factorial analysis of a two-level experiment.
    factorial_dialog: Option<crate::factorial_dialog::FactorialDialog>,
    sequential_dialog: Option<crate::sequential_dialog::SequentialDialog>,
    plot3d_dialog: Option<crate::plot3d_dialog::Plot3dDialog>,
    /// EWMA / CUSUM chart dialog.
    time_weighted_dialog: Option<TimeWeightedDialog>,
    gauge_dialog: Option<GaugeDialog>,
    distribution_dialog: Option<DistributionDialog>,
    distribution_fit_dialog: Option<DistributionFitDialog>,
    bivariate_dialog: Option<BivariateDialog>,
    /// The "create a designed experiment" dialog (independent of any dataset).
    design_dialog: Option<DesignDialog>,
    /// The "augment the design in this table" dialog (creates a new table).
    augment_dialog: Option<AugmentDialog>,
    /// The "simulate responses for this design" dialog.
    simulate_dialog: Option<crate::simulate_dialog::SimulateDialog>,
    /// Principal components, clustering, GLM or penalized regression dialog.
    phase2_dialog: Option<Phase2Dialog>,
    /// Correlations, scatterplot matrix, outlier distances or item analysis dialog.
    method_dialog: Option<crate::multivariate_methods_dialog::MethodDialog>,
    factor_dialog: Option<crate::factor_dialog::FactorDialog>,
    discriminant_dialog: Option<crate::discriminant_dialog::DiscriminantDialog>,
    /// Gaussian mixture or dimension reduction dialog.
    map_dialog: Option<crate::cluster_map_dialog::MapDialog>,
    text_dialog: Option<crate::text_dialog::TextDialog>,
    imputation_dialog: Option<crate::imputation_dialog::ImputationDialog>,
    /// Series and Signals ▸ Explore dialogs.
    series_explore_dialog: Option<crate::series_explore_dialog::SeriesExploreDialog>,
    signal_processing_dialog: Option<crate::signal_processing_dialog::SignalProcessingDialog>,
    image_analysis_dialog: Option<crate::image_analysis_dialog::ImageAnalysisDialog>,
    external_dialog: Option<crate::external_dialog::ExternalDialog>,
    /// Open Data dialogs waiting to be shown, the one in front first (one
    /// per file when several are opened at once).
    open_data: VecDeque<crate::open_data_dialog::OpenDataDialog>,
    /// Open image dialogs waiting to be shown, the one in front first.
    image_open: VecDeque<crate::image_dialog::ImageOpenDialog>,
    /// File ▸ Export data as ▸ Image…, when open.
    image_export: Option<crate::image_dialog::ImageExportDialog>,
    /// The Duplicate on another table dialog, when open.
    duplicate: Option<reuse::DuplicateDialog>,
    /// The New project from template dialog, when open.
    template: Option<reuse::TemplateDialog>,
    /// The `open_table` options of tables being opened from the Open Data
    /// dialog, by table name, for the recording.
    opening_options: Vec<(String, stat_data::JsonValue)>,
    /// Survival analysis dialog.
    survival_dialog: Option<SurvivalDialog>,
    reliability_dialog: Option<ReliabilityDialog>,
    forecast_dialog: Option<crate::forecast_dialog::ForecastDialog>,
    /// Comparison, proportion, rate or power dialog.
    compare_dialog: Option<CompareDialog>,
    /// Table operation dialog (Data ▸ Rows, Reshape, Merge, Summarize).
    table_op_dialog: Option<TableOpDialog>,
    /// The formula editor, for a column of the document it names.
    formula_dialog: Option<FormulaDialog>,
    /// User preferences (saved to the settings file when changed).
    settings: AppSettings,
    /// The Settings window is open.
    settings_open: bool,
    /// The Help window that is open, if any.
    help_window: Option<HelpWindow>,
    /// Unsaved-change marks of the projects, by project id.
    modified_marks: HashMap<u64, ModifiedMark>,
    /// Text typed in the sidebar's variable filter box.
    variable_filter: String,
    /// The Recode dialog, while open.
    recode_dialog: Option<table_view::recode::RecodeDialog>,
    value_labels_dialog: Option<table_view::value_labels::ValueLabelsDialog>,
    color_by_dialog: Option<row_styles::ColorByDialog>,
    rank_dialog: Option<table_view::rank::RankDialog>,
    /// The last settings of each dialog, per document, while the app runs.
    dialog_memory: dialog_kit::Memory,
    /// The report removed last, which the status bar offers to put back.
    removed_report: Option<RemovedReport>,
    /// Width of the results pane in side-by-side view (set by dragging the divider).
    results_width: Option<f32>,
    /// Project (by id) awaiting an answer about unsaved changes before it closes.
    confirm_close: Option<u64>,
    /// Quitting: each document with unsaved changes is asked about in turn.
    quitting: bool,
    /// Every unsaved document has been dealt with; let the window close.
    quit_confirmed: bool,
    /// Status-bar message and whether it reports an error.
    message: Option<(String, bool)>,
    /// Window title last sent, so it is only sent when it changes.
    title: String,
    /// The Script window.
    script: ScriptWindow,
    /// Open script notebook windows.
    notebook_windows: Vec<crate::notebook_window::NotebookWindow>,
    /// Naming a saved row subset (Data ▸ Row subsets).
    subset_prompt: Option<subsets::SubsetPrompt>,
    /// Number for the next notebook window (widget ids).
    next_notebook_serial: u64,
    /// Scripts ▸ Run script over a folder.
    batch: BatchWindow,
    /// Visualize ▸ Figure page…: plots gathered on one page.
    pub(crate) figure_page: FigurePageWindow,
    /// The sidebar's list of scripts in the scripts folder.
    script_library: ScriptLibrary,
    /// Data revision of each document (by id) when the running script started.
    script_revisions: Vec<(u64, u64)>,
    /// Report ids of each document (by id) when the running script started,
    /// in list order: the script names reports by these positions, which
    /// shift if reports are added or removed while it runs.
    script_reports: HashMap<u64, Vec<u64>>,
    /// Just-opened projects (document ids) whose run-on-open scripts wait
    /// for the user's go-ahead, asked about one at a time.
    ask_project_scripts: VecDeque<u64>,
    /// Project scripts waiting to run, one after another: (document id,
    /// script name, source).
    script_queue: VecDeque<(u64, String, String)>,
    /// Scripts ▸ Record actions: the script being written, while recording.
    recorder: Option<Recorder>,
    /// Scripts ▸ Stop recording was chosen this frame.
    stop_recording_requested: bool,
    /// How long the last frame took to build (shown in the status bar when
    /// Settings ▸ Show frame time is on).
    last_frame_time: std::time::Duration,
    /// Workers computing reports in the background (`None`: reports compute
    /// in the frame, as the headless tests and the web viewer do).
    report_jobs: Option<report_view::ReportJobs>,
    /// How the next document added was opened (for the recording).
    opening: Option<OpenedFrom>,
    /// The project notes are in a window of their own instead of the sidebar.
    notes_window: bool,
    /// A start-page task waiting for a table to open.
    pending_task: Option<welcome::Task>,
    /// Set when the app runs as the read-only dashboard viewer.
    viewer: Option<viewer::Viewer>,
    /// The "Build the web viewer" window, open when Publish as web app found no viewer.
    viewer_build: Option<viewer_build::ViewerBuild>,
    /// This run's recovery saves (`None` in the viewer, tests and the web build).
    recovery: Option<recovery::RecoveryWriter>,
    /// The offer to restore work left by a run that ended abnormally.
    recovery_offer: Option<recovery::RecoveryOffer>,
}

impl StatsApp {
    /// Opens each path given (or the sample data when there are none).
    pub fn new(cc: &eframe::CreationContext<'_>, #[allow(unused_mut)] mut paths: Vec<PathBuf>) -> Self {
        // Files opened from Finder before the window existed (macOS).
        #[cfg(target_os = "macos")]
        {
            crate::macos_open::set_context(&cc.egui_ctx);
            paths.extend(crate::macos_open::take_pending());
        }
        theme::install_fonts(&cc.egui_ctx);
        theme::apply_window_style(&cc.egui_ctx);
        let settings = load_settings();
        apply_color_scheme(&cc.egui_ctx, settings.color_scheme);
        theme::apply_text_scale(&cc.egui_ctx, settings.text_size.scale());
        theme::apply_tooltip_delay(&cc.egui_ctx, settings.tooltip_delay.seconds());
        let mut app = Self {
            documents: Vec::new(),
            projects: Vec::new(),
            next_project_id: 0,
            stale: HashMap::new(),
            confirm_remove: None,
            into_project: None,
            column_refusal: None,
            confirm_dashboard_removal: None,
            removed_dashboard: None,
            export_dialog: None,
            export_window: None,
            acting_window: None,
            active: 0,
            shown: 0,
            shown_dashboard: None,
            drawn_dashboard: None,
            next_dashboard_id: 0,
            windows: Vec::new(),
            last_window: None,
            main_focused: true,
            main_focus_pending: false,
            workspace_rect: None,
            open_dialogs: 0,
            next_document_id: 0,
            next_report_id: 0,
            dialog: None,
            chart_dialog: None,
            capability_dialog: None,
            quality_tool_dialog: None,
            monitoring_dialog: None,
            study_dialog: None,
            agreement_dialog: None,
            bayes_dialog: None,
            treatment_dialog: None,
            signal_dialog: None,
            evaluation_dialog: None,
            permutation_dialog: None,
            bootstrap_dialog: None,
            sheet_dialog: None,
            tabulation_dialog: None,
            factorial_dialog: None,
            sequential_dialog: None,
            plot3d_dialog: None,
            mixture_dialog: None,
            mixture_design_dialog: None,
            attribute_dialog: None,
            pareto_dialog: None,
            cause_effect_dialog: None,
            multi_vari_dialog: None,
            multivariate_dialog: None,
            anom_dialog: None,
            regression_dialog: None,
            time_weighted_dialog: None,
            gauge_dialog: None,
            distribution_dialog: None,
            distribution_fit_dialog: None,
            bivariate_dialog: None,
            design_dialog: None,
            augment_dialog: None,
            simulate_dialog: None,
            phase2_dialog: None,
            method_dialog: None,
            factor_dialog: None,
            discriminant_dialog: None,
            map_dialog: None,
            text_dialog: None,
            imputation_dialog: None,
            series_explore_dialog: None,
            signal_processing_dialog: None,
            image_analysis_dialog: None,
            external_dialog: None,
            open_data: VecDeque::new(),
            image_open: VecDeque::new(),
            image_export: None,
            duplicate: None,
            template: None,
            opening_options: Vec::new(),
            mixed_dialog: None,
            nonlinear_dialog: None,
            curve_template_dialog: None,
            tree_dialog: None,
            forest_dialog: None,
            boost_dialog: None,
            pls_dialog: None,
            svm_dialog: None,
            mars_dialog: None,
            neural_dialog: None,
            gaussian_process_dialog: None,
            search_dialog: None,
            functional_dialog: None,
            model_comparison_dialog: None,
            survival_dialog: None,
            reliability_dialog: None,
            forecast_dialog: None,
            compare_dialog: None,
            table_op_dialog: None,
            formula_dialog: None,
            settings,
            settings_open: false,
            help_window: None,
            modified_marks: HashMap::new(),
            variable_filter: String::new(),
            recode_dialog: None,
            value_labels_dialog: None,
            color_by_dialog: None,
            rank_dialog: None,
            dialog_memory: dialog_kit::Memory::default(),
            removed_report: None,
            results_width: None,
            confirm_close: None,
            quitting: false,
            quit_confirmed: false,
            message: None,
            title: String::new(),
            script: ScriptWindow::default(),
            notebook_windows: Vec::new(),
            subset_prompt: None,
            next_notebook_serial: 1,
            batch: BatchWindow::default(),
            figure_page: FigurePageWindow::default(),
            script_library: ScriptLibrary::default(),
            script_revisions: Vec::new(),
            script_reports: HashMap::new(),
            ask_project_scripts: VecDeque::new(),
            script_queue: VecDeque::new(),
            recorder: None,
            stop_recording_requested: false,
            last_frame_time: std::time::Duration::ZERO,
            report_jobs: (!cfg!(test) && !cfg!(target_arch = "wasm32"))
                .then(|| report_view::ReportJobs::new(&cc.egui_ctx)),
            opening: None,
            notes_window: false,
            pending_task: None,
            viewer: None,
            viewer_build: None,
            recovery: None,
            recovery_offer: None,
        };
        // Recovery saves start before anything opens, so opened projects
        // count as saved.
        #[cfg(not(any(test, target_arch = "wasm32")))]
        app.start_recovery();
        // With nothing to open (or nothing that opened), the start page shows.
        if !paths.is_empty() {
            app.open_paths(paths);
        }
        app
    }

    /// Show document `index` in the main window and make it the command target.
    fn set_active(&mut self, index: usize) {
        self.set_target(index);
        self.shown = self.active;
        self.shown_dashboard = None;
    }

    /// Make document `index` the one menus, dialogs and shortcuts act on,
    /// without changing what the main window shows. Open dialogs belong to
    /// the previous target, so they close (their settings are remembered).
    fn set_target(&mut self, index: usize) {
        if index != self.active {
            self.remember_open_dialogs();
            self.close_dialogs();
        }
        self.active = index.min(self.documents.len().saturating_sub(1));
    }

    /// Keep the settings of every open dialog for the active document (the
    /// one they belong to), so reopening a dialog there after a switch to
    /// another document restores them.
    fn remember_open_dialogs(&mut self) {
        let Some(doc) = self.documents.get(self.active) else { return };
        let (id, table) = (doc.id, &doc.table);
        let memory = &mut self.dialog_memory;
        macro_rules! keep {
            ($($slot:ident),* $(,)?) => {
                $(if let Some(d) = &self.$slot {
                    memory.keep(id, d, Some(table));
                })*
            };
        }
        keep!(
            table_op_dialog,
            phase2_dialog,
            method_dialog,
            factor_dialog,
            discriminant_dialog,
            map_dialog,
            text_dialog,
            imputation_dialog,
            series_explore_dialog,
            signal_processing_dialog,
            image_analysis_dialog,
            mixed_dialog,
            nonlinear_dialog,
            curve_template_dialog,
            tree_dialog,
            forest_dialog,
            boost_dialog,
            pls_dialog,
            svm_dialog,
            mars_dialog,
            neural_dialog,
            gaussian_process_dialog,
            search_dialog,
            functional_dialog,
            model_comparison_dialog,
            survival_dialog,
            reliability_dialog,
            forecast_dialog,
            compare_dialog,
            dialog,
            chart_dialog,
            capability_dialog,
            quality_tool_dialog,
            monitoring_dialog,
            study_dialog,
            agreement_dialog,
            bayes_dialog,
            signal_dialog, treatment_dialog,
            evaluation_dialog,
            tabulation_dialog,
            factorial_dialog,
            sequential_dialog,
            plot3d_dialog,
            permutation_dialog,
            bootstrap_dialog,
            mixture_dialog,
            augment_dialog,
            simulate_dialog,
            attribute_dialog,
            pareto_dialog,
            cause_effect_dialog,
            multi_vari_dialog,
            multivariate_dialog,
            anom_dialog,
            regression_dialog,
            time_weighted_dialog,
            gauge_dialog,
            distribution_dialog,
            distribution_fit_dialog,
            bivariate_dialog,
        );
    }

    fn close_dialogs(&mut self) {
        self.table_op_dialog = None;
        self.formula_dialog = None;
        self.rank_dialog = None;
        self.phase2_dialog = None;
        self.method_dialog = None;
        self.factor_dialog = None;
        self.discriminant_dialog = None;
        self.map_dialog = None;
        self.text_dialog = None;
        self.imputation_dialog = None;
        self.series_explore_dialog = None;
        self.signal_processing_dialog = None;
        self.image_analysis_dialog = None;
        self.mixed_dialog = None;
        self.nonlinear_dialog = None;
        self.curve_template_dialog = None;
        self.tree_dialog = None;
        self.forest_dialog = None;
        self.boost_dialog = None;
        self.pls_dialog = None;
        self.svm_dialog = None;
        self.mars_dialog = None;
        self.neural_dialog = None;
        self.gaussian_process_dialog = None;
        self.search_dialog = None;
        self.functional_dialog = None;
        self.model_comparison_dialog = None;
        self.survival_dialog = None;
        self.reliability_dialog = None;
        self.forecast_dialog = None;
        self.compare_dialog = None;
        self.dialog = None;
        self.chart_dialog = None;
        self.capability_dialog = None;
        self.quality_tool_dialog = None;
        self.monitoring_dialog = None;
        self.study_dialog = None;
        self.agreement_dialog = None;
        self.bayes_dialog = None;
        self.treatment_dialog = None;
        self.signal_dialog = None;
        self.evaluation_dialog = None;
        self.tabulation_dialog = None;
        self.factorial_dialog = None;
        self.sequential_dialog = None;
        self.plot3d_dialog = None;
        self.permutation_dialog = None;
        self.bootstrap_dialog = None;
        self.mixture_dialog = None;
        self.augment_dialog = None;
        self.simulate_dialog = None;
        self.attribute_dialog = None;
        self.pareto_dialog = None;
        self.cause_effect_dialog = None;
        self.multi_vari_dialog = None;
        self.multivariate_dialog = None;
        self.anom_dialog = None;
        self.regression_dialog = None;
        self.time_weighted_dialog = None;
        self.gauge_dialog = None;
        self.distribution_dialog = None;
        self.distribution_fit_dialog = None;
        self.bivariate_dialog = None;
    }

    /// Apply the current settings and save them for the next session.
    fn settings_changed(&mut self, ctx: &egui::Context) {
        apply_color_scheme(ctx, self.settings.color_scheme);
        theme::apply_text_scale(ctx, self.settings.text_size.scale());
        theme::apply_tooltip_delay(ctx, self.settings.tooltip_delay.seconds());
        if let Err(e) = save_settings(&self.settings) {
            self.error(format!("Settings apply now but could not be saved for next time: {e}"));
        }
    }

    /// The Settings window (File ▸ Settings…).
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                ui.label(RichText::new("Appearance").strong());
                ui.label(RichText::new("Colour scheme of the whole window, data grid and plots.").weak().small());
                changed = color_scheme_choices(ui, &mut self.settings.color_scheme);
                ui.add_space(8.0);
                ui.label(RichText::new("Text size").strong());
                ui.label(
                    RichText::new("Size of all text: menus, dialogs, the data grid, reports and plot labels.")
                        .weak()
                        .small(),
                );
                changed |= text_size_choices(ui, &mut self.settings.text_size);
                ui.add_space(8.0);
                ui.label(RichText::new("Tooltip delay").strong());
                ui.label(
                    RichText::new("How long the pointer must rest on a control before its explanation appears.")
                        .weak()
                        .small(),
                );
                changed |= tooltip_delay_choices(ui, &mut self.settings.tooltip_delay);
                ui.add_space(8.0);
                ui.label(RichText::new("Reports").strong());
                changed |= ui
                    .checkbox(&mut self.settings.mark_significant, "Mark p-values below 0.05 with ★")
                    .on_hover_text("In report tables, p-value cells below 0.05 get a ★ and strong text")
                    .changed();
                changed |= ui
                    .checkbox(&mut self.settings.group_thousands, "Separate thousands with commas (184,150,146)")
                    .on_hover_text(
                        "In report tables, numbers with five or more digits before the decimal point; copied \
                         tables keep the plain numbers",
                    )
                    .changed();
                changed |= ui
                    .checkbox(&mut self.settings.align_decimals, "Line up decimal points in each column")
                    .on_hover_text("In report tables, numbers in a column share one decimal-point position")
                    .changed();
                let mut tips = !self.settings.tips_dismissed;
                if ui
                    .checkbox(&mut tips, "Show getting-started tips")
                    .on_hover_text("The tips above the data grid, and the line on how to work plots above the reports")
                    .changed()
                {
                    self.settings.tips_dismissed = !tips;
                    changed = true;
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Opening data").strong());
                changed |= ui
                    .checkbox(
                        &mut self.settings.open_plain_directly,
                        "Open files directly when the table starts in the top-left cell",
                    )
                    .on_hover_text(
                        "Skip the Open Data dialog for text files and workbooks whose first row holds the column \
                         names and whose values start right below, and the Open image dialog for grey images within \
                         1024 × 1024 pixels; other files still show it",
                    )
                    .changed();
                ui.add_space(8.0);
                ui.label(RichText::new("Troubleshooting").strong());
                changed |= ui
                    .checkbox(&mut self.settings.show_frame_time, "Show frame time")
                    .on_hover_text(
                        "The status bar shows how long the window took to draw last time, and which report took \
                         longest; useful when the app feels slow",
                    )
                    .changed();
                ui.add_space(4.0);
                if let Some(path) = app_model::settings::settings_path() {
                    ui.label(RichText::new(format!("Saved in {}", path.display())).weak().small());
                }
            });
        if changed {
            self.settings_changed(ctx);
        }
        if !open {
            self.settings_open = false;
        }
    }

    fn info(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), false));
    }

    fn error(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), true));
    }

    /// Add `doc` as a new project of its own (saved as it is now) and show it.
    fn push_document(&mut self, doc: Document) {
        self.push_documents(vec![doc], NewProject::default());
    }

    /// Add `docs` as the tables of one new project and show the first.
    /// Unless `new.unsaved`, the project counts as saved as it is now.
    fn push_documents(&mut self, docs: Vec<Document>, new: NewProject) {
        // An untouched empty "Untitled" table has served its purpose once
        // anything else opens (a file, sample, design or new table of results).
        let pristine_new = docs.len() == 1
            && new.path.is_none()
            && !new.sample
            && docs.iter().all(|d| d.table.n_rows() == 0 && d.reports.is_empty() && d.source_path.is_none());
        if !pristine_new {
            while let Some(i) = self.pristine_document() {
                self.close(i);
            }
        }
        let project = self.next_project_id;
        self.next_project_id += 1;
        let NewProject { path, title, name, sample, scripts, unsaved, dashboards, notes, notebooks } = new;
        let dashboards = dashboards
            .into_iter()
            .map(|d| {
                self.next_dashboard_id += 1;
                dashboards::OpenDashboard::new(self.next_dashboard_id - 1, d)
            })
            .collect();
        self.projects.push(OpenProject {
            id: project,
            path,
            title,
            name,
            sample,
            scripts,
            saved_state: None,
            collapsed: false,
            tabs_folded: false,
            dashboards,
            notes,
            notebooks,
            folders: Vec::new(),
        });
        let first = self.documents.len();
        for mut doc in docs {
            doc.project = project;
            // Tables read from a file keep their ids; new ones get the next free one.
            if doc.table_id.as_str().is_empty() || self.documents[first..].iter().any(|d| d.table_id == doc.table_id) {
                doc.table_id = self.unused_table_id(project);
            }
            self.register_document(&mut doc);
            self.documents.push(doc);
        }
        if !unsaved {
            self.mark_project_saved(project);
        }
        self.set_active(first);
    }

    /// Give `doc` its application-wide id and tell the recorder about it.
    fn register_document(&mut self, doc: &mut Document) {
        doc.id = self.next_document_id;
        self.next_document_id += 1;
        let from = self.opening.take().unwrap_or(OpenedFrom::Data);
        if let Some(recorder) = &mut self.recorder {
            doc.history.set_logging(true);
            recorder.opened(doc.id, from, &doc.table, &doc.rows, &report_ids(&doc.reports));
        }
    }

    /// Scripts ▸ Record actions: start writing a script of what is done from
    /// now on, following every open table.
    fn start_recording(&mut self) {
        let mut recorder = Recorder::new();
        let names: Vec<String> = self.documents.iter().map(|d| d.name().to_owned()).collect();
        for (i, doc) in self.documents.iter_mut().enumerate() {
            doc.history.set_logging(true);
            let unique = names.iter().filter(|n| *n == doc.name()).count() == 1;
            recorder.existing(doc.id, i, i == self.active, unique, &doc.table, &doc.rows, &report_ids(&doc.reports));
        }
        self.recorder = Some(recorder);
        self.info("Recording: what you do now is written as a script. Scripts ▸ Stop recording shows it.");
    }

    /// Stop recording and show the script in the Script window (or copy it
    /// when the window holds unsaved work).
    fn stop_recording(&mut self, ctx: &egui::Context) {
        self.sync_recording();
        let Some(recorder) = self.recorder.take() else { return };
        for doc in &mut self.documents {
            doc.history.set_logging(false);
        }
        let script = recorder.script();
        if self.script.is_modified() || self.script.is_running() {
            ctx.copy_text(script);
            self.info("Recording copied to the clipboard (the Script window has unsaved changes).");
        } else {
            self.script.open_text(script);
            self.info("Recording stopped: the script is in the Script window, ready to run or save.");
        }
    }

    /// Bring the recording up to date with every open document.
    fn sync_recording(&mut self) {
        let Some(recorder) = &mut self.recorder else { return };
        for doc in &mut self.documents {
            let edits = doc.history.take_logged();
            if !edits.is_empty() {
                recorder.edits(doc.id, edits);
            }
            recorder.sync(doc.id, &doc.table, &doc.rows, &report_ids(&doc.reports));
        }
    }

    /// Add `table` as a new document showing its data; no report runs until
    /// the user asks for one.
    fn open_table(&mut self, table: DataTable, source: Option<PathBuf>) {
        let doc = self.imported_document(table, source);
        self.place_document(doc, NewProject::default());
    }

    /// Add `doc` to the project tables are being added to
    /// ([`StatsApp::into_project`]) and show it, or else as a new project
    /// started as `new`.
    fn place_document(&mut self, doc: Document, new: NewProject) {
        match self.into_project.filter(|&p| self.project(p).is_some()) {
            Some(project) => {
                let name = doc.name().to_owned();
                let index = self.add_to_project(doc, project);
                self.set_active(index);
                self.info(format!("Added “{name}” to the project “{}”.", self.project_name(project)));
            }
            None => self.push_documents(vec![doc], new),
        }
    }

    /// A document for a table read from a file (or made by a report),
    /// showing its data with no reports.
    fn imported_document(&mut self, table: DataTable, source: Option<PathBuf>) -> Document {
        if let Some(path) = &source {
            // A workbook with several sheets names each table "file – sheet".
            let base = app_model::file_formats::name_from_path(path);
            let sheet = table.name().strip_prefix(&format!("{base} – ")).map(str::to_owned);
            let options = self
                .opening_options
                .iter()
                .position(|(n, _)| n == table.name())
                .map(|i| self.opening_options.remove(i).1);
            self.opening = Some(OpenedFrom::File { path: path.display().to_string(), sheet, options });
        }
        let rows = RowStateManager::new(table.n_rows());
        let mut doc = Document::new(table, rows, Vec::new(), Workspace::Data);
        doc.source_path = source;
        doc
    }

    /// Open the sheets of one workbook as the tables of one project, named
    /// after the file (each table is still marked as imported from it).
    fn open_workbook(&mut self, tables: Vec<DataTable>, path: &Path) {
        if self.into_project.is_some_and(|p| self.project(p).is_some()) {
            for table in tables {
                self.open_table(table, Some(path.to_path_buf()));
            }
            return;
        }
        let mut tables = tables.into_iter();
        let Some(first) = tables.next() else { return };
        let doc = self.imported_document(first, Some(path.to_path_buf()));
        let title = Some(app_model::file_formats::name_from_path(path));
        self.push_documents(vec![doc], NewProject { title, ..NewProject::default() });
        let (project, shown) = (self.documents[self.active].project, self.active);
        for table in tables {
            let doc = self.imported_document(table, Some(path.to_path_buf()));
            self.add_to_project(doc, project);
        }
        self.mark_project_saved(project);
        self.set_active(shown);
    }

    /// Add an empty, editable data table (one numeric column, no rows).
    fn open_blank(&mut self) {
        let table = DataTable::new("Untitled", vec![stat_data::Column::numeric("Column 1", Vec::new())])
            .expect("a single empty column is a valid table");
        let rows = RowStateManager::new(0);
        self.place_document(Document::new(table, rows, Vec::new(), Workspace::Data), NewProject::default());
    }

    /// Add a new table read from the system clipboard's text, through the
    /// Open Data dialog.
    fn open_clipboard_table(&mut self) {
        let text = match read_clipboard() {
            Ok(text) => text,
            Err(message) => return self.error(message),
        };
        match crate::open_data_dialog::OpenDataDialog::from_clipboard(text) {
            Ok(dialog) => self.offer_open_data(dialog),
            Err(e) => self.error(format!("Could not read the clipboard as a table: {e}")),
        }
    }

    /// Show the Open Data dialog, or open at once when the table starts in
    /// the top-left cell and Settings say to open such files directly.
    fn offer_open_data(&mut self, mut dialog: crate::open_data_dialog::OpenDataDialog) {
        // Offer to add the tables to the open project (ticked when they were
        // asked for with File ▸ Add to this project).
        let project = self.into_project.or_else(|| self.documents.get(self.active).map(|d| d.project));
        if let Some(project) = project.filter(|&p| self.project(p).is_some()) {
            dialog.offer_project(project, self.project_name(project), self.into_project.is_some());
        }
        if self.settings.open_plain_directly && dialog.is_plain() {
            match dialog.finish() {
                Ok(crate::open_data_dialog::OpenDataOutcome::Done { origin, tables }) => {
                    self.opened_data(origin, tables)
                }
                Ok(_) => {}
                Err(e) => self.error(format!("Could not open: {e}")),
            }
        } else {
            self.open_data.push_back(dialog);
        }
    }

    /// Data ▸ Rows ▸ Use row as column names, as one undoable edit.
    fn use_row_as_names(&mut self) {
        let Some(doc) = self.documents.get(self.active) else { return };
        let selected = doc.rows.selected_rows();
        let row = doc.grid.cursor_row().or_else(|| (selected.len() == 1).then(|| selected[0]));
        let Some(row) = row else {
            return self.error("Click a cell in the row that holds the column names, or select that one row");
        };
        match app_model::open_data::use_row_as_names_edit(&doc.table, row) {
            Ok(edit) => {
                if self.apply_edit(edit) {
                    self.info(format!("Row {} now names the columns. Undo puts it back.", row + 1));
                }
            }
            Err(e) => self.error(app_model::clipboard::error_text(&e)),
        }
    }

    /// Show the Open image dialog, or open the image at once when it has
    /// no choices to make and Settings says to open plain files directly.
    /// Returns whether the table opened now.
    fn offer_open_image(&mut self, dialog: crate::image_dialog::ImageOpenDialog) -> bool {
        if self.settings.open_plain_directly && dialog.is_plain() {
            match dialog.finish() {
                Ok((table, info)) => {
                    self.opened_image(dialog.path().to_path_buf(), table, &info);
                    return true;
                }
                Err(e) => self.error(format!("Could not open: {e}")),
            }
        } else {
            self.image_open.push_back(dialog);
        }
        false
    }

    /// Add the table of an image: its note (size, reduction or crop) goes to
    /// the status line and the project's notes.
    pub(crate) fn opened_image(&mut self, path: PathBuf, table: DataTable, info: &app_model::image_table::ImageInfo) {
        self.open_table(table, Some(path.clone()));
        let note = info.note();
        if let Some(project) = self.documents.get(self.active).map(|d| d.project) {
            let details = self.recent_details(project, &path);
            self.settings.remember_file_with(&path, details);
            if let Some(p) = self.project_mut(project) {
                if !p.notes.is_empty() && !p.notes.ends_with('\n') {
                    p.notes.push('\n');
                }
                p.notes.push_str(&note);
                p.notes.push('\n');
            }
        }
        if let Err(e) = save_settings(&self.settings) {
            return self.error(format!("Could not save the recent-files list: {e}"));
        }
        self.info(note);
    }

    /// The Open image dialog in front, if any.
    fn open_image_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.image_open.front_mut() else { return };
        match dialog.show(ctx) {
            crate::image_dialog::ImageOpenOutcome::Open => {}
            crate::image_dialog::ImageOpenOutcome::Cancel => {
                self.image_open.pop_front();
            }
            crate::image_dialog::ImageOpenOutcome::Done(opened) => {
                self.image_open.pop_front();
                let crate::image_dialog::OpenedImage { path, table, info } = *opened;
                self.opened_image(path, table, &info);
            }
        }
    }

    /// File ▸ Export data as ▸ Image…, when open.
    fn image_export_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.image_export else { return };
        let Some(doc) = self.documents.iter().find(|d| d.id == dialog.document) else {
            self.image_export = None;
            return;
        };
        match dialog.show(ctx, &doc.table, &doc.rows) {
            Outcome::Open => {}
            Outcome::Cancel => self.image_export = None,
            Outcome::Reset => self.image_export = Some(crate::image_dialog::ImageExportDialog::new(doc.id, &doc.table)),
            Outcome::Run((grid, limits, format), _) => {
                let name = doc.name().to_owned();
                let Some(path) = file_io::pick_save_file("Export image", format.label(), format.extension(), &name)
                else {
                    return;
                };
                self.image_export = None;
                let written = app_model::image_table::encode_image(&grid, limits, format)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| format!("could not write {}: {e}", path.display())));
                match written {
                    Ok(()) => self.info(format!("Wrote a {} × {} image to {}", grid.cols, grid.rows, path.display())),
                    Err(e) => self.error(e),
                }
            }
        }
    }

    /// The Open Data dialog in front, if any.
    fn open_data_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.open_data.front_mut() else { return };
        match dialog.show(ctx) {
            crate::open_data_dialog::OpenDataOutcome::Open => {}
            crate::open_data_dialog::OpenDataOutcome::Cancel => {
                self.open_data.pop_front();
            }
            crate::open_data_dialog::OpenDataOutcome::Done { origin, tables } => {
                let join = self.open_data.pop_front().and_then(|d| d.joining_project());
                self.into_project = join;
                self.opened_data(origin, tables);
                self.into_project = None;
            }
        }
    }

    /// Add the tables the Open Data dialog read.
    fn opened_data(
        &mut self,
        origin: crate::open_data_dialog::Origin,
        tables: Vec<crate::open_data_dialog::OpenedTable>,
    ) {
        use crate::open_data_dialog::Origin;
        let notes: Vec<String> = if tables.len() > 1 {
            tables.iter().flat_map(|t| t.notes.iter().map(|n| format!("{}: {n}", t.table.name()))).collect()
        } else {
            tables.iter().flat_map(|t| t.notes.iter().cloned()).collect()
        };
        match origin {
            Origin::File(path) => {
                for t in &tables {
                    if let Some(options) = &t.options {
                        self.opening_options.push((t.table.name().to_owned(), options.clone()));
                    }
                }
                let mut tables: Vec<DataTable> = tables.into_iter().map(|t| t.table).collect();
                if tables.len() > 1 {
                    self.open_workbook(tables, &path);
                } else if let Some(t) = tables.pop() {
                    self.open_table(t, Some(path.clone()));
                }
                self.opening_options.clear();
                match self.documents.get(self.active).map(|d| d.project) {
                    Some(project) => {
                        let details = self.recent_details(project, &path);
                        self.settings.remember_file_with(&path, details);
                    }
                    None => self.settings.remember_file(&path),
                }
                if let Err(e) = save_settings(&self.settings) {
                    return self.error(format!("Could not save the recent-files list: {e}"));
                }
                if notes.is_empty() {
                    self.message = None;
                } else {
                    self.info(notes.join(" · "));
                }
            }
            Origin::Clipboard => {
                let Some(t) = tables.into_iter().next() else { return };
                let table = t.table;
                let rows = RowStateManager::new(table.n_rows());
                let summary = format!(
                    "New table from the clipboard: {} × {}",
                    count(table.n_rows(), "row", "rows"),
                    count(table.n_cols(), "column", "columns")
                );
                // A new table has never been saved.
                let new = NewProject { unsaved: true, ..NewProject::default() };
                self.place_document(Document::new(table, rows, Vec::new(), Workspace::Data), new);
                let mut message = summary;
                for note in notes {
                    message.push_str(". ");
                    message.push_str(note.trim_end_matches('.'));
                }
                self.info(format!("{message}."));
            }
            Origin::Web { source, secrets } => {
                let Some(t) = tables.into_iter().next() else { return };
                let mut source = *source;
                // Refresh from source reads the same block again.
                if let app_model::external::ExternalSource::Web(request) = &mut source {
                    request.open = t.options.is_some().then(|| t.settings.clone());
                }
                self.open_external(t.table, &notes, source, secrets);
            }
        }
    }

    /// Copy from the active grid (Edit menu); `rows_only` copies the selected
    /// rows with column names.
    fn copy_from_grid(&mut self, ctx: &egui::Context, rows_only: bool) {
        let Some(doc) = self.documents.get(self.active) else { return };
        let copied = if rows_only {
            let selected = doc.rows.selected_rows();
            (!selected.is_empty()).then(|| GridState::copy_rows(&doc.table, &selected))
        } else {
            doc.grid.copy_selection(&doc.table, &doc.rows)
        };
        match copied {
            Some((text, what)) => {
                ctx.copy_text(text);
                self.info(format!("{what}."));
            }
            None => self.error("Click a cell or select rows to copy"),
        }
    }

    /// Paste text into the active document with its top-left value at `(row, column)`.
    fn paste(&mut self, row: usize, column: usize, text: &str) {
        use app_model::clipboard::{error_text, paste_text};
        let Some(doc) = self.documents.get(self.active) else { return };
        match paste_text(&doc.table, row, column, text) {
            Ok((edit, summary)) => {
                if self.apply_edit(edit) {
                    self.info(format!("{} Undo removes the paste.", summary.describe()));
                }
            }
            Err(e) => self.error(error_text(&e)),
        }
    }

    /// Paste the system clipboard's text into the active grid (Edit menu).
    fn paste_into_grid(&mut self) {
        let Some(doc) = self.documents.get(self.active) else { return };
        let Some((row, column)) = doc.grid.paste_target(&doc.table) else {
            return self.error("Click the cell where the pasted values should start");
        };
        match read_clipboard() {
            Ok(text) => self.paste(row, column, &text),
            Err(message) => self.error(message),
        }
    }

    /// Open entry `n` of the sample catalog as a new document with its reports.
    fn open_sample(&mut self, n: usize) {
        let Some(info) = app_model::sample::catalog().get(n).copied() else { return };
        let app_model::sample::Sample { table, reports } = (info.build)();
        self.opening = Some(OpenedFrom::Sample(info.name.to_owned()));
        let rows = RowStateManager::new(table.n_rows());
        let reports = self.build_reports(reports, &table, &rows);
        let doc = Document::new(table, rows, reports, Workspace::SideBySide);
        self.push_documents(vec![doc], NewProject { sample: true, ..NewProject::default() });
        self.info(info.description);
    }

    /// A new report: computed in the background when workers run (it shows
    /// as computing until then), otherwise here and now.
    fn make_report(&self, id: u64, saved: app_model::SavedReport, table: &DataTable, rows: &RowStateManager) -> Report {
        if self.report_jobs.is_some() { Report::new_waiting(id, saved) } else { Report::new(id, saved, table, rows) }
    }

    /// Reports for `table` from their saved form, with new report ids.
    fn build_reports(
        &mut self,
        saved: Vec<app_model::SavedReport>,
        table: &DataTable,
        rows: &RowStateManager,
    ) -> Vec<Report> {
        saved
            .into_iter()
            .map(|saved| {
                let report = self.make_report(self.next_report_id, saved, table, rows);
                self.next_report_id += 1;
                report
            })
            .collect()
    }

    /// Add a newly created design as a new document, showing its data.
    fn open_design(&mut self, table: DataTable, description: String) {
        let rows = RowStateManager::new(table.n_rows());
        self.push_document(Document::new(table, rows, Vec::new(), Workspace::Data));
        self.info(format!("Created {description}. Enter results in the response column."));
    }

    /// Add an augmented design: to the project of document `source` (by id)
    /// when `keep`, else as a new document.
    fn open_augmented(&mut self, table: DataTable, description: String, source: u64, keep: bool) {
        let project = self.documents.iter().find(|d| d.id == source).map(|d| d.project);
        let Some(project) = project.filter(|_| keep) else { return self.open_design(table, description) };
        let name = app_model::table_ops::unique_name(table.name(), self.documents.iter().map(Document::name));
        let table = if name == table.name() {
            table
        } else {
            DataTable::new(name, table.columns().to_vec()).expect("the same columns under another name")
        };
        let rows = RowStateManager::new(table.n_rows());
        let index = self.add_to_project(Document::new(table, rows, Vec::new(), Workspace::Data), project);
        self.set_active(index);
        let name = self.project_name(project);
        self.info(format!("Created {description} in “{name}”. Enter results in the response column."));
    }

    /// Add simulated responses to the active table (one undoable edit) and
    /// their true coefficients as a new table in the same project as
    /// document `source`.
    fn add_simulated(&mut self, outcome: app_model::design_simulation::SimulateOutcome, source: u64) {
        let Some(doc) = self.documents.get(self.active).filter(|d| d.id == source) else { return };
        let n = doc.table.n_cols();
        let count = outcome.columns.len();
        let edits = outcome
            .columns
            .into_iter()
            .enumerate()
            .map(|(i, column)| Edit::InsertColumn { index: n + i, column })
            .collect();
        if !self.apply_edit(Edit::Batch { label: "add simulated responses".to_owned(), edits }) {
            return;
        }
        let name = outcome.coefficients.name().to_owned();
        let project = self.documents.iter().find(|d| d.id == source).map(|d| d.project);
        if let Some(project) = project {
            let name = app_model::table_ops::unique_name(&name, self.documents.iter().map(Document::name));
            let table = DataTable::new(name, outcome.coefficients.columns().to_vec())
                .expect("the same columns under another name");
            let rows = RowStateManager::new(table.n_rows());
            self.add_to_project(Document::new(table, rows, Vec::new(), Workspace::Data), project);
        }
        self.info(format!(
            "Added {} to the table; their true coefficients are in “{name}”. Undo removes the columns.",
            if count == 1 { "a simulated response".to_owned() } else { format!("{count} simulated responses") }
        ));
    }

    /// Open a created design with its reports (a declared split-plot design)
    /// and project scripts, on the data view.
    fn open_design_project(&mut self, project: app_model::Project, description: String) {
        let app_model::Project { table, rows, reports, scripts, .. } = project;
        let reports = self.build_reports(reports, &table, &rows);
        let split_plot = !scripts.is_empty();
        let doc = Document::new(table, rows, reports, Workspace::Data);
        self.push_documents(vec![doc], NewProject { scripts, ..NewProject::default() });
        if split_plot {
            self.info(format!(
                "Created {description}. Enter results in the response column, then fit the split-plot model \
                 (Model > Mixed model is filled in for it, or run the project script \"{}\").",
                app_model::doe::SPLIT_PLOT_SCRIPT
            ));
        } else {
            self.info(format!("Created {description}. Enter results in the response column."));
        }
    }

    /// Add a table made by a table operation on the active document: to the
    /// active document's project when `keep` (recording where its sources
    /// are in the project and what they held), else as a new, unsaved
    /// document of its own.
    fn open_derived(&mut self, derived: DerivedDocument, keep: bool) {
        let DerivedDocument { table, rows, notes, origin } = derived;
        let sources = origin
            .sources
            .iter()
            .filter_map(|name| self.documents.iter().find(|d| d.name() == name).map(|d| d.id))
            .collect();
        self.opening = Some(OpenedFrom::Derived { origin: origin.clone(), sources });
        let summary = format!(
            "{}: {} × {}",
            origin.describe(),
            count(table.n_rows(), "row", "rows"),
            count(table.n_cols(), "column", "columns")
        );
        let primary = self.documents.get(self.active).map(|d| (d.id, d.project));
        let mut doc = Document::new(table, rows, Vec::new(), Workspace::Data);
        let mut summary = summary;
        match primary.filter(|_| keep) {
            Some((primary, project)) => {
                doc.lineage = Some(self.record_lineage(origin, primary, project));
                let index = self.add_to_project(doc, project);
                self.set_active(index);
                summary.push_str(&format!(", in “{}”", self.project_name(project)));
            }
            None => {
                doc.lineage = Some(TableLineage::unlinked(origin));
                // A new table has never been saved.
                self.push_documents(vec![doc], NewProject { unsaved: true, ..NewProject::default() });
            }
        }
        let mut message = summary;
        for note in notes {
            message.push_str(". ");
            message.push_str(note.trim_end_matches('.'));
        }
        self.info(format!("{message}."));
    }

    /// Show the formula editor, and apply the formula it asks for.
    fn formula_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.formula_dialog else { return };
        let Some(doc) = self.documents.iter().find(|d| d.id == dialog.document) else {
            self.formula_dialog = None;
            return;
        };
        match dialog.show(ctx, &doc.table) {
            Outcome::Open | Outcome::Reset => {}
            Outcome::Cancel => self.formula_dialog = None,
            Outcome::Run((edit, message), _) => {
                if self.apply_edit(edit) {
                    self.formula_dialog = None;
                    self.info(message);
                }
            }
        }
    }

    /// Show the New rank column dialog, and add the column it asks for.
    fn rank_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.rank_dialog else { return };
        let Some(doc) = self.documents.iter().find(|d| d.id == dialog.document) else {
            self.rank_dialog = None;
            return;
        };
        match dialog.show(ctx, &doc.table) {
            Outcome::Open | Outcome::Reset => {}
            Outcome::Cancel => self.rank_dialog = None,
            Outcome::Run((edit, message), _) => {
                if self.apply_edit(edit) {
                    self.rank_dialog = None;
                    self.info(message);
                }
            }
        }
    }

    /// Show the Recode dialog, and apply the recoding it asks for.
    fn recode_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.recode_dialog else { return };
        let Some(doc) = self.documents.iter().find(|d| d.id == dialog.document) else {
            self.recode_dialog = None;
            return;
        };
        match dialog.show(ctx, &doc.table) {
            Outcome::Open | Outcome::Reset => {}
            Outcome::Cancel => self.recode_dialog = None,
            Outcome::Run((edit, message), _) => {
                let Some(index) = self.documents.iter().position(|d| d.id == dialog.document) else { return };
                match self.edit_document(index, edit) {
                    Ok(()) => {
                        self.recode_dialog = None;
                        self.info(message);
                    }
                    Err(message) => self.error(message),
                }
            }
        }
    }

    /// Show the Value labels dialog, and apply the labels it asks for.
    fn value_labels_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.value_labels_dialog else { return };
        let Some(doc) = self.documents.iter().find(|d| d.id == dialog.document) else {
            self.value_labels_dialog = None;
            return;
        };
        match dialog.show(ctx, &doc.table) {
            Outcome::Open | Outcome::Reset => {}
            Outcome::Cancel => self.value_labels_dialog = None,
            Outcome::Run((edit, message), _) => {
                let Some(index) = self.documents.iter().position(|d| d.id == dialog.document) else { return };
                match self.edit_document(index, edit) {
                    Ok(()) => {
                        self.value_labels_dialog = None;
                        self.info(message);
                    }
                    Err(message) => self.error(message),
                }
            }
        }
    }

    /// Show the table operation dialog, and carry out what it asks for.
    fn table_op_dialog(&mut self, ctx: &egui::Context) {
        let Some(active) = self.documents.get(self.active) else { return };
        let (doc_id, table) = (active.id, &active.table);
        let docs: Vec<Source<'_>> = self.documents.iter().map(|d| Source { table: &d.table, rows: &d.rows }).collect();
        let action = drive(&mut self.table_op_dialog, &mut self.dialog_memory, doc_id, table, |d| {
            d.show(ctx, &docs, self.active)
        });
        drop(docs);
        match action {
            None => {}
            Some((TableOpAction::Info(message), _)) => self.info(message),
            Some((TableOpAction::Edit(edit, message), _)) => {
                if self.apply_edit(edit) {
                    self.info(message);
                }
            }
            Some((TableOpAction::Derived(doc, keep), _)) => self.open_derived(*doc, keep),
        }
    }

    /// Add a saved project, re-running its tables' reports and putting back
    /// its layout (views, windows and dashboards).
    fn open_project(&mut self, file: ProjectFile, path: &Path) {
        self.opening = Some(OpenedFrom::Project(path.display().to_string()));
        self.open_project_file(file, NewProject { path: Some(path.to_path_buf()), ..NewProject::default() });
    }

    /// Open entry `n` of the sample projects (several tables and a
    /// dashboard), as a built-in sample: saving it writes a new file.
    fn open_project_sample(&mut self, n: usize) {
        let Some(info) = app_model::sample::project_samples().get(n).copied() else { return };
        self.opening = Some(OpenedFrom::Sample(info.name.to_owned()));
        let title = info.name.split_once(" — ").map_or(info.name, |(name, _)| name).to_owned();
        self.open_project_file(
            (info.build)(),
            NewProject { title: Some(title), sample: true, ..NewProject::default() },
        );
        self.info(info.description);
    }

    /// Add project `file` as a new open project started as `new` (its
    /// scripts and dashboards come from the file).
    fn open_project_file(&mut self, file: ProjectFile, new: NewProject) {
        let ProjectFile { tables, scripts, layout, dashboards, notes, notebooks, name, folders } = file;
        let unsaved = new.unsaved;
        let asks = scripts.iter().any(|s| s.run_on_open);
        let mut docs = Vec::new();
        for t in tables {
            let ProjectTable { id, table, rows, reports, external, lineage } = t;
            let reports = self.build_reports(reports, &table, &rows);
            let mut doc = Document::new(table, rows, reports, Workspace::Data);
            doc.workspace = doc.default_workspace();
            doc.table_id = id;
            doc.external = external;
            doc.lineage = lineage;
            docs.push(doc);
        }
        self.push_documents(docs, NewProject { scripts, dashboards, notes, notebooks, name, ..new });
        let project = self.documents[self.active].project;
        self.apply_layout(project, &layout);
        if !folders.is_empty() {
            self.apply_folders(project, &folders);
            if !unsaved {
                self.mark_project_saved(project);
            }
        }
        if asks {
            self.ask_project_scripts.push_back(project);
        }
    }

    fn open_paths(&mut self, paths: Vec<PathBuf>) {
        let mut errors = Vec::new();
        let mut notes = Vec::new();
        let mut opened = 0;
        // The document to show afterwards (by id): the last one opened or brought forward.
        let mut show: Option<u64> = None;
        for path in paths {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext.eq_ignore_ascii_case(stat_script::SCRIPT_EXTENSION) {
                self.open_script_file(path);
                continue;
            }
            // A project that is already open is brought forward instead of opened twice.
            if let Some(id) = self.projects.iter().find(|p| p.path.as_deref() == Some(path.as_path())).map(|p| p.id) {
                let shown = self.documents.get(self.shown).filter(|d| d.project == id);
                show = shown.or_else(|| self.documents.iter().find(|d| d.project == id)).map(|d| d.id);
                opened += 1;
                continue;
            }
            // Images open as tables of pixels, through the Open image dialog.
            if app_model::image_table::is_image_path(&path) {
                match crate::image_dialog::ImageOpenDialog::from_file(path.clone()) {
                    Ok(dialog) => {
                        if self.offer_open_image(dialog) {
                            show = self.documents.get(self.active).map(|d| d.id).or(show);
                            opened += 1;
                        }
                    }
                    Err(e) => {
                        self.settings.forget_file(&path);
                        errors.push(e);
                    }
                }
                continue;
            }
            // Text files and workbooks go through the Open Data dialog.
            if app_model::file_formats::FileKind::from_path(&path)
                .is_some_and(|k| k.is_delimited() || k == app_model::file_formats::FileKind::Spreadsheet)
            {
                let dialog = app_model::files::read_raw(&path).and_then(|raw| {
                    let raw = raw.ok_or_else(|| format!("{} is not a data file", path.display()))?;
                    crate::open_data_dialog::OpenDataDialog::from_file(path.clone(), raw)
                });
                match dialog {
                    Ok(dialog) => {
                        let queued = self.open_data.len();
                        self.offer_open_data(dialog);
                        if self.open_data.len() == queued {
                            show = self.documents.get(self.active).map(|d| d.id).or(show);
                            opened += 1;
                        }
                    }
                    Err(e) => {
                        self.settings.forget_file(&path);
                        errors.push(e);
                    }
                }
                continue;
            }
            // Projects are read here (one file can hold several tables);
            // everything else by the file readers.
            let read = if ext.eq_ignore_ascii_case(app_model::project::PROJECT_EXTENSION) {
                std::fs::read_to_string(&path)
                    .map_err(|e| format!("could not read {}: {e}", path.display()))
                    .and_then(|text| {
                        app_model::project::load_project_file_noting(&text)
                            .map_err(|e| format!("{}: {e}", path.display()))
                    })
                    .map(|(file, load_notes)| {
                        // Reports that could not be restored, formula columns on a cycle.
                        notes.extend(load_notes);
                        vec![ReadItem::Project(Box::new(file))]
                    })
            } else {
                file_io::open_path(&path).map(|items| {
                    items
                        .into_iter()
                        .map(|item| match item {
                            Opened::Project(p) => ReadItem::Project(Box::new(ProjectFile::from(*p))),
                            Opened::Table(t, n) => ReadItem::Table(t, n),
                        })
                        .collect()
                })
            };
            match read {
                Ok(items) => {
                    let several_tables = items.iter().filter(|i| matches!(i, ReadItem::Table(..))).count() > 1;
                    let mut sheets = Vec::new();
                    for item in items {
                        match item {
                            ReadItem::Project(p) => self.open_project(*p, &path),
                            ReadItem::Table(t, table_notes) => {
                                notes.extend(table_notes.into_iter().map(|n| format!("{}: {n}", t.name())));
                                if several_tables {
                                    // The sheets of one workbook open as one project.
                                    sheets.push(t);
                                    continue;
                                }
                                self.open_table(t, Some(path.clone()));
                            }
                        }
                        show = self.documents.get(self.active).map(|d| d.id).or(show);
                        opened += 1;
                    }
                    if !sheets.is_empty() {
                        opened += sheets.len();
                        self.open_workbook(sheets, &path);
                        show = self.documents.get(self.active).map(|d| d.id).or(show);
                    }
                    match self.documents.get(self.active).map(|d| d.project) {
                        Some(project) => {
                            let details = self.recent_details(project, &path);
                            self.settings.remember_file_with(&path, details);
                        }
                        None => self.settings.remember_file(&path),
                    }
                }
                Err(e) => {
                    self.settings.forget_file(&path);
                    errors.push(e);
                }
            }
        }
        // The recent-files list changed.
        if let Err(e) = save_settings(&self.settings) {
            errors.push(format!("Could not save the recent-files list: {e}"));
        }
        if opened > 0 {
            if let Some(i) = show.and_then(|id| self.documents.iter().position(|d| d.id == id)) {
                self.set_active(i);
            }
        }
        if errors.is_empty() {
            if !notes.is_empty() {
                self.info(notes.join(" · "));
            } else if opened > 0 {
                self.message = None;
            }
        } else {
            self.error(errors.join(" · "));
        }
    }

    /// Show a script file in the Script window (replacing an unchanged script).
    fn open_script_file(&mut self, path: PathBuf) {
        if self.script.is_modified() {
            self.script.open = true;
            self.error("Save or discard the changes in the Script window before opening another script");
            return;
        }
        if let Err(e) = self.script.open_file(path) {
            self.error(e);
        }
    }

    /// Start running a script on copies of the open documents. A script
    /// kept in a project passes its name and runs with that project's
    /// document as the current table.
    fn run_script(&mut self, ctx: &egui::Context, source: String, project_script: Option<(u64, &str)>) {
        if self.notebook_windows.iter().any(crate::notebook_window::NotebookWindow::is_running) {
            self.script.add_note("A notebook cell is running; run the script when it has finished.");
            return;
        }
        if let Some(i) = project_script.and_then(|(id, _)| self.documents.iter().position(|d| d.id == id)) {
            self.set_active(i);
        }
        let workspace = self.script_workspace();
        match project_script {
            Some((id, name)) => {
                let project_path = self
                    .documents
                    .iter()
                    .find(|d| d.id == id)
                    .and_then(|d| self.project(d.project))
                    .and_then(|p| p.path.clone());
                self.script.start(ctx, source, workspace, Some((name, project_path.as_deref())));
            }
            None => self.script.start(ctx, source, workspace, None),
        }
    }

    /// Copies of the open documents for a script run, noting each one's data
    /// revision and reports so the run's changes can be merged afterwards.
    fn script_workspace(&mut self) -> ScriptWorkspace {
        let documents = self
            .documents
            .iter()
            .map(|d| {
                let scripts = self.project(d.project).map_or(&[][..], |p| p.scripts.as_slice());
                OpenDocument { key: d.id, project: d.project(scripts) }
            })
            .collect();
        self.script_revisions = self.documents.iter().map(|d| (d.id, d.revision)).collect();
        self.script_reports =
            self.documents.iter().map(|d| (d.id, d.reports.iter().map(Report::id).collect())).collect();
        let active = (!self.documents.is_empty()).then_some(self.active);
        ScriptWorkspace { documents, active }
    }

    /// Start the next queued project script, if nothing is running.
    fn run_queued_script(&mut self, ctx: &egui::Context) {
        if self.script.is_running() {
            return;
        }
        while let Some((id, name, source)) = self.script_queue.pop_front() {
            // Scripts of a project closed in the meantime are dropped.
            if self.documents.iter().any(|d| d.id == id) {
                self.script.open = true;
                self.run_script(ctx, source, Some((id, &name)));
                return;
            }
        }
    }

    /// Keep the Script window's script in the active document's project,
    /// replacing a script of the same name.
    fn keep_script(&mut self, name: String, source: String, run_on_open: bool) {
        let Some(id) = self.documents.get(self.active).map(|d| d.project) else {
            self.script.add_note("Open a table first: scripts are kept in a table's project.");
            return;
        };
        let table = self.project_name(id);
        let Some(project) = self.project_mut(id) else { return };
        let kept = ProjectScript { name: name.clone(), source, run_on_open };
        match project.scripts.iter_mut().find(|s| s.name == name) {
            Some(existing) => *existing = kept,
            None => project.scripts.push(kept),
        }
        let project_path = project.path.clone();
        let when = if run_on_open { ", to run when the project opens (after asking)" } else { "" };
        self.script.add_note(format!("Kept “{name}” in “{table}”{when}. Save the project to store it."));
        self.script.kept_in_project(&name, &table, project_path.as_deref());
    }

    /// Do `op` with script `script` of the project of document `index`.
    fn project_script(&mut self, ctx: &egui::Context, index: usize, script: usize, op: ProjectScriptOp) {
        let Some((doc_id, id)) = self.documents.get(index).map(|d| (d.id, d.project)) else { return };
        let table = self.project_name(id);
        let Some(project) = self.projects.iter_mut().find(|p| p.id == id) else { return };
        let project_path = project.path.clone();
        let Some(kept) = project.scripts.get_mut(script) else { return };
        match op {
            ProjectScriptOp::Open => {
                if self.script.is_modified() {
                    self.script.open = true;
                    self.error("Save or discard the changes in the Script window before opening another script");
                    return;
                }
                let (name, source) = (kept.name.clone(), kept.source.clone());
                self.script.open_project_script(&name, &source, &table, project_path.as_deref());
            }
            ProjectScriptOp::Run => {
                let item = (doc_id, kept.name.clone(), kept.source.clone());
                self.script_queue.push_back(item);
                self.run_queued_script(ctx);
            }
            ProjectScriptOp::ToggleRunOnOpen => kept.run_on_open = !kept.run_on_open,
            ProjectScriptOp::Remove => {
                project.scripts.remove(script);
            }
        }
    }

    /// Carry out a request from the sidebar's script library.
    fn library_request(&mut self, ctx: &egui::Context, request: LibraryRequest) {
        match request {
            LibraryRequest::ChooseFolder => {
                let mut dialog = rfd::FileDialog::new().set_title("Scripts folder");
                if let Some(folder) = &self.settings.scripts_folder {
                    dialog = dialog.set_directory(folder);
                }
                if let Some(folder) = dialog.pick_folder() {
                    self.settings.scripts_folder = Some(folder);
                    self.script_library.refresh();
                    if let Err(e) = save_settings(&self.settings) {
                        self.error(format!("Could not save the scripts folder setting: {e}"));
                    }
                }
            }
            LibraryRequest::File(path, LibraryOp::Open) => self.open_script_file(path),
            LibraryRequest::File(path, LibraryOp::Run) => {
                if self.script.is_running() {
                    self.error("A script is already running");
                    return;
                }
                if self.script.is_modified() {
                    self.script.open = true;
                    self.error("Save or discard the changes in the Script window before running another script");
                    return;
                }
                match std::fs::read_to_string(&path) {
                    Ok(source) => {
                        self.open_script_file(path);
                        self.run_script(ctx, source, None);
                    }
                    Err(e) => self.error(format!("Could not read {}: {e}", path.display())),
                }
            }
            LibraryRequest::File(path, LibraryOp::KeepInProject) => {
                let source = match std::fs::read_to_string(&path) {
                    Ok(source) => source,
                    Err(e) => return self.error(format!("Could not read {}: {e}", path.display())),
                };
                let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let Some(id) = self.documents.get(self.active).map(|d| d.project) else { return };
                let table = self.project_name(id);
                let Some(project) = self.project_mut(id) else { return };
                let kept = ProjectScript { name: name.clone(), source, run_on_open: false };
                match project.scripts.iter_mut().find(|s| s.name == name) {
                    Some(existing) => *existing = kept,
                    None => project.scripts.push(kept),
                }
                self.info(format!("Kept “{name}” in “{table}”. Save the project to store it."));
            }
            LibraryRequest::Project(script, op) => {
                let op = match op {
                    ProjectRequest::Open => ProjectScriptOp::Open,
                    ProjectRequest::Run => ProjectScriptOp::Run,
                    ProjectRequest::Remove => ProjectScriptOp::Remove,
                };
                self.project_script(ctx, self.active, script, op);
            }
        }
    }

    /// Ask whether to run a just-opened project's run-on-open scripts.
    fn project_scripts_question(&mut self, ctx: &egui::Context) {
        let Some(&id) = self.ask_project_scripts.front() else { return };
        let Some(project) = self.project(id) else {
            self.ask_project_scripts.pop_front();
            return;
        };
        let names: Vec<String> =
            project.scripts.iter().filter(|s| s.run_on_open).map(|s| format!("“{}”", s.name)).collect();
        let name = self.project_name(id);
        enum Answer {
            Run,
            Skip,
            Show,
        }
        let mut choice = None;
        egui::Window::new("Run the project's scripts?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let what = if names.len() == 1 { "a script" } else { "scripts" };
                ui.label(
                    RichText::new(format!("“{name}” has {what} set to run when it opens: {}.", names.join(", ")))
                        .strong(),
                );
                ui.label(
                    "Scripts can change the data and read and write files on this computer. \
                     Run them only if you trust where the project came from.",
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Run").clicked() {
                        choice = Some(Answer::Run);
                    }
                    if ui.button("Don't run").clicked() {
                        choice = Some(Answer::Skip);
                    }
                    if ui.button("Show in Script window").on_hover_text("Read the script first; nothing runs").clicked()
                    {
                        choice = Some(Answer::Show);
                    }
                });
            });
        let Some(choice) = choice else { return };
        self.ask_project_scripts.pop_front();
        // The scripts run with the project's first table as the current one.
        let Some(index) = self.documents.iter().position(|d| d.project == id) else { return };
        let doc_id = self.documents[index].id;
        let Some(project) = self.project(id) else { return };
        let first = project.scripts.iter().position(|s| s.run_on_open);
        match choice {
            Answer::Run => {
                let queued: Vec<_> = project
                    .scripts
                    .iter()
                    .filter(|s| s.run_on_open)
                    .map(|s| (doc_id, s.name.clone(), s.source.clone()))
                    .collect();
                self.script_queue.extend(queued);
                self.run_queued_script(ctx);
            }
            Answer::Show => {
                if let Some(first) = first {
                    self.project_script(ctx, index, first, ProjectScriptOp::Open);
                }
            }
            Answer::Skip => {}
        }
    }

    /// Merge what a finished script did into the open documents. A document
    /// edited or closed while the script ran is left alone, and the script's
    /// version opens as a new table instead. Returns notes on what changed.
    fn apply_script_run(&mut self, run: WorkspaceRun) -> Vec<String> {
        let mut notes = Vec::new();
        // New tables open behind the current one, which keeps the script's main result in view.
        let front = self.documents.get(self.shown).map(|d| d.id);
        let target = self.documents.get(self.active).map(|d| d.id);
        for change in run.changes {
            match change {
                DocumentChange::New { project } => {
                    let name = project.table.name().to_owned();
                    self.open_script_table(*project);
                    notes.push(format!("Opened “{name}” in a new tab."));
                }
                DocumentChange::Changed {
                    key,
                    edits,
                    selected,
                    excluded,
                    hidden,
                    colors,
                    subsets,
                    reports,
                    project,
                } => {
                    let name = project.table.name().to_owned();
                    let index = self.documents.iter().position(|d| d.id == key);
                    let current =
                        index.is_some_and(|i| self.script_revisions.contains(&(key, self.documents[i].revision)));
                    let merged = match index {
                        None => Err(format!("“{name}” was closed while the script ran")),
                        Some(_) if !current => Err(format!("“{name}” was edited while the script ran")),
                        Some(i) => {
                            let rows = RowChanges { selected, excluded, hidden, colors: colors.map(|c| *c), subsets };
                            self.merge_script_change(i, edits, rows, reports)
                        }
                    };
                    match merged {
                        Ok(summary) if summary.is_empty() => {}
                        Ok(summary) => notes.push(format!("“{name}”: {summary}.")),
                        Err(why) => {
                            self.open_script_table(*project);
                            notes.push(format!("{why}, so the script's version opened as a new table."));
                        }
                    }
                }
            }
        }
        if let Some(i) = front.and_then(|id| self.documents.iter().position(|d| d.id == id)) {
            self.set_active(i);
        }
        if let Some(i) = target.and_then(|id| self.documents.iter().position(|d| d.id == id)) {
            self.set_target(i);
        }
        self.script_revisions.clear();
        self.script_reports.clear();
        notes
    }

    /// Replay a script's changes on document `index`: its data edits as one
    /// undoable step, then row states and the report list. Returns a summary.
    fn merge_script_change(
        &mut self,
        index: usize,
        edits: Vec<Edit>,
        row_changes: RowChanges,
        reports: Option<Vec<(Option<usize>, app_model::SavedReport)>>,
    ) -> Result<String, String> {
        let mut parts = Vec::new();
        if !edits.is_empty() {
            let n = edits.len();
            self.edit_document(index, Edit::Batch { label: "script changes".into(), edits })?;
            parts.push(format!("{n} data edit(s), undone together by Edit ▸ Undo"));
        }
        let doc = &mut self.documents[index];
        let RowChanges { selected, excluded, hidden, colors, subsets } = row_changes;
        if let Some(selected) = selected {
            doc.rows.select(selected, SelectionMode::Replace);
        }
        let n = doc.rows.len();
        if let Some(excluded) = excluded {
            if (0..n).filter(|&r| doc.rows.is_excluded_flag(r)).ne(excluded.iter().copied()) {
                doc.rows.set_excluded(0..n, false);
                doc.rows.set_excluded(excluded.iter().copied(), true);
                parts.push(format!("{} row(s) now excluded", excluded.len()));
            }
        }
        if let Some(hidden) = hidden {
            if (0..n).filter(|&r| doc.rows.is_hidden_flag(r)).ne(hidden.iter().copied()) {
                doc.rows.set_hidden(0..n, false);
                doc.rows.set_hidden(hidden.iter().copied(), true);
                parts.push(format!("{} row(s) now hidden", hidden.len()));
            }
        }
        if let Some(styles) = colors {
            styles.apply_to(&mut doc.rows);
        }
        if let Some(subsets) = subsets {
            if app_model::action::RowSubsets::of(&doc.rows) != subsets && subsets.apply_to(&mut doc.rows).is_ok() {
                parts.push(match &subsets.active {
                    Some(name) => format!("row subset “{name}” active"),
                    None => format!("{} row subset(s) saved", subsets.subsets.len()),
                });
            }
        }
        if let Some(reports) = reports {
            // Keep each report the script left alone (with its plot views);
            // rebuild changed ones under the same id; add new ones.
            let mut old: Vec<Option<Report>> = std::mem::take(&mut doc.reports).into_iter().map(Some).collect();
            let started_with = self.script_reports.get(&doc.id).map_or(&[][..], Vec::as_slice);
            let (mut added, mut changed) = (0, 0);
            for (key, saved) in reports {
                // `key` is a position in the list as it was when the script started.
                let kept = key
                    .and_then(|k| started_with.get(k))
                    .and_then(|&id| old.iter_mut().find(|r| r.as_ref().is_some_and(|r| r.id() == id)))
                    .and_then(Option::take);
                let report = match kept {
                    Some(r) if r.saved() == saved => r,
                    Some(r) => {
                        changed += 1;
                        Report::new(r.id(), saved, &doc.table, &doc.rows)
                    }
                    None => {
                        added += 1;
                        let r = Report::new(self.next_report_id, saved, &doc.table, &doc.rows);
                        self.next_report_id += 1;
                        r
                    }
                };
                doc.reports.push(report);
            }
            // Reports added while the script ran were never seen by it: keep them, at the top.
            let (unseen, closed): (Vec<Report>, Vec<Report>) =
                old.into_iter().flatten().partition(|r| !started_with.contains(&r.id()));
            let closed = closed.len();
            doc.reports.splice(0..0, unseen);
            for (n, what) in [(added, "new"), (changed, "changed"), (closed, "closed")] {
                if n > 0 {
                    parts.push(format!("{n} {what} report(s)"));
                }
            }
            if added > 0 && doc.workspace == Workspace::Data {
                doc.workspace = Workspace::SideBySide;
            }
        }
        Ok(parts.join(", "))
    }

    /// Add a table a script made or showed as a new, unsaved document.
    fn open_script_table(&mut self, project: Project) {
        let Project { table, rows, reports, origin, external, scripts } = project;
        let reports = self.build_reports(reports, &table, &rows);
        let mut doc = Document::new(table, rows, reports, Workspace::Data);
        doc.workspace = doc.default_workspace();
        doc.lineage = origin.map(TableLineage::unlinked);
        doc.external = external;
        // A new table has never been saved.
        self.push_documents(vec![doc], NewProject { scripts, unsaved: true, ..NewProject::default() });
    }
    /// Add a report from a dialog, or put it in place of the report it edited.
    fn place_report(&mut self, request: ReportRequest, place: Placement) {
        if let Placement::Replace(id) = place {
            let Some(doc) = self.documents.get_mut(self.active) else { return };
            if let Some(report) = doc.reports.iter_mut().find(|r| r.id() == id) {
                report.replace(request.into(), &doc.table, &doc.rows);
                return;
            }
            // The report was closed while its dialog was open: add it again.
        }
        let search = matches!(request, ReportRequest::ModelSearch(_));
        self.add_report(request);
        // A new model search also opens its best model's full report, below it.
        if search {
            let id = self.next_report_id;
            let Some(doc) = self.documents.get_mut(self.active) else { return };
            if let Some(best) = doc.reports.first().and_then(Report::best_model_report) {
                doc.reports.insert(1.min(doc.reports.len()), Report::new(id, best, &doc.table, &doc.rows));
                self.next_report_id += 1;
            }
        }
    }

    /// Open the dialog of report `id` filled from its settings, to change
    /// and update it in place ("Edit analysis…").
    fn edit_report(&mut self, id: u64) {
        let Some(doc) = self.documents.get(self.active) else { return };
        let Some(report) = doc.reports.iter().find(|r| r.id() == id) else { return };
        // The dialogs read the request alone; the report keeps its filter and switcher.
        let saved = report.request();
        let table = &doc.table;
        fn edit<D: Remember>(slot: &mut Option<D>, dialog: Option<D>, id: u64) -> bool {
            let Some(mut dialog) = dialog else { return false };
            dialog.set_edit(Some(id));
            *slot = Some(dialog);
            true
        }
        let opened = edit(&mut self.dialog, ModelDialog::from_saved(&saved, table), id)
            || edit(&mut self.distribution_dialog, DistributionDialog::from_saved(&saved, table), id)
            || edit(&mut self.distribution_fit_dialog, DistributionFitDialog::from_saved(&saved, table), id)
            || edit(&mut self.bivariate_dialog, BivariateDialog::from_saved(&saved, table), id)
            || edit(&mut self.compare_dialog, CompareDialog::from_saved(&saved, table), id)
            || edit(&mut self.phase2_dialog, Phase2Dialog::from_saved(&saved, table), id)
            || edit(
                &mut self.method_dialog,
                crate::multivariate_methods_dialog::MethodDialog::from_saved(&saved, table),
                id,
            )
            || edit(&mut self.factor_dialog, crate::factor_dialog::FactorDialog::from_saved(&saved, table), id)
            || edit(
                &mut self.discriminant_dialog,
                crate::discriminant_dialog::DiscriminantDialog::from_saved(&saved, table),
                id,
            )
            || edit(&mut self.map_dialog, crate::cluster_map_dialog::MapDialog::from_saved(&saved, table), id)
            || edit(&mut self.text_dialog, crate::text_dialog::TextDialog::from_saved(&saved, table), id)
            || edit(
                &mut self.imputation_dialog,
                crate::imputation_dialog::ImputationDialog::from_saved(&saved, table),
                id,
            )
            || edit(
                &mut self.signal_processing_dialog,
                crate::signal_processing_dialog::SignalProcessingDialog::from_saved(&saved, table),
                id,
            )
            || edit(
                &mut self.image_analysis_dialog,
                crate::image_analysis_dialog::ImageAnalysisDialog::from_saved(&saved, table),
                id,
            )
            || edit(
                &mut self.series_explore_dialog,
                crate::series_explore_dialog::SeriesExploreDialog::from_saved(&saved, table),
                id,
            )
            || edit(&mut self.mixed_dialog, crate::mixed_dialog::MixedDialog::from_saved(&saved, table), id)
            || edit(&mut self.tree_dialog, crate::tree_dialog::TreeDialog::from_saved(&saved, table), id)
            || edit(&mut self.forest_dialog, crate::forest_dialog::ForestDialog::from_saved(&saved, table), id)
            || edit(&mut self.boost_dialog, crate::boost_dialog::BoostDialog::from_saved(&saved, table), id)
            || edit(&mut self.pls_dialog, crate::pls_dialog::PlsDialog::from_saved(&saved, table), id)
            || edit(&mut self.svm_dialog, crate::svm_dialog::SvmDialog::from_saved(&saved, table), id)
            || edit(&mut self.mars_dialog, crate::mars_dialog::MarsDialog::from_saved(&saved, table), id)
            || edit(&mut self.neural_dialog, crate::neural_dialog::NeuralDialog::from_saved(&saved, table), id)
            || edit(
                &mut self.gaussian_process_dialog,
                crate::gaussian_process_dialog::GaussianProcessDialog::from_saved(&saved, table),
                id,
            )
            || edit(&mut self.search_dialog, crate::search_dialog::SearchDialog::from_saved(&saved, table), id)
            || edit(
                &mut self.functional_dialog,
                crate::functional_dialog::FunctionalDialog::from_saved(&saved, table),
                id,
            )
            || edit(
                &mut self.model_comparison_dialog,
                crate::comparison_dialog::ComparisonDialog::from_saved(&saved, table),
                id,
            )
            || edit(
                &mut self.curve_template_dialog,
                crate::curve_template_dialog::CurveTemplateDialog::from_saved(&saved, table),
                id,
            )
            || edit(
                &mut self.nonlinear_dialog,
                crate::nonlinear_dialog::NonlinearDialog::from_saved(&saved, table),
                id,
            )
            || edit(&mut self.regression_dialog, RegressionDialog::from_saved(&saved, table), id)
            || edit(&mut self.mixture_dialog, MixtureDialog::from_saved(&saved, table), id)
            || edit(&mut self.anom_dialog, AnomDialog::from_saved(&saved, table), id)
            || edit(&mut self.pareto_dialog, ParetoDialog::from_saved(&saved, table), id)
            || edit(&mut self.cause_effect_dialog, CauseEffectDialog::from_saved(&saved, table), id)
            || edit(&mut self.multi_vari_dialog, MultiVariDialog::from_saved(&saved, table), id)
            || edit(&mut self.chart_dialog, ChartDialog::from_saved(&saved, table), id)
            || edit(&mut self.attribute_dialog, AttributeDialog::from_saved(&saved, table), id)
            || edit(&mut self.time_weighted_dialog, TimeWeightedDialog::from_saved(&saved, table), id)
            || edit(&mut self.capability_dialog, CapabilityDialog::from_saved(&saved, table), id)
            || edit(&mut self.multivariate_dialog, MultivariateDialog::from_saved(&saved, table), id)
            || edit(&mut self.gauge_dialog, GaugeDialog::from_saved(&saved, table), id)
            || edit(&mut self.quality_tool_dialog, QualityToolDialog::from_saved(&saved, table), id)
            || edit(
                &mut self.monitoring_dialog,
                crate::monitoring_dialog::MonitoringDialog::from_saved(&saved, table),
                id,
            )
            || edit(&mut self.study_dialog, StudyDialog::from_saved(&saved, table), id)
            || edit(&mut self.agreement_dialog, AgreementDialog::from_saved(&saved, table), id)
            || edit(&mut self.bayes_dialog, crate::bayes_dialog::BayesDialog::from_saved(&saved, table), id)
            || edit(&mut self.signal_dialog, crate::signal_dialog::SignalDialog::from_saved(&saved, table), id)
            || edit(
                &mut self.treatment_dialog,
                crate::treatment_dialog::TreatmentDialog::from_saved(&saved, table),
                id,
            )
            || edit(
                &mut self.evaluation_dialog,
                crate::evaluation_dialog::EvaluationDialog::from_saved(&saved, table),
                id,
            )
            || edit(
                &mut self.factorial_dialog,
                crate::factorial_dialog::FactorialDialog::from_saved(&saved, table),
                id,
            )
            || edit(
                &mut self.sequential_dialog,
                crate::sequential_dialog::SequentialDialog::from_saved(&saved, table),
                id,
            )
            || edit(
                &mut self.tabulation_dialog,
                crate::tabulation_dialog::TabulationDialog::from_saved(&saved, table),
                id,
            )
            || edit(&mut self.plot3d_dialog, crate::plot3d_dialog::Plot3dDialog::from_saved(&saved, table), id)
            || edit(
                &mut self.permutation_dialog,
                crate::resampling_dialogs::PermutationDialog::from_saved(&saved, table),
                id,
            )
            || edit(&mut self.reliability_dialog, ReliabilityDialog::from_saved(&saved, table), id)
            || edit(&mut self.forecast_dialog, crate::forecast_dialog::ForecastDialog::from_saved(&saved, table), id)
            || edit(&mut self.survival_dialog, SurvivalDialog::from_saved(&saved, table), id);
        if !opened {
            self.info("This report has no dialog to edit it in; change it from its ⋯ Options menu.".to_owned());
        }
    }

    fn add_report(&mut self, request: ReportRequest) {
        self.add_report_to(self.active, request);
    }

    /// Add a report at the top of document `index`'s results, and show them.
    fn add_report_to(&mut self, index: usize, request: ReportRequest) {
        let id = self.next_report_id;
        let Some(doc) = self.documents.get_mut(index) else { return };
        let saved = pooled_on_imputations(request.into(), &doc.table);
        let background = self.report_jobs.is_some();
        let report = if background { Report::new_waiting(id, saved) } else { Report::new(id, saved, &doc.table, &doc.rows) };
        self.next_report_id += 1;
        doc.reports.insert(0, report);
        self.reveal(index, View::Results);
    }

    /// Save the project of document `index` (all its tables, scripts and
    /// layout) to its file, asking for one if needed, or always with `ask`.
    /// Returns `true` if it was saved.
    fn save(&mut self, index: usize, ask: bool) -> bool {
        let Some(id) = self.documents.get(index).map(|d| d.project) else { return false };
        let Some(project) = self.project(id) else { return false };
        // A built-in sample has no file of its own to overwrite: always ask.
        let ask = ask || project.sample;
        let path = match (&project.path, ask) {
            (Some(p), false) => p.clone(),
            _ => match file_io::pick_project_destination(&self.project_name(id)) {
                Some(p) => p,
                None => return false,
            },
        };
        let mut file = self.project_file(id, true);
        // Save as names the file, so the file's name is the project's name from now on.
        if ask {
            file.name = None;
        }
        match file_io::write_project_file(&file, &path) {
            Ok(()) => {
                if let Some(project) = self.project_mut(id) {
                    project.path = Some(path.clone());
                    project.sample = false;
                    if ask {
                        project.name = None;
                    }
                }
                self.mark_project_saved(id);
                self.modified_marks.remove(&id);
                let details = self.recent_details(id, &path);
                self.settings.remember_file_with(&path, details);
                // Not being able to update the recent-files list is no reason to report a failed save.
                let _ = save_settings(&self.settings);
                self.sync_recording();
                // Scripts save one table at a time, so only a one-table project is recorded.
                if let [doc] = self.members(id).as_slice() {
                    let doc = self.documents[*doc].id;
                    if let Some(r) = &mut self.recorder {
                        r.saved(doc, &path.display().to_string());
                    }
                }
                self.info(format!("Saved project {}", path.display()));
                true
            }
            Err(e) => {
                self.error(e);
                false
            }
        }
    }

    fn export(&mut self, index: usize, format: ExportFormat, labels: bool) {
        let Some(doc) = self.documents.get(index) else { return };
        let Some(path) = file_io::pick_export_destination(doc.name(), format) else { return };
        let labelled = labels.then(|| app_model::files::with_value_labels(&doc.table)).flatten();
        match file_io::export_table(labelled.as_ref().unwrap_or(&doc.table), format, &path) {
            Ok(()) => {
                let (id, n) = (doc.id, doc.table.n_rows());
                self.sync_recording();
                if let Some(r) = &mut self.recorder {
                    r.saved(id, &path.display().to_string());
                }
                self.info(format!("Exported {} to {}", count(n, "row", "rows"), path.display()));
            }
            Err(e) => self.error(e),
        }
    }

    /// Open the export dialog for document `index`, from report `report`
    /// when given (else from the menu or shortcut), in the window it was
    /// asked from: the report's own window, the results window holding it,
    /// or the window whose keys were pressed.
    pub(crate) fn open_export_dialog(&mut self, index: usize, report: Option<u64>) {
        let Some(doc) = self.documents.get(index) else { return };
        let window = match report {
            Some(id) if doc.reports.iter().any(|r| r.id() == id && r.in_own_window) => Some((doc.id, View::Report(id))),
            Some(_) if self.in_window(doc.id, View::Results) => Some((doc.id, View::Results)),
            Some(_) => None,
            None => self.acting_window,
        };
        let (format, paper) = (self.settings.report_format, self.settings.paper);
        self.export_dialog = Some(crate::report_export_ui::ExportDialog::new(doc.id, report, format, paper));
        self.export_window = window;
    }

    /// The export dialog, when it shows in window `window` (`None` for the
    /// main window), and the export it asks for.
    fn export_dialog_window(&mut self, ctx: &egui::Context, window: Option<(u64, View)>) {
        // A dialog whose window has closed moves to the main window.
        if self.export_window.is_some_and(|(d, v)| !self.windows.iter().any(|w| w.document == d && w.view == v)) {
            self.export_window = None;
        }
        if self.export_window != window {
            return;
        }
        let Some(dialog) = &mut self.export_dialog else { return };
        let Some(index) = self.documents.iter().position(|d| d.id == dialog.doc) else {
            self.export_dialog = None;
            return;
        };
        let doc = &self.documents[index];
        let title = dialog.report.and_then(|id| doc.reports.iter().find(|r| r.id() == id)).map(Report::title);
        let outcome = dialog.show(ctx, title.as_deref(), doc.table.name(), doc.reports.len());
        let choice = match outcome {
            dialog_kit::Outcome::Open => return,
            dialog_kit::Outcome::Run(choice, _) => choice,
            dialog_kit::Outcome::Cancel | dialog_kit::Outcome::Reset => {
                self.export_dialog = None;
                return;
            }
        };
        self.export_dialog = None;
        if (self.settings.report_format, self.settings.paper) != (choice.format, choice.paper) {
            self.settings.report_format = choice.format;
            self.settings.paper = choice.paper;
            self.settings_changed(ctx);
        }
        // A report still computing in the background is finished here, so
        // the export shows its result rather than a placeholder.
        let doc = &mut self.documents[index];
        for report in &mut doc.reports {
            report.bring_up_to_date(&doc.table, &doc.rows);
        }
        let doc = &self.documents[index];
        let one = choice.report.is_some();
        let parts = crate::report_export_ui::prepare(
            doc.reports
                .iter()
                .filter(|r| choice.report.is_none_or(|id| r.id() == id))
                .map(Report::for_export)
                .collect(),
            one,
            choice.skip_collapsed,
        );
        let n = parts.len();
        if n == 0 {
            self.error("Every report is collapsed: there is nothing open to export".to_owned());
            return;
        }
        match crate::report_export_ui::export(&parts, one, &doc.table, &doc.rows, choice.format, choice.paper) {
            None => {}
            Some(Ok(path)) => {
                let what = if one { "the report".to_owned() } else { count(n, "report", "reports") };
                self.info(format!("Saved {what} as {}", path.display()));
            }
            Some(Err(e)) => self.error(e),
        }
    }

    /// Close document `index`: its whole project when it is the only table
    /// (asking first about unsaved changes), otherwise just this table,
    /// removed from the project (asking first when other tables of the
    /// project were made from it).
    fn request_close(&mut self, index: usize) {
        let Some((doc_id, project)) = self.documents.get(index).map(|d| (d.id, d.project)) else { return };
        if self.members(project).len() <= 1 {
            self.request_close_project(project);
        } else if !self.dependents(doc_id).is_empty() || !self.dashboards_emptied_by(index).is_empty() {
            self.confirm_remove = Some(doc_id);
        } else {
            self.remove_table(index);
        }
    }

    /// Close every table of project `id`, asking first if it has unsaved changes.
    fn request_close_project(&mut self, id: u64) {
        if self.project_modified(id) {
            self.confirm_close = Some(id);
        } else {
            self.close_project(id);
        }
    }

    /// Close every table of project `id` without asking.
    fn close_project(&mut self, id: u64) {
        while let Some(i) = self.documents.iter().position(|d| d.project == id) {
            self.close(i);
        }
    }

    /// Remove table `index` from its project (the project stays open, with
    /// an unsaved change).
    fn remove_table(&mut self, index: usize) {
        let Some(doc) = self.documents.get(index) else { return };
        let (name, project) = (doc.name().to_owned(), doc.project);
        self.close(index);
        let project = self.project_name(project);
        self.info(format!("Removed “{name}” from “{project}”. Save the project to keep the change."));
    }

    /// Close document `index` without asking. Tables of its project made
    /// from it keep their data but lose the link to it; a project left with
    /// no tables closes.
    fn close(&mut self, index: usize) {
        if index >= self.documents.len() {
            return;
        }
        self.sync_recording();
        if let Some(r) = &mut self.recorder {
            r.closed(self.documents[index].id);
        }
        // Open dialogs belong to the active document; a closed one's settings
        // are forgotten just below.
        self.remember_open_dialogs();
        let closed = self.documents.remove(index);
        self.dialog_memory.forget(closed.id);
        self.stale.remove(&closed.id);
        if index < self.active || self.active >= self.documents.len() {
            self.active = self.active.saturating_sub(1);
        }
        if index < self.shown || self.shown >= self.documents.len() {
            self.shown = self.shown.saturating_sub(1);
        }
        // Its windows close with it (its project's dashboard windows move to another of its tables).
        self.windows.retain(|w| w.document != closed.id || matches!(w.view, View::Dashboard(_)));
        if self.last_window.is_some_and(|(doc, _)| doc == closed.id) {
            self.last_window = None;
        }
        self.unlink_table(closed.project, &closed.table_id);
        if self.documents.iter().any(|d| d.project == closed.project) {
            let n = self.drop_tiles_of_table(closed.project, &closed.table_id);
            if n > 0 {
                self.info(format!(
                    "Closed “{}” and took its {} off the project's dashboards",
                    closed.name(),
                    count(n, "tile", "tiles")
                ));
            }
        } else {
            self.projects.retain(|p| p.id != closed.project);
            self.modified_marks.remove(&closed.project);
            self.recovery_forget(closed.project);
        }
        self.close_dialogs();
    }

    fn perform(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::PickAndOpen => {
                if let Some(paths) = file_io::pick_files_to_open() {
                    self.open_paths(paths);
                }
            }
            Action::OpenPaths(paths) => self.open_paths(paths),
            Action::OpenSample(n) => self.open_sample(n),
            Action::OpenManual(manual) => match manuals::open(manual) {
                Ok(_) => self.info(format!("Opened the {} in your PDF viewer", manual.title())),
                Err(e) => self.error(e),
            },
            Action::NewTable => self.open_blank(),
            Action::StartTask(task) => self.start_task(task),
            Action::OpenOfKind(kind) => {
                if let Some(paths) = file_io::pick_files_of_kind(kind) {
                    self.open_paths(paths);
                }
            }
            Action::ImportExternal(kind) => {
                self.external_dialog = Some(crate::external_dialog::ExternalDialog::new(kind));
            }
            Action::NewTableFromClipboard => self.open_clipboard_table(),
            Action::AddToProject(project, source) => {
                self.into_project = Some(project);
                match source {
                    AddSource::Blank => self.open_blank(),
                    AddSource::Clipboard => self.open_clipboard_table(),
                    AddSource::File => {
                        if let Some(paths) = file_io::pick_files_to_open() {
                            self.open_paths(paths);
                        }
                    }
                }
                self.into_project = None;
            }
            Action::Copy { rows_only } => self.copy_from_grid(ctx, rows_only),
            Action::Paste => self.paste_into_grid(),
            Action::Cut | Action::ClearCells | Action::FillDown => {
                let Some(doc) = self.documents.get(self.active) else { return };
                let Some((rows, columns)) = doc.grid.selection(&doc.table) else {
                    return self.error("Click a cell or a column heading first");
                };
                match action {
                    Action::Cut => {
                        if let Some((text, ..)) = doc.grid.cut_selection(&doc.table) {
                            ctx.copy_text(text);
                        }
                        self.clear_cells(&rows, &columns);
                    }
                    Action::ClearCells => self.clear_cells(&rows, &columns),
                    _ if rows.len() < 2 => self.error("Select two or more rows of cells to fill down"),
                    _ => self.fill_down(&rows, &columns),
                }
            }
            Action::UndoRemoveReport => {
                let Some(removed) = self.removed_report.take() else { return };
                let Some(doc) = self.documents.iter_mut().find(|d| d.id == removed.document) else { return };
                let report = Report::new(self.next_report_id, removed.saved, &doc.table, &doc.rows);
                self.next_report_id += 1;
                doc.reports.insert(removed.index.min(doc.reports.len()), report);
                self.info(format!("Put back “{}”", removed.title));
            }
            Action::Find => {
                if let Some(doc) = self.documents.get_mut(self.active) {
                    doc.grid.open_find();
                    self.reveal(self.active, View::Data);
                }
            }
            Action::SelectAllCells => {
                if let Some(doc) = self.documents.get_mut(self.active) {
                    doc.grid.select_all(&doc.table);
                    self.reveal(self.active, View::Data);
                }
            }
            Action::Activate(i) => {
                // Choosing the document already shown brings its windows forward.
                if i == self.shown {
                    if let Some(id) = self.documents.get(i).map(|d| d.id) {
                        self.raise_windows_of(id);
                    }
                }
                self.set_active(i);
            }
            Action::PopOut { view: View::Dashboard(d), .. } | Action::DashboardWindow(d) => self.pop_out_dashboard(d),
            Action::BringBack { view: View::Dashboard(d), .. } => self.bring_back_dashboard(d),
            Action::ShowWindow { view: View::Dashboard(d), .. } => self.raise_dashboard_window(d),
            Action::PopOut { doc, view } => self.pop_out(doc, view),
            Action::BringBack { doc, view } => self.bring_back(doc, view),
            Action::ShowWindow { doc, view } => self.raise_window(doc, view),
            Action::OpenProjectSample(n) => self.open_project_sample(n),
            Action::NewDashboard(project) => {
                if let Some(id) = self.new_dashboard(project) {
                    self.show_dashboard(id);
                    self.info("Added a dashboard. Add tiles with + Add tile, or Add to dashboard from a report's ⋯ Options menu.");
                }
            }
            Action::ShowDashboard(d) => self.show_dashboard(d),
            Action::DashboardUndo(d) => self.dashboard_undo_redo(d, true),
            Action::DashboardRedo(d) => self.dashboard_undo_redo(d, false),
            Action::ExportDashboard(d) => self.export_dashboard(d),
            Action::RemoveDashboard(d) => self.confirm_dashboard_removal = Some(d),
            Action::Rename(target, name) => self.rename(target, &name),
            Action::Folder(project, op) => self.folder_op(ctx, project, op),
            Action::ShowReport { doc, report } => self.show_report(ctx, doc, report),
            Action::OpenNotebook(project, name) => self.open_notebook(project, &name),
            Action::Save(i) => {
                self.save(i, false);
            }
            Action::SaveAs(i) => {
                self.save(i, true);
            }
            Action::SaveTemplate(i) => self.save_template(i),
            Action::NewFromTemplate => self.new_from_template(),
            Action::Export(i, format, labels) => self.export(i, format, labels),
            Action::ExportImage(i) => {
                if let Some(doc) = self.documents.get(i) {
                    self.image_export = Some(crate::image_dialog::ImageExportDialog::new(doc.id, &doc.table));
                }
            }
            Action::ExportReports(i) => self.open_export_dialog(i, None),
            Action::Close(i) => self.request_close(i),
            Action::CloseProject(id) => self.request_close_project(id),
            Action::UpdateFromSource(id) => self.update_from_source(id),
            Action::NewGraph => self.add_report(ReportRequest::Graph(Box::default())),
            Action::ProjectScript { doc, script, op } => self.project_script(ctx, doc, script, op),
            Action::Library(request) => self.library_request(ctx, request),
            Action::OpenScript => {
                let ext = stat_script::SCRIPT_EXTENSION;
                if let Some(path) = file_io::pick_open_file("Open script", "Rhai script", &[ext]) {
                    self.open_script_file(path);
                }
            }
            Action::GraphFromPipeline => match report_view::open_pipeline() {
                None => {}
                Some(Err(message)) => self.error(message),
                Some(Ok(spec)) => {
                    let missing: Vec<String> = self
                        .documents
                        .get(self.active)
                        .map(|d| {
                            app_model::graph_json::missing_columns(&spec, &d.table)
                                .into_iter()
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    if missing.is_empty() {
                        self.add_report(ReportRequest::Graph(Box::new(spec)));
                    } else {
                        self.error(format!(
                            "This dataset lacks the graph's column(s): {}. Rename columns to match, then open it again.",
                            missing.join(", ")
                        ));
                    }
                }
            },
            Action::Grid(requests) => {
                for request in requests {
                    self.handle_grid(request);
                }
            }
            Action::GridIn(index, requests) => {
                if index >= self.documents.len() {
                    return;
                }
                self.set_target(index);
                for request in requests {
                    self.handle_grid(request);
                }
            }
            Action::UseRowAsNames => self.use_row_as_names(),
        }
    }

    /// Turn a grid request into an [`Edit`] of the active document and apply it.
    fn handle_grid(&mut self, request: GridRequest) {
        let request = match request {
            GridRequest::Undo => return self.undo_redo(true),
            GridRequest::Redo => return self.undo_redo(false),
            GridRequest::Paste { row, column, text } => return self.paste(row, column, &text),
            GridRequest::Clear { rows, columns } => return self.clear_cells(&rows, &columns),
            GridRequest::FillDown { rows, columns } => return self.fill_down(&rows, &columns),
            GridRequest::Recode(column) => {
                if let Some(doc) = self.documents.get(self.active) {
                    self.recode_dialog = Some(table_view::recode::RecodeDialog::new(doc.id, &doc.table, column));
                }
                return;
            }
            GridRequest::ValueLabels(column) => {
                if let Some(doc) = self.documents.get(self.active) {
                    self.value_labels_dialog =
                        Some(table_view::value_labels::ValueLabelsDialog::new(doc.id, &doc.table, column));
                }
                return;
            }
            GridRequest::ColorBy(column) => return self.open_color_by(column),
            GridRequest::Apply { edit, message } => {
                if self.apply_edit(edit) {
                    self.info(message);
                }
                return;
            }
            GridRequest::Formula { column, at } => {
                if let Some(doc) = self.documents.get(self.active) {
                    self.formula_dialog = Some(FormulaDialog::new(doc.id, &doc.table, column, at));
                }
                return;
            }
            other => other,
        };
        let Some(doc) = self.documents.get(self.active) else { return };
        let table = &doc.table;
        let mut note = None;
        let mut deselect_columns = false;
        let removing = matches!(request, GridRequest::RemoveColumn(_) | GridRequest::RemoveColumns(_));
        let edit = match request {
            GridRequest::SetCell { row, column, text } => Edit::set_cell_text(table, row, column, &text),
            GridRequest::InsertRows { at, count } => {
                if count > 1 {
                    note = Some(format!("Added {count} rows. Undo removes them."));
                }
                Ok(Edit::insert_empty_rows(table, at, count))
            }
            GridRequest::RemoveRows(rows) => Ok(Edit::RemoveRows { rows }),
            GridRequest::InsertColumn { at, kind, count, name } => {
                let edit = Edit::new_columns(table, at, kind, count, &name);
                if count > 1 {
                    let names = match &edit {
                        Edit::Batch { edits, .. } => match (edits.first(), edits.last()) {
                            (Some(Edit::InsertColumn { column: a, .. }), Some(Edit::InsertColumn { column: b, .. })) => {
                                format!(" ({} to {})", a.name, b.name)
                            }
                            _ => String::new(),
                        },
                        _ => String::new(),
                    };
                    note = Some(format!("Added {count} columns{names}. Undo removes them."));
                }
                Ok(edit)
            }
            GridRequest::RemoveColumn(index) => Ok(Edit::RemoveColumn { index }),
            GridRequest::RemoveColumns(columns) => {
                deselect_columns = true;
                Ok(Edit::remove_columns(&columns))
            }
            GridRequest::RenameColumn(index, name) => Ok(Edit::RenameColumn { index, name }),
            GridRequest::ConvertColumn(index, kind) => Edit::convert_column(table, index, kind).map(|(edit, lost)| {
                if lost > 0 {
                    let name = table.column(index).map_or("", |c| c.name.as_str());
                    note = Some(format!(
                        "{lost} value(s) in “{name}” could not be converted and are now missing — Undo restores them"
                    ));
                }
                edit
            }),
            GridRequest::SwapDayMonth(index) => Edit::swap_day_month(table, index).map(|(edit, lost)| {
                if lost > 0 {
                    let name = table.column(index).map_or("", |c| c.name.as_str());
                    note = Some(format!(
                        "{lost} date(s) in “{name}” have a day above 12, so cannot be day first; they are now missing — \
                         Undo restores them"
                    ));
                }
                edit
            }),
            GridRequest::SetLevel(index, level) => Ok(Edit::SetLevel { index, level }),
            GridRequest::SetHover(index, hover) => Ok(Edit::SetHover { index, hover }),
            GridRequest::Sort { column, descending } => {
                let name = table.column(column).map_or(String::new(), |c| c.name.clone());
                app_model::table_ops::sort_edit(table, &[(name, descending)])
            }
            GridRequest::ClearFormula(index) => Ok(Edit::SetFormula { index, formula: None }),
            GridRequest::Formula { .. } => unreachable!("handled above"),
            GridRequest::Paste { .. }
            | GridRequest::Clear { .. }
            | GridRequest::FillDown { .. }
            | GridRequest::Apply { .. }
            | GridRequest::Recode(_)
            | GridRequest::ValueLabels(_)
            | GridRequest::ColorBy(_) => {
                unreachable!("handled above")
            }
            GridRequest::Undo | GridRequest::Redo => unreachable!("handled above"),
        };
        match edit {
            Ok(edit) => {
                if self.apply_edit(edit) {
                    if let Some(n) = note {
                        self.info(n);
                    }
                    if deselect_columns {
                        if let Some(doc) = self.documents.get_mut(self.active) {
                            doc.grid.clear_columns();
                        }
                    }
                } else if removing {
                    // A refusal to delete is easy to miss in the status bar: say why in a box.
                    if let Some((message, true)) = self.message.take() {
                        self.column_refusal = Some(message);
                    }
                }
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    /// Clear the cells of `rows` × `columns` in the active document (Delete, Cut).
    fn clear_cells(&mut self, rows: &[usize], columns: &[usize]) {
        let Some(doc) = self.documents.get(self.active) else { return };
        let computed = columns.iter().any(|&j| doc.table.column(j).is_some_and(|c| c.formula.is_some()));
        match Edit::clear_cells(&doc.table, rows, columns) {
            Some(edit) => {
                let label = edit.label();
                if self.apply_edit(edit) {
                    let skipped =
                        if computed { " (formula columns are computed, so they were left as they are)" } else { "" };
                    self.info(format!("{}{skipped}. Undo puts them back.", capitalized(&label)));
                }
            }
            None if computed => {
                self.error("Formula cells are computed and cannot be cleared; convert the formula to values first");
            }
            None => {}
        }
    }

    /// Copy the first of `rows` into the others in each of `columns` (Fill down).
    fn fill_down(&mut self, rows: &[usize], columns: &[usize]) {
        let Some(doc) = self.documents.get(self.active) else { return };
        match Edit::fill_down(&doc.table, rows, columns) {
            Some(edit) => {
                let label = edit.label();
                if self.apply_edit(edit) {
                    self.info(format!("{}. Undo puts the old values back.", capitalized(&label)));
                }
            }
            None => self.info("Nothing to fill: the cells below already match the first row"),
        }
    }

    /// Add the columns reports asked for (saved clusters or scores), as undoable edits.
    fn add_report_columns(&mut self, doc_index: usize) {
        let Some(doc) = self.documents.get_mut(doc_index) else { return };
        let requested: Vec<report_view::NewColumn> =
            doc.reports.iter_mut().flat_map(|r| std::mem::take(&mut r.new_columns)).collect();
        self.add_columns(doc_index, requested);
        let Some(doc) = self.documents.get_mut(doc_index) else { return };
        let runs: Vec<Result<(Edit, usize), String>> = doc
            .reports
            .iter_mut()
            .filter_map(|r| if std::mem::take(&mut r.runs_requested) { r.proposed_runs(&doc.table) } else { None })
            .collect();
        for result in runs {
            match result.and_then(|(edit, n)| self.edit_document(doc_index, edit).map(|()| n)) {
                Ok(n) => self.info(format!(
                    "Added {} at the end of the table. Measure the responses and type them in; Undo removes them.",
                    if n == 1 { "1 proposed run".to_owned() } else { format!("{n} proposed runs") }
                )),
                Err(message) => self.error(message),
            }
        }
    }

    /// Add columns asked for by reports (or dashboard tiles) to document
    /// `doc_index`'s table, as undoable edits.
    fn add_columns(&mut self, doc_index: usize, requested: Vec<report_view::NewColumn>) {
        for report_view::NewColumn(c) in requested {
            let Some(doc) = self.documents.get(doc_index) else { return };
            let name =
                if doc.table.find(&c.name).is_some() { doc.table.unused_column_name(&c.name) } else { c.name.clone() };
            let column = c.into_column(name.clone());
            let index = doc.table.n_cols();
            match self.edit_document(doc_index, Edit::InsertColumn { index, column }) {
                Ok(()) => self.info(format!("Added column “{name}”. Undo removes it.")),
                Err(message) => self.error(message),
            }
        }
    }

    /// If removing column `index` would break a report, the message saying so.
    fn column_in_use(doc: &Document, index: usize) -> Option<String> {
        let users: Vec<String> = doc
            .reports
            .iter()
            .filter(|r| r.uses_column(index, &doc.table))
            .map(|r| format!("“{}”", r.title()))
            .collect();
        (!users.is_empty()).then(|| {
            let name = doc.table.column(index).map_or("", |c| c.name.as_str());
            let (reports, those) = if users.len() == 1 { ("report", "that report") } else { ("reports", "those reports") };
            format!(
                "Column “{name}” is used by {} {reports}: {}. Close {those} (× on the report's heading) before \
                 deleting it.",
                users.len(),
                users.join(", ")
            )
        })
    }

    /// Apply an edit to the active document. Returns `true` on success.
    fn apply_edit(&mut self, edit: Edit) -> bool {
        match self.edit_document(self.active, edit) {
            Ok(()) => {
                self.message = None;
                true
            }
            Err(message) => {
                self.error(message);
                false
            }
        }
    }

    /// Apply an undoable edit to document `index`, keeping its reports on
    /// their columns.
    ///
    /// # Errors
    /// Why the edit was refused (a report uses a column it removes, or it
    /// does not fit the data); nothing changes then.
    fn edit_document(&mut self, index: usize, edit: Edit) -> Result<(), String> {
        let Some(doc) = self.documents.get_mut(index) else { return Err("the table is no longer open".into()) };
        if let Some(message) =
            edit.removed_columns(doc.table.n_cols()).into_iter().find_map(|c| Self::column_in_use(doc, c))
        {
            return Err(message);
        }
        let removed = edit.removed_columns(doc.table.n_cols());
        if let Some(message) = removed.into_iter().find_map(|c| self.tile_column_use(index, c)) {
            return Err(message);
        }
        let Some(doc) = self.documents.get_mut(index) else { return Err("the table is no longer open".into()) };
        let names = column_names(&doc.table);
        let moved = moves_rows(&edit);
        let shift = doc.history.apply(edit, Arc::make_mut(&mut doc.table), &mut doc.rows).map_err(|e| e.to_string())?;
        if moved {
            doc.reports.iter_mut().for_each(Report::note_rows_moved);
        }
        // A new edit clears the redo list, and with it any undone update.
        doc.lineage_redo.clear();
        // Undo now belongs to this edit, not to a dashboard removed before it.
        self.removed_dashboard = None;
        doc.revision += 1;
        let id = doc.id;
        follow_columns(doc, shift.clone(), &names);
        self.dashboards_follow_columns(index, &shift, &names);
        self.dialogs_follow_columns(id, &shift, names.len());
        Ok(())
    }

    /// Keep open dialogs of document `id` on their columns after an edit
    /// that inserted or removed columns (`n_before`: the column count
    /// before it). The Recode dialog follows its column (and closes if it
    /// was removed); analysis dialogs, which hold column positions of their
    /// own, close when existing columns moved, rather than silently
    /// analysing different columns.
    fn dialogs_follow_columns(&mut self, id: u64, shifts: &[ColumnShift], n_before: usize) {
        if let Some(dialog) = self.recode_dialog.as_mut().filter(|d| d.document == id) {
            if !shifts.iter().all(|&s| dialog.follow(s)) {
                self.recode_dialog = None;
            }
        }
        if let Some(dialog) = self.value_labels_dialog.as_mut().filter(|d| d.document == id) {
            if !shifts.iter().all(|&s| dialog.follow(s)) {
                self.value_labels_dialog = None;
            }
        }
        let active = self.documents.get(self.active).is_some_and(|d| d.id == id);
        if moves_existing_columns(shifts, n_before) {
            // Remembered settings hold column positions too.
            self.dialog_memory.forget(id);
            if active {
                self.close_dialogs();
            }
        }
    }

    fn undo_redo(&mut self, undo: bool) {
        // Undo first brings back a dashboard just removed from this project.
        if undo && self.restore_removed_dashboard() {
            return;
        }
        let Some(doc) = self.documents.get_mut(self.active) else { return };
        let next = if undo { doc.history.next_undo() } else { doc.history.next_redo() };
        let Some((label, edit)) = next else { return };
        let label = label.to_owned();
        if let Some(message) =
            edit.removed_columns(doc.table.n_cols()).into_iter().find_map(|c| Self::column_in_use(doc, c))
        {
            self.error(message);
            return;
        }
        let removed = edit.removed_columns(doc.table.n_cols());
        if let Some(message) = removed.into_iter().find_map(|c| self.tile_column_use(self.active, c)) {
            self.error(message);
            return;
        }
        let Some(doc) = self.documents.get_mut(self.active) else { return };
        let names = column_names(&doc.table);
        let result = if undo {
            doc.history.undo(Arc::make_mut(&mut doc.table), &mut doc.rows)
        } else {
            doc.history.redo(Arc::make_mut(&mut doc.table), &mut doc.rows)
        };
        match result {
            Some(Ok(shift)) => {
                doc.revision += 1;
                // Undo and redo may put back or take away rows.
                doc.reports.iter_mut().for_each(Report::note_rows_moved);
                // Undoing an update puts back the lineage the old values were made with.
                if label == app_model::table_ops::UPDATE_LABEL {
                    let (from, to) = if undo {
                        (&mut doc.lineage_undo, &mut doc.lineage_redo)
                    } else {
                        (&mut doc.lineage_redo, &mut doc.lineage_undo)
                    };
                    if let Some(lineage) = from.pop() {
                        to.extend(doc.lineage.replace(lineage));
                    }
                }
                let id = doc.id;
                follow_columns(doc, shift.clone(), &names);
                self.dashboards_follow_columns(self.active, &shift, &names);
                self.dialogs_follow_columns(id, &shift, names.len());
                self.info(format!("{} {label}", if undo { "Undid" } else { "Redid" }));
            }
            Some(Err(e)) => self.error(e.to_string()),
            None => {}
        }
    }

    /// Keyboard shortcuts pressed in a window: the main window (`window`
    /// `None`) or the window of a view, acting on document `doc` (an index).
    /// In a view's window, ⌘W closes that window rather than the document.
    fn keyboard_shortcuts(
        &mut self,
        ctx: &egui::Context,
        actions: &mut Vec<Action>,
        doc: usize,
        window: Option<(u64, View)>,
    ) {
        if ctx.input_mut(|i| i.consume_shortcut(&SETTINGS_SHORTCUT)) {
            self.settings_open = true;
        }
        // Quitting goes through the main window's close request, which asks
        // about unsaved changes first (sent to it explicitly: from another
        // window, a plain close would close only that window).
        if ctx.input_mut(|i| i.consume_shortcut(&QUIT_SHORTCUT)) {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
        // Undo / redo go to a text field while one is being edited.
        if !ctx.wants_keyboard_input() && !self.documents.is_empty() {
            let (redo, undo) = ctx.input_mut(|i| {
                // Redo first: the plain ⌘Z pattern also matches ⌘⇧Z.
                let redo = i.consume_shortcut(&redo_shortcut()) || i.consume_shortcut(&REDO_ALT_SHORTCUT);
                (redo, i.consume_shortcut(&UNDO_SHORTCUT))
            });
            // In a dashboard (its window, or the main window showing it) they undo dashboard edits.
            let dashboard = match window {
                Some((_, View::Dashboard(d))) => Some(d),
                Some(_) => None,
                None => self.shown_dashboard(),
            };
            match (dashboard, redo, undo) {
                (Some(d), true, _) => actions.push(Action::DashboardRedo(d)),
                (Some(d), false, true) => actions.push(Action::DashboardUndo(d)),
                (None, true, _) => actions.push(Action::Grid(vec![GridRequest::Redo])),
                (None, false, true) => actions.push(Action::Grid(vec![GridRequest::Undo])),
                _ => {}
            }
        }
        // ⌘S in the script editor saves the script (the Script window handles it).
        let script_focus = self.script.editor_has_focus(ctx);
        let (open, save_as, save, close, new, dropped) = ctx.input_mut(|i| {
            // Save as first: the plain ⌘S pattern also matches ⌘⇧S.
            let save_as = !script_focus && i.consume_shortcut(&save_as_shortcut());
            (
                i.consume_shortcut(&OPEN_SHORTCUT),
                save_as,
                !script_focus && i.consume_shortcut(&SAVE_SHORTCUT),
                i.consume_shortcut(&CLOSE_SHORTCUT),
                i.consume_shortcut(&NEW_SHORTCUT),
                i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect::<Vec<_>>(),
            )
        });
        if open {
            actions.push(Action::PickAndOpen);
        }
        if new {
            actions.push(Action::NewTable);
        }
        if !script_focus
            && !self.documents.is_empty()
            && ctx.input_mut(|i| i.consume_shortcut(&EXPORT_REPORTS_SHORTCUT))
        {
            actions.push(Action::ExportReports(doc));
        }
        if !script_focus && !self.documents.is_empty() && ctx.input_mut(|i| i.consume_shortcut(&FIND_SHORTCUT)) {
            actions.push(Action::Find);
        }
        if save_as && !self.documents.is_empty() {
            actions.push(Action::SaveAs(doc));
        } else if save && !self.documents.is_empty() {
            actions.push(Action::Save(doc));
        }
        // Data, Results and Side by side are the main window's views.
        if !ctx.wants_keyboard_input() && window.is_none() && self.shown_dashboard().is_none() {
            if let Some(doc) = self.documents.get_mut(self.shown) {
                let views = [Workspace::Data, Workspace::Results, Workspace::SideBySide];
                for (shortcut, view) in WORKSPACE_SHORTCUTS.iter().zip(views) {
                    if ctx.input_mut(|i| i.consume_shortcut(shortcut)) {
                        doc.workspace = view;
                    }
                }
            }
        }
        if close {
            match window {
                Some((doc, view)) => actions.push(Action::BringBack { doc, view }),
                // ⌘W on a dashboard goes back to its project's table.
                None if self.shown_dashboard().is_some() => actions.push(Action::Activate(self.shown)),
                None if !self.documents.is_empty() => actions.push(Action::Close(doc)),
                None => {}
            }
        }
        if !dropped.is_empty() {
            actions.push(Action::OpenPaths(dropped));
        }
    }

    fn menu_bar(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let has_doc = !self.documents.is_empty();
        let active = self.active;
        let scripts = self
            .documents
            .get(active)
            .and_then(|d| self.project(d.project))
            .map(|p| p.scripts.clone())
            .unwrap_or_default();
        let several = self.documents.get(active).map(|d| d.project).filter(|&p| self.members(p).len() > 1);
        let active_project = self.documents.get(active).map(|d| d.project);
        // The dashboard the Edit menu's Undo and Redo act on, with their labels.
        let dashboard: Option<(u64, String, Option<String>, Option<String>)> =
            self.command_dashboard().and_then(|d| self.dashboard(d)).map(|o| {
                let undo = o.history.next_undo().map(str::to_owned);
                let redo = o.history.next_redo().map(str::to_owned);
                (o.id, o.dashboard.name.clone(), undo, redo)
            });
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                crate::menus::menu_button(ui, "File", |ui| {
                    if ui.add(egui::Button::new("Open…").shortcut_text(ctx.format_shortcut(&OPEN_SHORTCUT))).clicked()
                    {
                        actions.push(Action::PickAndOpen);
                        ui.close_menu();
                    }
                    let recent = self.settings.recent_files.clone();
                    ui.add_enabled_ui(!recent.is_empty(), |ui| {
                        ui.menu_button("Open recent", |ui| recent_files_items(ui, &recent, actions));
                    })
                    .response
                    .on_disabled_hover_text("Files you open or save are listed here");
                    if ui
                        .add(egui::Button::new("New data table").shortcut_text(ctx.format_shortcut(&NEW_SHORTCUT)))
                        .clicked()
                    {
                        actions.push(Action::NewTable);
                        ui.close_menu();
                    }
                    if ui
                        .button("New table from clipboard")
                        .on_hover_text(
                            "Tab- or comma-separated text, such as cells copied from a spreadsheet; \
                             a first line of names becomes the column names",
                        )
                        .clicked()
                    {
                        actions.push(Action::NewTableFromClipboard);
                        ui.close_menu();
                    }
                    let project = self.documents.get(self.active).map(|d| d.project);
                    ui.add_enabled_ui(project.is_some(), |ui| {
                        ui.menu_button("Add to this project", |ui| {
                            let Some(project) = project else { return };
                            ui.label(RichText::new(format!("Tables join “{}”", self.project_name(project))).weak());
                            let items = [
                                ("Empty table", AddSource::Blank, "An empty table to type or paste into"),
                                ("Table from clipboard", AddSource::Clipboard, "Cells copied from a spreadsheet"),
                                ("Data file…", AddSource::File, "A text file, workbook or JSON file"),
                            ];
                            for (label, source, tip) in items {
                                if ui.button(label).on_hover_text(tip).clicked() {
                                    actions.push(Action::AddToProject(project, source));
                                    ui.close_menu();
                                }
                            }
                        });
                    })
                    .response
                    .on_disabled_hover_text("Open a table first; new tables then join its project");
                    if ui
                        .button("New project from template…")
                        .on_hover_text(
                            "Build a saved template's reports, graphs and dashboards on an open table, a data file \
                             or each data file in a folder",
                        )
                        .clicked()
                    {
                        actions.push(Action::NewFromTemplate);
                        ui.close_menu();
                    }
                    if ui
                        .button("Import from database…")
                        .on_hover_text("Rows of an SQL query: SQLite files, PostgreSQL, MySQL or MariaDB servers, ODBC data sources")
                        .clicked()
                    {
                        self.external_dialog =
                            Some(crate::external_dialog::ExternalDialog::new(crate::external_dialog::ExternalKind::Database));
                        ui.close_menu();
                    }
                    if ui
                        .button("Import from the web…")
                        .on_hover_text("JSON or CSV fetched from an address (HTTP GET), with optional headers")
                        .clicked()
                    {
                        self.external_dialog =
                            Some(crate::external_dialog::ExternalDialog::new(crate::external_dialog::ExternalKind::Web));
                        ui.close_menu();
                    }
                    ui.menu_button("Open sample data", |ui| {
                        // One submenu per topic, so the list fits on any screen.
                        let catalog = app_model::sample::catalog();
                        for topic in app_model::sample::SampleTopic::ALL {
                            ui.menu_button(topic.label(), |ui| {
                                // Wide enough that "dataset — what it demonstrates" sits on one line.
                                ui.set_min_width(460.0);
                                let font = egui::TextStyle::Button.resolve(ui.style());
                                let (strong, weak) = (ui.visuals().text_color(), ui.visuals().weak_text_color());
                                for (n, info) in catalog.iter().enumerate().filter(|(_, s)| s.topic == topic) {
                                    let (dataset, uses) = info.name.split_once(" — ").unwrap_or((info.name, ""));
                                    let mut label = egui::text::LayoutJob::default();
                                    label.append(dataset, 0.0, egui::TextFormat::simple(font.clone(), strong));
                                    if !uses.is_empty() {
                                        label.append(
                                            &format!("  —  {uses}"),
                                            0.0,
                                            egui::TextFormat::simple(font.clone(), weak),
                                        );
                                    }
                                    if ui.add(egui::Button::new(label)).on_hover_text(info.description).clicked() {
                                        actions.push(Action::OpenSample(n));
                                        ui.close_menu();
                                    }
                                }
                            });
                        }
                        ui.separator();
                        ui.menu_button("Projects", |ui| {
                            ui.set_min_width(460.0);
                            for (n, info) in app_model::sample::project_samples().iter().enumerate() {
                                if ui.button(info.name).on_hover_text(info.description).clicked() {
                                    actions.push(Action::OpenProjectSample(n));
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                    ui.separator();
                    ui.add_enabled_ui(has_doc, |ui| document_file_items(ui, active, &scripts, several, actions, true));
                    ui.separator();
                    ui.add_enabled_ui(has_doc, |ui| {
                        let new = ui.button("New dashboard").on_hover_text(
                            "A grid of report tiles and notes from this project's tables, with filters shared by every tile",
                        );
                        if new.clicked() {
                            if let Some(project) = active_project {
                                actions.push(Action::NewDashboard(project));
                            }
                            ui.close_menu();
                        }
                    });
                    let export = ui
                        .add_enabled(dashboard.is_some(), egui::Button::new("Export dashboard as HTML…"))
                        .on_hover_text("One web page with the dashboard's tiles on the same grid")
                        .on_disabled_hover_text("Show a dashboard first");
                    if export.clicked() {
                        if let Some((d, ..)) = &dashboard {
                            actions.push(Action::ExportDashboard(*d));
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .add(egui::Button::new("Settings…").shortcut_text(ctx.format_shortcut(&SETTINGS_SHORTCUT)))
                        .on_hover_text("Colour scheme and text size")
                        .clicked()
                    {
                        self.settings_open = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .add(egui::Button::new("Quit AnovaCraft").shortcut_text(ctx.format_shortcut(&QUIT_SHORTCUT)))
                        .on_hover_text("Asks about each document with unsaved changes first")
                        .clicked()
                    {
                        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                        ui.close_menu();
                    }
                });
                let n_documents = self.documents.len();
                let restorable = self.restorable_dashboard();
                let Some(doc) = self.documents.get_mut(self.active) else {
                    for name in ["Edit", "Data", "Describe", "Compare", "Model", "Multivariate"] {
                        planned_menu(ui, name, "Open a dataset first");
                    }
                    design_menu(
                        ui,
                        &self.dialog_memory,
                        &mut self.design_dialog,
                        &mut self.mixture_design_dialog,
                        &mut self.sheet_dialog,
                        None,
                    );
                    for name in ["Quality", "Reliability", "Series and Signals", "Visualize"] {
                        planned_menu(ui, name, "Open a dataset first");
                    }
                    self.scripts_menu(ui, actions);
                    self.help_menu(ui, actions);
                    return;
                };
                crate::menus::menu_button(ui, "Edit", |ui| {
                    if let Some((d, name, undo, redo)) = &dashboard {
                        // Showing a dashboard: Undo and Redo act on its edits.
                        ui.label(RichText::new(format!("Dashboard “{name}”")).weak().small());
                        let undo_text = undo.as_ref().map_or("Undo".to_owned(), |l| format!("Undo {}", l.to_lowercase()));
                        let undo_button =
                            egui::Button::new(undo_text).shortcut_text(ctx.format_shortcut(&UNDO_SHORTCUT));
                        if ui.add_enabled(undo.is_some(), undo_button).clicked() {
                            actions.push(Action::DashboardUndo(*d));
                            ui.close_menu();
                        }
                        let redo_text = redo.as_ref().map_or("Redo".to_owned(), |l| format!("Redo {}", l.to_lowercase()));
                        let redo_button =
                            egui::Button::new(redo_text).shortcut_text(ctx.format_shortcut(&redo_shortcut()));
                        if ui.add_enabled(redo.is_some(), redo_button).clicked() {
                            actions.push(Action::DashboardRedo(*d));
                            ui.close_menu();
                        }
                    } else {
                        let undo = doc.history.next_undo().map(|(l, _)| l.to_owned());
                        let redo = doc.history.next_redo().map(|(l, _)| l.to_owned());
                        let undo_text = match &restorable {
                            Some(name) => format!("Undo remove dashboard “{name}”"),
                            None => undo.map_or("Undo".to_owned(), |l| format!("Undo {l}")),
                        };
                        let undo_button =
                            egui::Button::new(undo_text).shortcut_text(ctx.format_shortcut(&UNDO_SHORTCUT));
                        let can_undo = restorable.is_some() || doc.history.next_undo().is_some();
                        if ui.add_enabled(can_undo, undo_button).clicked() {
                            actions.push(Action::Grid(vec![GridRequest::Undo]));
                            ui.close_menu();
                        }
                        let redo_text = redo.map_or("Redo".to_owned(), |l| format!("Redo {l}"));
                        let redo_button =
                            egui::Button::new(redo_text).shortcut_text(ctx.format_shortcut(&redo_shortcut()));
                        if ui.add_enabled(doc.history.next_redo().is_some(), redo_button).clicked() {
                            actions.push(Action::Grid(vec![GridRequest::Redo]));
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    let cut = egui::Button::new("Cut").shortcut_text(ctx.format_shortcut(&CUT_SHORTCUT));
                    if ui.add(cut).on_hover_text("Copy the selected cells, then clear them").clicked() {
                        actions.push(Action::Cut);
                        ui.close_menu();
                    }
                    let copy = egui::Button::new("Copy").shortcut_text(ctx.format_shortcut(&COPY_SHORTCUT));
                    if ui
                        .add(copy)
                        .on_hover_text(
                            "The selected columns (with their names), else the block of cells, else two or more \
                             selected rows, else the cell at the cursor, as tab-separated text",
                        )
                        .clicked()
                    {
                        actions.push(Action::Copy { rows_only: false });
                        ui.close_menu();
                    }
                    let n_selected = doc.rows.counts().selected;
                    let copy_rows = egui::Button::new("Copy selected rows with column names");
                    if ui.add_enabled(n_selected > 0, copy_rows).on_disabled_hover_text("Select rows first").clicked() {
                        actions.push(Action::Copy { rows_only: true });
                        ui.close_menu();
                    }
                    let paste = egui::Button::new("Paste").shortcut_text(ctx.format_shortcut(&PASTE_SHORTCUT));
                    if ui
                        .add(paste)
                        .on_hover_text(
                            "Tab- or comma-separated text from the clipboard, starting at the cell cursor; \
                             adds rows and columns as needed",
                        )
                        .clicked()
                    {
                        actions.push(Action::Paste);
                        ui.close_menu();
                    }
                    let clear = egui::Button::new("Clear cells").shortcut_text(ctx.format_shortcut(&CLEAR_SHORTCUT));
                    if ui.add(clear).on_hover_text("Make the selected cells missing; undoable").clicked() {
                        actions.push(Action::ClearCells);
                        ui.close_menu();
                    }
                    let fill = egui::Button::new("Fill down").shortcut_text(ctx.format_shortcut(&FILL_DOWN_SHORTCUT));
                    if ui
                        .add(fill)
                        .on_hover_text("Copy the first row of the selected cells into the rows below it")
                        .clicked()
                    {
                        actions.push(Action::FillDown);
                        ui.close_menu();
                    }
                    let all =
                        egui::Button::new("Select all cells").shortcut_text(ctx.format_shortcut(&SELECT_ALL_SHORTCUT));
                    if ui.add(all).clicked() {
                        actions.push(Action::SelectAllCells);
                        ui.close_menu();
                    }
                    let find = egui::Button::new("Find…").shortcut_text(ctx.format_shortcut(&FIND_SHORTCUT));
                    if ui.add(find).on_hover_text("Find text in the grid's cells and select matching rows").clicked() {
                        actions.push(Action::Find);
                        ui.close_menu();
                    }
                    ui.separator();
                    let n = doc.table.n_rows();
                    // Also the grid toolbar's + Row menu.
                    ui.menu_button("Add rows", |ui| {
                        let count = table_view::how_many(ui, table_view::ROWS_KEY, table_view::MAX_ROWS, None);
                        let add = format!("Add {} at the end", table_view::plural(count, "row"));
                        if ui.button(add).clicked() {
                            actions.push(Action::Grid(vec![GridRequest::InsertRows { at: n, count }]));
                            ui.close_menu();
                        }
                    });
                    let selected = doc.rows.selected_rows();
                    let delete = egui::Button::new(format!("Delete selected rows ({})", selected.len()));
                    if ui.add_enabled(!selected.is_empty(), delete).clicked() {
                        actions.push(Action::Grid(vec![GridRequest::RemoveRows(selected)]));
                        ui.close_menu();
                    }
                    let columns = doc.grid.selected_columns().to_vec();
                    let delete = egui::Button::new(format!("Delete selected columns ({})", columns.len()));
                    if ui
                        .add_enabled(!columns.is_empty(), delete)
                        .on_disabled_hover_text("Select columns first by clicking their headings")
                        .clicked()
                    {
                        actions.push(Action::Grid(vec![GridRequest::RemoveColumns(columns)]));
                        ui.close_menu();
                    }
                    ui.separator();
                    let at = doc.table.n_cols();
                    // The same choices, in the same words, as the grid toolbar's + Column.
                    ui.menu_button("Add column", |ui| {
                        let count =
                            table_view::how_many(ui, table_view::COLUMNS_KEY, table_view::MAX_COLUMNS, None);
                        let name = table_view::column_name_field(ui);
                        for (label, kind) in table_view::column_kinds() {
                            if ui.button(label).clicked() {
                                let name = name.clone();
                                actions.push(Action::Grid(vec![GridRequest::InsertColumn { at, kind, count, name }]));
                                table_view::forget_column_name(ui.ctx());
                                ui.close_menu();
                            }
                        }
                        if ui
                            .button("Formula…")
                            .on_hover_text("A column computed from the others, recomputed whenever the data changes")
                            .clicked()
                        {
                            actions.push(Action::Grid(vec![GridRequest::Formula { column: None, at }]));
                            ui.close_menu();
                        }
                        // Also Data ▸ New random column…
                        if ui
                            .button("Random values…")
                            .on_hover_text("A seeded column of random draws from any distribution; undoable")
                            .clicked()
                        {
                            self.compare_dialog =
                                opened!(self.dialog_memory, doc, CompareDialog::new(CompareKind::RandomColumn, &doc.table));
                            ui.close_menu();
                        }
                    });
                });
                crate::menus::menu_button(ui, "Data", |ui| {
                    let refresh = egui::Button::new("Refresh from source");
                    let response = ui
                        .add_enabled(doc.external.is_some(), refresh)
                        .on_disabled_hover_text("For tables imported from a database or the web");
                    let response = match &doc.external {
                        Some(source) => response.on_hover_text(format!(
                            "Read the table again from {}; undoable",
                            source.describe()
                        )),
                        None => response,
                    };
                    if response.clicked() {
                        if let Some(source) = doc.external.clone() {
                            self.external_dialog = Some(crate::external_dialog::ExternalDialog::refresh(
                                doc.id,
                                source,
                                doc.secrets.clone(),
                            ));
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    let rows = &mut doc.rows;
                    let counts = rows.counts();
                    let any_selected = counts.selected > 0;
                    let n_cols = doc.table.n_cols();
                    ui.menu_button("Row states", |ui| {
                        if ui.button("Select all rows").clicked() {
                            rows.select(0..rows.len(), stat_data::SelectionMode::Replace);
                            ui.close_menu();
                        }
                        if ui.button("Clear selection").clicked() {
                            rows.clear_selection();
                            ui.close_menu();
                        }
                        if ui.button("Invert selection").clicked() {
                            rows.invert_selection();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(any_selected, egui::Button::new("Exclude selected rows from analyses"))
                            .clicked()
                        {
                            rows.exclude_selected();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(any_selected, egui::Button::new("Include selected rows in analyses"))
                            .clicked()
                        {
                            rows.set_excluded(rows.selected_rows(), false);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(counts.excluded > 0, egui::Button::new("Include all rows in analyses"))
                            .clicked()
                        {
                            rows.include_all();
                            ui.close_menu();
                        }
                        if ui.add_enabled(counts.excluded > 0, egui::Button::new("Select excluded rows")).clicked() {
                            let excluded: Vec<usize> = (0..rows.len()).filter(|&r| rows.is_excluded(r)).collect();
                            rows.select(excluded, stat_data::SelectionMode::Replace);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.add_enabled(any_selected, egui::Button::new("Hide selected rows in plots")).clicked() {
                            rows.hide_selected();
                            ui.close_menu();
                        }
                        if ui.add_enabled(any_selected, egui::Button::new("Show selected rows in plots")).clicked() {
                            rows.set_hidden(rows.selected_rows(), false);
                            ui.close_menu();
                        }
                        if ui.add_enabled(counts.hidden > 0, egui::Button::new("Show all rows in plots")).clicked() {
                            rows.unhide_all();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(n_cols > 0, egui::Button::new("Colour and marker by column…"))
                            .on_hover_text(
                                "Give every row a colour (and a marker shape) from its value in a column; plots \
                                 without a grouping of their own draw them",
                            )
                            .clicked()
                        {
                            actions.push(Action::Grid(vec![GridRequest::ColorBy(None)]));
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(rows.has_row_styles(), egui::Button::new("Clear row colours and markers"))
                            .clicked()
                        {
                            rows.clear_row_styles();
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Row subsets", |ui| subsets::subset_menu_items(ui, doc, &mut self.subset_prompt));
                    let table = &doc.table;
                    let mut open = |kind| {
                        self.table_op_dialog =
                            opened!(self.dialog_memory, doc, TableOpDialog::new(kind, table, n_documents, active));
                    };
                    let several = n_documents > 1;
                    ui.menu_button("Rows", |ui| {
                        for (kind, label, tip) in [
                            (TableOpKind::Sort, "Sort…", "Order rows by one or more columns; undoable"),
                            (
                                TableOpKind::RemoveDuplicates,
                                "Remove duplicates…",
                                "Keep the first of each set of repeated rows; undoable",
                            ),
                            (
                                TableOpKind::Subset,
                                "Subset and sample…",
                                "Chosen rows and columns, or a random sample, as a new table",
                            ),
                        ] {
                            if ui.button(label).on_hover_text(tip).clicked() {
                                open(kind);
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        if ui
                            .button("Use row as column names")
                            .on_hover_text(
                                "The row of the cell cursor (or the one selected row) becomes the column names and \
                                 is removed; undoable",
                            )
                            .clicked()
                        {
                            actions.push(Action::UseRowAsNames);
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Reshape", |ui| {
                        for (kind, label, tip) in [
                            (
                                TableOpKind::Stack,
                                "Stack columns…",
                                "Wide to long: columns become a label and a value column",
                            ),
                            (
                                TableOpKind::Unstack,
                                "Unstack by label…",
                                "Long to wide: one column per level of a label column",
                            ),
                            (TableOpKind::Transpose, "Transpose…", "Rows become columns"),
                            (
                                TableOpKind::DistanceMatrix,
                                "Distance matrix…",
                                "The distance between every pair of rows (Euclidean, Gower, Jaccard, Bray–Curtis…) \
                                 as a new table",
                            ),
                        ] {
                            if ui.button(label).on_hover_text(tip).clicked() {
                                open(kind);
                                ui.close_menu();
                            }
                        }
                    });
                    ui.menu_button("Merge", |ui| {
                        let tip = if several { "" } else { " (open another table first)" };
                        for (kind, label, what) in [
                            (
                                TableOpKind::CombineRows,
                                "Combine rows…",
                                "Append other tables' rows, matching columns by name",
                            ),
                            (TableOpKind::Join, "Join…", "Match rows of another table by key columns"),
                        ] {
                            if ui
                                .add_enabled(several, egui::Button::new(label))
                                .on_hover_text(format!("{what}{tip}"))
                                .on_disabled_hover_text(format!("{what}{tip}"))
                                .clicked()
                            {
                                open(kind);
                                ui.close_menu();
                            }
                        }
                    });
                    if ui
                        .button("Summarize by group…")
                        .on_hover_text("Statistics of columns for each group, as a new table")
                        .clicked()
                    {
                        open(TableOpKind::Summarize);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(doc.table.n_cols() > 0, egui::Button::new("Recode values…"))
                        .on_hover_text("Give each distinct value of a column a new one: merge categories, fix spelling")
                        .clicked()
                    {
                        let column = doc.grid.current_column().unwrap_or(0);
                        actions.push(Action::Grid(vec![GridRequest::Recode(column)]));
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(doc.table.n_cols() > 0, egui::Button::new("Value labels…"))
                        .on_hover_text(
                            "Name what stored values stand for (1 = Treatment); analyses, plots and keys show the names",
                        )
                        .clicked()
                    {
                        let column = doc.grid.current_column().unwrap_or(0);
                        actions.push(Action::Grid(vec![GridRequest::ValueLabels(column)]));
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(doc.table.n_cols() > 0, egui::Button::new("Impute missing values…"))
                        .on_hover_text(
                            "Fill in missing values of numeric columns from each row's other values (EM, low-rank \
                             SVD, or the column mean or median); see the missing-data patterns",
                        )
                        .clicked()
                    {
                        self.imputation_dialog = opened!(
                            self.dialog_memory,
                            doc,
                            crate::imputation_dialog::ImputationDialog::new(&doc.table)
                        );
                        ui.close_menu();
                    }
                    if ui
                        .button("New random column…")
                        .on_hover_text("A seeded column of random draws from any distribution; undoable")
                        .clicked()
                    {
                        self.compare_dialog =
                            opened!(self.dialog_memory, doc, CompareDialog::new(CompareKind::RandomColumn, &doc.table));
                        ui.close_menu();
                    }
                    if ui
                        .button("New rank column…")
                        .on_hover_text(
                            "A formula column of the ranks of a numeric column, optionally within groups, with a \
                             choice of how ties are ranked",
                        )
                        .clicked()
                    {
                        let column = doc.grid.current_column();
                        self.rank_dialog = Some(table_view::rank::RankDialog::new(doc.id, &doc.table, column));
                        ui.close_menu();
                    }
                });
                let table = &doc.table;
                crate::menus::menu_button(ui, "Describe", |ui| {
                    if ui.button("Distribution…").clicked() {
                        self.distribution_dialog = opened!(self.dialog_memory, doc, DistributionDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Identify distribution…")
                        .on_hover_text("Fit fourteen distributions to one column and rank them by goodness of fit")
                        .clicked()
                    {
                        let current = doc.grid.current_column();
                        self.distribution_fit_dialog =
                            opened!(self.dialog_memory, doc, DistributionFitDialog::new(table, current));
                        ui.close_menu();
                    }
                    if ui.button("Bivariate…").clicked() {
                        self.bivariate_dialog = opened!(self.dialog_memory, doc, BivariateDialog::default());
                        ui.close_menu();
                    }
                    if ui
                        .button("Multi-way table…")
                        .on_hover_text(
                            "Counts, percentages and summary statistics by several nested row and column \
                             classifications",
                        )
                        .clicked()
                    {
                        self.tabulation_dialog = opened!(
                            self.dialog_memory,
                            doc,
                            crate::tabulation_dialog::TabulationDialog::new(
                                crate::tabulation_dialog::TabulationTool::MultiwayTable,
                            )
                        );
                        ui.close_menu();
                    }
                    if ui
                        .button("Multiple response…")
                        .on_hover_text(
                            "Check-all-that-apply answers: how often each item was picked, as a share of cases \
                             and of picks, with a cross-tab and tests by group",
                        )
                        .clicked()
                    {
                        self.compare_dialog = opened!(
                            self.dialog_memory,
                            doc,
                            CompareDialog::new(CompareKind::MultipleResponseItems, table)
                        );
                        ui.close_menu();
                    }
                    // Also Data ▸ Summarize by group…: where a summary-statistics table is looked for first.
                    if ui
                        .button("Summarize by group…")
                        .on_hover_text("Statistics of columns for each group, as a new table")
                        .clicked()
                    {
                        self.table_op_dialog = opened!(
                            self.dialog_memory,
                            doc,
                            TableOpDialog::new(TableOpKind::Summarize, table, n_documents, active)
                        );
                        ui.close_menu();
                    }
                    if ui
                        .button("Area under a curve…")
                        .on_hover_text(
                            "Trapezoid area of sampled x–y data above a baseline, with its standard error from \
                             replicates and the peaks above the baseline",
                        )
                        .clicked()
                    {
                        self.compare_dialog =
                            opened!(self.dialog_memory, doc, CompareDialog::new(CompareKind::Area, table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Pharmacokinetic parameters…")
                        .on_hover_text(
                            "Non-compartmental analysis of concentration–time profiles: Cmax, Tmax, AUC, terminal \
                             half-life, clearance and volume per subject, geometric means by treatment",
                        )
                        .clicked()
                    {
                        self.reliability_dialog =
                            opened!(self.dialog_memory, doc, ReliabilityDialog::pharmacokinetics());
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .button("Distribution calculator…")
                        .on_hover_text("Probabilities, quantiles and density of any of 22 distributions; uses no data")
                        .clicked()
                    {
                        self.compare_dialog =
                            opened!(self.dialog_memory, doc, CompareDialog::new(CompareKind::Calculator, table));
                        ui.close_menu();
                    }
                });
                crate::menus::menu_button(ui, "Compare", |ui| {
                    let items = [
                        (
                            CompareKind::OneSample,
                            "One sample vs a value…",
                            "t, z, signed-rank, sign and standard-deviation tests; equivalence",
                        ),
                        (
                            CompareKind::Groups,
                            "Two or more groups…",
                            "t tests, ANOVA, rank tests, equal variances, all-pairs comparisons",
                        ),
                        (
                            CompareKind::ManyTests,
                            "Two groups, many variables…",
                            "A t or rank-sum test of two groups for every chosen column, with adjusted p-values and \
                             a volcano plot",
                        ),
                        (
                            CompareKind::Paired,
                            "Paired or repeated measures…",
                            "Paired t, signed-rank, sign test; repeated-measures ANOVA, Friedman",
                        ),
                        (
                            CompareKind::FactorialRepeated,
                            "Factorial repeated measures…",
                            "Two- and three-way repeated-measures ANOVA with sphericity corrections and simple effects",
                        ),
                        (
                            CompareKind::Nested,
                            "Nested comparison…",
                            "Nested t test or one-way ANOVA: several measurements per subject, subjects within groups",
                        ),
                        (
                            CompareKind::Proportions,
                            "Proportions…",
                            "Exact binomial, two-proportion, Fisher exact and McNemar tests",
                        ),
                        (CompareKind::Rates, "Event rates…", "Exact Poisson tests of rates and rate ratios"),
                        (
                            CompareKind::GoodnessOfFit,
                            "Goodness of fit for counts…",
                            "Chi-square tests of categories against given proportions, or counts against a Poisson",
                        ),
                        (
                            CompareKind::Crossover,
                            "Crossover trial and equivalence…",
                            "Two-period crossover: treatment, period and carryover effects; average equivalence",
                        ),
                    ];
                    for (kind, label, tip) in items {
                        if ui.button(label).on_hover_text(tip).clicked() {
                            self.compare_dialog = opened!(self.dialog_memory, doc, CompareDialog::new(kind, table));
                            ui.close_menu();
                        }
                    }
                    if ui
                        .button("Stratified tables and exact tests…")
                        .on_hover_text(
                            "Cochran–Mantel–Haenszel tests, the common odds ratio and Breslow–Day test across \
                             strata; Fisher's exact test for r × c tables",
                        )
                        .clicked()
                    {
                        self.tabulation_dialog = opened!(
                            self.dialog_memory,
                            doc,
                            crate::tabulation_dialog::TabulationDialog::new(
                                crate::tabulation_dialog::TabulationTool::Stratified,
                            )
                        );
                        ui.close_menu();
                    }
                    if ui
                        .button("Permutation test…")
                        .on_hover_text(
                            "Two groups, paired columns or a correlation, against every rearrangement of the data \
                             (or random ones): no normal assumption",
                        )
                        .clicked()
                    {
                        self.permutation_dialog =
                            opened!(self.dialog_memory, doc, crate::resampling_dialogs::PermutationDialog::default());
                        ui.close_menu();
                    }
                    if ui
                        .button("Adjust and combine p-values…")
                        .on_hover_text(
                            "A column of p-values adjusted for multiple testing (Bonferroni, Holm, Benjamini–Hochberg, \
                             q-values and more) and combined by Fisher's and Stouffer's methods",
                        )
                        .clicked()
                    {
                        self.compare_dialog =
                            opened!(self.dialog_memory, doc, CompareDialog::new(CompareKind::PValues, table));
                        ui.close_menu();
                    }
                    if let Some(tool) = bayes_submenu(ui) {
                        self.bayes_dialog =
                            opened!(self.dialog_memory, doc, crate::bayes_dialog::BayesDialog::new(tool, table));
                    }
                    ui.separator();
                    if ui
                        .button("Power and sample size…")
                        .on_hover_text("Plan a study: power, required sample size, or detectable difference")
                        .clicked()
                    {
                        self.compare_dialog =
                            opened!(self.dialog_memory, doc, CompareDialog::new(CompareKind::Power, table));
                        ui.close_menu();
                    }
                });
                crate::menus::menu_button(ui, "Model", |ui| {
                    menu_heading(ui, "Linear models");
                    if ui.button("Linear model…").clicked() {
                        self.dialog = opened!(self.dialog_memory, doc, ModelDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Response surface (second-order model)…")
                        .on_hover_text("Fit a quadratic model, find the optimum, ridge paths and contour plots")
                        .clicked()
                    {
                        self.dialog = opened!(self.dialog_memory, doc, ModelDialog::surface(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Mixture model…")
                        .on_hover_text(
                            "Scheffé models for blends whose components sum to one, with a ternary contour plot",
                        )
                        .clicked()
                    {
                        self.mixture_dialog = opened!(self.dialog_memory, doc, MixtureDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Stepwise selection…")
                        .on_hover_text("Choose effects by p-values, AIC or BIC, for one response or each of several")
                        .clicked()
                    {
                        self.dialog = opened!(self.dialog_memory, doc, ModelDialog::stepwise(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Best subsets…")
                        .on_hover_text("The best models of each size among every subset of the candidate effects")
                        .clicked()
                    {
                        self.dialog = opened!(self.dialog_memory, doc, ModelDialog::best_subsets(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Screen many responses…")
                        .on_hover_text(
                            "The same model fitted to every chosen response column, with false-discovery-rate \
                             adjusted p-values per effect, volcano and forest plots",
                        )
                        .clicked()
                    {
                        self.compare_dialog =
                            opened!(self.dialog_memory, doc, CompareDialog::new(CompareKind::Screening, table));
                        ui.close_menu();
                    }
                    menu_heading(ui, "Generalized, nonlinear and mixed");
                    if ui.button("Generalized linear model…").clicked() {
                        self.phase2_dialog =
                            opened!(self.dialog_memory, doc, Phase2Dialog::new(Phase2Kind::Glm, table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Ordinal or nominal logistic regression…")
                        .on_hover_text(
                            "A response with several categories: proportional odds for ordered categories, \
                             baseline-category logits for unordered ones; odds ratios and predicted probabilities",
                        )
                        .clicked()
                    {
                        self.dialog = opened!(self.dialog_memory, doc, ModelDialog::categorical(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Nonlinear regression…")
                        .on_hover_text(
                            "Least squares for a curve (exponential, logistic, Gompertz, Michaelis–Menten, power, \
                             rational) or a formula with named parameters, with profile intervals and bands",
                        )
                        .clicked()
                    {
                        self.nonlinear_dialog =
                            opened!(self.dialog_memory, doc, crate::nonlinear_dialog::NonlinearDialog::new());
                        ui.close_menu();
                    }
                    if ui
                        .button("Curve templates…")
                        .on_hover_text(
                            "Ready-made curves for assays: immunoassay standard curves, receptor binding, enzyme \
                             kinetics and inhibition, dose–response, growth and survival, with starting values \
                             from the data and assay-specific results",
                        )
                        .clicked()
                    {
                        self.curve_template_dialog = opened!(
                            self.dialog_memory,
                            doc,
                            crate::curve_template_dialog::CurveTemplateDialog::new()
                        );
                        ui.close_menu();
                    }
                    if ui
                        .button("Dose–response (probit)…")
                        .on_hover_text(
                            "Proportion responding against dose: probit, logit or complementary log-log line, \
                             effective doses (ED50) with fiducial limits",
                        )
                        .clicked()
                    {
                        self.regression_dialog = opened!(self.dialog_memory, doc, RegressionDialog::dose_response());
                        ui.close_menu();
                    }
                    if ui
                        .button("Mixed model…")
                        .on_hover_text(
                            "Fixed effects plus random intercepts and slopes for grouping columns (subjects, batches, \
                             sites); normal, binomial, Poisson, Gamma or negative binomial responses",
                        )
                        .clicked()
                    {
                        // A declared split-plot design fills in its mixed model.
                        let fresh = doc
                            .reports
                            .iter()
                            .find_map(|r| match r.saved() {
                                app_model::SavedReport::DeclaredDesign { request } => {
                                    app_model::split_plot_analysis(table, &request)
                                }
                                _ => None,
                            })
                            .and_then(|saved| crate::mixed_dialog::MixedDialog::from_saved(&saved, table))
                            .unwrap_or_else(|| crate::mixed_dialog::MixedDialog::new(table));
                        self.mixed_dialog = opened!(self.dialog_memory, doc, fresh);
                        ui.close_menu();
                    }
                    if let Some(tool) = bayes_submenu(ui) {
                        self.bayes_dialog =
                            opened!(self.dialog_memory, doc, crate::bayes_dialog::BayesDialog::new(tool, table));
                    }
                    if ui
                        .button("Treatment effects (observational data)…")
                        .on_hover_text(
                            "Effect of a two-level treatment adjusted for confounders: propensity scores, \
                             covariate balance, matching, inverse-probability or doubly robust weighting, and \
                             subgroup effects",
                        )
                        .clicked()
                    {
                        self.treatment_dialog =
                            opened!(self.dialog_memory, doc, crate::treatment_dialog::TreatmentDialog::new(table));
                        ui.close_menu();
                    }
                    menu_heading(ui, "Method comparison and stability");
                    for (label, hover, open) in [
                        (
                            "Method comparison…",
                            "Deming and Passing–Bablok regression of one measurement method on another, with a \
                             difference plot",
                            RegressionDialog::method_comparison as fn() -> RegressionDialog,
                        ),
                        (
                            "Shelf life…",
                            "Stability data of several batches: poolability tests and the time the confidence \
                             bound meets the acceptance limit",
                            RegressionDialog::shelf_life,
                        ),
                    ] {
                        if ui.button(label).on_hover_text(hover).clicked() {
                            self.regression_dialog = opened!(self.dialog_memory, doc, open());
                            ui.close_menu();
                        }
                    }
                    menu_heading(ui, "Predictive models");
                    if ui.button("Penalized regression (lasso, ridge)…").clicked() {
                        self.phase2_dialog =
                            opened!(self.dialog_memory, doc, Phase2Dialog::new(Phase2Kind::Regularized, table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Decision tree…")
                        .on_hover_text(
                            "Classification or regression tree: repeated splits on the most informative predictor, \
                             pruned on held-out rows; fit measures by training, validation and test rows",
                        )
                        .clicked()
                    {
                        self.tree_dialog = opened!(self.dialog_memory, doc, crate::tree_dialog::TreeDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Random forest…")
                        .on_hover_text(
                            "Many trees, each grown on a bootstrap sample with a random subset of predictors per \
                             split, averaged; out-of-bag error and permutation importance",
                        )
                        .clicked()
                    {
                        self.forest_dialog =
                            opened!(self.dialog_memory, doc, crate::forest_dialog::ForestDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Boosted trees…")
                        .on_hover_text(
                            "Small trees added one at a time, each correcting the fit so far (gradient boosting); \
                             validation rows choose the number of rounds",
                        )
                        .clicked()
                    {
                        self.boost_dialog =
                            opened!(self.dialog_memory, doc, crate::boost_dialog::BoostDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Partial least squares…")
                        .on_hover_text(
                            "Regression on a few latent factors that summarize many correlated predictors; \
                             held-out rows choose the number of factors (a categorical response gives PLS-DA)",
                        )
                        .clicked()
                    {
                        self.pls_dialog = opened!(self.dialog_memory, doc, crate::pls_dialog::PlsDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Support vector machine…")
                        .on_hover_text(
                            "Maximum-margin classification or ε-insensitive regression with linear, radial basis \
                             or polynomial kernels; held-out rows tune the cost and γ",
                        )
                        .clicked()
                    {
                        self.svm_dialog = opened!(self.dialog_memory, doc, crate::svm_dialog::SvmDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Adaptive regression splines…")
                        .on_hover_text(
                            "Multivariate adaptive regression splines: a continuous response built from \
                             hinge functions with knots found in the data and their products, pruned by GCV or \
                             held-out rows",
                        )
                        .clicked()
                    {
                        self.mars_dialog = opened!(self.dialog_memory, doc, crate::mars_dialog::MarsDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Neural network…")
                        .on_hover_text(
                            "A fully connected network with one or two hidden layers (tanh, logistic, rectified \
                             linear or identity nodes) for a continuous or categorical response; held-out rows tune \
                             the weight penalty",
                        )
                        .clicked()
                    {
                        self.neural_dialog =
                            opened!(self.dialog_memory, doc, crate::neural_dialog::NeuralDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Gaussian process…")
                        .on_hover_text(
                            "A smooth surface through a continuous response with its uncertainty everywhere, one \
                             length scale per predictor; suits computer experiments and small designed experiments",
                        )
                        .clicked()
                    {
                        self.gaussian_process_dialog = opened!(
                            self.dialog_memory,
                            doc,
                            crate::gaussian_process_dialog::GaussianProcessDialog::new(table)
                        );
                        ui.close_menu();
                    }
                    if ui
                        .button("Model search…")
                        .on_hover_text(
                            "Fit every predictive model that suits the response with its default settings, rank \
                             them by held-out error, and open the best one's report",
                        )
                        .clicked()
                    {
                        self.search_dialog =
                            opened!(self.dialog_memory, doc, crate::search_dialog::SearchDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Discriminant analysis…")
                        .on_hover_text(
                            "Linear or quadratic normal-theory classification from numeric predictors, with \
                             posterior probabilities, fit measures by role and canonical functions",
                        )
                        .clicked()
                    {
                        self.discriminant_dialog = opened!(
                            self.dialog_memory,
                            doc,
                            crate::discriminant_dialog::DiscriminantDialog::new(table)
                        );
                        ui.close_menu();
                    }
                    menu_heading(ui, "Compare and optimize");
                    if ui
                        .button("Compare models…")
                        .on_hover_text(
                            "Fit measures side by side for saved prediction columns, by validation role, with ROC, \
                             lift, gains and calibration plots (or actual and residual plots)",
                        )
                        .clicked()
                    {
                        self.model_comparison_dialog =
                            opened!(self.dialog_memory, doc, crate::comparison_dialog::ComparisonDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Several responses (desirability, simulation)…")
                        .on_hover_text(
                            "One model per response on the same factors (every term, or each response's own terms \
                             chosen stepwise), explored together: linked slice plots, desirability goals, a search \
                             for the best settings, and simulated defect rates",
                        )
                        .clicked()
                    {
                        self.phase2_dialog =
                            opened!(self.dialog_memory, doc, Phase2Dialog::new(Phase2Kind::SeveralResponses, table));
                        ui.close_menu();
                    }
                });
                crate::menus::menu_button(ui, "Multivariate", |ui| {
                    use crate::multivariate_methods_dialog::{MethodDialog, MethodKind};
                    menu_heading(ui, "Relationships");
                    for (label, hint, kind) in [
                        (
                            "Correlations…",
                            "Pearson and Spearman correlations of several columns with intervals and tests, partial \
                             correlations and a heat map",
                            MethodKind::Correlations,
                        ),
                        (
                            "Scatterplot matrix…",
                            "Every pair of columns in a grid of linked plots, with density ellipses and histograms",
                            MethodKind::ScatterMatrix,
                        ),
                        (
                            "Outlier distances…",
                            "Mahalanobis, jackknifed and T² distances of each row with reference limits",
                            MethodKind::OutlierDistances,
                        ),
                        (
                            "Item analysis…",
                            "Reliability of a multi-item scale: Cronbach's alpha, item–rest correlations, alpha if \
                             an item is deleted",
                            MethodKind::ItemAnalysis,
                        ),
                    ] {
                        if ui.button(label).on_hover_text(hint).clicked() {
                            self.method_dialog = opened!(self.dialog_memory, doc, MethodDialog::new(kind, table));
                            ui.close_menu();
                        }
                    }
                    menu_heading(ui, "Reduce dimensions");
                    if ui.button("Principal components…").clicked() {
                        self.phase2_dialog =
                            opened!(self.dialog_memory, doc, Phase2Dialog::new(Phase2Kind::Pca, table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Factor analysis…")
                        .on_hover_text(
                            "A few common factors behind the correlations of several columns: maximum-likelihood or \
                             principal-axis extraction, varimax or promax rotation, factor scores",
                        )
                        .clicked()
                    {
                        self.factor_dialog =
                            opened!(self.dialog_memory, doc, crate::factor_dialog::FactorDialog::new(table));
                        ui.close_menu();
                    }
                    if ui.button("Correspondence analysis…").clicked() {
                        self.phase2_dialog =
                            opened!(self.dialog_memory, doc, Phase2Dialog::new(Phase2Kind::Correspondence, table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Dimension reduction (MDS, t-SNE, UMAP)…")
                        .on_hover_text(
                            "A two-dimensional map of the rows by classical or metric multidimensional scaling, \
                             t-SNE or UMAP, from columns or a distance matrix",
                        )
                        .clicked()
                    {
                        use crate::cluster_map_dialog::{MapDialog, MapKind};
                        self.map_dialog = opened!(self.dialog_memory, doc, MapDialog::new(MapKind::Embedding, table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Principal coordinates…")
                        .on_hover_text(
                            "A map of the rows from any distance (Gower for mixed types, Jaccard, Bray–Curtis…) or \
                             a distance matrix, with every eigenvalue and corrections for negative ones",
                        )
                        .clicked()
                    {
                        self.reliability_dialog =
                            opened!(self.dialog_memory, doc, ReliabilityDialog::principal_coordinates());
                        ui.close_menu();
                    }
                    menu_heading(ui, "Clusters");
                    if ui.button("Clustering…").clicked() {
                        self.phase2_dialog =
                            opened!(self.dialog_memory, doc, Phase2Dialog::new(Phase2Kind::Cluster, table));
                        ui.close_menu();
                    }
                    {
                        use crate::cluster_map_dialog::{MapDialog, MapKind};
                        for (label, hint, kind) in [
                            (
                                "Gaussian mixture clustering…",
                                "Clusters as a mixture of normal distributions with several covariance shapes; the \
                                 number of clusters and the shape chosen by BIC; membership probabilities",
                                MapKind::GaussianMixture,
                            ),
                            (
                                "Latent class analysis…",
                                "Hidden classes behind several categorical items: class sizes and each class's \
                                 response probabilities, the number of classes chosen by BIC; class membership \
                                 probabilities",
                                MapKind::LatentClass,
                            ),
                        ] {
                            if ui.button(label).on_hover_text(hint).clicked() {
                                self.map_dialog = opened!(self.dialog_memory, doc, MapDialog::new(kind, table));
                                ui.close_menu();
                            }
                        }
                    }
                    menu_heading(ui, "Groups");
                    if ui
                        .button("Multivariate analysis of variance…")
                        .on_hover_text(
                            "Several responses on the same model effects: Wilks, Pillai, Hotelling–Lawley and Roy \
                             tests, contrasts and canonical analysis (MANOVA)",
                        )
                        .clicked()
                    {
                        self.dialog = opened!(self.dialog_memory, doc, ModelDialog::manova(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Discriminant analysis…")
                        .on_hover_text(
                            "Classify rows into the classes of a categorical column from numeric measurements \
                             (linear or quadratic rule), with canonical functions, a canonical plot and posterior \
                             probabilities",
                        )
                        .clicked()
                    {
                        self.discriminant_dialog = opened!(
                            self.dialog_memory,
                            doc,
                            crate::discriminant_dialog::DiscriminantDialog::new(table)
                        );
                        ui.close_menu();
                    }
                    menu_heading(ui, "Text");
                    {
                        use crate::text_dialog::{TextDialog, TextKind};
                        for (label, hint, kind) in [
                            (
                                "Text exploration…",
                                "Word and phrase frequencies of a text column, a word cloud, semantic dimensions \
                                 (latent semantic analysis) and clusters of the documents",
                                TextKind::Explore,
                            ),
                            (
                                "Topic model…",
                                "Topics as mixes of words and each document as a mix of topics (latent Dirichlet \
                                 allocation); topic proportions as columns",
                                TextKind::Topics,
                            ),
                            (
                                "Sentiment scores…",
                                "A positive-to-negative score for each document from a sentiment lexicon, with \
                                 negation handling; scores as columns",
                                TextKind::Sentiment,
                            ),
                        ] {
                            if ui.button(label).on_hover_text(hint).clicked() {
                                self.text_dialog = opened!(self.dialog_memory, doc, TextDialog::new(kind, table));
                                ui.close_menu();
                            }
                        }
                    }
                    menu_heading(ui, "Curves");
                    if ui
                        .button("Functional data…")
                        .on_hover_text(
                            "Each curve (rows sharing an identifier, measured over time) as one observation: \
                             smoothing in a B-spline or Fourier basis, landmark and continuous registration, \
                             functional principal components, and the scores modelled on curve-level factors with \
                             a profiler of the predicted curve",
                        )
                        .clicked()
                    {
                        self.functional_dialog =
                            opened!(self.dialog_memory, doc, crate::functional_dialog::FunctionalDialog::new(table));
                        ui.close_menu();
                    }
                });
                design_menu(
                    ui,
                    &self.dialog_memory,
                    &mut self.design_dialog,
                    &mut self.mixture_design_dialog,
                    &mut self.sheet_dialog,
                    Some(DesignTableTools {
                        table,
                        study: &mut self.study_dialog,
                        augment: &mut self.augment_dialog,
                        simulate: &mut self.simulate_dialog,
                        evaluation: &mut self.evaluation_dialog,
                        factorial: &mut self.factorial_dialog,
                        sequential: &mut self.sequential_dialog,
                    }),
                );
                crate::menus::menu_button(ui, "Quality", |ui| {
                    menu_heading(ui, "Control charts");
                    if ui
                        .button("Control chart…")
                        .on_hover_text(
                            "Individuals, subgroup mean, moving-average, three-way (between/within), zone and \
                             short-run charts",
                        )
                        .clicked()
                    {
                        self.chart_dialog = opened!(self.dialog_memory, doc, ChartDialog::default());
                        ui.close_menu();
                    }
                    if ui
                        .button("Count and rare-event chart (p, np, c, u, p′, u′, g, t)…")
                        .on_hover_text(
                            "Counts of defective units or defects per subgroup, overdispersion-adjusted p′ and u′, \
                             and the gaps between rare events",
                        )
                        .clicked()
                    {
                        self.attribute_dialog = opened!(self.dialog_memory, doc, AttributeDialog::default());
                        ui.close_menu();
                    }
                    if ui
                        .button("EWMA chart…")
                        .on_hover_text("Exponentially weighted moving average: sensitive to small sustained shifts")
                        .clicked()
                    {
                        self.time_weighted_dialog = opened!(self.dialog_memory, doc, TimeWeightedDialog::new(true));
                        ui.close_menu();
                    }
                    if ui
                        .button("CUSUM chart…")
                        .on_hover_text("Cumulative sums above and below target: detects small sustained shifts")
                        .clicked()
                    {
                        self.time_weighted_dialog = opened!(self.dialog_memory, doc, TimeWeightedDialog::new(false));
                        ui.close_menu();
                    }
                    if ui
                        .button("Multivariate control chart…")
                        .on_hover_text(
                            "Hotelling T², generalized variance |S| and MEWMA charts of several correlated \
                             measurements, with variable contributions to each signal",
                        )
                        .clicked()
                    {
                        self.multivariate_dialog = opened!(self.dialog_memory, doc, MultivariateDialog::default());
                        ui.close_menu();
                    }
                    if ui
                        .button("Principal-components control chart…")
                        .on_hover_text(
                            "T² and squared prediction error charts of many correlated measurements from a \
                             principal-components model of a reference period, with contributions and a change point",
                        )
                        .clicked()
                    {
                        self.monitoring_dialog = opened!(
                            self.dialog_memory,
                            doc,
                            crate::monitoring_dialog::MonitoringDialog::new(crate::monitoring_dialog::MonitoringTool::PcaChart)
                        );
                        ui.close_menu();
                    }
                    if ui
                        .button("Monitoring alert and action limits…")
                        .on_hover_text(
                            "Environmental monitoring: alert and action levels for counts, positive samples or \
                             readings by location, from the best-fitting distribution, with early-warning rules",
                        )
                        .clicked()
                    {
                        self.monitoring_dialog = opened!(
                            self.dialog_memory,
                            doc,
                            crate::monitoring_dialog::MonitoringDialog::new(crate::monitoring_dialog::MonitoringTool::Limits)
                        );
                        ui.close_menu();
                    }
                    menu_heading(ui, "Process capability");
                    if ui
                        .button("Process capability…")
                        .on_hover_text("Cp, Cpk, Pp, Ppk and nonconforming rates against specification limits")
                        .clicked()
                    {
                        self.capability_dialog = opened!(self.dialog_memory, doc, CapabilityDialog::default());
                        ui.close_menu();
                    }
                    if ui
                        .button("Capability summary page…")
                        .on_hover_text(
                            "Capability indices with control charts, the last subgroups, a histogram and a \
                             probability plot on one page",
                        )
                        .clicked()
                    {
                        self.capability_dialog = opened!(self.dialog_memory, doc, CapabilityDialog::summary_page());
                        ui.close_menu();
                    }
                    if ui
                        .button("Between/within capability…")
                        .on_hover_text(
                            "Capability of batches or streams, with between- and within-subgroup σ separated",
                        )
                        .clicked()
                    {
                        self.quality_tool_dialog =
                            opened!(self.dialog_memory, doc, QualityToolDialog::new(QualityTool::BetweenWithin));
                        ui.close_menu();
                    }
                    if ui
                        .button("Attribute capability…")
                        .on_hover_text(
                            "Percent defective (binomial) or defects per unit (Poisson) with exact intervals",
                        )
                        .clicked()
                    {
                        self.quality_tool_dialog =
                            opened!(self.dialog_memory, doc, QualityToolDialog::new(QualityTool::AttributeCapability));
                        ui.close_menu();
                    }
                    menu_heading(ui, "Measurement systems");
                    if ui
                        .button("Gauge R&R…")
                        .on_hover_text("Crossed, nested (destructive) or expanded gauge studies")
                        .clicked()
                    {
                        self.gauge_dialog = opened!(self.dialog_memory, doc, GaugeDialog::new(table));
                        ui.close_menu();
                    }
                    if ui
                        .button("Gauge study worksheet…")
                        .on_hover_text("A new table of randomized runs for a crossed or nested gauge study")
                        .clicked()
                    {
                        let fresh = SheetDialog::new(SheetKind::GaugeWorksheet);
                        self.sheet_dialog = Some(self.dialog_memory.recall(0, fresh, None, &[]));
                        ui.close_menu();
                    }
                    for (label, hint, tool) in [
                        (
                            "Bias and linearity…",
                            "Bias at each reference value and how it changes across the range",
                            StudyTool::Linearity,
                        ),
                        (
                            "Gauge repeatability (Cg, Cgk)…",
                            "Repeated measurements of one master part against a tolerance",
                            StudyTool::Repeatability,
                        ),
                        (
                            "Intraclass correlation…",
                            "Shrout–Fleiss reliability forms and how well the gauge tracks the product",
                            StudyTool::Icc,
                        ),
                    ] {
                        if ui.button(label).on_hover_text(hint).clicked() {
                            self.study_dialog = opened!(self.dialog_memory, doc, StudyDialog::new(tool, table));
                            ui.close_menu();
                        }
                    }
                    for (label, hint, tool) in [
                        (
                            "Attribute agreement…",
                            "How often appraisers agree with themselves, each other and a standard when they rate \
                             parts into categories: percent agreement, kappa and concordance",
                            AgreementTool::Agreement,
                        ),
                        (
                            "Attribute gauge bias and repeatability…",
                            "A go/no-go gauge's bias and repeatability from acceptance counts at reference values",
                            AgreementTool::Gauge,
                        ),
                    ] {
                        if ui.button(label).on_hover_text(hint).clicked() {
                            self.agreement_dialog = opened!(self.dialog_memory, doc, AgreementDialog::new(tool, table));
                            ui.close_menu();
                        }
                    }
                    menu_heading(ui, "Acceptance sampling");
                    if ui
                        .button("Acceptance sampling plan…")
                        .on_hover_text("Single, double and variables plans from risk points; OC, AOQ and ATI curves")
                        .clicked()
                    {
                        self.quality_tool_dialog =
                            opened!(self.dialog_memory, doc, QualityToolDialog::new(QualityTool::Acceptance));
                        ui.close_menu();
                    }
                    menu_heading(ui, "Problem-solving tools");
                    if ui
                        .button("Analysis of means…")
                        .on_hover_text(
                            "Compare each group's mean, proportion or rate with the overall value against \
                             decision limits",
                        )
                        .clicked()
                    {
                        self.anom_dialog = opened!(self.dialog_memory, doc, AnomDialog::default());
                        ui.close_menu();
                    }
                    if ui
                        .button("Pareto chart…")
                        .on_hover_text("Defect categories ranked by count, with the cumulative percentage")
                        .clicked()
                    {
                        self.pareto_dialog = opened!(self.dialog_memory, doc, ParetoDialog::default());
                        ui.close_menu();
                    }
                    if ui
                        .button("Cause-and-effect diagram…")
                        .on_hover_text("A fishbone diagram drawn from columns of causes and sub-causes")
                        .clicked()
                    {
                        self.cause_effect_dialog = opened!(self.dialog_memory, doc, CauseEffectDialog::default());
                        ui.close_menu();
                    }
                    if ui
                        .button("Multi-vari chart…")
                        .on_hover_text("Means and values of a response across up to four nested or crossed factors")
                        .clicked()
                    {
                        self.multi_vari_dialog = opened!(self.dialog_memory, doc, MultiVariDialog::default());
                        ui.close_menu();
                    }
                });
                crate::menus::menu_button(ui, "Reliability", |ui| {
                    menu_heading(ui, "Life data");
                    if ui
                        .button("Survival curves…")
                        .on_hover_text("Kaplan–Meier curves, log-rank test, quick Weibull and lognormal fits")
                        .clicked()
                    {
                        self.survival_dialog = opened!(self.dialog_memory, doc, SurvivalDialog::default());
                        ui.close_menu();
                    }
                    if ui
                        .button("Life distributions…")
                        .on_hover_text(
                            "Fourteen distributions fitted to failure times with right, left or interval censoring: \
                             model comparison, probability plots, percentiles, Weibull with known shape",
                        )
                        .clicked()
                    {
                        self.reliability_dialog =
                            opened!(self.dialog_memory, doc, ReliabilityDialog::life_distribution());
                        ui.close_menu();
                    }
                    if ui
                        .button("Competing risks…")
                        .on_hover_text(
                            "Several causes of failure: cumulative incidence per cause, Gray's test, subdistribution \
                             hazards regression, failure modes combined into the system's life",
                        )
                        .clicked()
                    {
                        self.reliability_dialog =
                            opened!(self.dialog_memory, doc, ReliabilityDialog::competing_risks());
                        ui.close_menu();
                    }
                    menu_heading(ui, "Life data with predictors");
                    if ui
                        .button("Proportional hazards…")
                        .on_hover_text(
                            "Cox regression of censored failure times on predictors: hazard ratios, strata, a test \
                             of proportional hazards, adjusted survival curves",
                        )
                        .clicked()
                    {
                        self.reliability_dialog =
                            opened!(self.dialog_memory, doc, ReliabilityDialog::proportional_hazards());
                        ui.close_menu();
                    }
                    if ui
                        .button("Parametric survival regression…")
                        .on_hover_text(
                            "Weibull, lognormal, log-logistic or exponential life on predictors with right, left or \
                             interval censoring: time ratios, percentiles and failure probabilities",
                        )
                        .clicked()
                    {
                        self.reliability_dialog =
                            opened!(self.dialog_memory, doc, ReliabilityDialog::survival_regression());
                        ui.close_menu();
                    }
                    if ui
                        .button("Life–stress models…")
                        .on_hover_text(
                            "Accelerated life tests: life distribution whose location depends on temperature, \
                             voltage or other stresses; activation energy, acceleration factors, life at use \
                             conditions",
                        )
                        .clicked()
                    {
                        self.reliability_dialog = opened!(self.dialog_memory, doc, ReliabilityDialog::life_stress());
                        ui.close_menu();
                    }
                    if ui
                        .button("Fatigue (stress–life) curves…")
                        .on_hover_text(
                            "Cycles to failure against stress with run-outs censored: Basquin and other S–N \
                             curves, B10 and median curves with a lower bound, fatigue strength, endurance limit",
                        )
                        .clicked()
                    {
                        self.reliability_dialog = opened!(self.dialog_memory, doc, ReliabilityDialog::fatigue());
                        ui.close_menu();
                    }
                    if ui
                        .button("Degradation…")
                        .on_hover_text(
                            "Measurements drifting towards a failure threshold: unit paths and pseudo failure \
                             times, random-effects paths, or destructive tests",
                        )
                        .clicked()
                    {
                        self.reliability_dialog = opened!(self.dialog_memory, doc, ReliabilityDialog::degradation());
                        ui.close_menu();
                    }
                    menu_heading(ui, "Field and repair data");
                    if ui
                        .button("Warranty returns…")
                        .on_hover_text(
                            "Shipments and returns by period: life data from the counts, fitted distributions and \
                             a forecast of future returns with prediction limits",
                        )
                        .clicked()
                    {
                        self.reliability_dialog = opened!(self.dialog_memory, doc, ReliabilityDialog::warranty());
                        ui.close_menu();
                    }
                    if ui
                        .button("Repairable systems…")
                        .on_hover_text(
                            "Mean cumulative repairs per system with confidence limits, two populations compared, \
                             process fits and trend tests",
                        )
                        .clicked()
                    {
                        self.reliability_dialog = opened!(self.dialog_memory, doc, ReliabilityDialog::recurrence());
                        ui.close_menu();
                    }
                    if ui
                        .button("Reliability growth…")
                        .on_hover_text(
                            "Power-law (Crow–AMSAA) model of a developing system's failures: intensity, MTBF, \
                             goodness of fit, phases",
                        )
                        .clicked()
                    {
                        self.reliability_dialog = opened!(self.dialog_memory, doc, ReliabilityDialog::growth());
                        ui.close_menu();
                    }
                    menu_heading(ui, "Planning");
                    if ui
                        .button("Test plans…")
                        .on_hover_text(
                            "Demonstration, estimation and accelerated life test plans: how many units, for how long",
                        )
                        .clicked()
                    {
                        self.reliability_dialog = opened!(self.dialog_memory, doc, ReliabilityDialog::test_plan());
                        ui.close_menu();
                    }
                });
                crate::menus::menu_button(ui, "Series and Signals", |ui| {
                    use crate::series_explore_dialog::{ExploreKind, SeriesExploreDialog};
                    menu_heading(ui, "Explore");
                    for (kind, label, tip) in [
                        (
                            ExploreKind::Describe,
                            "Series description…",
                            "Time series plot, optional Box–Cox and differencing, autocorrelation and partial \
                             autocorrelation with limits, Ljung–Box and unit-root (ADF, KPSS) tests, spectrum",
                        ),
                        (
                            ExploreKind::Decompose,
                            "Decomposition…",
                            "Trend, seasonal and remainder by classical moving averages (additive or \
                             multiplicative) or STL; seasonal indices and strengths",
                        ),
                        (
                            ExploreKind::Trend,
                            "Trend models…",
                            "Linear, quadratic, exponential-growth or logistic S-curve against time, or a moving \
                             average: accuracy (MAPE, MAD, MSD) and forecasts",
                        ),
                        (
                            ExploreKind::CrossCorrelation,
                            "Cross-correlation…",
                            "Correlation of an input and an output series at leads and lags, optionally after \
                             prewhitening the input",
                        ),
                    ] {
                        if ui.button(label).on_hover_text(tip).clicked() {
                            self.series_explore_dialog =
                                opened!(self.dialog_memory, doc, SeriesExploreDialog::new(kind, &doc.table));
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    menu_heading(ui, "Forecasting models");
                    if ui
                        .button("Exponential smoothing…")
                        .on_hover_text(
                            "Level, trend and seasonal smoothing (simple, Holt, damped, Holt–Winters or the best by \
                             AICc): forecasts with prediction limits and residual checks",
                        )
                        .clicked()
                    {
                        let fresh = crate::forecast_dialog::ForecastDialog::smoothing(&doc.table);
                        self.forecast_dialog = opened!(self.dialog_memory, doc, fresh);
                        ui.close_menu();
                    }
                    if ui
                        .button("ARIMA…")
                        .on_hover_text(
                            "Seasonal ARIMA by exact maximum likelihood, orders chosen or searched by AICc: \
                             coefficients, forecasts with prediction limits and residual checks",
                        )
                        .clicked()
                    {
                        let fresh = crate::forecast_dialog::ForecastDialog::arima(&doc.table);
                        self.forecast_dialog = opened!(self.dialog_memory, doc, fresh);
                        ui.close_menu();
                    }
                    ui.separator();
                    menu_heading(ui, "Signals and spectra");
                    for (kind, label, tip) in crate::signal_dialog::MENU {
                        if ui.button(label).on_hover_text(tip).clicked() {
                            let fresh = crate::signal_dialog::SignalDialog::new(kind, &doc.table);
                            self.signal_dialog = opened!(self.dialog_memory, doc, fresh);
                            ui.close_menu();
                        }
                    }
                    for kind in crate::signal_processing_dialog::SignalKind::ALL {
                        if ui.button(kind.menu_label()).on_hover_text(kind.hint()).clicked() {
                            let fresh = crate::signal_processing_dialog::SignalProcessingDialog::new(kind, &doc.table);
                            self.signal_processing_dialog = opened!(self.dialog_memory, doc, fresh);
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    menu_heading(ui, "Images");
                    for kind in crate::image_analysis_dialog::ImageKind::ALL {
                        if ui.button(kind.menu_label()).on_hover_text(kind.hint()).clicked() {
                            let fresh = crate::image_analysis_dialog::ImageAnalysisDialog::new(kind, &doc.table);
                            self.image_analysis_dialog = opened!(self.dialog_memory, doc, fresh);
                            ui.close_menu();
                        }
                    }
                });
                crate::menus::menu_button(ui, "Visualize", |ui| {
                    if ui
                        .button("Graph canvas…")
                        .on_hover_text(
                            "Drag columns onto axis, panel, color, size and shape shelves; combine points, \
                             smoothers, fit lines and box plots",
                        )
                        .clicked()
                    {
                        actions.push(Action::NewGraph);
                        ui.close_menu();
                    }
                    if ui
                        .button("Graph from pipeline file…")
                        .on_hover_text("Rebuild a graph saved with “Save as graph pipeline” on this dataset")
                        .clicked()
                    {
                        actions.push(Action::GraphFromPipeline);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .button("3-D scatter plot…")
                        .on_hover_text("Rows placed by three columns in a box you can turn; colour, size and shape")
                        .clicked()
                    {
                        use crate::plot3d_dialog::{Plot3dDialog, Plot3dKind};
                        self.plot3d_dialog = opened!(self.dialog_memory, doc, Plot3dDialog::new(Plot3dKind::Scatter));
                        ui.close_menu();
                    }
                    if ui
                        .button("Contour and surface plot…")
                        .on_hover_text(
                            "A response over two columns: contour plot and rotatable surface through the data \
                             (triangulation or thin-plate spline)",
                        )
                        .clicked()
                    {
                        use crate::plot3d_dialog::{Plot3dDialog, Plot3dKind};
                        self.plot3d_dialog = opened!(self.dialog_memory, doc, Plot3dDialog::new(Plot3dKind::Surface));
                        ui.close_menu();
                    }
                    if ui
                        .button("Image plot…")
                        .on_hover_text(
                            "A column drawn as a picture over x and y (an image opened as a table, or any evenly \
                             spaced grid), with a colour bar; select pixels by dragging a box",
                        )
                        .clicked()
                    {
                        let fresh = crate::plot3d_dialog::Plot3dDialog::image(&doc.table);
                        self.plot3d_dialog = opened!(self.dialog_memory, doc, fresh);
                        ui.close_menu();
                    }
                    // Also Multivariate ▸ Scatterplot matrix…
                    if ui
                        .button("Scatterplot matrix…")
                        .on_hover_text("Every pair of columns in a grid of linked plots, with density ellipses and histograms")
                        .clicked()
                    {
                        use crate::multivariate_methods_dialog::{MethodDialog, MethodKind};
                        self.method_dialog =
                            opened!(self.dialog_memory, doc, MethodDialog::new(MethodKind::ScatterMatrix, &doc.table));
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .button("Figure page…")
                        .on_hover_text(
                            "Several plots on one page with panel letters, saved as one PNG, SVG, PDF or EPS file \
                             at a size in millimetres or inches",
                        )
                        .clicked()
                    {
                        self.figure_page.open = true;
                        ui.close_menu();
                    }
                });
                self.scripts_menu(ui, actions);
                self.help_menu(ui, actions);
            });
        });
    }

    /// Re-check the active document for unsaved changes when something that
    /// is saved may have changed: its data, row states other than the
    /// selection, or reports and their options, or (for scripts, notes and
    /// the like, which leave no other trace) every two seconds. A check
    /// compares revisions and a hash of the project's small parts, so it
    /// costs little however large the tables are.
    fn refresh_modified_marks(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.modified_marks.retain(|id, _| self.projects.iter().any(|p| p.id == *id));
        let mut projects: Vec<u64> =
            [self.active, self.shown].iter().filter_map(|&i| self.documents.get(i)).map(|d| d.project).collect();
        projects.dedup();
        for id in projects {
            let key: Vec<ModifiedKey> = self
                .documents
                .iter_mut()
                .filter(|d| d.project == id)
                .map(|d| ModifiedKey {
                    edits: d.revision,
                    data: d.table.revision(),
                    rows: d.rows.saved_revision(),
                    reports: d
                        .reports
                        .iter_mut()
                        .map(|r| {
                            r.sync_options_revision();
                            (r.id(), r.options_revision())
                        })
                        .collect(),
                })
                .collect();
            let stale = self.modified_marks.get(&id).is_none_or(|m| m.key != key || now - m.checked_at > 2.0);
            if stale {
                let modified = self.project_modified(id);
                self.modified_marks.insert(id, ModifiedMark { key, checked_at: now, modified });
            }
        }
    }

    /// Whether project `id` had unsaved changes when last checked.
    fn project_shows_modified(&self, id: u64) -> bool {
        self.modified_marks.get(&id).is_some_and(|m| m.modified)
    }

    /// Whether document `doc`'s project had unsaved changes when last checked.
    fn shows_modified(&self, doc: &Document) -> bool {
        self.project_shows_modified(doc.project)
    }

    /// The Scripts menu: the Script window, script files and recording.
    fn scripts_menu(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        crate::menus::menu_button(ui, "Scripts", |ui| {
            if ui
                .button("Script window")
                .on_hover_text("Write and run Rhai scripts that work on the open tables")
                .clicked()
            {
                self.script.open = true;
                ui.close_menu();
            }
            if ui.button("Open script…").clicked() {
                actions.push(Action::OpenScript);
                ui.close_menu();
            }
            if ui
                .button("New notebook")
                .on_hover_text(
                    "A notebook of notes and code cells kept in the current table's project; run cells one by one \
                     and see their output under each",
                )
                .clicked()
            {
                self.new_notebook();
                ui.close_menu();
            }
            self.notebook_menu_items(ui);
            if ui
                .button("Run script over a folder…")
                .on_hover_text("Run a script once per data file in a folder and collect the results in a table")
                .clicked()
            {
                self.batch.open = true;
                ui.close_menu();
            }
            match &self.recorder {
                None => {
                    if ui
                        .button("⏺ Record actions")
                        .on_hover_text(
                            "Write what you do from now on (opening tables, edits, row states, analyses \
                                     and their options, table operations, saving) as a script that repeats it",
                        )
                        .clicked()
                    {
                        self.start_recording();
                        ui.close_menu();
                    }
                }
                Some(r) => {
                    if ui
                        .button(format!("⏹ Stop recording ({} lines)", r.len()))
                        .on_hover_text("Stop and show the script in the Script window")
                        .clicked()
                    {
                        self.stop_recording_requested = true;
                        ui.close_menu();
                    }
                }
            }
        });
    }

    /// The Help menu: the two manuals, keyboard shortcuts and About.
    fn help_menu(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        crate::menus::menu_button(ui, "Help", |ui| {
            if ui
                .button("Quick guide")
                .on_hover_text("Every menu item in plain language, for new users; opens as a PDF in your PDF viewer")
                .clicked()
            {
                actions.push(Action::OpenManual(manuals::Manual::QuickGuide));
                ui.close_menu();
            }
            if ui
                .button("Reference manual")
                .on_hover_text("Every option and method in detail; opens as a PDF in your PDF viewer")
                .clicked()
            {
                actions.push(Action::OpenManual(manuals::Manual::Reference));
                ui.close_menu();
            }
            if ui.button("Keyboard shortcuts").clicked() {
                self.help_window = Some(HelpWindow::Shortcuts);
                ui.close_menu();
            }
            if ui
                .button("Show crash reports")
                .on_hover_text("Open the folder where AnovaCraft writes a report each time it stops on an internal error")
                .clicked()
            {
                self.show_crash_report(None);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("About AnovaCraft").clicked() {
                self.help_window = Some(HelpWindow::About);
                ui.close_menu();
            }
        });
    }

    /// The Help menu's windows: keyboard shortcuts, or About.
    fn help_windows(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(which) = self.help_window else { return };
        let mut open = true;
        if which == HelpWindow::Licences {
            egui::Window::new("Open-source licences")
                .open(&mut open)
                .collapsible(false)
                .default_size([720.0, 560.0])
                .show(ctx, notices::show);
            if !open {
                self.help_window = None;
            }
            return;
        }
        let title = match which {
            HelpWindow::Shortcuts => "Keyboard shortcuts",
            HelpWindow::About => "About AnovaCraft",
            HelpWindow::Licences => unreachable!(),
        };
        let mut show_licences = false;
        egui::Window::new(title).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| match which {
            HelpWindow::Shortcuts => shortcut_table(ui),
            HelpWindow::About => {
                ui.heading("AnovaCraft");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(6.0);
                ui.label("An open-source statistics application: data, analyses, designed experiments and graphs.");
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Copyright © 2026 The AnovaCraft authors. \
                         Licensed under the Apache License 2.0 or the MIT licence, at your option.",
                    )
                    .weak()
                    .small(),
                );
                ui.label(
                    RichText::new(
                        "Built with open-source fonts, icons, data and Rust libraries, \
                         each under its own licence.",
                    )
                    .weak()
                    .small(),
                );
                ui.label(RichText::new(INDEPENDENCE_NOTICE).weak().small());
                if ui
                    .button("Open-source licences")
                    .on_hover_text("The copyright notices and licence texts of everything AnovaCraft is built with")
                    .clicked()
                {
                    show_licences = true;
                }
                ui.horizontal(|ui| {
                    if ui.link("Quick guide").clicked() {
                        actions.push(Action::OpenManual(manuals::Manual::QuickGuide));
                    }
                    if ui.link("Reference manual").clicked() {
                        actions.push(Action::OpenManual(manuals::Manual::Reference));
                    }
                    ui.hyperlink_to("Manuals online", manuals::ONLINE_URL)
                        .on_hover_text("The manuals' pages on GitHub, in your web browser");
                });
            }
            HelpWindow::Licences => {}
        });
        if !open {
            self.help_window = None;
        }
        if show_licences {
            self.help_window = Some(HelpWindow::Licences);
        }
    }

    /// The left sidebar: open projects and their tables (right-click for
    /// file actions), then the active table's variables and row counts.
    fn sidebar(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        egui::SidePanel::left("variables").resizable(true).default_width(230.0).show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Open projects").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Open…").on_hover_text("Open data files or projects in new tabs").clicked() {
                        actions.push(Action::PickAndOpen);
                    }
                });
            });
            egui::ScrollArea::vertical().id_salt("documents_scroll").max_height(200.0).show(ui, |ui| {
                self.project_list(ui, actions);
                if self.documents.is_empty() {
                    ui.label(RichText::new("None — use File ▸ Open…").weak());
                }
            });
            ui.separator();
            let project = self
                .documents
                .get(self.active)
                .and_then(|d| self.project(d.project))
                .map(|p| (self.project_name(p.id), p.scripts.clone()));
            let project = project.as_ref().map(|(name, scripts)| (name.as_str(), scripts.as_slice()));
            let folder = self.settings.scripts_folder.as_deref();
            for request in self.script_library.show(ui, folder, project) {
                actions.push(Action::Library(request));
            }
            ui.separator();
            let target_elsewhere = self.active != self.shown;
            let made_from = self.documents.get(self.active).and_then(|d| self.made_from(d));
            let Some(doc) = self.documents.get_mut(self.active) else { return };
            ui.heading(doc.name());
            if let Some(made_from) = made_from {
                ui.label(RichText::new(made_from).weak().small());
            }
            if target_elsewhere {
                // The main window shows another document: say which one this is about.
                ui.label(RichText::new("From the window you used last; menus act on this dataset").weak().small());
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Variables").strong());
                if doc.table.n_cols() > 8 || !self.variable_filter.is_empty() {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.variable_filter)
                            .hint_text("Filter")
                            .desired_width(f32::INFINITY),
                    )
                    .on_hover_text("Show only variables whose names contain this text");
                }
            });
            let filter = self.variable_filter.to_lowercase();
            let mut requests = Vec::new();
            let mut focus = None;
            let (table, grid) = (&doc.table, &mut doc.grid);
            egui::ScrollArea::vertical()
                .id_salt(("variables_scroll", doc.id))
                .max_height(ui.available_height() * 0.6)
                .show(ui, |ui| {
                    let mut shown = 0;
                    for (j, col) in table.columns().iter().enumerate() {
                        if !filter.is_empty() && !col.name.to_lowercase().contains(&filter) {
                            continue;
                        }
                        shown += 1;
                        let payload = ColumnDrag { name: col.name.clone(), from: None };
                        let drag = ui
                            .dnd_drag_source(egui::Id::new(("variable", doc.id, col.name.as_str())), payload, |ui| {
                                ui.horizontal(|ui| {
                                    table_view::level_badge(ui, col.level);
                                    ui.label(&col.name);
                                    if col.temporal().is_some() {
                                        ui.label(
                                            RichText::new(table_view::kind_description(col.kind())).weak().small(),
                                        );
                                    }
                                    if col.formula.is_some() {
                                        ui.label(RichText::new("ƒ").weak().italics());
                                    }
                                })
                            })
                            .response;
                        // Clicks go to a separate click-only area over the same
                        // row, so dragging onto a graph shelf still works.
                        let click = ui
                            .interact(drag.rect, egui::Id::new(("variable_click", doc.id, j)), egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::Grab)
                            .on_hover_text(format!(
                                "{} — {} {}{}\nClick to select the column in the data grid, right-click for its \
                                 menu, or drag onto a graph canvas shelf",
                                col.name,
                                col.level.label(),
                                table_view::kind_description(col.kind()),
                                col.formula.as_deref().map(|f| format!(", computed by  {f}")).unwrap_or_default()
                            ));
                        if click.clicked() {
                            focus = Some(j);
                        }
                        click.context_menu(|ui| table_view::column_menu(ui, j, col, grid, &mut requests));
                    }
                    if shown == 0 && !filter.is_empty() {
                        ui.label(RichText::new("No variable names contain that text").weak());
                    }
                });
            if let Some(j) = focus {
                // Selecting a column ends a selection of rows, as in the grid.
                doc.grid.focus_column(j);
                doc.rows.clear_selection();
            }
            // Renaming happens in the grid's heading, so show the grid.
            let reveal = focus.is_some() || !requests.is_empty() || (doc.grid.is_renaming() && !self.grid_on_screen());
            if !requests.is_empty() {
                actions.push(Action::Grid(requests));
            }
            if reveal {
                self.reveal(self.active, View::Data);
            }
            let doc = &self.documents[self.active];
            ui.separator();
            ui.label(RichText::new("Rows").strong());
            let c = doc.rows.counts();
            // Rows you excluded or hid yourself; rows outside the active subset are counted apart.
            let excluded = (0..c.total).filter(|&r| doc.rows.is_excluded_flag(r)).count();
            let hidden = (0..c.total).filter(|&r| doc.rows.is_hidden_flag(r)).count();
            egui::Grid::new("row_counts").num_columns(2).spacing([24.0, 2.0]).show(ui, |ui| {
                let mut lines =
                    vec![("Total", c.total), ("Selected", c.selected), ("Excluded", excluded), ("Hidden", hidden)];
                if c.outside_subset > 0 {
                    lines.push(("Outside subset", c.outside_subset));
                }
                for (label, value) in lines {
                    ui.label(label);
                    ui.monospace(value.to_string());
                    ui.end_row();
                }
            });
            self.subset_sidebar(ui);
            ui.separator();
            self.notes_section(ui);
        });
    }

    /// One tab per open document along the bottom edge of the window, a
    /// project of several tables or with dashboards grouped under its own
    /// header. The × closes a tab; right-click offers the same actions as
    /// the sidebar.
    fn document_tabs(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        egui::TopBottomPanel::bottom("document_tabs").show(ctx, |ui| {
            egui::ScrollArea::horizontal().id_salt("document_tabs_scroll").show(ui, |ui| {
                ui.horizontal(|ui| {
                    for project in self.project_order() {
                        self.project_tabs(ui, project, actions);
                    }
                    if ui.small_button("+").on_hover_text("Open data files or projects").clicked() {
                        actions.push(Action::PickAndOpen);
                    }
                });
            });
        });
    }

    fn status_bar(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            // Row counts sit at the right edge; the rest fills what is left,
            // with a long message shortened ("…") rather than running under
            // the counts or off the window.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.settings.show_frame_time {
                    ui.label(RichText::new(frame_time_text(ctx, self.last_frame_time)).monospace().small())
                        .on_hover_text("Time the last frame took to draw, and the report that took longest in it");
                    ui.separator();
                }
                if let Some(doc) = self.documents.get(self.active) {
                    let c = doc.rows.counts();
                    let (excluded, hidden) = doc.rows.own_flag_counts();
                    let mut parts = vec![format!("{} selected", c.selected)];
                    if excluded > 0 {
                        parts.push(format!("{excluded} excluded"));
                    }
                    if hidden > 0 {
                        parts.push(format!("{hidden} hidden"));
                    }
                    if c.outside_subset > 0 {
                        parts.push(format!("{} outside the subset", c.outside_subset));
                    }
                    // Name the dataset when it is not the one the main window shows.
                    let name = if self.active == self.shown { String::new() } else { format!("{}: ", doc.name()) };
                    ui.add(
                        egui::Label::new(format!("{name}{} · {}", count(c.total, "row", "rows"), parts.join(" · ")))
                            .extend(),
                    );
                    let cell = doc.grid.cursor().and_then(|(r, j)| {
                        let col = doc.table.column(j).filter(|_| r < doc.table.n_rows())?;
                        let value = col.cell_text(r);
                        let value = match col.label_of(&value) {
                            Some(label) => format!("{value} ({label})"),
                            None if value.is_empty() => "missing".to_owned(),
                            None => value,
                        };
                        Some(format!("Row {} · {} = {value}", r + 1, col.name))
                    });
                    if let Some(cell) = cell {
                        ui.separator();
                        ui.add(egui::Label::new(RichText::new(cell).monospace()).truncate())
                            .on_hover_text("The cell at the grid's cursor");
                    }
                    ui.separator();
                    ui.add(egui::Label::new(RichText::new("Selecting rows ?").weak()).extend()).on_hover_text(
                        "Select rows by clicking or dragging in a plot or on the row numbers · Shift adds (a run, \
                         on row numbers) · ⌘/Ctrl toggles · ⌥/Alt removes",
                    );
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if let Some(removed) = &self.removed_report {
                        let left = UNDO_REMOVE_SECONDS - (ui.input(|i| i.time) - removed.at);
                        if left > 0.0 {
                            ui.label(format!("Removed “{}”", removed.title));
                            if ui.link("Undo remove").on_hover_text("Put the report back where it was").clicked() {
                                actions.push(Action::UndoRemoveReport);
                            }
                            ui.separator();
                            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(left));
                        }
                    }
                    if let Some(r) = &self.recorder {
                        ui.label(RichText::new("⏺ Recording").color(theme::ERROR).strong()).on_hover_text(format!(
                            "{} lines so far · Scripts ▸ Stop recording shows the script",
                            r.len()
                        ));
                        ui.separator();
                    }
                    let updatable = self.stale.values().filter(|s| !s.changed.is_empty()).count();
                    if updatable > 0 {
                        let text = if updatable == 1 {
                            "⚠ 1 table can be updated".to_owned()
                        } else {
                            format!("⚠ {updatable} tables can be updated")
                        };
                        let (color, _) = theme::stale_colors(ui.visuals().dark_mode);
                        ui.label(RichText::new(text).color(color)).on_hover_text(
                            "Their source tables were edited after they were made. Each shows Update above its data; \
                             nothing changes until you choose it.",
                        );
                        ui.separator();
                    }
                    match &self.message {
                        Some((m, true)) => {
                            ui.add(egui::Label::new(RichText::new(format!("⚠ {m}")).color(theme::ERROR)).truncate());
                        }
                        Some((m, false)) => {
                            ui.add(egui::Label::new(format!("✔ {m}")).truncate());
                        }
                        None => {}
                    }
                });
            });
        });
    }

    fn workspace_tabs(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let show_tips = !self.settings.tips_dismissed;
        let mut dismiss = false;
        let Some(doc) = self.documents.get_mut(self.shown) else { return };
        egui::TopBottomPanel::top("workspace_tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut doc.workspace, Workspace::Data, "Data");
                ui.selectable_value(&mut doc.workspace, Workspace::Results, format!("Results ({})", doc.reports.len()));
                ui.selectable_value(&mut doc.workspace, Workspace::SideBySide, "Side by side");
                ui.separator();
                let (view, tip) = match doc.workspace {
                    Workspace::Data => (View::Data, "Show the data grid in a window of its own"),
                    _ => (View::Results, "Show the results in a window of its own"),
                };
                if windows::new_window_button(ui).on_hover_text(tip).clicked() {
                    actions.push(Action::PopOut { doc: doc.id, view });
                }
            });
            if show_tips {
                dismiss = getting_started_tips(ui);
            }
        });
        if dismiss {
            self.settings.tips_dismissed = true;
            self.settings_changed(ctx);
        }
    }

    /// Lays out the data grid and/or results. Side by side, the results live in a
    /// resizable right-hand panel and the grid in the central panel: egui panels
    /// never overlap, and dragging the shared edge resizes both at once.
    fn workspace_panes(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let choices = std::sync::Arc::new(self.dashboard_choices(self.shown));
        ctx.data_mut(|d| d.insert_temp(report_view::dashboard_choices_id(), choices));
        self.drawn_dashboard = self.shown_dashboard();
        if let Some(dashboard) = self.shown_dashboard() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.push_id(("main_dashboard", dashboard), |ui| self.dashboard_view(ui, dashboard, false, actions));
            });
            return;
        }
        let (grid_away, results_away) = match self.documents.get(self.shown) {
            Some(doc) => (self.in_window(doc.id, View::Data), self.in_window(doc.id, View::Results)),
            None => (false, false),
        };
        let Some(doc) = self.documents.get_mut(self.shown) else {
            let data = self.welcome_data();
            egui::CentralPanel::default().show(ctx, |ui| welcome::welcome_page(ui, &data, actions));
            return;
        };
        let shown = self.shown;
        let undo = doc.history.next_undo().map(|(l, _)| l.to_owned());
        let redo = doc.history.next_redo().map(|(l, _)| l.to_owned());
        let (id, table, rows, reports, grid) = (doc.id, &doc.table, &mut doc.rows, &mut doc.reports, &mut doc.grid);
        let mut requests = Vec::new();
        let mut divider = None;
        // With the grid or the results in a window of their own, Side by
        // side has only one view to show here: it fills the whole width,
        // under a one-line note about the other.
        let side_by_side = doc.workspace == Workspace::SideBySide && !grid_away && !results_away;
        let gutter = divider_gutter(&ctx.style());
        if side_by_side {
            // The application owns the results pane's width. (egui's own panel
            // resizing remembers the width the *contents* used, so a wide report
            // would pin the pane at its maximum and the divider could no longer
            // be dragged.)
            let area = ctx.available_rect();
            let max = (area.width() - MIN_PANE_WIDTH).max(MIN_PANE_WIDTH);
            let mut width = self.results_width.unwrap_or(area.width() * 0.5);
            // Apply this frame's divider drag before laying out, so it follows the pointer without lag.
            if let Some(r) = ctx.read_response(divider_id()) {
                if let (true, Some(p)) = (r.dragged(), r.interact_pointer_pos()) {
                    width = area.right() - p.x;
                }
            }
            width = width.clamp(MIN_PANE_WIDTH, max);
            self.results_width = Some(width);
            // Keep the reports' widgets out of the divider's grab zone.
            let frame = egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin {
                left: gutter,
                right: 8,
                top: 2,
                bottom: 2,
            });
            egui::SidePanel::right("results_pane")
                .frame(frame)
                .resizable(false)
                .show_separator_line(false)
                .exact_width(width)
                .show(ctx, |ui| {
                    ui.push_id(("results", id), |ui| {
                        if report_view::show_reports(ui, reports, table, rows) {
                            actions.push(Action::ExportReports(shown));
                        }
                    })
                });
            divider = Some((area.right() - width, area.y_range()));
        }
        // Likewise keep the data grid's vertical scroll bar clear of the divider.
        let mut central = egui::Frame::central_panel(&ctx.style());
        if side_by_side {
            central = central.inner_margin(egui::Margin { left: 8, right: gutter, top: 8, bottom: 8 });
        }
        egui::CentralPanel::default().frame(central).show(ctx, |ui| {
            if side_by_side {
                // Clip to the area inside the margins, so nothing here (in particular
                // a scroll bar) can be drawn on or catch presses meant for the divider.
                let inner = ui.max_rect();
                ui.set_clip_rect(ui.clip_rect().intersect(inner.expand2(egui::vec2(0.0, 8.0))));
            }
            ui.push_id(("workspace", id), |ui| match doc.workspace {
                Workspace::SideBySide if results_away => {
                    windows::away_note(ui, id, View::Results, reports.len(), actions);
                    if grid_away {
                        windows::away_panel(ui, id, View::Data, reports.len(), actions);
                    } else {
                        requests = table_view::show(ui, table, rows, grid, undo.as_deref(), redo.as_deref());
                    }
                }
                Workspace::SideBySide if grid_away => {
                    windows::away_note(ui, id, View::Data, reports.len(), actions);
                    if report_view::show_reports(ui, reports, table, rows) {
                        actions.push(Action::ExportReports(shown));
                    }
                }
                Workspace::Data if grid_away => {
                    windows::away_panel(ui, id, View::Data, reports.len(), actions);
                }
                Workspace::Data | Workspace::SideBySide => {
                    requests = table_view::show(ui, table, rows, grid, undo.as_deref(), redo.as_deref());
                }
                Workspace::Results if results_away => {
                    windows::away_panel(ui, id, View::Results, reports.len(), actions);
                }
                Workspace::Results => {
                    if report_view::show_reports(ui, reports, table, rows) {
                        actions.push(Action::ExportReports(shown));
                    }
                }
            })
        });
        if !requests.is_empty() {
            actions.push(Action::GridIn(shown, requests));
        }
        if let Some((x, y_range)) = divider {
            divider_handle(ctx, x, y_range);
        }
    }

    /// File ▸ Import from database / the web, and Data ▸ Refresh from source.
    fn external_data_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.external_dialog.as_mut() else { return };
        match dialog.show(ctx) {
            crate::external_dialog::ExternalOutcome::Open => {}
            crate::external_dialog::ExternalOutcome::Cancel => self.external_dialog = None,
            crate::external_dialog::ExternalOutcome::Read { table, notes, source, secrets, refresh } => {
                self.external_dialog = None;
                match refresh {
                    Some(id) => self.refresh_document(id, &table, secrets),
                    None => self.open_external(table, &notes, *source, secrets),
                }
            }
            crate::external_dialog::ExternalOutcome::Cells { read, source, secrets, name } => {
                self.external_dialog = None;
                if let app_model::external::WebRead::Cells { sheet, delimiter, settings, notes } = read {
                    let dialog = crate::open_data_dialog::OpenDataDialog::from_web(
                        *source, secrets, name, sheet, delimiter, settings, notes,
                    );
                    self.offer_open_data(dialog);
                }
            }
        }
    }

    /// Open a table read from a database or the web as a new, unsaved document.
    fn open_external(
        &mut self,
        table: DataTable,
        notes: &[String],
        source: app_model::external::ExternalSource,
        secrets: app_model::external::Secrets,
    ) {
        let summary = format!(
            "Read {} × {} from {}",
            count(table.n_rows(), "row", "rows"),
            count(table.n_cols(), "column", "columns"),
            source.describe()
        );
        let mut doc = self.imported_document(table, None);
        doc.external = Some(source);
        doc.secrets = Some(secrets);
        // A new table has never been saved.
        self.push_documents(vec![doc], NewProject { unsaved: true, ..NewProject::default() });
        let mut message = summary;
        for note in notes {
            message.push_str(". ");
            message.push_str(note.trim_end_matches('.'));
        }
        self.info(format!("{message}."));
    }

    /// Replace document `id`'s rows with a fresh read of its source, as one
    /// undoable edit.
    fn refresh_document(&mut self, id: u64, fresh: &DataTable, secrets: app_model::external::Secrets) {
        let Some(index) = self.documents.iter().position(|d| d.id == id) else {
            return self.error("The table to refresh is no longer open");
        };
        let edit = match app_model::external::refresh_edit(&self.documents[index].table, fresh, &[]) {
            Ok(edit) => edit,
            Err(e) => return self.error(format!("Could not refresh: {e}")),
        };
        self.documents[index].secrets = Some(secrets);
        match self.edit_document(index, edit) {
            Ok(()) => {
                let name = self.documents[index].name().to_owned();
                self.info(format!(
                    "Refreshed “{name}”: {}. Undo puts back the previous rows.",
                    count(fresh.n_rows(), "row", "rows")
                ));
            }
            Err(message) => self.error(format!("Could not refresh: {message}")),
        }
    }

    fn analysis_dialogs(&mut self, ctx: &egui::Context) {
        let Some(doc) = self.documents.get(self.active) else { return };
        let table = &doc.table;
        let doc_id = doc.id;
        let memory = &mut self.dialog_memory;
        let mut new_reports: Vec<(ReportRequest, Placement)> = Vec::new();
        if let Some(start) = crate::resampling_dialogs::take_bootstrap_request(ctx) {
            self.bootstrap_dialog = Some(crate::resampling_dialogs::BootstrapDialog::new(start));
        }
        if let Some((request, place)) =
            drive(&mut self.permutation_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some((request, place)) = drive(&mut self.bootstrap_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some(run) = drive(&mut self.dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push(run);
        }
        if let Some((requests, place)) =
            drive(&mut self.distribution_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            // Reports are inserted at the top, so add in reverse to keep the
            // chosen columns in table order; an edited report takes the first.
            let mut requests = requests.into_iter().map(ReportRequest::Distribution);
            let first = requests.next();
            new_reports.extend(requests.rev().map(|r| (r, Placement::New)));
            new_reports.extend(first.map(|r| (r, place)));
        }
        if let Some((request, place)) =
            drive(&mut self.distribution_fit_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::DistributionFit(request), place));
        }
        if let Some((request, place)) = drive(&mut self.bivariate_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::Bivariate(request), place));
        }
        if let Some((request, place)) = drive(&mut self.gauge_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::Gauge(request), place));
        }
        // Other open tables, as sources of saved control limits and k-means starting centers.
        let others: Vec<(&str, &DataTable)> =
            self.documents.iter().filter(|d| d.id != doc_id).map(|d| (d.name(), &*d.table)).collect();
        if let Some((request, place)) =
            drive(&mut self.chart_dialog, memory, doc_id, table, |d| d.show_with(ctx, table, &others))
        {
            new_reports.push((ReportRequest::ControlChart(request), place));
        }
        if let Some((request, place)) = drive(&mut self.nonlinear_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::Nonlinear(request), place));
        }
        if let Some((request, place)) =
            drive(&mut self.curve_template_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::Nonlinear(request), place));
        }
        if let Some((request, place)) = drive(&mut self.mixed_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::Mixed(Box::new(request)), place));
        }
        if let Some((request, place)) = drive(&mut self.tree_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::DecisionTree(request), place));
        }
        if let Some((request, place)) = drive(&mut self.forest_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::RandomForest(request), place));
        }
        if let Some((request, place)) = drive(&mut self.boost_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::BoostedTrees(request), place));
        }
        if let Some((request, place)) = drive(&mut self.pls_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::PartialLeastSquares(request), place));
        }
        if let Some((request, place)) = drive(&mut self.svm_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::SupportVectorMachine(request), place));
        }
        if let Some((request, place)) = drive(&mut self.mars_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::Mars(request), place));
        }
        if let Some((request, place)) = drive(&mut self.neural_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::NeuralNetwork(request), place));
        }
        if let Some((request, place)) =
            drive(&mut self.gaussian_process_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::GaussianProcess(request), place));
        }
        if let Some((request, place)) = drive(&mut self.search_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::ModelSearch(request), place));
        }
        if let Some((request, place)) =
            drive(&mut self.functional_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::FunctionalData(request), place));
        }
        if let Some((request, place)) =
            drive(&mut self.model_comparison_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::ModelComparison(request), place));
        }
        if let Some(run) = drive(&mut self.phase2_dialog, memory, doc_id, table, |d| d.show_with(ctx, table, &others)) {
            new_reports.push(run);
        }
        if let Some(run) = drive(&mut self.method_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push(run);
        }
        if let Some((request, place)) = drive(&mut self.factor_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::FactorAnalysis(request), place));
        }
        if let Some((request, place)) =
            drive(&mut self.discriminant_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::DiscriminantAnalysis(request), place));
        }
        if let Some(run) = drive(&mut self.map_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push(run);
        }
        if let Some(run) = drive(&mut self.text_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push(run);
        }
        let mut stacked_table = None;
        if let Some((run, place)) = drive(&mut self.imputation_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            if run.stacked_table {
                stacked_table = run.request.stacked_operation(table);
            }
            new_reports.push((ReportRequest::Imputation(run.request), place));
        }
        if let Some((requests, place)) =
            drive(&mut self.series_explore_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            // As for distributions: reports go on top, so add in reverse; an edited report takes the first.
            let mut requests = requests.into_iter().map(ReportRequest::SeriesExplore);
            let first = requests.next();
            new_reports.extend(requests.rev().map(|r| (r, Placement::New)));
            new_reports.extend(first.map(|r| (r, place)));
        }
        if let Some((requests, place)) =
            drive(&mut self.signal_processing_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            let mut requests = requests.into_iter().map(ReportRequest::SignalProcessing);
            let first = requests.next();
            new_reports.extend(requests.rev().map(|r| (r, Placement::New)));
            new_reports.extend(first.map(|r| (r, place)));
        }
        if let Some((requests, place)) =
            drive(&mut self.image_analysis_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            let mut requests = requests.into_iter().map(ReportRequest::ImageAnalysis);
            let first = requests.next();
            new_reports.extend(requests.rev().map(|r| (r, Placement::New)));
            new_reports.extend(first.map(|r| (r, place)));
        }
        let mut random_column = None;
        match drive(&mut self.compare_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            Some((CompareOutput::Report(request), place)) => new_reports.push((*request, place)),
            Some((CompareOutput::Column(column), _)) => random_column = Some(std::sync::Arc::unwrap_or_clone(column)),
            None => {}
        }
        if let Some((request, place)) = drive(&mut self.mixture_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::Mixture(request), place));
        }
        if let Some((request, place)) = drive(&mut self.attribute_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::Attribute(request), place));
        }
        // Opened from the Design menu, which cannot reach the remembered settings.
        if let Some(dialog) = self.study_dialog.take_if(|d| d.needs_recall()) {
            self.study_dialog = Some(memory.recall(doc_id, dialog, Some(table), doc.grid.selected_columns()));
        }
        if let Some((request, place)) = drive(&mut self.study_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((*request, place));
        }
        if let Some((request, place)) = drive(&mut self.agreement_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some((request, place)) = drive(&mut self.bayes_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((*request, place));
        }
        if let Some((request, place)) = drive(&mut self.treatment_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::Comparison(app_model::SavedReport::TreatmentEffects { request }), place));
        }
        if let Some((request, place)) = drive(&mut self.signal_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some(dialog) = self.evaluation_dialog.take_if(|d| d.needs_recall()) {
            self.evaluation_dialog = Some(memory.recall(doc_id, dialog, Some(table), doc.grid.selected_columns()));
        }
        if let Some((request, place)) =
            drive(&mut self.evaluation_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some(dialog) = self.factorial_dialog.take_if(|d| d.needs_recall()) {
            self.factorial_dialog = Some(memory.recall(doc_id, dialog, Some(table), doc.grid.selected_columns()));
        }
        if let Some((request, place)) = drive(&mut self.factorial_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some(dialog) = self.sequential_dialog.take_if(|d| d.needs_recall()) {
            self.sequential_dialog = Some(memory.recall(doc_id, dialog, Some(table), doc.grid.selected_columns()));
        }
        if let Some((request, place)) =
            drive(&mut self.sequential_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some((request, place)) =
            drive(&mut self.tabulation_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some((request, place)) = drive(&mut self.plot3d_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((request, place));
        }
        if let Some((request, place)) =
            drive(&mut self.quality_tool_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some((request, place)) =
            drive(&mut self.monitoring_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some(((request, options), place)) =
            drive(&mut self.pareto_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::Pareto(request, options), place));
        }
        if let Some((request, place)) =
            drive(&mut self.cause_effect_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::CauseEffect(request), place));
        }
        if let Some((request, place)) =
            drive(&mut self.multi_vari_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::MultiVari(request), place));
        }
        if let Some((request, place)) =
            drive(&mut self.multivariate_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::MultivariateChart(request), place));
        }
        if let Some((request, place)) = drive(&mut self.anom_dialog, memory, doc_id, table, |d| d.show(ctx, table)) {
            new_reports.push((ReportRequest::Anom(request), place));
        }
        if let Some((request, place)) =
            drive(&mut self.regression_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some((request, place)) =
            drive(&mut self.capability_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::Capability(request), place));
        }
        if let Some((request, place)) =
            drive(&mut self.time_weighted_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::TimeWeighted(request), place));
        }
        if let Some((request, place)) =
            drive(&mut self.reliability_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((*request, place));
        }
        if let Some((requests, place)) = drive(&mut self.forecast_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            // One report per series, kept in table order (reports go in at the
            // top); an edited report takes the first.
            let mut requests = requests.into_iter().map(|saved| ReportRequest::Reliability(Box::new(saved)));
            let first = requests.next();
            new_reports.extend(requests.rev().map(|r| (r, Placement::New)));
            new_reports.extend(first.map(|r| (r, place)));
        }
        if let Some((request, place)) = drive(&mut self.survival_dialog, memory, doc_id, table, |d| d.show(ctx, table))
        {
            new_reports.push((ReportRequest::Survival(request), place));
        }
        // Opened from the Design menu, which cannot reach the remembered settings.
        if let Some(dialog) = self.augment_dialog.take_if(|d| d.needs_recall()) {
            self.augment_dialog = Some(memory.recall(doc_id, dialog, Some(table), doc.grid.selected_columns()));
        }
        let augmented = drive(&mut self.augment_dialog, memory, doc_id, table, |d| d.show(ctx, table));
        if let Some(dialog) = self.simulate_dialog.take_if(|d| d.needs_recall()) {
            self.simulate_dialog = Some(memory.recall(doc_id, dialog, Some(table), doc.grid.selected_columns()));
        }
        let simulated = drive(&mut self.simulate_dialog, memory, doc_id, table, |d| d.show(ctx, table));
        for (request, place) in new_reports {
            self.place_report(request, place);
        }
        if let Some(((table, description, keep), _)) = augmented {
            self.open_augmented(table, description, doc_id, keep);
        }
        if let Some((outcome, _)) = simulated {
            self.add_simulated(outcome, doc_id);
        }
        if let Some(op) = stacked_table {
            self.make_derived(self.active, &op);
        }
        if let Some(mut column) = random_column {
            let Some(doc) = self.documents.get(self.active) else { return };
            if doc.table.find(&column.name).is_some() {
                column.name = doc.table.unused_column_name(&column.name);
            }
            let (index, name) = (doc.table.n_cols(), column.name.clone());
            if self.apply_edit(Edit::InsertColumn { index, column }) {
                self.info(format!("Added random column “{name}”. Undo removes it."));
            }
        }
    }

    /// The "save changes?" question for a document being closed (or for each
    /// unsaved document while quitting).
    fn close_confirmation(&mut self, ctx: &egui::Context) {
        let Some(project_id) = self.confirm_close else { return };
        let (Some(index), Some(project)) =
            (self.documents.iter().position(|d| d.project == project_id), self.project(project_id))
        else {
            self.confirm_close = None;
            return;
        };
        let (sample, has_file) = (project.sample, project.path.is_some());
        let name = self.project_name(project_id);
        let quitting = self.quitting;
        let mut choice = None;
        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let when = if quitting { "before quitting" } else { "before closing" };
                ui.label(RichText::new(format!("Save the changes to “{name}” {when}?")).strong());
                ui.label(if sample {
                    "This is a built-in sample, so it is saved as a new project file; the sample itself is not changed."
                } else {
                    "Unsaved data edits, reports and row states will be lost if you don't save."
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let save_label = if has_file && !sample { "Save" } else { "Save…" };
                    if ui.button(save_label).clicked() {
                        choice = Some(CloseChoice::Save);
                    }
                    if ui.button("Save As…").clicked() {
                        choice = Some(CloseChoice::SaveAs);
                    }
                    if ui.button("Don't Save").clicked() {
                        choice = Some(CloseChoice::Discard);
                    }
                    let cancel = if quitting { "Cancel (keep working)" } else { "Cancel" };
                    if ui.button(cancel).clicked() {
                        choice = Some(CloseChoice::Cancel);
                    }
                });
            });
        let Some(choice) = choice else { return };
        self.confirm_close = None;
        let closed = match choice {
            CloseChoice::Cancel => false,
            CloseChoice::Discard => true,
            CloseChoice::Save => self.save(index, false),
            CloseChoice::SaveAs => self.save(index, true),
        };
        if closed {
            self.close_project(project_id);
        } else {
            // Cancelled, or the save was cancelled or failed: stop quitting.
            self.quitting = false;
        }
    }

    /// Quitting: ask about each document with unsaved changes in turn, then
    /// close the window.
    fn continue_quit(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_confirmed {
            if self.projects.iter().any(|p| self.project_modified(p.id)) || self.script.is_modified() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.quitting = true;
            } else {
                self.quit_confirmed = true;
            }
        }
        if !self.quitting || self.confirm_close.is_some() || self.script.is_asking() {
            return;
        }
        let modified = self.projects.iter().map(|p| p.id).find(|&id| self.project_modified(id));
        match modified.and_then(|id| self.documents.iter().position(|d| d.project == id)) {
            Some(index) => {
                self.set_active(index);
                self.confirm_close = Some(self.documents[index].project);
                // The question is asked in the main window.
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
            }
            None if self.script.is_modified() => self.script.ask_before_quit(),
            None => {
                self.quitting = false;
                self.quit_confirmed = true;
                self.script.stop();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

impl StatsApp {
    /// Hand the report settings (significance marks, number format, plot
    /// hints) to the report views drawn this frame.
    fn publish_display_settings(&self, ctx: &egui::Context) {
        let mark = self.settings.mark_significant;
        let format = report_view::NumberFormat {
            group_thousands: self.settings.group_thousands,
            align_decimals: self.settings.align_decimals,
        };
        let hint = !self.settings.tips_dismissed;
        ctx.data_mut(|d| {
            d.insert_temp(report_view::mark_significant_id(), mark);
            d.insert_temp(report_view::number_format_id(), format);
            d.insert_temp(report_view::show_plot_hint_id(), hint);
        });
    }
}

impl eframe::App for StatsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.viewer.is_some() {
            return self.viewer_frame(ctx);
        }
        let frame_started = std::time::Instant::now();
        report_view::set_background_reports(ctx, self.report_jobs.is_some());
        self.publish_display_settings(ctx);
        // Which window was used last, and so which document commands act on.
        self.track_main_window(ctx);
        // Reports asking for (or to leave) a window of their own.
        self.sync_report_windows();
        // Which derived tables' sources changed since they were made.
        self.refresh_staleness();
        // Reports finished in the background since the last frame.
        let _ = self.install_report_jobs(None);
        // Every document with a view on screen: refits, removals and what its reports asked for.
        let mut queued = false;
        for index in self.on_screen_documents() {
            queued |= self.service_document(ctx, index);
        }
        // Give the reports just queued a moment, so quick ones show in this
        // frame and only slow ones as updating.
        if queued && self.install_report_jobs(Some(std::time::Duration::from_millis(30))) {
            ctx.request_repaint();
        }
        if let Some(run) = self.script.poll() {
            for note in self.apply_script_run(run) {
                self.script.add_note(note);
            }
            self.run_queued_script(ctx);
        }
        self.poll_notebooks(ctx);

        let mut actions = Vec::new();
        // Files opened from Finder while running (double-click, Open With, Dock).
        #[cfg(target_os = "macos")]
        {
            let files = crate::macos_open::take_pending();
            if !files.is_empty() {
                actions.push(Action::OpenPaths(files));
            }
        }
        // Native windows each get their own keys (see `show_windows`); drawn
        // inside this one (tests), the keys go to the window used last.
        let window = if ctx.embed_viewports() { self.last_window } else { None };
        if window.is_none() && self.main_focus_pending && windows::shortcut_pressed(ctx) {
            // Keys pressed in the main window act on the document it shows.
            self.main_focus_pending = false;
            self.set_target(self.shown);
        }
        let mut keys = Vec::new();
        self.keyboard_shortcuts(ctx, &mut keys, self.active, window);
        self.menu_bar(ctx, &mut actions);
        // Bottom panels: the first added sits lowest, so the tabs run along the window's bottom edge.
        self.document_tabs(ctx, &mut actions);
        self.status_bar(ctx, &mut actions);
        self.sidebar(ctx, &mut actions);
        if let Some(doc) = self.documents.get(self.active) {
            let counts = doc.rows.counts();
            dialog_kit::set_row_counts(ctx, dialog_kit::RowCounts { total: counts.total, excluded: counts.excluded });
            let names = doc.table.columns().iter().map(|c| c.name.clone()).collect();
            dialog_kit::set_column_names(ctx, std::sync::Arc::new(names));
            dialog_kit::set_project_name(ctx, self.project_name(doc.project));
        }
        self.analysis_dialogs(ctx);
        self.recovery_window(ctx);
        self.recovery_tick(ctx);
        self.external_data_dialog(ctx);
        self.open_data_dialog(ctx);
        self.open_image_dialog(ctx);
        self.image_export_dialog(ctx);
        self.duplicate_dialog(ctx);
        self.template_dialog(ctx);
        self.table_op_dialog(ctx);
        self.formula_dialog(ctx);
        self.rank_dialog(ctx);
        self.recode_dialog(ctx);
        self.value_labels_dialog(ctx);
        self.color_by_window(ctx);
        if self.script.open {
            let columns = self
                .documents
                .get(self.active)
                .map(|d| d.table.columns().iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default();
            self.script.set_columns(columns);
        }
        match self.script.show(ctx) {
            Some(ScriptEvent::Run { source }) => self.run_script(ctx, source, None),
            Some(ScriptEvent::KeepInProject { name, source, run_on_open }) => {
                self.keep_script(name, source, run_on_open);
            }
            Some(ScriptEvent::CancelQuit) => self.quitting = false,
            None => {}
        }
        self.show_notebooks(ctx);
        self.subset_prompt_window(ctx);
        self.figure_page.show(ctx);
        crate::plot_export::show_dialog(ctx);
        if let Some(table) = self.batch.show(ctx, &self.script.current_script()) {
            let rows = RowStateManager::new(table.n_rows());
            self.open_script_table(Project {
                table,
                rows,
                reports: Vec::new(),
                origin: None,
                external: None,
                scripts: Vec::new(),
            });
        }
        if let Some(((project, description), _)) =
            drive_without_table(&mut self.design_dialog, &mut self.dialog_memory, |d| d.show(ctx))
        {
            self.open_design_project(project, description);
        }
        if let Some(mut dialog) = self.sheet_dialog.take_if(|d| d.needs_recall()) {
            dialog = self.dialog_memory.recall(0, dialog, None, &[]);
            self.sheet_dialog = Some(dialog);
        }
        if let Some(dialog) = &mut self.sheet_dialog {
            match dialog.show(ctx) {
                Outcome::Open | Outcome::Reset => {}
                Outcome::Cancel => {
                    self.dialog_memory.keep(0, dialog, None);
                    self.sheet_dialog = None;
                }
                Outcome::Run((table, description), _) => {
                    self.dialog_memory.keep(0, dialog, None);
                    self.sheet_dialog = None;
                    self.open_design(table, description);
                }
            }
        }
        if let Some(((table, description), _)) =
            drive_without_table(&mut self.mixture_design_dialog, &mut self.dialog_memory, |d| d.show(ctx))
        {
            self.open_design(table, description);
        }
        // Panel order matters: each panel takes space from what is left.
        self.workspace_rect = Some(ctx.available_rect());
        if self.shown_dashboard().is_none() {
            self.workspace_tabs(ctx, &mut actions);
            self.source_banner(ctx, &mut actions);
        }
        self.workspace_panes(ctx, &mut actions);
        self.continue_quit(ctx);
        self.close_confirmation(ctx);
        self.remove_confirmation(ctx);
        self.column_refusal_box(ctx);
        self.dashboard_removal_confirmation(ctx);
        self.project_scripts_question(ctx);
        self.settings_window(ctx);
        self.export_dialog_window(ctx, None);
        self.help_windows(ctx, &mut actions);

        // Keys act for the window they were pressed in.
        self.acting_window = window;
        for action in keys {
            self.perform(ctx, action);
        }
        self.acting_window = None;
        for action in actions {
            self.perform(ctx, action);
        }
        // A start-page task waiting for data asks for it in front of the page.
        self.task_data_dialog(ctx);
        // Views in windows of their own (each window's actions are carried out there).
        self.show_windows(ctx);
        self.show_notes_window(ctx);
        self.viewer_build_window(ctx);
        self.raise_main_for_new_dialog(ctx);
        if self.stop_recording_requested {
            self.stop_recording_requested = false;
            self.stop_recording(ctx);
        }
        // While a button is held (dragging a selection box, an axis or a
        // slider) only the end result is recorded.
        if !ctx.input(|i| i.pointer.any_down()) {
            self.sync_recording();
        }
        self.refresh_modified_marks(ctx);
        let dashboard = self.shown_dashboard().and_then(|d| self.dashboard(d)).map(|o| o.dashboard.name.clone());
        let title = match self.documents.get(self.shown) {
            Some(doc) if dashboard.is_some() => {
                let edited = if self.shows_modified(doc) { " (edited)" } else { "" };
                let name = dashboard.unwrap_or_default();
                format!("{} · {name}{edited} — AnovaCraft", self.project_name(doc.project))
            }
            Some(doc) => {
                let edited = if self.shows_modified(doc) { " (edited)" } else { "" };
                match self.members(doc.project).len() {
                    1 => format!("{}{edited} — AnovaCraft", doc.name()),
                    _ => format!("{} · {}{edited} — AnovaCraft", self.project_name(doc.project), doc.name()),
                }
            }
            None => "AnovaCraft".to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        self.last_frame_time = frame_started.elapsed();
    }
}

/// Save / export / close items for document `index` (File menu and context menus).
/// The file items for document `index`: save its project, export the
/// table, the project's scripts, and close. `several` is the project's id
/// when it holds other tables too: Close then removes just this table, and
/// Close project closes them all.
fn document_file_items(
    ui: &mut egui::Ui,
    index: usize,
    scripts: &[ProjectScript],
    several: Option<u64>,
    actions: &mut Vec<Action>,
    shortcuts: bool,
) {
    let ctx = ui.ctx().clone();
    let mut save = egui::Button::new("Save project");
    if shortcuts {
        save = save.shortcut_text(ctx.format_shortcut(&SAVE_SHORTCUT));
    }
    let what = if several.is_some() {
        "Every table of the project, with its row states and reports and the window layout, in one file"
    } else {
        "Data, row states and reports in one file that reopens with its reports"
    };
    if ui.add(save).on_hover_text(what).clicked() {
        actions.push(Action::Save(index));
        ui.close_menu();
    }
    if ui.button("Save project as…").clicked() {
        actions.push(Action::SaveAs(index));
        ui.close_menu();
    }
    if ui
        .button("Save project as template…")
        .on_hover_text(
            "The project's reports, graphs, dashboards, scripts and plot styles without its data, to build again on \
             new data with File ▸ New project from template",
        )
        .clicked()
    {
        actions.push(Action::SaveTemplate(index));
        ui.close_menu();
    }
    ui.menu_button("Export data as", |ui| {
        let labels_id = egui::Id::new("export_value_labels");
        let mut labels = ui.data_mut(|d| *d.get_persisted_mut_or_default::<bool>(labels_id));
        for format in ExportFormat::ALL {
            if ui.button(format.label()).clicked() {
                actions.push(Action::Export(index, format, labels));
                ui.close_menu();
            }
        }
        if ui
            .button("Image (PNG or 16-bit TIFF)…")
            .on_hover_text("A value column written as a grey picture, one pixel per row placed by its x and y columns")
            .clicked()
        {
            actions.push(Action::ExportImage(index));
            ui.close_menu();
        }
        ui.separator();
        if ui
            .checkbox(&mut labels, "Write value labels")
            .on_hover_text("Columns with value labels are written as their labels instead of the stored values")
            .changed()
        {
            ui.data_mut(|d| d.insert_persisted(labels_id, labels));
        }
    });
    let mut export = egui::Button::new("Export reports…");
    if shortcuts {
        export = export.shortcut_text(ctx.format_shortcut(&EXPORT_REPORTS_SHORTCUT));
    }
    if ui
        .add(export)
        .on_hover_text("Save this table's reports as a PDF, Word, PowerPoint, HTML, Markdown or Excel file")
        .clicked()
    {
        actions.push(Action::ExportReports(index));
        ui.close_menu();
    }
    if !scripts.is_empty() {
        ui.menu_button("Scripts", |ui| {
            for (i, script) in scripts.iter().enumerate() {
                ui.menu_button(&script.name, |ui| {
                    let mut push = |op| actions.push(Action::ProjectScript { doc: index, script: i, op });
                    if ui.button("Run").clicked() {
                        push(ProjectScriptOp::Run);
                        ui.close_menu();
                    }
                    if ui.button("Open in Script window").clicked() {
                        push(ProjectScriptOp::Open);
                        ui.close_menu();
                    }
                    let mut on_open = script.run_on_open;
                    if ui
                        .checkbox(&mut on_open, "Run when the project opens")
                        .on_hover_text("Opening the project asks before running it")
                        .clicked()
                    {
                        push(ProjectScriptOp::ToggleRunOnOpen);
                    }
                    ui.separator();
                    if ui.button("Remove from project").clicked() {
                        push(ProjectScriptOp::Remove);
                        ui.close_menu();
                    }
                });
            }
        });
    }
    ui.separator();
    let mut close = egui::Button::new(if several.is_some() { "Remove table from project" } else { "Close" });
    if shortcuts {
        close = close.shortcut_text(ctx.format_shortcut(&CLOSE_SHORTCUT));
    }
    let close = ui.add(close);
    let close = if several.is_some() {
        close.on_hover_text("Close this table and take it out of its project (save the project to keep the change)")
    } else {
        close
    };
    if close.clicked() {
        actions.push(Action::Close(index));
        ui.close_menu();
    }
    if let Some(project) = several {
        if ui.button("Close project").on_hover_text("Close every table of this project").clicked() {
            actions.push(Action::CloseProject(project));
            ui.close_menu();
        }
    }
}

fn document_context_menu(
    ui: &mut egui::Ui,
    index: usize,
    scripts: &[ProjectScript],
    several: Option<u64>,
    actions: &mut Vec<Action>,
) {
    if ui.button("Show").clicked() {
        actions.push(Action::Activate(index));
        ui.close_menu();
    }
    ui.separator();
    document_file_items(ui, index, scripts, several, actions, false);
}

/// Draws the side-by-side divider on the left edge of the results pane: a
/// colored band with a grip in the middle, so it is distinct from ordinary
/// separator lines. It brightens and widens while the pointer is over it (which
/// includes while it is being dragged, since the edge follows the pointer).
/// Margin (in points) between the pane divider and the widgets on either side.
///
/// The divider can be grabbed within `resize_grab_radius_side` of it, and egui
/// counts a pointer within `interact_radius` of a widget as over that widget,
/// preferring whichever was drawn later — here the panes' contents. Keeping
/// widgets further away than both radii together (plus a small safety
/// allowance) means a press near the divider always resizes the panes.
fn divider_gutter(style: &egui::Style) -> i8 {
    let i = &style.interaction;
    (i.resize_grab_radius_side + i.interact_radius + 3.0).ceil().clamp(8.0, 40.0) as i8
}

fn divider_id() -> egui::Id {
    egui::Id::new("results_pane_divider")
}

/// The draggable divider between the data grid and the results pane, with its
/// grip. It is registered after both panes on their layer, so it wins the
/// pointer over anything in them (egui prefers the widget added last), and a
/// press within its band always resizes the panes.
fn divider_handle(ctx: &egui::Context, x: f32, y_range: egui::Rangef) {
    const GRAB: f32 = 6.0;
    let rect = egui::Rect::from_x_y_ranges(x - GRAB..=x + GRAB, y_range);
    let ui = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("results_pane_divider_ui"),
        egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(rect),
    );
    let response = ui.interact(rect, divider_id(), egui::Sense::drag());
    let active = response.hovered() || response.dragged();
    if active {
        ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let (color, band_width) = if active { (theme::DIVIDER_ACTIVE, 4.0) } else { (theme::DIVIDER, 3.0) };
    let painter = ctx.layer_painter(egui::LayerId::background());
    let half = band_width / 2.0;
    painter.rect_filled(egui::Rect::from_x_y_ranges(x - half..=x + half, y_range), 0.0_f32, color);
    let grip = egui::pos2(x, y_range.center());
    painter.rect_filled(egui::Rect::from_center_size(grip, egui::vec2(10.0, 48.0)), 5.0_f32, color);
    for dy in [-12.0, -4.0, 4.0, 12.0] {
        painter.circle_filled(grip + egui::vec2(0.0, dy), 1.6, egui::Color32::WHITE);
    }
}

/// The system clipboard's text, or a message saying why there is none.
fn read_clipboard() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|e| format!("Could not read text from the clipboard ({e})"))
}

fn column_names(table: &DataTable) -> Vec<String> {
    table.columns().iter().map(|c| c.name.clone()).collect()
}

/// Show the dialog in `slot`, if one is open. When it closes (run or
/// cancelled) its settings are remembered for document `doc` and the slot
/// empties; a run returns its result and where the report goes.
fn drive<D: Remember, T>(
    slot: &mut Option<D>,
    memory: &mut dialog_kit::Memory,
    doc: u64,
    table: &DataTable,
    show: impl FnOnce(&mut D) -> Outcome<T>,
) -> Option<(T, Placement)> {
    let dialog = slot.as_mut()?;
    match show(dialog) {
        Outcome::Open | Outcome::Reset => None,
        Outcome::Cancel => {
            memory.keep(doc, dialog, Some(table));
            *slot = None;
            None
        }
        Outcome::Run(output, place) => {
            memory.keep(doc, dialog, Some(table));
            *slot = None;
            Some((output, place))
        }
    }
}

/// [`drive`] for a dialog that reads no table (designs): its settings are
/// remembered for all documents.
fn drive_without_table<D: Remember, T>(
    slot: &mut Option<D>,
    memory: &mut dialog_kit::Memory,
    show: impl FnOnce(&mut D) -> Outcome<T>,
) -> Option<(T, Placement)> {
    let dialog = slot.as_mut()?;
    let outcome = show(dialog);
    if matches!(outcome, Outcome::Cancel | Outcome::Run(..)) {
        memory.keep(0, dialog, None);
        *slot = None;
    }
    match outcome {
        Outcome::Run(output, place) => Some((output, place)),
        _ => None,
    }
}

/// Whether a report of this kind can be reopened in its dialog ("Edit
/// analysis…" in its options menu).
pub(crate) fn editable(saved: &app_model::SavedReport) -> bool {
    use app_model::SavedReport as S;
    matches!(
        saved,
        S::Linear { .. }
            | S::Scatter3d { .. }
            | S::DataSurface { .. }
            | S::ImagePlot { .. }
            | S::Surface { .. }
            | S::Stepwise { .. }
            | S::BestSubsets { .. }
            | S::CategoricalLogistic { .. }
            | S::Distribution { .. }
            | S::Bivariate { .. }
            | S::DistributionFit { .. }
            | S::OneSample { .. }
            | S::Bayesian { .. }
            | S::TreatmentEffects { .. }
            | S::Signal { .. }
            | S::Groups { .. }
            | S::Paired { .. }
            | S::Proportions { .. }
            | S::Rates { .. }
            | S::Power { .. }
            | S::GoodnessOfFit { .. }
            | S::Crossover { .. }
            | S::FactorialRepeated { .. }
            | S::Nested { .. }
            | S::DistributionCalculator { .. }
            | S::AdjustPValues { .. }
            | S::AreaUnderCurve { .. }
            | S::MultipleResponseItems { .. }
            | S::ManyTests { .. }
            | S::Screening { .. }
            | S::Pca { .. }
            | S::Cluster { .. }
            | S::Glm { .. }
            | S::Regularized { .. }
            | S::Correspondence { .. }
            | S::MultipleResponses { .. }
            | S::Correlations { .. }
            | S::OutlierDistances { .. }
            | S::ItemAnalysis { .. }
            | S::Manova { .. }
            | S::FactorAnalysis { .. }
            | S::DiscriminantAnalysis { .. }
            | S::GaussianMixture { .. }
            | S::LatentClass { .. }
            | S::TextExploration { .. }
            | S::TopicModel { .. }
            | S::Sentiment { .. }
            | S::Embedding { .. }
            | S::Mixed { .. }
            | S::DecisionTree { .. }
            | S::RandomForest { .. }
            | S::BoostedTrees { .. }
            | S::PartialLeastSquares { .. }
            | S::SupportVectorMachine { .. }
            | S::Mars { .. }
            | S::NeuralNetwork { .. }
            | S::GaussianProcess { .. }
            | S::SequentialOptimization { .. }
            | S::ModelSearch { .. }
            | S::FunctionalData { .. }
            | S::ModelComparison { .. }
            | S::Nonlinear { .. }
            | S::DoseResponse { .. }
            | S::MethodComparison { .. }
            | S::ShelfLife { .. }
            | S::Mixture { .. }
            | S::Anom { .. }
            | S::Pareto { .. }
            | S::CauseEffect { .. }
            | S::MultiVari { .. }
            | S::ControlChart { .. }
            | S::Attribute { .. }
            | S::TimeWeighted { .. }
            | S::Capability { .. }
            | S::MultivariateChart { .. }
            | S::Gauge { .. }
            | S::BetweenWithinCapability { .. }
            | S::AttributeCapability { .. }
            | S::AcceptanceSampling { .. }
            | S::GaugeLinearity { .. }
            | S::GaugeRepeatability { .. }
            | S::IntraclassCorrelation { .. }
            | S::AttributeAgreement { .. }
            | S::AttributeGauge { .. }
            | S::PcaChart { .. }
            | S::MonitoringLimits { .. }
            | S::MultiwayTable { .. }
            | S::StratifiedTables { .. }
            | S::PermutationTest { .. }
            | S::SignalToNoise { .. }
            | S::DeclaredDesign { .. }
            | S::DesignEvaluation { .. }
            | S::FactorialAnalysis { .. }
            | S::Survival { .. }
            | S::Growth { .. }
            | S::Recurrence { .. }
            | S::TestPlan { .. }
            | S::LifeDistribution { .. }
            | S::CompetingRisks { .. }
            | S::Warranty { .. }
            | S::LifeStress { .. }
            | S::Degradation { .. }
            | S::ProportionalHazards { .. }
            | S::SurvivalRegression { .. }
            | S::Imputation { .. }
            | S::SeriesExplore { .. }
            | S::SignalProcessing { .. }
            | S::ImageAnalysis { .. }
            | S::ExponentialSmoothing { .. }
            | S::Arima { .. }
            | S::PrincipalCoordinates { .. }
            | S::Pharmacokinetics { .. }
            | S::Fatigue { .. }
    )
}

/// Whether `shifts` (applied in order to a table of `n_before` columns)
/// moved or removed any column that was there before; appending columns at
/// the end does not.
fn moves_existing_columns(shifts: &[ColumnShift], n_before: usize) -> bool {
    let mut n = n_before;
    for shift in shifts {
        match *shift {
            ColumnShift::Inserted(at) if at >= n => n += 1,
            ColumnShift::Inserted(_) | ColumnShift::Removed(_) => return true,
        }
    }
    false
}

/// Keep reports pointing at the same columns after one was inserted or
/// removed, or renamed (`before`: the column names before the edit).
fn follow_columns(doc: &mut Document, shifts: Vec<ColumnShift>, before: &[String]) {
    if !shifts.is_empty() {
        for shift in shifts {
            for report in &mut doc.reports {
                report.remap_columns(shift, &doc.table, &doc.rows);
            }
        }
    } else if before.len() == doc.table.n_cols() {
        for (old, col) in before.iter().zip(doc.table.columns()) {
            if *old != col.name {
                for report in &mut doc.reports {
                    report.rename_column(old, &col.name);
                }
            }
        }
    }
}

/// A small, faint heading over a group of related items in a menu.
fn menu_heading(ui: &mut egui::Ui, heading: &str) {
    ui.label(RichText::new(heading).weak().small());
}

/// The Bayesian submenu (listed under both Compare and Model): the tool
/// whose item was clicked.
fn bayes_submenu(ui: &mut egui::Ui) -> Option<crate::bayes_dialog::BayesTool> {
    let mut chosen = None;
    ui.menu_button("Bayesian", |ui| {
        for (label, hint, tool) in crate::bayes_dialog::MENU {
            if ui.button(label).on_hover_text(hint).clicked() {
                chosen = Some(tool);
                ui.close_menu();
            }
        }
    });
    chosen
}

/// The dialogs of the Design menu's "This table" group and the table they read.
struct DesignTableTools<'a> {
    table: &'a DataTable,
    study: &'a mut Option<StudyDialog>,
    augment: &'a mut Option<AugmentDialog>,
    simulate: &'a mut Option<crate::simulate_dialog::SimulateDialog>,
    evaluation: &'a mut Option<crate::evaluation_dialog::EvaluationDialog>,
    factorial: &'a mut Option<crate::factorial_dialog::FactorialDialog>,
    sequential: &'a mut Option<crate::sequential_dialog::SequentialDialog>,
}

/// The Design menu: each item opens the design dialog at that design type,
/// the mixture design dialog or a design-table dialog; with an open table
/// (`tools`) it also offers the analyses that read a table as a design.
/// The Design menu's groups of designs (indices into
/// [`design_dialog::KIND_NAMES`]), also listed by the start page.
const DESIGN_GROUPS: [(&str, &[usize]); 5] = [
    ("Factorial", &[0, 1, 2, 9]),
    ("Screening", &[3, 4, design_dialog::CATEGORICAL_SCREENING]),
    ("Response surface", &[5, 6]),
    ("Optimal", &[7, 8]),
    ("Space filling", &[10, 11, 12, 13]),
];

fn design_menu(
    ui: &mut egui::Ui,
    memory: &dialog_kit::Memory,
    dialog: &mut Option<DesignDialog>,
    mixture: &mut Option<MixtureDesignDialog>,
    sheet: &mut Option<SheetDialog>,
    tools: Option<DesignTableTools<'_>>,
) {
    crate::menus::menu_button(ui, "Design", |ui| {
        for (heading, kinds) in DESIGN_GROUPS {
            menu_heading(ui, heading);
            for &i in kinds {
                if ui.button(format!("{}…", design_dialog::KIND_NAMES[i])).clicked() {
                    *dialog = Some(memory.recall(0, DesignDialog::new(i), None, &[]));
                    ui.close_menu();
                }
            }
            if heading == "Optimal"
                && ui
                    .button("Split-plot (hard-to-change factors)…")
                    .on_hover_text(
                        "D-optimal design whose hard-to-change factors stay fixed within whole plots (and very hard \
                         ones within whole plots of subplots), with Whole Plot columns and the mixed model to analyse it",
                    )
                    .clicked()
            {
                *dialog = Some(memory.recall(0, DesignDialog::split_plot(), None, &[]));
                ui.close_menu();
            }
        }
        menu_heading(ui, "Mixture");
        for (label, centroid, hint) in [
            ("Simplex lattice…", false, "Blends in steps of 1/m; supports the Scheffé model of degree m"),
            ("Simplex centroid…", true, "Equal parts of every subset of components; supports the special cubic"),
        ] {
            if ui.button(label).on_hover_text(hint).clicked() {
                *mixture = Some(memory.recall(0, MixtureDesignDialog::new(centroid), None, &[]));
                ui.close_menu();
            }
        }
        if ui
            .button("Extreme vertices…")
            .on_hover_text(
                "Vertices (and edge and face centroids) of a region with lower and upper bounds per component",
            )
            .clicked()
        {
            *mixture = Some(memory.recall(0, MixtureDesignDialog::extreme_vertices(), None, &[]));
            ui.close_menu();
        }
        menu_heading(ui, "Robust design");
        if ui
            .button("Robust design (orthogonal arrays)…")
            .on_hover_text("Control factors in an inner array crossed with noise factors in an outer array")
            .clicked()
        {
            *sheet = Some(SheetDialog::opening(SheetKind::RobustDesign));
            ui.close_menu();
        }
        menu_heading(ui, "Measurement");
        if ui
            .button("Gauge study worksheet…")
            .on_hover_text("A new table of randomized runs for a crossed or nested gauge study")
            .clicked()
        {
            *sheet = Some(SheetDialog::opening(SheetKind::GaugeWorksheet));
            ui.close_menu();
        }
        if let Some(DesignTableTools { table, study, augment, simulate, evaluation, factorial, sequential }) = tools {
            ui.separator();
            menu_heading(ui, "This table");
            if ui
                .button("Signal-to-noise analysis…")
                .on_hover_text("Signal-to-noise ratios and response tables for an inner/outer array experiment")
                .clicked()
            {
                *study = Some(StudyDialog::opening(StudyTool::SignalToNoise, table));
                ui.close_menu();
            }
            if ui
                .button("Declare table as design…")
                .on_hover_text(
                    "Treat existing columns as factors and responses; check balance, correlation and estimability",
                )
                .clicked()
            {
                *study = Some(StudyDialog::opening(StudyTool::DeclareDesign, table));
                ui.close_menu();
            }
            if ui
                .button("Augment design…")
                .on_hover_text(
                    "Add fold-over, center, axial or D-optimal runs to the design in this table, as a new table",
                )
                .clicked()
            {
                *augment = Some(AugmentDialog::opening(table));
                ui.close_menu();
            }
            if ui
                .button("Simulate responses…")
                .on_hover_text(
                    "Add made-up responses from random coefficients (few active effects, interactions mostly with \
                     active parents), to try out the analysis before running the experiment",
                )
                .clicked()
            {
                *simulate = Some(crate::simulate_dialog::SimulateDialog::opening(table));
                ui.close_menu();
            }
            if ui
                .button("Evaluate design…")
                .on_hover_text(
                    "Before collecting data: power of each term, prediction variance, aliasing and efficiencies; \
                     compare two designs",
                )
                .clicked()
            {
                *evaluation = Some(crate::evaluation_dialog::EvaluationDialog::opening(table));
                ui.close_menu();
            }
            if ui
                .button("Factorial analysis…")
                .on_hover_text(
                    "Effects of a two-level experiment: normal and half-normal plots, Lenth's method, main-effect, \
                     interaction and cube plots, dispersion effects",
                )
                .clicked()
            {
                *factorial = Some(crate::factorial_dialog::FactorialDialog::opening(table));
                ui.close_menu();
            }
            if ui
                .button("Propose next runs…")
                .on_hover_text(
                    "Sequential optimization: fit a Gaussian process to the runs so far and choose the next batch \
                     by expected improvement, then add them to the table",
                )
                .clicked()
            {
                *sequential = Some(crate::sequential_dialog::SequentialDialog::opening(table));
                ui.close_menu();
            }
        }
    });
}

/// Switch egui between light, dark and the operating system's appearance.
fn apply_color_scheme(ctx: &egui::Context, scheme: ColorScheme) {
    ctx.set_theme(match scheme {
        ColorScheme::System => egui::ThemePreference::System,
        ColorScheme::Light => egui::ThemePreference::Light,
        ColorScheme::Dark => egui::ThemePreference::Dark,
    });
}

/// Radio buttons for the colour scheme; `true` when the choice changed.
fn color_scheme_choices(ui: &mut egui::Ui, scheme: &mut ColorScheme) -> bool {
    let before = *scheme;
    for c in ColorScheme::ALL {
        ui.radio_value(scheme, c, c.label());
    }
    *scheme != before
}

/// Radio buttons for the text size, each beside a sample drawn at that size;
/// `true` when the choice changed.
fn text_size_choices(ui: &mut egui::Ui, size: &mut TextSize) -> bool {
    let before = *size;
    egui::Grid::new("text_size_choices").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        for t in TextSize::ALL {
            ui.radio_value(size, t, t.label());
            let sample = RichText::new("Mean = 12.47 · p = 0.031").size(theme::example_text_size(t.scale()));
            if ui.add(egui::Label::new(sample).sense(egui::Sense::click())).clicked() {
                *size = t;
            }
            ui.end_row();
        }
    });
    *size != before
}

/// Radio buttons for the tooltip delay; `true` when the choice changed.
fn tooltip_delay_choices(ui: &mut egui::Ui, delay: &mut TooltipDelay) -> bool {
    let before = *delay;
    ui.horizontal(|ui| {
        for d in TooltipDelay::ALL {
            ui.radio_value(delay, d, d.label());
        }
    });
    *delay != before
}

/// `text` with its first letter in upper case.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// A document's tooltip, saying so when it has unsaved changes (marked • on its tab).
fn edited_tooltip(tooltip: String, edited: bool) -> String {
    if edited { format!("{tooltip}\n• Unsaved changes") } else { tooltip }
}

/// Tips shown above the workspace until dismissed (Settings turns them back
/// on). Returns `true` when "Got it" is clicked.
fn getting_started_tips(ui: &mut egui::Ui) -> bool {
    let mut dismiss = false;
    egui::Frame::group(ui.style()).fill(ui.visuals().faint_bg_color).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Getting started").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                dismiss = ui.button("Got it").on_hover_text("Hide these tips (Settings can show them again)").clicked();
            });
        });
        for tip in [
            "Each variable in the sidebar shows its measurement level; click it to find its column, or drag it onto \
             a graph shelf.",
            "Side by side (⌘3 / Ctrl+3) shows the data next to the results, so you can watch both.",
            "Select points in any plot, or rows in the grid, and the same rows light up everywhere.",
            "Right-click column headings, row numbers, report headings and plots for what you can do with them.",
        ] {
            ui.label(format!("• {tip}"));
        }
    });
    dismiss
}

/// File ▸ Open recent: one item per remembered file, most recent first.
fn recent_files_items(ui: &mut egui::Ui, recent: &[PathBuf], actions: &mut Vec<Action>) {
    for path in recent {
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
            actions.push(Action::OpenPaths(vec![path.clone()]));
            ui.close_menu();
        }
    }
}

/// The keyboard shortcuts, for Help ▸ Keyboard shortcuts.
fn shortcut_table(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let key = |s: &KeyboardShortcut| ctx.format_shortcut(s);
    // "⌘" on a Mac, "Ctrl+" elsewhere.
    let command = key(&SELECT_ALL_SHORTCUT).trim_end_matches('A').to_owned();
    let rows: Vec<(&str, Vec<(String, &str)>)> = vec![
        (
            "Files",
            vec![
                (key(&OPEN_SHORTCUT), "Open a data file or project"),
                (key(&NEW_SHORTCUT), "New data table"),
                (key(&SAVE_SHORTCUT), "Save the project"),
                (key(&save_as_shortcut()), "Save the project as…"),
                (key(&EXPORT_REPORTS_SHORTCUT), "Export reports (PDF, Word, PowerPoint, …)"),
                (key(&CLOSE_SHORTCUT), "Close the dataset (in a view's own window: close that window)"),
                (key(&SETTINGS_SHORTCUT), "Settings"),
            ],
        ),
        (
            "Views",
            vec![
                (key(&WORKSPACE_SHORTCUTS[0]), "Data"),
                (key(&WORKSPACE_SHORTCUTS[1]), "Results"),
                (key(&WORKSPACE_SHORTCUTS[2]), "Side by side"),
            ],
        ),
        (
            "Editing the grid",
            vec![
                (key(&UNDO_SHORTCUT), "Undo"),
                (key(&redo_shortcut()), "Redo"),
                (key(&CUT_SHORTCUT), "Cut"),
                (key(&COPY_SHORTCUT), "Copy"),
                (key(&PASTE_SHORTCUT), "Paste"),
                (key(&CLEAR_SHORTCUT), "Clear the selected cells"),
                (key(&FILL_DOWN_SHORTCUT), "Fill down"),
                (key(&SELECT_ALL_SHORTCUT), "Select all cells"),
                (key(&FIND_SHORTCUT), "Find in the table"),
                (
                    key(&table_view::INSERT_SHORTCUT),
                    "Insert rows above the cursor (as many as the block spans), or columns left of the selected headings",
                ),
                ("Enter, F2, or type".to_owned(), "Edit the cell"),
                (
                    "Enter / Tab".to_owned(),
                    "Commit and move down / right (Shift reverses); Enter on the last row adds a row",
                ),
                ("Escape".to_owned(), "Cancel the edit"),
            ],
        ),
        (
            "Moving in the grid (add Shift to extend a block)",
            vec![
                ("Arrow keys".to_owned(), "One cell"),
                (format!("{command}arrow"), "To the edge of the table"),
                ("Home / End".to_owned(), "First / last column"),
                ("Page Up / Page Down".to_owned(), "20 rows"),
            ],
        ),
    ];
    egui::Grid::new("shortcut_table").num_columns(2).spacing([24.0, 3.0]).show(ui, |ui| {
        for (group, items) in rows {
            ui.label(RichText::new(group).strong());
            ui.end_row();
            for (keys, what) in items {
                ui.monospace(keys);
                ui.label(what);
                ui.end_row();
            }
        }
    });
}

fn planned_menu(ui: &mut egui::Ui, name: &str, why: &str) {
    ui.add_enabled(false, egui::Button::new(name)).on_disabled_hover_text(why);
}

/// The settings saved by the last session. Tests start from the defaults,
/// so the developer's own settings never change what they see.
/// "Frame 12 ms", with the slowest report of the frame when one took
/// noticeable time ("Frame 212 ms · slowest: Distribution of x 205 ms").
fn frame_time_text(ctx: &egui::Context, frame: std::time::Duration) -> String {
    let mut text = format!("Frame {:.0} ms", frame.as_secs_f64() * 1000.0);
    if let Some((title, took)) = report_view::slowest_report(ctx).filter(|(_, t)| t.as_secs_f64() >= 0.002) {
        text.push_str(&format!(" · slowest: {} {:.0} ms", report_view::short_title(&title, 30), took.as_secs_f64() * 1000.0));
    }
    text
}

/// `true` if `edit` adds, removes or reorders rows (so reports computed
/// before it point at the wrong rows).
fn moves_rows(edit: &Edit) -> bool {
    match edit {
        Edit::InsertRows { .. } | Edit::RemoveRows { .. } | Edit::ReorderRows { .. } => true,
        Edit::Batch { edits, .. } => edits.iter().any(moves_rows),
        _ => false,
    }
}

fn load_settings() -> AppSettings {
    // The web viewer has no settings file.
    if cfg!(any(test, target_arch = "wasm32")) { AppSettings::default() } else { AppSettings::load() }
}

/// Save the settings for the next session (not from tests, which must
/// leave the developer's own settings file alone).
fn save_settings(settings: &AppSettings) -> std::io::Result<()> {
    if cfg!(any(test, target_arch = "wasm32")) { Ok(()) } else { settings.save() }
}

pub mod crash;
mod dashboards;
mod folders;
#[cfg(test)]
mod gui_tests;
mod notebooks;
mod manuals;
mod notes;
mod notices;
mod projects;
mod recovery;
mod renaming;
mod reuse;
mod row_styles;
mod subsets;
mod viewer;
mod viewer_build;
#[cfg(not(target_arch = "wasm32"))]
pub use viewer::publish_sample;
mod welcome;
mod windows;

/// `n` with its noun in the singular or plural ("1 row", "3 rows").
pub(crate) fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_edits_that_move_existing_columns_close_dialogs() {
        use ColumnShift::{Inserted, Removed};
        assert!(!moves_existing_columns(&[], 3));
        // Appending at the end (once or several times) leaves every column in place.
        assert!(!moves_existing_columns(&[Inserted(3)], 3));
        assert!(!moves_existing_columns(&[Inserted(3), Inserted(4)], 3));
        assert!(moves_existing_columns(&[Inserted(1)], 3));
        assert!(moves_existing_columns(&[Inserted(3), Inserted(1)], 3));
        assert!(moves_existing_columns(&[Removed(2)], 3));
    }
}
