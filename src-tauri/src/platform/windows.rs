use super::{SYSTEM_PAUSE, SYSTEM_RESUME};
use crate::{
    display::DisplayInfo,
    error::{Error, Result},
};
use std::{
    os::windows::ffi::OsStrExt,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, WebviewWindow};
use windows::{
    core::{w, BOOL, HSTRING, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM},
        Graphics::Gdi::MapWindowPoints,
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL,
                COINIT_APARTMENTTHREADED,
            },
            LibraryLoader::GetModuleHandleW,
            Power::{RegisterPowerSettingNotification, POWERBROADCAST_SETTING},
            RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION},
            SystemServices::GUID_CONSOLE_DISPLAY_STATE,
        },
        UI::{
            Shell::{DesktopWallpaper, IDesktopWallpaper, DWPOS_FILL},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, FindWindowExW,
                FindWindowW, GetMessageW, GetWindow, GetWindowLongPtrW, RegisterClassW,
                SendMessageTimeoutW, SetLayeredWindowAttributes, SetParent, SetTimer,
                SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW,
                DEVICE_NOTIFY_WINDOW_HANDLE, GWL_EXSTYLE, GWL_STYLE, GW_CHILD, GW_HWNDLAST,
                HWND_BOTTOM, LWA_ALPHA, MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND,
                PBT_POWERSETTINGCHANGE, SET_WINDOW_POS_FLAGS, SMTO_NORMAL, SPIF_SENDCHANGE,
                SPIF_UPDATEINIFILE, SPI_GETSCREENSAVERRUNNING, SPI_SETDESKWALLPAPER,
                SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOWNOACTIVATE,
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_EX_STYLE, WM_POWERBROADCAST, WM_TIMER,
                WM_WTSSESSION_CHANGE, WNDCLASSW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE,
                WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_OVERLAPPED, WS_POPUP,
                WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
            },
        },
    },
};

// Undocumented Progman message that makes Explorer create the WorkerW layer behind the icons.
const SPAWN_WORKERW: u32 = 0x052C;
const SCREENSAVER_TIMER: usize = 1;
const DISPLAY_STATE_OFF: u32 = 0;
const SCREENSAVER_POLL_MS: u32 = 2000;

static EVENTS_APP: OnceLock<AppHandle> = OnceLock::new();
static DISPLAY_ON: AtomicBool = AtomicBool::new(true);
static SCREENSAVER_RUNNING: AtomicBool = AtomicBool::new(false);

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

pub fn describe_displays(_app: &AppHandle, _displays: &mut [DisplayInfo]) {}

pub fn watch_system_events(app: &AppHandle) {
    if EVENTS_APP.set(app.clone()).is_err() {
        return;
    }
    let spawned = thread::Builder::new()
        .name("wallkika-system-events".into())
        .spawn(|| {
            if let Err(err) = unsafe { listen_for_system_events() } {
                log::warn!("Could not watch lock, sleep and screen saver events: {err}");
            }
        });
    if let Err(err) = spawned {
        log::warn!("Could not start the system events thread: {err}");
    }
}

// A hidden top-level window: power broadcasts never reach message-only windows.
unsafe fn listen_for_system_events() -> Result<()> {
    let failed = |err: windows::core::Error| Error::platform(err.message());
    let instance = GetModuleHandleW(None).map_err(failed)?;
    let class = w!("WallKikaSystemEvents");
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(on_system_event),
        hInstance: instance.into(),
        lpszClassName: class,
        ..Default::default()
    };
    if RegisterClassW(&window_class) == 0 {
        return Err(failed(windows::core::Error::from_thread()));
    }
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        w!("WallKika"),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        None,
        None,
        Some(instance.into()),
        None,
    )
    .map_err(failed)?;
    WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION).map_err(failed)?;
    RegisterPowerSettingNotification(
        HANDLE(hwnd.0),
        &GUID_CONSOLE_DISPLAY_STATE,
        DEVICE_NOTIFY_WINDOW_HANDLE,
    )
    .map_err(failed)?;
    SetTimer(Some(hwnd), SCREENSAVER_TIMER, SCREENSAVER_POLL_MS, None);

    let mut message = MSG::default();
    while GetMessageW(&mut message, None, 0, 0).0 > 0 {
        DispatchMessageW(&message);
    }
    Ok(())
}

unsafe extern "system" fn on_system_event(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_WTSSESSION_CHANGE => match wparam.0 as u32 {
            WTS_SESSION_LOCK => notify(SYSTEM_PAUSE, "Session locked"),
            WTS_SESSION_UNLOCK => notify(SYSTEM_RESUME, "Session unlocked"),
            _ => {}
        },
        WM_POWERBROADCAST => match wparam.0 as u32 {
            PBT_APMSUSPEND => notify(SYSTEM_PAUSE, "Going to sleep"),
            PBT_APMRESUMEAUTOMATIC => notify(SYSTEM_RESUME, "Woke up"),
            PBT_POWERSETTINGCHANGE => display_state_changed(lparam),
            _ => {}
        },
        WM_TIMER if wparam.0 == SCREENSAVER_TIMER => poll_screensaver(),
        _ => {}
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

unsafe fn display_state_changed(lparam: LPARAM) {
    let setting = &*(lparam.0 as *const POWERBROADCAST_SETTING);
    if setting.PowerSetting != GUID_CONSOLE_DISPLAY_STATE || setting.DataLength < 4 {
        return;
    }
    let on = std::ptr::read_unaligned(setting.Data.as_ptr().cast::<u32>()) != DISPLAY_STATE_OFF;
    if DISPLAY_ON.swap(on, Ordering::SeqCst) != on {
        match on {
            true => notify(SYSTEM_RESUME, "Display on"),
            false => notify(SYSTEM_PAUSE, "Display off"),
        }
    }
}

fn poll_screensaver() {
    let mut running = BOOL(0);
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETSCREENSAVERRUNNING,
            0,
            Some((&mut running as *mut BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    let running = read.is_ok() && running.as_bool();
    if SCREENSAVER_RUNNING.swap(running, Ordering::SeqCst) != running {
        match running {
            true => notify(SYSTEM_PAUSE, "Screen saver started"),
            false => notify(SYSTEM_RESUME, "Screen saver stopped"),
        }
    }
}

fn notify(event: &'static str, reason: &str) {
    log::info!("{reason} → {event}");
    if let Some(app) = EVENTS_APP.get() {
        let _ = app.emit(event, ());
    }
}

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
