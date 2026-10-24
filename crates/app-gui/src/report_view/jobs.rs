//! Reports computed on background threads, so that editing data, excluding
//! rows or changing a report's filter never waits for a slow analysis.
//!
//! A report that needs computing again hands a [`Job`] to [`ReportJobs`]:
//! its request, display options and scope ([`JobKey`]), with snapshots of
//! the table (shared, see `Document::table`) and the row states. A worker
//! builds a fresh report body and its sections; the app installs the result
//! on the next frame ([`super::Report::install`]) if the report still wants
//! what was computed, and otherwise queues it again. Each report has at most
//! one job running, so a burst of edits costs at most two computations.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_model::project::SavedReport;
use app_model::report::ReportSection;
use app_model::report_scope::ReportScope;
use eframe::egui;
use stat_core::interrupt::Interrupt;
use stat_data::{DataTable, RowStateManager};

use super::{ReportBody, assemble_sections};

/// Threads computing reports. Engines with parallel loops of their own
/// (forests, boosting, cross-validation) still spread over every core.
const WORKERS: usize = 2;

/// What a report was (or is being) computed from: the data and exclusion
/// revisions, the request with its display options, and the row filter
/// and column picker.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct JobKey {
    pub at: (u64, u64),
    pub saved: SavedReport,
    pub scope: ReportScope,
}

/// A report's computation in progress.
pub(super) struct Pending {
    pub generation: u64,
    /// What it computes.
    pub key: JobKey,
    /// Cancel and progress: engines with long loops check it and count
    /// their steps (see [`stat_core::interrupt`]).
    pub interrupt: Arc<Interrupt>,
}

/// One report to compute.
struct Job {
    report: u64,
    generation: u64,
    key: JobKey,
    table: Arc<DataTable>,
    rows: Arc<RowStateManager>,
    interrupt: Arc<Interrupt>,
}

/// A computed report body, its pooled sections and the sections it shows.
pub(super) struct Computed {
    pub body: ReportBody,
    pub pooled: Vec<ReportSection>,
    pub sections: Arc<Result<Vec<ReportSection>, String>>,
}

/// A finished job: the report it is for, which of its jobs, what it was
/// computed from, and the result (or why the analysis stopped).
pub struct Done {
    pub(super) report: u64,
    pub(super) generation: u64,
    pub(super) key: JobKey,
    pub(super) outcome: Result<Box<Computed>, String>,
}

impl Done {
    /// The report the job was for.
    #[must_use]
    pub fn report(&self) -> u64 {
        self.report
    }
}

/// The background workers and the queue between them and the app.
pub struct ReportJobs {
    jobs: Sender<Job>,
    /// The workers' end of the queue, kept so that jobs can be queued even
    /// with no worker (tests hold jobs back this way).
    _queue: Arc<Mutex<Receiver<Job>>>,
    done: Receiver<Done>,
    next_generation: u64,
    /// Jobs submitted whose results have not been taken yet.
    outstanding: usize,
}

impl ReportJobs {
    /// Start the workers; each finished job asks `ctx` for a repaint so the
    /// app installs it without waiting for input.
    #[must_use]
    pub fn new(ctx: &egui::Context) -> Self {
        Self::with_workers(ctx, WORKERS)
    }

    /// [`Self::new`] with `workers` threads; with none, jobs wait in the
    /// queue for ever.
    #[must_use]
    pub(crate) fn with_workers(ctx: &egui::Context, workers: usize) -> Self {
        let (jobs, queue) = channel::<Job>();
        let (finished, done) = channel::<Done>();
        let queue = Arc::new(Mutex::new(queue));
        for i in 0..workers {
            let queue = Arc::clone(&queue);
            let finished = finished.clone();
            let ctx = ctx.clone();
            let spawned = std::thread::Builder::new().name(format!("report-worker-{i}")).spawn(move || {
                loop {
                    // The lock is held only while waiting for the next job.
                    let job = match queue.lock() {
                        Ok(queue) => queue.recv(),
                        Err(_) => return,
                    };
                    let Ok(job) = job else { return };
                    // A job cancelled before it started is not run.
                    let outcome = if job.interrupt.is_cancelled() {
                        Err("cancelled".to_owned())
                    } else {
                        stat_core::interrupt::with(Arc::clone(&job.interrupt), || run(&job))
                    };
                    let done = Done { report: job.report, generation: job.generation, key: job.key, outcome };
                    if finished.send(done).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
            // Without a worker the queue fills and the reports show as
            // updating; nothing else breaks.
            debug_assert!(spawned.is_ok(), "could not start a report worker");
        }
        Self { jobs, _queue: queue, done, next_generation: 1, outstanding: 0 }
    }

    /// Queue `key` for report `report` on snapshots of the data; returns
    /// the job's state for the report to keep.
    pub(super) fn submit(
        &mut self,
        report: u64,
        key: JobKey,
        table: Arc<DataTable>,
        rows: Arc<RowStateManager>,
    ) -> Pending {
        let generation = self.next_generation;
        self.next_generation += 1;
        let interrupt = Interrupt::new();
        let job = Job { report, generation, key: key.clone(), table, rows, interrupt: Arc::clone(&interrupt) };
        if self.jobs.send(job).is_ok() {
            self.outstanding += 1;
        }
        Pending { generation, key, interrupt }
    }

    /// Finished jobs. With `wait`, waits up to that long for every job
    /// still running to finish, so quick reports update in the frame that
    /// asked for them and only slow ones show as updating.
    pub fn take(&mut self, wait: Option<Duration>) -> Vec<Done> {
        let mut out: Vec<Done> = self.done.try_iter().collect();
        if let Some(wait) = wait {
            let deadline = Instant::now() + wait;
            while out.len() < self.outstanding {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.done.recv_timeout(left) {
                    Ok(done) => out.push(done),
                    Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
                }
            }
        }
        self.outstanding = self.outstanding.saturating_sub(out.len());
        out
    }

    /// `true` while some job has not finished.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.outstanding > 0
    }
}

/// Compute `job`, turning a panic in the analysis into an error message
/// (and a crash report marked as caught).
fn run(job: &Job) -> Result<Box<Computed>, String> {
    crate::app::crash::guarded(|| compute(&job.key, &job.table, &job.rows))
}

/// The report `key` asks for, computed on `table` and `rows` as
/// [`super::Report::new`] computes it.
pub(super) fn compute(key: &JobKey, table: &DataTable, rows: &RowStateManager) -> Box<Computed> {
    let filtered = key.scope.apply(table, rows);
    let body = ReportBody::compute(key.saved.clone(), table, filtered.as_ref().unwrap_or(rows));
    let pooled = key.scope.pooled_sections(&key.saved, table, rows);
    let sections = Arc::new(assemble_sections(&body, &key.scope, &pooled));
    Box::new(Computed { body, pooled, sections })
}
