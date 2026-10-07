use crate::{
    display::{self, DisplayInfo},
    error::{Error, Result},
    layout::{Layout, LayoutMode, RenderMode, Wallpaper},
    media::MediaKind,
    platform,
    protocol::MediaScope,
    settings::Settings,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use tauri::{webview::Color, AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LAYOUT_CHANGED: &str = "layout-changed";
const LIVE_LABEL_PREFIX: &str = "wallpaper-";
const READY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    All,
    Display(String),
}

impl From<Option<String>> for Target {
    fn from(display: Option<String>) -> Self {
        display.map_or(Self::All, Self::Display)
    }
}

pub struct Engine {
    ops: Mutex<()>,
    state: Mutex<State>,
    settings_path: PathBuf,
}

#[derive(Default)]
struct State {
    layout: Layout,
    live: Vec<LiveWindow>,
    ready: HashSet<String>,
    next_label: u64,
}

#[derive(Clone)]
struct LiveWindow {
    label: String,
    display: DisplayInfo,
    created: Instant,
}

impl Engine {
    pub fn new(settings_path: PathBuf) -> Self {
        let layout = Settings::load(&settings_path).layout;
        Self {
            ops: Mutex::new(()),
            state: Mutex::new(State {
                layout,
                ..State::default()
            }),
            settings_path,
        }
    }

    pub fn layout(&self) -> Layout {
        lock(&self.state).layout.clone()
    }

    pub fn mark_ready(&self, label: &str) {
        lock(&self.state).ready.insert(label.to_string());
    }

    pub fn display_name_of(&self, label: &str) -> Option<String> {
        lock(&self.state)
            .live
            .iter()
            .find(|window| window.label == label)
            .map(|window| window.display.name.clone())
    }

    pub fn apply(&self, app: &AppHandle, path: &Path, target: &Target) -> Result<Wallpaper> {
        let _op = lock(&self.ops);
        let path = path
            .canonicalize()
            .map_err(|_| Error::NotFound(path.to_path_buf()))?;
        if !path.is_file() {
            return Err(Error::NotFound(path));
        }
        let kind = MediaKind::from_path(&path)
            .ok_or_else(|| Error::Unsupported(describe_extension(&path)))?;
        allow_media_access(app, &path, kind);

        let mode = match target {
            Target::All if kind.is_static() => match platform::set_static_wallpaper(app, &path) {
                Ok(()) => RenderMode::Native,
                Err(err) => {
                    log::warn!("The OS rejected the image ({err}); rendering it with WallKika");
                    RenderMode::Live
                }
            },
            _ => RenderMode::Live,
        };
        let wallpaper = Wallpaper {
            name: file_name(&path),
            path,
            kind,
            mode,
        };

        let previous = self.layout();
        match target {
            Target::All => {
                let mut state = lock(&self.state);
                state.layout.mode = LayoutMode::Mirror;
                state.layout.all = Some(wallpaper.clone());
            }
            Target::Display(id) => {
                let displays = display::list(app)?;
                let mut state = lock(&self.state);
                state.layout.switch_to(LayoutMode::PerDisplay, &displays);
                state.layout.displays.insert(id.clone(), wallpaper.clone());
            }
        }
        if let Err(err) = self.refresh_windows(app) {
            lock(&self.state).layout = previous;
            let _ = self.refresh_windows(app);
            self.publish(app);
            return Err(err);
        }

        log::info!(
            "Wallpaper for {}: {} ({kind:?}, {mode:?})",
            describe_target(target),
            wallpaper.path.display()
        );
        self.publish(app);
        Ok(wallpaper)
    }

    pub fn clear(&self, app: &AppHandle, target: &Target) {
        let _op = lock(&self.ops);
        {
            let mut state = lock(&self.state);
            match target {
                Target::All => state.layout.clear_live(),
                Target::Display(id) => {
                    state.layout.displays.remove(id);
                }
            }
        }
        if let Err(err) = self.refresh_windows(app) {
            log::warn!("Could not update the wallpaper windows: {err}");
        }
        self.publish(app);
    }

    pub fn set_mode(&self, app: &AppHandle, mode: LayoutMode) -> Result<()> {
        let _op = lock(&self.ops);
        let displays = display::list(app)?;
        lock(&self.state).layout.switch_to(mode, &displays);
        let result = self.reconcile(app, &displays);
        self.publish(app);
        result
    }

    pub fn restore(&self, app: &AppHandle) {
        let _op = lock(&self.ops);
        let forgot = lock(&self.state).layout.forget_missing_files();
        for wallpaper in self.layout().wallpapers() {
            allow_media_access(app, &wallpaper.path, wallpaper.kind);
        }
        if let Err(err) = self.refresh_windows(app) {
            log::error!("Could not restore the live wallpapers: {err}");
        }
        if forgot {
            log::warn!("Forgot saved wallpapers whose files no longer exist");
            self.publish(app);
        }
    }

    pub fn sync_live_windows(&self, app: &AppHandle, displays: &[DisplayInfo]) -> Result<()> {
        let _op = lock(&self.ops);
        self.reconcile(app, displays)
    }

    fn refresh_windows(&self, app: &AppHandle) -> Result<()> {
        let displays = display::list(app)?;
        self.reconcile(app, &displays)
    }

    fn reconcile(&self, app: &AppHandle, displays: &[DisplayInfo]) -> Result<()> {
        let (wanted, current, ready) = {
            let state = lock(&self.state);
            (
                state.layout.live_displays(displays),
                state.live.clone(),
                state.ready.clone(),
            )
        };
        let stalled = |window: &LiveWindow| {
            !ready.contains(&window.label) && window.created.elapsed() > READY_TIMEOUT
        };
        let (mut live, closed): (Vec<_>, Vec<_>) = current.into_iter().partition(|window| {
            wanted.contains(&window.display)
                && app.get_webview_window(&window.label).is_some()
                && !stalled(window)
        });
        for window in &closed {
            if stalled(window) {
                log::warn!("{} never loaded; recreating it", window.label);
            }
            lock(&self.state).ready.remove(&window.label);
            if let Some(webview) = app.get_webview_window(&window.label) {
                if let Err(err) = webview.destroy() {
                    log::warn!("Could not close {}: {err}", window.label);
                }
            }
        }

        let kept = live.len();
        let mut result = Ok(());
        for display in wanted.iter() {
            if live.iter().any(|window| &window.display == display) {
                continue;
            }
            let label = {
                let mut state = lock(&self.state);
                state.next_label += 1;
                format!("{LIVE_LABEL_PREFIX}{}", state.next_label)
            };
            match create_live_window(app, &label, display) {
                Ok(()) => live.push(LiveWindow {
                    label,
                    display: display.clone(),
                    created: Instant::now(),
                }),
                Err(err) => {
                    result = Err(err);
                    break;
                }
            }
        }

        let opened = live.len() - kept;
        if opened > 0 || !closed.is_empty() {
            log::info!(
                "Live wallpaper windows: {} (opened {opened}, closed {})",
                live.len(),
                closed.len()
            );
        }
        let none_left = live.is_empty();
        lock(&self.state).live = live;
        if !closed.is_empty() && none_left {
            if let Err(err) = platform::on_main_thread(app, platform::after_live_windows_closed) {
                log::warn!("Could not refresh the desktop: {err}");
            }
        }
        result
    }

    fn publish(&self, app: &AppHandle) {
        let layout = self.layout();
        if let Err(err) = app.emit(LAYOUT_CHANGED, &layout) {
            log::warn!("Could not broadcast the layout: {err}");
        }
        if let Err(err) = Settings::new(layout).save(&self.settings_path) {
            log::warn!("Could not save settings: {err}");
        }
    }
}

fn create_live_window(app: &AppHandle, label: &str, display: &DisplayInfo) -> Result<()> {
    let (x, y, width, height) = display.logical_rect();
    let display_id =
        serde_json::to_string(&display.id).map_err(|err| Error::platform(err.to_string()))?;
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App("wallpaper.html".into()))
        .title("WallKika")
        .position(x, y)
        .inner_size(width, height)
        .decorations(false)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .closable(false)
        .shadow(false)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .visible(false)
        .visible_on_all_workspaces(true)
        .background_color(Color(0, 0, 0, 255))
        .disable_drag_drop_handler()
        .additional_browser_args(platform::BROWSER_ARGS)
        .initialization_script(format!("window.__WALLKIKA_DISPLAY__ = {display_id};"))
        .build()?;

    let display = display.clone();
    platform::on_main_thread(app, move || platform::attach_live_window(&window, &display))?
}

fn allow_media_access(app: &AppHandle, path: &Path, kind: MediaKind) {
    let scope = app.state::<MediaScope>();
    match (kind, path.parent()) {
        (MediaKind::Web, Some(dir)) => scope.allow_dir(dir),
        _ => scope.allow_file(path),
    }
}

fn describe_target(target: &Target) -> String {
    match target {
        Target::All => "all displays".into(),
        Target::Display(id) => format!("display {id}"),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn describe_extension(path: &Path) -> String {
    path.extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_else(|| "no extension".into())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
