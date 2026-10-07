mod commands;
mod display;
mod engine;
mod error;
mod layout;
mod media;
mod platform;
mod protocol;
mod settings;
mod tray;
mod updates;

use engine::{Engine, Target};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub const PANEL_LABEL: &str = "main";
pub const WALLPAPER_ERROR: &str = "wallpaper-error";
const SHOW_ABOUT: &str = "show-about";

#[derive(Debug, Default, PartialEq)]
struct Launch {
    file: Option<PathBuf>,
    display: Option<usize>,
    background: bool,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    platform::prepare_process();

    let builder = tauri::Builder::default()
        // Must be the first plugin: a second launch forwards its args here and exits.
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            let args = args.get(1..).unwrap_or_default();
            handle_second_launch(app, parse_launch(args, Path::new(&cwd)));
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(protocol::MediaScope::default())
        .manage(updates::Updates::default())
        .register_asynchronous_uri_scheme_protocol(protocol::SCHEME, protocol::handle)
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            app.manage(Engine::new(settings_path));
            create_panel(app.handle())?;
            tray::create(app.handle())?;

            let args: Vec<String> = std::env::args().skip(1).collect();
            let launch = parse_launch(&args, &std::env::current_dir().unwrap_or_default());
            if launch.background {
                hide_panel(app.handle());
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                handle.state::<Engine>().restore(&handle);
                if let Some(file) = launch.file {
                    apply_or_report(&handle, &file, launch.display);
                }
                display::spawn_watcher(handle);
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == PANEL_LABEL {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    hide_panel(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_overview,
            commands::choose_wallpaper,
            commands::set_wallpaper,
            commands::clear_wallpaper,
            commands::set_layout_mode,
            commands::get_layout,
            commands::report_renderer,
            commands::report_update,
            commands::open_update,
        ]);

    #[cfg(target_os = "macos")]
    let builder = builder.menu(app_menu).on_menu_event(|app, event| {
        if event.id().as_ref() == "app-about" {
            show_about(app);
        }
    });

    builder
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
        .inner_size(460.0, 900.0)
        .min_inner_size(400.0, 560.0)
        .center()
        .additional_browser_args(platform::BROWSER_ARGS)
        .build()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn app_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    let about = MenuItem::with_id(app, "app-about", "About WallKika", true, None::<&str>)?;
    let wallkika = Submenu::with_items(
        app,
        "WallKika",
        true,
        &[
            &about,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    Menu::with_items(app, &[&wallkika, &edit, &window])
}

pub(crate) fn show_panel(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        let _ = panel.unminimize();
        let _ = panel.show();
        let _ = panel.set_focus();
    }
}

// Hidden windows leave Alt+Tab and the taskbar on Windows/Linux; macOS also needs the accessory policy.
pub(crate) fn hide_panel(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        let _ = panel.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

pub(crate) fn show_about(app: &AppHandle) {
    show_panel(app);
    let _ = app.emit(SHOW_ABOUT, ());
}

pub(crate) fn report_error(app: &AppHandle, err: &error::Error) {
    log::error!("{err}");
    let _ = app.emit(WALLPAPER_ERROR, err.to_string());
}

fn handle_second_launch(app: &AppHandle, launch: Launch) {
    match launch.file {
        Some(file) => {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                apply_or_report(&app, &file, launch.display)
            });
        }
        None if launch.background => hide_panel(app),
        None => show_panel(app),
    }
}

fn apply_or_report(app: &AppHandle, file: &Path, display: Option<usize>) {
    let result = resolve_target(app, display)
        .and_then(|target| app.state::<Engine>().apply(app, file, &target));
    if let Err(err) = result {
        report_error(app, &err);
    }
}

fn resolve_target(app: &AppHandle, display: Option<usize>) -> error::Result<Target> {
    let Some(number) = display else {
        return Ok(Target::All);
    };
    let displays = display::list(app)?;
    number
        .checked_sub(1)
        .and_then(|index| displays.get(index))
        .map(|found| Target::Display(found.id.clone()))
        .ok_or_else(|| {
            error::Error::platform(format!(
                "There is no display {number}; WallKika sees {}",
                displays.len()
            ))
        })
}

fn parse_launch(args: &[String], cwd: &Path) -> Launch {
    let mut launch = Launch::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--background" => launch.background = true,
            "--display" => launch.display = args.next().and_then(|n| n.parse().ok()),
            _ if arg.starts_with('-') || launch.file.is_some() => {}
            _ => {
                let path = cwd.join(arg);
                if path.is_file() {
                    launch.file = Some(path);
                }
            }
        }
    }
    launch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn parses_file_display_and_background() {
        let dir = std::env::temp_dir().join(format!("wallkika-{}-launch", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("clip.mp4"), b"x").unwrap();

        let launch = parse_launch(&args(&["--display", "2", "clip.mp4", "--background"]), &dir);
        assert_eq!(
            launch,
            Launch {
                file: Some(dir.join("clip.mp4")),
                display: Some(2),
                background: true,
            }
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ignores_missing_files_and_unknown_flags() {
        let launch = parse_launch(&args(&["-psn_0_123", "missing.mp4"]), Path::new("/"));
        assert_eq!(launch, Launch::default());
    }
}
