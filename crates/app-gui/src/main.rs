//! AnovaCraft desktop application.
//!
//! Run with `cargo run -p app-gui` to open the built-in sample dataset, or
//! `cargo run -p app-gui -- file1.csv file2.acraft …` to open files, each in
//! its own tab. Data files (CSV, TSV, delimited text, JSON, Excel and
//! OpenDocument spreadsheets) and project files can also be opened from the
//! File menu or dropped onto the window.
//!
//! All application logic (analyses, report content, brushing) lives in the
//! `app-model` crate; this crate only draws it and forwards user input.
// Release builds on Windows run as a windowed program, without a console window.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod agreement_dialog;
mod anom_dialog;
mod app;
mod attribute_dialog;
mod augment_dialog;
mod axis_drop;
mod batch_window;
mod bayes_dialog;
mod boost_dialog;
mod capability_dialog;
mod chart_dialog;
mod cluster_map_dialog;
mod code_editor;
mod compare_ui;
mod comparison_dialog;
mod competing_risks_dialog;
mod contour_view;
mod curve_template_dialog;
mod degradation_dialog;
mod describe_dialogs;
mod design_dialog;
mod diagram_dialog;
mod diagram_view;
mod dialog_kit;
mod discriminant_dialog;
mod evaluation_dialog;
mod external_dialog;
mod factor_dialog;
mod factorial_dialog;
mod figure_page_window;
mod file_io;
mod filter_panel;
mod forecast_dialog;
mod forest_dialog;
mod formula_dialog;
mod functional_dialog;
mod gauge_dialog;
mod heat_map_view;
mod icons;
mod image_dialog;
mod imputation_dialog;
mod life_distribution_dialog;
mod life_stress_dialog;
mod fatigue_dialog;
mod pcoa_dialog;
mod pk_dialog;
#[cfg(target_os = "macos")]
mod macos_open;
mod mars_dialog;
mod menus;
mod mixed_dialog;
mod mixture_design_dialog;
mod mixture_dialog;
mod model_dialog;
mod monitoring_dialog;
mod model_terms_ui;
mod multivariate_dialog;
mod multivariate_methods_dialog;
mod gaussian_process_dialog;
mod neural_dialog;
mod nonlinear_dialog;
mod notebook_window;
mod open_data_dialog;
mod phase2_dialogs;
mod plot3d_dialog;
mod plot_axes;
#[cfg(not(target_arch = "wasm32"))]
mod plot_clipboard;
mod plot_export;
mod plot_style_menu;
mod plot_size;
mod pls_dialog;
mod quality_tools_dialog;
mod reference_lines_ui;
mod regression_dialogs;
mod reliability_dialog;
mod report_export_ui;
mod report_scope_ui;
mod report_view;
mod resampling_dialogs;
mod response_explorer;
mod scatter3d_view;
mod script_edit;
mod script_library;
mod script_window;
mod search_dialog;
mod sequential_dialog;
mod series_explore_dialog;
mod several_responses_view;
mod simulate_dialog;
mod signal_dialog;
mod signal_processing_dialog;
mod image_analysis_dialog;
mod study_dialogs;
mod surface_profiler;
mod surface_view;
mod survival_dialog;
mod survival_model_dialog;
mod svm_dialog;
mod table_ops_dialog;
mod table_view;
mod tabulation_dialog;
mod text_dialog;
mod theme;
mod time_weighted_dialog;
mod treatment_dialog;
mod tree_dialog;
mod tree_view;
mod warranty_dialog;
#[cfg(target_arch = "wasm32")]
mod web_main;
#[cfg(target_arch = "wasm32")]
mod web_shims;
mod word_cloud_view;

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
use eframe::egui;

/// In a browser the app runs as the read-only dashboard viewer, started by
/// `web_main` once the page has loaded.
#[cfg(target_arch = "wasm32")]
fn main() {
    web_main::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    // Every command-line argument is a file to open, each in its own tab;
    // `--viewer project.acraft` shows that project's dashboards as the web
    // viewer does instead.
    let mut paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    // `--publish-sample folder`: publish the Greenhouse sample's dashboard
    // there and exit (scripts/build-web-viewer.sh --serve).
    if paths.first().is_some_and(|p| p.as_os_str() == "--publish-sample") {
        let Some(folder) = paths.get(1) else {
            eprintln!("--publish-sample needs a folder");
            std::process::exit(2);
        };
        match app::publish_sample(folder) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
    let viewer = if paths.first().is_some_and(|p| p.as_os_str() == "--viewer") {
        let Some(path) = paths.get(1).cloned() else {
            eprintln!("--viewer needs a project file");
            std::process::exit(2);
        };
        let file = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| app_model::project::load_project_file(&text).map_err(|e| e.to_string()));
        match file {
            Ok(file) => {
                let title = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                paths.clear();
                Some((file, title))
            }
            Err(e) => {
                eprintln!("could not read {}: {e}", path.display());
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("AnovaCraft")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([900.0, 560.0])
        .with_drag_and_drop(true);
    // Window and taskbar icon (the macOS app bundle also carries it as .icns).
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    #[allow(unused_mut)] // changed only on macOS
    let mut options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: wgpu_options(),
        ..Default::default()
    };
    // The system's default menu bar has a Quit item (⌘Q) that ends the program
    // without a close request, so unsaved changes could be lost without a
    // question. Without it, ⌘Q reaches the app, which asks first.
    #[cfg(target_os = "macos")]
    {
        options.event_loop_builder = Some(Box::new(|builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            builder.with_default_menu(false);
        }));
    }
    // A panic leaves a crash report, and the next launch offers to restore
    // the work saved for recovery.
    app::crash::install_panic_hook();
    // Files opened from Finder arrive as Apple Events; listen before launch.
    #[cfg(target_os = "macos")]
    macos_open::install();
    eframe::run_native(
        "AnovaCraft",
        options,
        Box::new(move |cc| {
            Ok(Box::new(match viewer {
                Some((file, title)) => app::StatsApp::new_viewer(cc, file, title),
                None => app::StatsApp::new(cc, paths),
            }))
        }),
    )
}

/// The GPU device asks for the largest buffers the adapter allows, not
/// wgpu's portable default of 256 MB: a frame whose geometry needs a bigger
/// buffer is a fatal error, and Apple GPUs allow several gigabytes. (Plots
/// with very many points are drawn as pictures and stay far below either.)
#[cfg(not(target_arch = "wasm32"))]
fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
    let mut setup = WgpuSetupCreateNew::default();
    let base = std::sync::Arc::clone(&setup.device_descriptor);
    setup.device_descriptor = std::sync::Arc::new(move |adapter| {
        let mut descriptor = base(adapter);
        let allowed = adapter.limits();
        descriptor.required_limits.max_buffer_size =
            descriptor.required_limits.max_buffer_size.max(allowed.max_buffer_size);
        descriptor
    });
    WgpuConfiguration { wgpu_setup: WgpuSetup::CreateNew(setup), ..Default::default() }
}
