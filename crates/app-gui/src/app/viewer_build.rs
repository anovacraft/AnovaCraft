//! "Publish as web app…" needs the web viewer (the app built for
//! WebAssembly, `scripts/build-web-viewer.sh`), which an ordinary
//! `cargo run` does not make. When it is missing, Publish writes nothing
//! and this window says so, shows the command that builds it and, in a
//! source checkout, builds it in the background and then carries on with
//! the publish.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use super::*;

/// The script that builds the web viewer, relative to the repository.
const BUILD_SCRIPT: &str = "scripts/build-web-viewer.sh";

/// The open "Build the web viewer" window.
pub(super) struct ViewerBuild {
    /// The dashboard to publish once the viewer is there.
    pub dashboard: u64,
    /// The source checkout holding the build script, if one was found.
    pub repository: Option<PathBuf>,
    /// The build running in the background, with its output so far.
    running: Option<Running>,
    /// Why the last build failed (the end of its output).
    failed: Option<String>,
}

struct Running {
    lines: Receiver<BuildEvent>,
    output: Vec<String>,
}

enum BuildEvent {
    Line(String),
    Done(bool),
}

impl ViewerBuild {
    pub(super) fn new(dashboard: u64) -> Self {
        Self { dashboard, repository: repository(), running: None, failed: None }
    }

    /// The Terminal command that builds the viewer.
    pub(super) fn command(&self) -> String {
        match &self.repository {
            Some(root) => format!("cd \"{}\" && ./{BUILD_SCRIPT}", root.display()),
            None => format!("./{BUILD_SCRIPT}   (in the AnovaCraft source folder)"),
        }
    }
}

/// The source checkout this program was built from, when its build script
/// is still there: the folder above `target/` beside the program, else the
/// folder it was compiled in.
fn repository() -> Option<PathBuf> {
    let has_script = |dir: &Path| dir.join(BUILD_SCRIPT).is_file();
    let from_exe =
        std::env::current_exe().ok().and_then(|exe| exe.ancestors().find(|d| has_script(d)).map(Path::to_path_buf));
    from_exe.or_else(|| {
        let compiled = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        has_script(&compiled).then(|| compiled.canonicalize().unwrap_or(compiled))
    })
}

/// Run the build script in `root` on a background thread, sending its
/// output line by line and then whether it succeeded.
fn start_build(root: &Path) -> Result<Receiver<BuildEvent>, String> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let (tx, rx) = channel();
    let mut child = Command::new("bash")
        .arg("-c")
        // Started from Finder the app may not have Cargo's folder on its path.
        .arg(format!("export PATH=\"$HOME/.cargo/bin:$PATH\"; ./{BUILD_SCRIPT} 2>&1"))
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start {BUILD_SCRIPT}: {e}"))?;
    let stdout = child.stdout.take().ok_or("no output from the build")?;
    std::thread::Builder::new()
        .name("web-viewer-build".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = tx.send(BuildEvent::Line(line));
            }
            let ok = child.wait().is_ok_and(|s| s.success());
            let _ = tx.send(BuildEvent::Done(ok));
        })
        .map_err(|e| format!("could not start the build: {e}"))?;
    Ok(rx)
}

impl StatsApp {
    /// Draw the "Build the web viewer" window, if open, and follow its build.
    pub(super) fn viewer_build_window(&mut self, ctx: &egui::Context) {
        let Some(build) = &mut self.viewer_build else { return };
        // Follow the build.
        let mut finished = None;
        if let Some(run) = &mut build.running {
            while let Ok(event) = run.lines.try_recv() {
                match event {
                    BuildEvent::Line(line) => run.output.push(line),
                    BuildEvent::Done(ok) => finished = Some(ok),
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if let Some(ok) = finished {
            let run = build.running.take().expect("a build was running");
            if ok && super::dashboards::viewer_bundle().is_some() {
                let id = build.dashboard;
                self.viewer_build = None;
                self.info("Built the web viewer; choose the folder to publish to");
                self.publish_dashboard(id);
                return;
            }
            let tail = run.output.iter().rev().take(12).rev().cloned().collect::<Vec<_>>().join("\n");
            build.failed = Some(if tail.is_empty() { "The build stopped without output.".to_owned() } else { tail });
        }

        let command = build.command();
        let mut close = false;
        let mut start = false;
        egui::Window::new("Build the web viewer")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_max_width(520.0);
                ui.label(
                    "Publishing a dashboard needs the web viewer: AnovaCraft built for web browsers \
                     (anovacraft_viewer.js and anovacraft_viewer_bg.wasm). An ordinary build of the app \
                     does not make it, so nothing has been published yet.",
                );
                ui.add_space(6.0);
                ui.label("Build it once with this Terminal command (it takes a few minutes):");
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(&command).monospace()).wrap());
                    if ui.small_button("Copy").on_hover_text("Copy the command").clicked() {
                        ui.ctx().copy_text(command.clone());
                    }
                });
                ui.label(
                    RichText::new(
                        "The script adds what it needs (the wasm32 target and wasm-bindgen) the first time. \
                         Then choose Publish as web app… again.",
                    )
                    .weak()
                    .small(),
                );
                if let Some(run) = &build.running {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        let last = run.output.last().map_or("Starting…", String::as_str);
                        ui.add(egui::Label::new(RichText::new(last).monospace().small()).truncate());
                    });
                }
                if let Some(failed) = &build.failed {
                    ui.add_space(6.0);
                    ui.colored_label(theme::ERROR, "The build failed:");
                    egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(failed).monospace().small()).wrap());
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if build.repository.is_some() && build.running.is_none() {
                        let label = if build.failed.is_some() { "Try again" } else { "Build it now" };
                        if ui.button(label).on_hover_text("Run the build here, then publish").clicked() {
                            start = true;
                        }
                    }
                    let cancel = if build.running.is_some() { "Stop waiting" } else { "Close" };
                    if ui.button(cancel).clicked() {
                        close = true;
                    }
                });
            });
        if start {
            if let Some(root) = build.repository.clone() {
                match start_build(&root) {
                    Ok(lines) => {
                        build.failed = None;
                        build.running = Some(Running { lines, output: Vec::new() });
                    }
                    Err(e) => build.failed = Some(e),
                }
            }
        }
        if close {
            self.viewer_build = None;
        }
    }
}
