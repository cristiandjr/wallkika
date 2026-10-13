use crate::{
    display::{self, DisplayInfo},
    engine::{Engine, Target},
    error::{Error, Result},
    layout::{Layout, LayoutMode, Screensaver, Wallpaper},
    media, screensaver, tray,
    updates::{self, UpdateInfo, Updates},
    PANEL_LABEL,
};
use percent_encoding::percent_decode_str;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{
    ipc::{InvokeBody, Request},
    AppHandle, Manager, WebviewWindow,
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    layout: Layout,
    displays: Vec<DisplayInfo>,
    platform: &'static str,
    version: String,
    update_feed: Option<String>,
    update: Option<UpdateInfo>,
    screensaver: screensaver::Info,
}

#[tauri::command]
pub async fn get_overview(app: AppHandle) -> Result<Overview> {
    background(move || {
        Ok(Overview {
            layout: app.state::<Engine>().layout(),
            displays: display::list(&app)?,
            platform: std::env::consts::OS,
            version: app.package_info().version.to_string(),
            update_feed: updates::feed_url(),
            update: app.state::<Updates>().get(),
            screensaver: screensaver::info(&app),
        })
    })
    .await
}

#[tauri::command]
pub async fn choose_wallpaper(
    app: AppHandle,
    display: Option<String>,
) -> Result<Option<Wallpaper>> {
    background(move || match pick_file(&app) {
        Some(path) => app
            .state::<Engine>()
            .apply(&app, &path, &Target::from(display))
            .map(Some),
        None => Ok(None),
    })
    .await
}

#[tauri::command]
pub async fn set_wallpaper(
    app: AppHandle,
    path: PathBuf,
    display: Option<String>,
) -> Result<Wallpaper> {
    background(move || {
        app.state::<Engine>()
            .apply(&app, &path, &Target::from(display))
    })
    .await
}

#[tauri::command]
pub async fn clear_wallpaper(app: AppHandle, display: Option<String>) -> Result<()> {
    background(move || {
        app.state::<Engine>().clear(&app, &Target::from(display));
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn set_layout_mode(app: AppHandle, mode: LayoutMode) -> Result<()> {
    background(move || app.state::<Engine>().set_mode(&app, mode)).await
}

#[tauri::command]
pub async fn set_screensaver(
    app: AppHandle,
    screensaver: Screensaver,
) -> Result<screensaver::Info> {
    background(move || {
        if screensaver.enabled && screensaver::info(&app).screensaver {
            screensaver::install(&app)?;
        }
        app.state::<Engine>().set_screensaver(&app, screensaver);
        Ok(screensaver::info(&app))
    })
    .await
}

#[tauri::command]
pub async fn get_screensaver_info(app: AppHandle) -> Result<screensaver::Info> {
    background(move || Ok(screensaver::info(&app))).await
}

#[tauri::command]
pub fn lock_screen_request(app: AppHandle) -> Option<Wallpaper> {
    screensaver::lock_screen_request(&app, &app.state::<Engine>().layout())
}

#[tauri::command]
pub async fn set_lock_screen_image(app: AppHandle, request: Request<'_>) -> Result<()> {
    let InvokeBody::Raw(image) = request.body() else {
        return Err(Error::platform(
            "The lock screen picture must be sent as bytes",
        ));
    };
    let source = request
        .headers()
        .get("wallpaper-path")
        .and_then(|value| value.to_str().ok())
        .map(|value| PathBuf::from(percent_decode_str(value).decode_utf8_lossy().as_ref()))
        .ok_or_else(|| Error::platform("Missing the wallpaper of the lock screen picture"))?;
    let image = image.clone();
    background(move || {
        let layout = app.state::<Engine>().layout();
        screensaver::set_lock_screen(&app, &layout, &source, &image)
    })
    .await
}

#[tauri::command]
pub fn open_screensaver_settings(app: AppHandle) -> Result<()> {
    screensaver::open_settings(&app)
}

#[tauri::command]
pub fn get_layout(app: AppHandle, window: WebviewWindow) -> Layout {
    let engine = app.state::<Engine>();
    engine.mark_ready(window.label());
    engine.layout()
}

#[tauri::command]
pub fn report_renderer(
    app: AppHandle,
    window: WebviewWindow,
    status: String,
    detail: Option<String>,
) {
    let display = app
        .try_state::<Engine>()
        .and_then(|engine| engine.display_name_of(window.label()))
        .map(|name| format!(" · {name}"))
        .unwrap_or_default();
    let detail = detail.map(|d| format!(": {d}")).unwrap_or_default();
    log::info!("[{}{display}] {status}{detail}", window.label());
}

#[tauri::command]
pub fn exit_screensaver(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn report_update(app: AppHandle, version: String, url: String) -> Result<()> {
    if !updates::is_release_url(&url) {
        return Err(Error::platform(
            "Ignoring an update outside this repository",
        ));
    }
    log::info!("New version available: {version} ({url})");
    app.state::<Updates>().set(UpdateInfo { version, url });
    tray::refresh(&app)?;
    Ok(())
}

#[tauri::command]
pub fn open_update(app: AppHandle) -> Result<()> {
    let update = app
        .state::<Updates>()
        .get()
        .ok_or_else(|| Error::platform("There is no new version yet"))?;
    app.opener()
        .open_url(update.url, None::<&str>)
        .map_err(|err| Error::platform(err.to_string()))
}

fn pick_file(app: &AppHandle) -> Option<PathBuf> {
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Choose a wallpaper")
        .add_filter("Image, GIF, video or HTML", &media::all_extensions())
        .add_filter("Images", media::IMAGE_EXTENSIONS)
        .add_filter("GIF", media::GIF_EXTENSIONS)
        .add_filter("Videos", media::VIDEO_EXTENSIONS)
        .add_filter("Web pages", media::WEB_EXTENSIONS);
    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        if panel.is_visible().unwrap_or(false) {
            dialog = dialog.set_parent(&panel);
        }
    }
    dialog.blocking_pick_file()?.into_path().ok()
}

pub async fn background<T, F>(f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|err| Error::platform(err.to_string()))?
}
