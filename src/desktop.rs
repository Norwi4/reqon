//! Native Windows/desktop entry point; excluded from the headless core.
use crate::{app, benchmark, ide_bridge, migration, mock, storage, window_frame};

pub fn run_desktop() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--ide-bridge") {
        ide_bridge::serve();
        return Ok(());
    }
    if args.iter().any(|a| a == "--fixture-server") {
        let port = args
            .iter()
            .position(|a| a == "--port")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(9473);
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
                    .await
                    .expect("fixture port");
                mock::serve(listener, std::collections::BTreeMap::new(), true).await;
            });
        return Ok(());
    }
    let explicit = args
        .iter()
        .position(|a| a == "--data-dir")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);
    let exe = std::env::current_exe().unwrap_or_else(|_| "Reqon.exe".into());
    let root = migration::default_root(&exe, explicit.clone());
    let source = args
        .iter()
        .position(|a| a == "--migrate-from")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from)
        .or_else(|| migration::auto_source(&root, explicit.is_some()));
    if let Some(source) = source {
        match migration::migrate(&source, &root) {
            Ok(report) => {
                if let Some(path) = args
                    .iter()
                    .position(|a| a == "--migration-report")
                    .and_then(|i| args.get(i + 1))
                {
                    storage::atomic_write(
                        std::path::Path::new(path),
                        serde_json::to_vec_pretty(&report).unwrap(),
                    )
                    .expect("write upgrade report");
                }
            }
            Err(error) => {
                let message = format!(
                    "The workspace could not be upgraded. Your previous data has not been changed.\n\n{error:#}"
                );
                eprintln!("{message}");
                if !args.iter().any(|a| a == "--migrate-only") {
                    rfd::MessageDialog::new()
                        .set_title("Reqon — workspace upgrade")
                        .set_description(&message)
                        .set_level(rfd::MessageLevel::Error)
                        .show();
                }
                std::process::exit(1);
            }
        }
    }
    if args.iter().any(|a| a == "--migrate-only") {
        return Ok(());
    }
    if args.iter().any(|a| a == "--benchmark") {
        match benchmark::run(&root) {
            Ok(result) => {
                let output = args
                    .iter()
                    .position(|a| a == "--output")
                    .and_then(|i| args.get(i + 1))
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| root.join("benchmark.json"));
                let bytes = serde_json::to_vec_pretty(&result).unwrap();
                if let Err(error) = storage::atomic_write(&output, &bytes) {
                    eprintln!("Benchmark report: {error:#}");
                }
                println!("{}", String::from_utf8_lossy(&bytes));
            }
            Err(error) => eprintln!("Benchmark failed: {error:#}"),
        }
        return Ok(());
    }
    let app_icon = image::load_from_memory(include_bytes!(
        "../assets/brand/circular-v1/png/reqon-256.png"
    ))
    .expect("embedded Reqon app icon must be a valid PNG")
    .into_rgba8();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_icon(eframe::egui::IconData {
                width: app_icon.width(),
                height: app_icon.height(),
                rgba: app_icon.into_raw(),
            })
            .with_inner_size([1510., 944.])
            .with_min_inner_size([940., 640.])
            .with_decorations(false)
            .with_resizable(true)
            .with_title("Reqon"),
        ..Default::default()
    };
    eframe::run_native(
        "Reqon",
        options,
        Box::new(move |cc| {
            // Winit's frameless shadow inserts a one-pixel non-client strip on Windows.
            // Keep the client origin aligned with the reference window instead.
            #[cfg(windows)]
            if let Some(window) = cc.winit_window() {
                use winit::platform::windows::{
                    CornerPreference, IconExtWindows, WindowExtWindows,
                };
                // Winit sets ICON_SMALL from the viewport. Windows uses ICON_BIG
                // independently for the taskbar and Alt+Tab.
                let taskbar_icon = image::load_from_memory(include_bytes!(
                    "../assets/brand/circular-v1/png/reqon-256.png"
                ))
                .expect("embedded Reqon taskbar icon must be a valid PNG")
                .into_rgba8();
                let fallback_icon = winit::window::Icon::from_rgba(
                    taskbar_icon.as_raw().clone(),
                    taskbar_icon.width(),
                    taskbar_icon.height(),
                )
                .expect("embedded Reqon taskbar icon must contain RGBA pixels");
                let scaled = |logical: f64| {
                    let pixels = (logical * window.scale_factor()).round() as u32;
                    Some(winit::dpi::PhysicalSize::new(pixels, pixels))
                };
                // The packaged EXE contains individually drawn ICO sizes. Direct
                // cargo runs use the embedded PNG without needing loose files.
                window.set_window_icon(Some(
                    winit::window::Icon::from_resource(1, scaled(16.))
                        .unwrap_or_else(|_| fallback_icon.clone()),
                ));
                window.set_taskbar_icon(Some(
                    winit::window::Icon::from_resource(1, scaled(32.)).unwrap_or(fallback_icon),
                ));
                window.set_undecorated_shadow(false);
                // Ask DWM to round the actual native window. Windows removes the
                // rounding automatically while maximized or snapped.
                window.set_corner_preference(CornerPreference::Round);
                if let Err(error) = window_frame::windows::install(window) {
                    eprintln!("Native window resize: {error:#}");
                }
            }
            Ok(Box::new(app::App::new(cc, root)?))
        }),
    )
}
