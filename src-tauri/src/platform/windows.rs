use crate::{
    display::DisplayInfo,
    error::{Error, Result},
};
use std::{os::windows::ffi::OsStrExt, path::Path, thread, time::Duration};
use tauri::{AppHandle, WebviewWindow};
use windows::{
    core::{w, BOOL, HSTRING, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, POINT, WPARAM},
        Graphics::Gdi::MapWindowPoints,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
        },
        UI::{
            Shell::{DesktopWallpaper, IDesktopWallpaper, DWPOS_FILL},
            WindowsAndMessaging::{
                EnumWindows, FindWindowExW, FindWindowW, GetWindow, GetWindowLongPtrW,
                SendMessageTimeoutW, SetLayeredWindowAttributes, SetParent, SetWindowLongPtrW,
                SetWindowPos, ShowWindow, SystemParametersInfoW, GWL_EXSTYLE, GWL_STYLE, GW_CHILD,
                GW_HWNDLAST, HWND_BOTTOM, LWA_ALPHA, SET_WINDOW_POS_FLAGS, SMTO_NORMAL,
                SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER, SWP_NOACTIVATE,
                SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOWNOACTIVATE, WS_CHILD, WS_EX_LAYERED,
                WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
};

// Undocumented Progman message that makes Explorer create the WorkerW layer behind the icons.
const SPAWN_WORKERW: u32 = 0x052C;

struct DesktopHost {
    parent: HWND,
    icons: Option<HWND>,
    workerw: Option<HWND>,
    raised: bool,
}

pub fn prepare_process() {}

pub fn set_static_wallpaper(_app: &AppHandle, path: &Path) -> Result<()> {
    let path = path.to_path_buf();
    thread::spawn(move || {
        set_with_desktop_wallpaper(&path).or_else(|err| {
            log::warn!("IDesktopWallpaper failed ({err}); using SystemParametersInfo");
            set_with_system_parameters(&path)
        })
    })
    .join()
    .map_err(|_| Error::platform("The wallpaper thread panicked"))?
}

fn set_with_desktop_wallpaper(path: &Path) -> Result<()> {
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| {
            let wallpaper: IDesktopWallpaper =
                CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL)?;
            wallpaper.SetPosition(DWPOS_FILL)?;
            wallpaper.SetWallpaper(PCWSTR::null(), &HSTRING::from(path))
        })();
        if initialized {
            CoUninitialize();
        }
        result.map_err(|err| Error::platform(err.message()))
    }
}

fn set_with_system_parameters(path: &Path) -> Result<()> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(wide.as_mut_ptr().cast()),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    }
    .map_err(|err| Error::platform(err.message()))
}

pub fn attach_live_window(window: &WebviewWindow, display: &DisplayInfo) -> Result<()> {
    let hwnd = window.hwnd()?;
    let host = desktop_host()?;
    let (width, height) = (display.width as i32, display.height as i32);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_STYLE,
            (style & !(WS_POPUP.0 as isize)) | WS_CHILD.0 as isize,
        );
        let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE)
            | (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) as isize;
        if host.raised {
            ex_style |= WS_EX_LAYERED.0 as isize;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style);
        if host.raised {
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)
                .map_err(|err| Error::platform(err.message()))?;
        }

        SetParent(hwnd, Some(host.parent)).map_err(|err| Error::platform(err.message()))?;

        let mut origin = [POINT {
            x: display.x,
            y: display.y,
        }];
        MapWindowPoints(None, Some(host.parent), &mut origin);
        let (insert_after, zorder) = match (host.raised, host.icons) {
            (true, Some(icons)) => (Some(icons), SET_WINDOW_POS_FLAGS(0)),
            _ => (None, SWP_NOZORDER),
        };
        SetWindowPos(
            hwnd,
            insert_after,
            origin[0].x,
            origin[0].y,
            width,
            height,
            SWP_NOACTIVATE | zorder,
        )
        .map_err(|err| Error::platform(err.message()))?;

        if let (true, Some(workerw)) = (host.raised, host.workerw) {
            keep_last_child(host.parent, workerw);
        }
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    Ok(())
}

pub fn name_displays(_app: &AppHandle, _displays: &mut [DisplayInfo]) {}

pub fn after_live_windows_closed() {
    unsafe {
        let raised = FindWindowW(w!("Progman"), PCWSTR::null())
            .map(is_raised_desktop)
            .unwrap_or(false);
        // Repaints the classic desktop; on the raised desktop it would destroy the WorkerW.
        if !raised {
            let _ = SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, None, SPIF_UPDATEINIFILE);
        }
    }
}

fn desktop_host() -> Result<DesktopHost> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null())
            .map_err(|_| Error::platform("Progman window not found (is Explorer running?)"))?;
        SendMessageTimeoutW(
            progman,
            SPAWN_WORKERW,
            WPARAM(0xD),
            LPARAM(1),
            SMTO_NORMAL,
            1000,
            None,
        );

        if is_raised_desktop(progman) {
            let icons =
                FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null()).ok();
            let workerw =
                retry(|| FindWindowExW(Some(progman), None, w!("WorkerW"), PCWSTR::null()).ok());
            return Ok(DesktopHost {
                parent: progman,
                icons,
                workerw,
                raised: true,
            });
        }

        let workerw = retry(classic_workerw)
            .ok_or_else(|| Error::platform("Could not find the desktop WorkerW window"))?;
        Ok(DesktopHost {
            parent: workerw,
            icons: None,
            workerw: Some(workerw),
            raised: false,
        })
    }
}

fn is_raised_desktop(progman: HWND) -> bool {
    let ex_style = unsafe { GetWindowLongPtrW(progman, GWL_EXSTYLE) };
    ex_style & WS_EX_NOREDIRECTIONBITMAP.0 as isize != 0
}

fn retry(find: impl Fn() -> Option<HWND>) -> Option<HWND> {
    find().or_else(|| {
        thread::sleep(Duration::from_millis(500));
        find()
    })
}

fn classic_workerw() -> Option<HWND> {
    let mut found: Option<HWND> = None;
    unsafe {
        let _ = EnumWindows(
            Some(find_workerw),
            LPARAM(&mut found as *mut Option<HWND> as isize),
        );
    }
    found
}

unsafe extern "system" fn find_workerw(top: HWND, found: LPARAM) -> BOOL {
    let has_icons = FindWindowExW(Some(top), None, w!("SHELLDLL_DefView"), PCWSTR::null()).is_ok();
    if has_icons {
        if let Ok(workerw) = FindWindowExW(None, Some(top), w!("WorkerW"), PCWSTR::null()) {
            *(found.0 as *mut Option<HWND>) = Some(workerw);
            return BOOL(0);
        }
    }
    BOOL(1)
}

unsafe fn keep_last_child(parent: HWND, workerw: HWND) {
    let last = GetWindow(parent, GW_CHILD).and_then(|first| GetWindow(first, GW_HWNDLAST));
    if last.ok() != Some(workerw) {
        let _ = SetWindowPos(
            workerw,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}
