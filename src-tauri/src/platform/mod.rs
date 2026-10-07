use crate::error::{Error, Result};
use tauri::AppHandle;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use self::linux as imp;
#[cfg(target_os = "macos")]
use self::macos as imp;
#[cfg(windows)]
use self::windows as imp;

pub use imp::{
    after_live_windows_closed, attach_live_window, describe_displays, prepare_process,
    set_static_wallpaper,
};

// WebView2 needs identical flags on every window; these keep Chromium rendering behind the desktop icons.
pub const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows --disable-renderer-backgrounding";

pub fn on_main_thread<T, F>(app: &AppHandle, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })?;
    rx.recv()
        .map_err(|_| Error::platform("The main thread did not respond"))
}
