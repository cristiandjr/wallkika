use crate::{
    commands, display,
    engine::{allow_media_access, file_name},
    error::{Error, Result},
    layout::{Layout, RenderMode, Wallpaper},
    media::{self, MediaKind},
    platform,
    protocol::{self, MediaScope},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io,
    path::{Path, PathBuf},
};
use tauri::{
    webview::Color, AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindowBuilder, Wry,
};
use tauri_plugin_log::{Target, TargetKind};

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

pub use imp::{install, open_settings};

const LABEL_PREFIX: &str = "screensaver-";
const LOCK_SCREEN_SOURCE: &str = "lock-screen-source.txt";
const LOCK_SCREEN_PREFIX: &str = "lock-screen-";
const MAX_STILL_BYTES: usize = 40 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub screensaver: bool,
    pub selected: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Show,
    Embed(u64),
    Preview,
}

impl Mode {
    pub fn from_args(args: &[String]) -> Option<Self> {
        let first = args.first()?.to_ascii_lowercase();
        if first == "--screensaver" {
            let host = std::env::var("XSCREENSAVER_WINDOW")
                .ok()
                .and_then(|id| parse_window_id(&id));
            return Some(host.map_or(Self::Show, Self::Embed));
        }
        // Windows runs screen savers with /s, previews them with /p <hwnd> and configures them with /c.
        match first.get(..2) {
            Some("/s") if cfg!(windows) => Some(Self::Show),
            Some("/p") if cfg!(windows) => Some(Self::Preview),
            _ => None,
        }
    }
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaverConfig {
    path: Option<PathBuf>,
    kind: Option<MediaKind>,
}

pub fn info(app: &AppHandle) -> Info {
    let screensaver = imp::supports_screensaver();
    Info {
        screensaver,
        selected: screensaver.then(|| imp::selected(app)).flatten(),
    }
}

pub fn sync(app: &AppHandle, layout: &Layout) {
    let wallpaper = layout.screensaver_wallpaper();
    let result = config_dir(app).and_then(|dir| {
        write_config(&dir, wallpaper)?;
        if wallpaper.is_none() {
            let _ = fs::remove_file(dir.join(LOCK_SCREEN_SOURCE));
        }
        imp::set_active(app, wallpaper.is_some())
    });
    if let Err(err) = result {
        log::warn!("Could not update the screen saver: {err}");
    }
}

pub fn lock_screen_request(app: &AppHandle, layout: &Layout) -> Option<Wallpaper> {
    let wallpaper = layout.screensaver_wallpaper()?;
    let applied = fs::read_to_string(config_dir(app).ok()?.join(LOCK_SCREEN_SOURCE)).ok();
    needs_still(wallpaper, applied.as_deref()).then(|| wallpaper.clone())
}

// Displays that appear later get the system wallpaper of the moment, so the still is applied again.
pub fn reapply_lock_screen(app: &AppHandle, layout: &Layout) {
    if !imp::LOCK_SCREEN_SHOWS_DESKTOP {
        return;
    }
    let Some(wallpaper) = layout.screensaver_wallpaper() else {
        return;
    };
    let Ok(dir) = config_dir(app) else {
        return;
    };
    let applied = fs::read_to_string(dir.join(LOCK_SCREEN_SOURCE)).ok();
    if needs_still(wallpaper, applied.as_deref()) || applied.is_none() {
        return;
    }
    let still = still_path(&dir, wallpaper);
    if still.is_file() {
        if let Err(err) = imp::set_lock_screen(app, &still) {
            log::warn!("Could not apply the lock screen picture again: {err}");
        }
    }
}

fn needs_still(wallpaper: &Wallpaper, applied: Option<&str>) -> bool {
    let already_on_desktop = imp::LOCK_SCREEN_SHOWS_DESKTOP && wallpaper.mode == RenderMode::Native;
    wallpaper.kind != MediaKind::Web
        && !already_on_desktop
        && applied != Some(source_key(wallpaper).as_str())
}

pub fn set_lock_screen(app: &AppHandle, layout: &Layout, source: &Path, jpeg: &[u8]) -> Result<()> {
    let wallpaper = layout
        .screensaver_wallpaper()
        .filter(|wallpaper| wallpaper.path == source)
        .ok_or_else(|| Error::platform("The screen saver wallpaper changed meanwhile"))?;
    if !jpeg.starts_with(&[0xFF, 0xD8]) || jpeg.len() > MAX_STILL_BYTES {
        return Err(Error::platform(
            "The lock screen picture is not a valid JPEG",
        ));
    }
    let key = source_key(wallpaper);
    let dir = config_dir(app)?;
    let still = still_path(&dir, wallpaper);
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        let old_still = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(LOCK_SCREEN_PREFIX) && name.ends_with(".jpg"));
        if old_still && path != still {
            let _ = fs::remove_file(path);
        }
    }
    fs::write(&still, jpeg)?;
    imp::set_lock_screen(app, &still)?;
    fs::write(dir.join(LOCK_SCREEN_SOURCE), key)?;
    log::info!("Lock screen shows a still of {}", wallpaper.name);
    Ok(())
}

fn still_path(dir: &Path, wallpaper: &Wallpaper) -> PathBuf {
    dir.join(format!(
        "{LOCK_SCREEN_PREFIX}{:016x}.jpg",
        hash(&source_key(wallpaper))
    ))
}

fn source_key(wallpaper: &Wallpaper) -> String {
    format!("{:?}:{}", wallpaper.kind, wallpaper.path.display())
}

fn hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

pub fn run(mode: Mode, context: tauri::Context<Wry>) {
    if mode == Mode::Preview {
        return;
    }
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .clear_targets()
                .target(Target::new(TargetKind::LogDir {
                    file_name: Some("screensaver".into()),
                }))
                .build(),
        )
        .manage(MediaScope::default())
        .register_asynchronous_uri_scheme_protocol(protocol::SCHEME, protocol::handle)
        .setup(move |app| {
            let wallpaper = load(app.handle());
            match &wallpaper {
                Some(wallpaper) => log::info!("Screen saver showing {}", wallpaper.path.display()),
                None => log::info!("Screen saver has nothing to show"),
            }
            open_windows(app.handle(), mode, wallpaper.as_ref())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::report_renderer,
            commands::exit_screensaver,
        ])
        .build(context)
        .expect("failed to start the WallKika screen saver")
        .run(|_, _| {});
}

fn config_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(app.path().app_config_dir()?.join("screensaver"))
}

fn load(app: &AppHandle) -> Option<Wallpaper> {
    let file = config_dir(app).ok()?.join("config.json");
    let config: SaverConfig = serde_json::from_slice(&fs::read(file).ok()?).ok()?;
    let path = media::canonical_path(&config.path?).ok()?;
    let kind = config.kind?;
    allow_media_access(app, &path, kind);
    Some(Wallpaper {
        name: file_name(&path),
        path,
        kind,
        mode: RenderMode::Live,
    })
}

fn open_windows(app: &AppHandle, mode: Mode, wallpaper: Option<&Wallpaper>) -> Result<()> {
    let wallpaper =
        serde_json::to_string(&wallpaper).map_err(|err| Error::platform(err.to_string()))?;
    let script = format!("window.__WALLKIKA_SCREENSAVER__ = {wallpaper};");
    if let Mode::Embed(host) = mode {
        let window = window_builder(app, &format!("{LABEL_PREFIX}1"), &script)
            .visible(false)
            .build()?;
        return imp::embed(&window, host);
    }
    for (index, display) in display::list(app)?.iter().enumerate() {
        let (x, y, width, height) = display.logical_rect();
        let window = window_builder(app, &format!("{LABEL_PREFIX}{}", index + 1), &script)
            .position(x, y)
            .inner_size(width, height)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .build()?;
        // Windows converts logical positions with the primary display's scale, which breaks mixed DPI.
        if cfg!(windows) {
            window.set_position(PhysicalPosition::new(display.x, display.y))?;
            window.set_size(PhysicalSize::new(display.width, display.height))?;
        }
        if index == 0 {
            let _ = window.set_focus();
        }
    }
    Ok(())
}

fn window_builder<'a>(
    app: &'a AppHandle,
    label: &str,
    script: &str,
) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("wallpaper.html".into()))
        .title("WallKika")
        .decorations(false)
        .resizable(false)
        .shadow(false)
        .skip_taskbar(true)
        .background_color(Color(0, 0, 0, 255))
        .disable_drag_drop_handler()
        .additional_browser_args(platform::BROWSER_ARGS)
        .initialization_script(script)
}

fn write_config(dir: &Path, wallpaper: Option<&Wallpaper>) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let config = wallpaper.map_or_else(SaverConfig::default, |wallpaper| SaverConfig {
        path: Some(saver_media(dir, wallpaper)),
        kind: Some(wallpaper.kind),
    });

    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let is_media = path.file_stem().is_some_and(|stem| stem == "media");
        if is_media && config.path.as_deref() != Some(path.as_path()) {
            fs::remove_file(path)?;
        }
    }

    let json = serde_json::to_vec_pretty(&config)?;
    let config_path = dir.join("config.json");
    if fs::read(&config_path).ok().as_deref() != Some(json.as_slice()) {
        fs::write(config_path, json)?;
    }
    Ok(())
}

// The macOS screen saver runs sandboxed: a hard link next to its config skips the folders macOS protects.
fn saver_media(dir: &Path, wallpaper: &Wallpaper) -> PathBuf {
    if !cfg!(target_os = "macos") || wallpaper.kind == MediaKind::Web {
        return wallpaper.path.clone();
    }
    let extension = wallpaper
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase();
    let media = dir.join(format!("media.{extension}"));
    match link_media(&wallpaper.path, &media) {
        Ok(()) => media,
        Err(err) => {
            log::warn!(
                "Could not link {} for the screen saver: {err}",
                wallpaper.path.display()
            );
            wallpaper.path.clone()
        }
    }
}

fn link_media(source: &Path, target: &Path) -> io::Result<()> {
    if same_file(source, target) {
        return Ok(());
    }
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::hard_link(source, target)
}

#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(_: &Path, _: &Path) -> bool {
    false
}

fn parse_window_id(id: &str) -> Option<u64> {
    let id = id.trim();
    match id.strip_prefix("0x").or_else(|| id.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => id.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wallkika-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read_config(dir: &Path) -> SaverConfig {
        serde_json::from_slice(&fs::read(dir.join("config.json")).unwrap()).unwrap()
    }

    fn wallpaper(path: &Path, kind: MediaKind) -> Wallpaper {
        Wallpaper {
            path: path.to_path_buf(),
            name: file_name(path),
            kind,
            mode: RenderMode::Live,
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn links_the_chosen_media_and_clears_it_when_disabled() {
        let dir = temp_dir("saver");
        let source = dir.join("Matrix Video.MP4");
        fs::write(&source, b"video").unwrap();
        let saver = dir.join("screensaver");

        write_config(&saver, Some(&wallpaper(&source, MediaKind::Video))).unwrap();
        let config = read_config(&saver);
        assert_eq!(config.kind, Some(MediaKind::Video));
        assert_eq!(config.path, Some(saver.join("media.mp4")));
        assert!(same_file(&source, &saver.join("media.mp4")));

        write_config(&saver, None).unwrap();
        assert_eq!(read_config(&saver), SaverConfig::default());
        assert!(!saver.join("media.mp4").exists());
        assert!(source.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn web_wallpapers_point_to_their_page() {
        let dir = temp_dir("saver-web");
        let page = dir.join("index.html");
        fs::write(&page, b"<html></html>").unwrap();
        let saver = dir.join("screensaver");
        write_config(&saver, Some(&wallpaper(&page, MediaKind::Web))).unwrap();
        assert_eq!(read_config(&saver).path, Some(page));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parses_screen_saver_launches() {
        let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(Mode::from_args(&args(&["--screensaver"])), Some(Mode::Show));
        assert_eq!(Mode::from_args(&args(&["video.mp4"])), None);
        assert_eq!(Mode::from_args(&args(&[])), None);
        let windows = |mode| if cfg!(windows) { Some(mode) } else { None };
        assert_eq!(Mode::from_args(&args(&["/S"])), windows(Mode::Show));
        assert_eq!(
            Mode::from_args(&args(&["/p", "1234"])),
            windows(Mode::Preview)
        );
        assert_eq!(Mode::from_args(&args(&["/c:1234"])), None);
    }

    #[test]
    fn lock_screen_needs_a_still_only_for_new_pictures() {
        let video = wallpaper(Path::new("/clips/matrix.mp4"), MediaKind::Video);
        assert!(needs_still(&video, None));
        assert!(needs_still(&video, Some("Video:/clips/other.mp4")));
        assert!(!needs_still(&video, Some(&source_key(&video))));
        assert!(!needs_still(
            &wallpaper(Path::new("/web/index.html"), MediaKind::Web),
            None
        ));

        let mut photo = wallpaper(Path::new("/photos/beach.jpg"), MediaKind::Image);
        photo.mode = RenderMode::Native;
        assert_eq!(needs_still(&photo, None), !imp::LOCK_SCREEN_SHOWS_DESKTOP);
    }

    #[test]
    fn reads_x11_window_ids() {
        assert_eq!(parse_window_id("0x3a00007"), Some(0x3a00007));
        assert_eq!(parse_window_id(" 4194311 "), Some(4194311));
        assert_eq!(parse_window_id("window"), None);
    }
}
