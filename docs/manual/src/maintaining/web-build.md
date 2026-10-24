# Web build

AnovaCraft runs in a web browser through WebAssembly as the **read-only
dashboard viewer** that [Publish as web app…](../reports/dashboards.md#publishing-as-a-web-app)
puts in its folder. This page explains how that build is made and checked,
and what differs from the desktop app at run time.

## Building the viewer

```sh
./scripts/build-web-viewer.sh           # writes target/web-viewer
./scripts/build-web-viewer.sh --serve   # also publishes the Greenhouse sample
                                        # and serves it on http://localhost:8000
```

The script needs the `wasm32-unknown-unknown` target (it adds it with rustup
if missing) and `wasm-bindgen-cli` at exactly the version of the
`wasm-bindgen` crate in `Cargo.lock` (it installs that with `cargo install`
when it is missing). It builds `app-gui` in release mode for that target,
runs `wasm-bindgen --target web`, and shrinks the result with `wasm-opt`
when binaryen is installed. The two files it writes,
`anovacraft_viewer.js` and `anovacraft_viewer_bg.wasm`, are what Publish as
web app… copies. The app looks for them in, in order:

1. the folder named by the `ANOVACRAFT_WEB_VIEWER` environment variable;
2. a `web-viewer` folder beside the program, one level up (so a Cargo build
   in `target/release` finds `target/web-viewer`), or in a macOS app
   bundle's `Resources`.

Without them it writes nothing and opens the "Build the web viewer" window
(`app/viewer_build.rs`), which shows this command and, in a source
checkout, runs it in the background and then carries on with the publish.

`app-gui --viewer project.acraft` shows a project the way the viewer does,
in a desktop window; `app-gui --publish-sample folder` writes the published
Greenhouse sample to a folder (what `--serve` serves).

## Checking it

```sh
./scripts/check-web-build.sh
```

runs `cargo check` for `wasm32-unknown-unknown` on the engine crates,
`app-model` and `stat-script` without their default features, `stat-viz`
with its `egui` feature, and `app-gui`. It is not part of CI; run it after
changing dependencies or adding platform-specific code. The viewer's logic
is tested natively (`gui_tests/viewer.rs` drives it with `egui_kittest`);
the only browser-only check is loading a published folder in a browser.

## How the app builds for the web

| Crate | For `wasm32-unknown-unknown` |
|---|---|
| All engine crates (`stat-core` … `stat-timeseries`, `stat-strd`) | Compile unchanged. |
| `stat-viz`, with and without its `egui` feature | Compiles unchanged. |
| `app-model` | Without its default features: no database drivers or web access (its `sqlite`, `postgres`, `mysql`, `web` and `odbc` features). |
| `stat-script` | Without its `external-data` feature (which passes app-model's default features on). |
| `app-gui` | Compiles; on this target it starts as the viewer. |

What `app-gui` does differently on this target:

- **Dependencies:** the database drivers and web import, the native file
  dialogs (`rfd`) and the clipboard (`arboard`) are native-only
  dependencies. `web_shims` stands in for the dialogs (always cancelled)
  and the clipboard (always empty); the viewer offers no command that uses
  them.
- **Entry point:** `main` calls `web_main::start`, which fetches the
  project file next to the page (`project.acraft`, or the one a
  `?project=` query parameter names), reads it with `load_project_file`
  and starts `eframe::WebRunner` on the page's `anovacraft_canvas`,
  drawing with WebGL. Errors and panics are shown in the page's
  `anovacraft_status` line.
- **Viewer mode:** `StatsApp::new_viewer` opens the project and shows its
  dashboards full window (`app/viewer.rs`): no menus, sidebar, tabs or
  windows of their own. Its dashboards are read-only (`OpenDashboard::read_only`):
  the toolbar keeps the name and Undo/Redo; tile strips have no move
  handle or buttons; there is no resize grip; and only filter, picker and
  tile display edits are applied (`DashboardOutcome::keep_view_changes`).
- **Settings:** the viewer uses the default settings and saves none.

### At run time

- **Threads:** the engine's `rayon` loops (random forests, bootstrap and
  permutation loops) run on one thread, which rayon does by itself when
  threads are unavailable. Results are the same (each tree and replicate
  has its own seed); they take longer. The script window, batch runs and
  external data, which start threads, are not part of the viewer.
- **Clock:** `std::time::SystemTime::now` and `Instant::now` panic on this
  target. Nothing the viewer draws calls them (the engine uses them only in
  tests); `html_export::export_date` must not be called there.
- **Files:** `std::fs` compiles but every call fails; the project comes from
  the page.
- **Graphics:** the web build uses eframe's WebGL (glow) renderer, which
  every current browser has; the desktop app keeps wgpu.

### Size

The build uses the `wasm-release` profile (optimized for size, with
link-time optimization, stripped): about 20 MB of WebAssembly before
`wasm-opt`, 7 MB gzipped, which web servers usually apply on their own. It links every
analysis, since `run_report` dispatches to all of them, plus egui and the
bundled fonts. Leaving out analyses a dashboard does not use would need
`run_report` split by feature; that is not worth doing for now.
