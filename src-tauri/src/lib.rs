mod commands;
mod display;
mod engine;
mod error;
mod media;
mod platform;
mod protocol;
mod settings;
mod tray;

use engine::Engine;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub const PANEL_LABEL: &str = "main";
pub const WALLPAPER_ERROR: &str = "wallpaper-error";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    platform::prepare_process();

    tauri::Builder::default()
        // Must be the first plugin: a second launch forwards its args here and exits.
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            let args = args.get(1..).unwrap_or_default();
            match file_from_args(args, Path::new(&cwd)) {
                Some(path) => apply_in_background(app.clone(), path),
                None => show_panel(app),
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .manage(protocol::MediaScope::default())
        .register_asynchronous_uri_scheme_protocol(protocol::SCHEME, protocol::handle)
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            app.manage(Engine::new(settings_path));
            create_panel(app.handle())?;
            tray::create(app.handle())?;

            let args: Vec<String> = std::env::args().skip(1).collect();
            let cwd = std::env::current_dir().unwrap_or_default();
            let startup_file = file_from_args(&args, &cwd);
            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                match startup_file {
                    Some(path) => apply_or_report(&handle, &path),
                    None => handle.state::<Engine>().restore(&handle),
                }
                display::spawn_watcher(handle);
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == PANEL_LABEL {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_overview,
            commands::choose_wallpaper,
            commands::set_wallpaper,
            commands::stop_live_wallpaper,
            commands::current_wallpaper,
            commands::report_renderer,
        ])
        .build(tauri::generate_context!())
        .expect("failed to start WallKika")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show_panel(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

fn create_panel(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, PANEL_LABEL, WebviewUrl::App("index.html".into()))
        .title("WallKika")
        .inner_size(460.0, 840.0)
        .min_inner_size(400.0, 560.0)
        .center()
        .additional_browser_args(platform::BROWSER_ARGS)
        .build()?;
    Ok(())
}

pub(crate) fn show_panel(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        let _ = panel.unminimize();
        let _ = panel.show();
        let _ = panel.set_focus();
    }
}

pub(crate) fn report_error(app: &AppHandle, err: &error::Error) {
    log::error!("{err}");
    let _ = app.emit(WALLPAPER_ERROR, err.to_string());
}

fn apply_or_report(app: &AppHandle, path: &Path) {
    if let Err(err) = app.state::<Engine>().apply(app, path) {
        report_error(app, &err);
    }
}

fn apply_in_background(app: AppHandle, path: PathBuf) {
    tauri::async_runtime::spawn_blocking(move || apply_or_report(&app, &path));
}

fn file_from_args(args: &[String], cwd: &Path) -> Option<PathBuf> {
    args.iter()
        .filter(|arg| !arg.starts_with('-'))
        .map(|arg| cwd.join(arg))
        .find(|path| path.is_file())
}
