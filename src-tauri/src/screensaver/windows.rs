use super::config_dir;
use crate::error::{Error, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, Manager, WebviewWindow};
use windows::{
    core::{w, HSTRING, PCWSTR},
    Storage::StorageFile,
    System::UserProfile::LockScreen,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ,
            RRF_RT_REG_SZ,
        },
        UI::WindowsAndMessaging::{
            SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETSCREENSAVEACTIVE,
        },
    },
};

pub const LOCK_SCREEN_SHOWS_DESKTOP: bool = false;

const DESKTOP_KEY: PCWSTR = w!("Control Panel\\Desktop");
const SAVER_VALUE: PCWSTR = w!("SCRNSAVE.EXE");
const PREVIOUS_SAVER_FILE: &str = "previous-screensaver.txt";

pub fn supports_screensaver() -> bool {
    true
}

pub fn selected(app: &AppHandle) -> Option<bool> {
    let ours = saver_path(app).ok()?;
    let current = read_saver().unwrap_or_default();
    Some(current.eq_ignore_ascii_case(&ours.to_string_lossy()))
}

pub fn set_lock_screen(_app: &AppHandle, still: &Path) -> Result<()> {
    let failed = |err: windows::core::Error| Error::platform(err.message());
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(still))
        .and_then(|operation| operation.join())
        .map_err(failed)?;
    LockScreen::SetImageFileAsync(&file)
        .and_then(|operation| operation.join())
        .map_err(failed)
}

pub fn install(app: &AppHandle) -> Result<()> {
    let source = std::env::current_exe()?;
    let target = saver_path(app)?;
    if source == target || fs::read(&source).ok() == fs::read(&target).ok() {
        return Ok(());
    }
    if let Some(folder) = target.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::copy(&source, &target)?;
    log::info!("Screen saver installed at {}", target.display());
    Ok(())
}

pub fn set_active(app: &AppHandle, active: bool) -> Result<()> {
    if active {
        install(app)?;
    }
    let ours = saver_path(app)?.to_string_lossy().into_owned();
    let current = read_saver();
    let selected = current
        .as_deref()
        .is_some_and(|path| path.eq_ignore_ascii_case(&ours));
    let previous = config_dir(app)?.join(PREVIOUS_SAVER_FILE);

    if active && !selected {
        fs::write(&previous, current.unwrap_or_default())?;
        write_saver(Some(&ours))?;
        log::info!("WallKika is now the Windows screen saver");
    } else if !active && selected {
        let restored = fs::read_to_string(&previous).unwrap_or_default();
        let restored = restored.trim();
        write_saver(Some(restored).filter(|path| !path.is_empty()))?;
        let _ = fs::remove_file(&previous);
        log::info!("Restored the previous Windows screen saver");
    }
    Ok(())
}

pub fn open_settings(_app: &AppHandle) -> Result<()> {
    Command::new("control.exe")
        .arg("desk.cpl,,@screensaver")
        .spawn()?;
    Ok(())
}

pub fn embed(_window: &WebviewWindow, _host: u64) -> Result<()> {
    Err(Error::platform("Only X11 screen savers can host WallKika"))
}

fn saver_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(app.path().app_local_data_dir()?.join("WallKika.scr"))
}

fn read_saver() -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            DESKTOP_KEY,
            SAVER_VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

fn write_saver(path: Option<&str>) -> Result<()> {
    let status = unsafe {
        match path {
            Some(path) => {
                let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    DESKTOP_KEY,
                    SAVER_VALUE,
                    REG_SZ.0,
                    Some(wide.as_ptr().cast()),
                    std::mem::size_of_val(wide.as_slice()) as u32,
                )
            }
            None => RegDeleteKeyValueW(HKEY_CURRENT_USER, DESKTOP_KEY, SAVER_VALUE),
        }
    };
    if status != ERROR_SUCCESS && !(path.is_none() && status == ERROR_FILE_NOT_FOUND) {
        return Err(Error::platform(format!(
            "Could not change the Windows screen saver (error {})",
            status.0
        )));
    }
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_SETSCREENSAVEACTIVE,
            1,
            None,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        );
    }
    Ok(())
}
