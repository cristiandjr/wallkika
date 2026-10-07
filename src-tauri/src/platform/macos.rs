use super::on_main_thread;
use crate::{
    display::DisplayInfo,
    error::{Error, Result},
};
use objc2::{runtime::AnyObject, MainThreadMarker};
use objc2_app_kit::{
    NSImageScaling, NSScreen, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
    NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey, NSWorkspaceDesktopImageScalingKey,
};
use objc2_foundation::{NSDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString, NSURL};
use std::path::Path;
use tauri::{AppHandle, WebviewWindow};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowLevelForKey(key: i32) -> i32;
}

const CG_DESKTOP_WINDOW_LEVEL_KEY: i32 = 2;

pub fn prepare_process() {}

pub fn set_static_wallpaper(app: &AppHandle, path: &Path) -> Result<()> {
    let path = path.to_path_buf();
    on_main_thread(app, move || set_on_every_screen(&path))?
}

fn set_on_every_screen(path: &Path) -> Result<()> {
    let mtm = main_thread()?;
    let path = path
        .to_str()
        .ok_or_else(|| Error::platform("The file path is not valid UTF-8"))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));

    let scaling = NSNumber::numberWithUnsignedInteger(NSImageScaling::ScaleProportionallyUpOrDown.0);
    let clipping = NSNumber::numberWithBool(true);
    let (scaling, clipping): (&AnyObject, &AnyObject) = (&scaling, &clipping);
    let options = unsafe {
        NSDictionary::from_slices(
            &[NSWorkspaceDesktopImageScalingKey, NSWorkspaceDesktopImageAllowClippingKey],
            &[scaling, clipping],
        )
    };

    let workspace = NSWorkspace::sharedWorkspace();
    for screen in NSScreen::screens(mtm).iter() {
        unsafe { workspace.setDesktopImageURL_forScreen_options_error(&url, &screen, &options) }
            .map_err(|err| Error::platform(err.localizedDescription().to_string()))?;
    }
    Ok(())
}

pub fn attach_live_window(window: &WebviewWindow, display: &DisplayInfo) -> Result<()> {
    let mtm = main_thread()?;
    let ns_window = window.ns_window()?.cast::<NSWindow>();
    // SAFETY: Tauri returns this window's live NSWindow and we are on the main thread.
    let ns_window = unsafe { ns_window.as_ref() }
        .ok_or_else(|| Error::platform("The window has no NSWindow"))?;

    let desktop_level = unsafe { CGWindowLevelForKey(CG_DESKTOP_WINDOW_LEVEL_KEY) };
    ns_window.setLevel(desktop_level as isize);
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    ns_window.setIgnoresMouseEvents(true);
    ns_window.setHasShadow(false);
    ns_window.setCanHide(false);
    ns_window.setExcludedFromWindowsMenu(true);
    ns_window.setAnimationBehavior(NSWindowAnimationBehavior::None);
    ns_window.setFrame_display(cocoa_frame(mtm, display), true);
    ns_window.orderFrontRegardless();
    Ok(())
}

pub fn after_live_windows_closed() {}

pub fn name_displays(app: &AppHandle, displays: &mut [DisplayInfo]) {
    let rects: Vec<_> = displays.iter().map(DisplayInfo::logical_rect).collect();
    let names = on_main_thread(app, move || screen_names(&rects)).unwrap_or_default();
    for (display, name) in displays.iter_mut().zip(names) {
        if let Some(name) = name {
            display.name = name;
        }
    }
}

fn screen_names(rects: &[(f64, f64, f64, f64)]) -> Vec<Option<String>> {
    let Ok(mtm) = main_thread() else {
        return Vec::new();
    };
    let screens = NSScreen::screens(mtm);
    let primary_height = primary_height(mtm);
    rects
        .iter()
        .map(|&rect| {
            let wanted = to_cocoa(rect, primary_height);
            screens
                .iter()
                .find(|screen| same_rect(screen.frame(), wanted))
                .map(|screen| screen.localizedName().to_string())
        })
        .collect()
}

fn cocoa_frame(mtm: MainThreadMarker, display: &DisplayInfo) -> NSRect {
    let wanted = to_cocoa(display.logical_rect(), primary_height(mtm));
    NSScreen::screens(mtm)
        .iter()
        .map(|screen| screen.frame())
        .find(|frame| same_rect(*frame, wanted))
        .unwrap_or(wanted)
}

fn primary_height(mtm: MainThreadMarker) -> f64 {
    NSScreen::screens(mtm)
        .firstObject()
        .map(|screen| screen.frame().size.height)
        .unwrap_or_default()
}

fn to_cocoa((x, y, width, height): (f64, f64, f64, f64), primary_height: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(x, primary_height - (y + height)),
        NSSize::new(width, height),
    )
}

fn same_rect(a: NSRect, b: NSRect) -> bool {
    let close = |p: f64, q: f64| (p - q).abs() < 2.0;
    close(a.origin.x, b.origin.x)
        && close(a.origin.y, b.origin.y)
        && close(a.size.width, b.size.width)
        && close(a.size.height, b.size.height)
}

fn main_thread() -> Result<MainThreadMarker> {
    MainThreadMarker::new().ok_or_else(|| Error::platform("Must run on the main thread"))
}
