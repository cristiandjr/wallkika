use crate::{
    display::{self, DisplayInfo},
    engine::{Engine, Wallpaper},
    error::{Error, Result},
    media, PANEL_LABEL,
};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    current: Option<Wallpaper>,
    displays: Vec<DisplayInfo>,
    platform: &'static str,
}

#[tauri::command]
pub async fn get_overview(app: AppHandle) -> Result<Overview> {
    background(move || {
        Ok(Overview {
            current: app.state::<Engine>().current(),
            displays: display::list(&app)?,
            platform: std::env::consts::OS,
        })
    })
    .await
}

#[tauri::command]
pub async fn choose_wallpaper(app: AppHandle) -> Result<Option<Wallpaper>> {
    background(move || match pick_file(&app) {
        Some(path) => app.state::<Engine>().apply(&app, &path).map(Some),
        None => Ok(None),
    })
    .await
}

#[tauri::command]
pub async fn set_wallpaper(app: AppHandle, path: PathBuf) -> Result<Wallpaper> {
    background(move || app.state::<Engine>().apply(&app, &path)).await
}

#[tauri::command]
pub async fn stop_live_wallpaper(app: AppHandle) -> Result<()> {
    background(move || {
        app.state::<Engine>().stop_live(&app);
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn current_wallpaper(app: AppHandle) -> Option<Wallpaper> {
    app.state::<Engine>().current()
}

#[tauri::command]
pub fn report_renderer(window: WebviewWindow, status: String, detail: Option<String>) {
    let detail = detail.map(|d| format!(": {d}")).unwrap_or_default();
    log::info!("[{}] {status}{detail}", window.label());
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
