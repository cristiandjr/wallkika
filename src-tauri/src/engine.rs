use crate::{
    display::{self, DisplayInfo},
    error::{Error, Result},
    media::MediaKind,
    platform,
    protocol::MediaScope,
    settings::Settings,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
};
use tauri::{
    webview::Color, AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

pub const WALLPAPER_CHANGED: &str = "wallpaper-changed";
const LIVE_LABEL_PREFIX: &str = "wallpaper-";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderMode {
    Native,
    Live,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wallpaper {
    pub path: PathBuf,
    pub name: String,
    pub kind: MediaKind,
    pub mode: RenderMode,
}

pub struct Engine {
    ops: Mutex<()>,
    state: Mutex<State>,
    settings_path: PathBuf,
}

#[derive(Default)]
struct State {
    current: Option<Wallpaper>,
    live_displays: Option<Vec<DisplayInfo>>,
    generation: u64,
}

impl Engine {
    pub fn new(settings_path: PathBuf) -> Self {
        let current = Settings::load(&settings_path).current;
        Self {
            ops: Mutex::new(()),
            state: Mutex::new(State {
                current,
                ..State::default()
            }),
            settings_path,
        }
    }

    pub fn current(&self) -> Option<Wallpaper> {
        lock(&self.state).current.clone()
    }

    pub fn apply(&self, app: &AppHandle, path: &Path) -> Result<Wallpaper> {
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

        let mode = if kind.is_static() {
            match platform::set_static_wallpaper(app, &path) {
                Ok(()) => RenderMode::Native,
                Err(err) => {
                    log::warn!("The OS rejected the image ({err}); rendering it with WallKika");
                    RenderMode::Live
                }
            }
        } else {
            RenderMode::Live
        };

        let wallpaper = Wallpaper {
            name: file_name(&path),
            path,
            kind,
            mode,
        };
        // Must be stored before creating windows: each new window fetches it on load.
        lock(&self.state).current = Some(wallpaper.clone());

        let shown = match mode {
            RenderMode::Native => {
                self.close_live_windows(app);
                Ok(())
            }
            RenderMode::Live => {
                display::list(app).and_then(|displays| self.ensure_live_windows(app, &displays))
            }
        };
        if let Err(err) = shown {
            self.close_live_windows(app);
            lock(&self.state).current = None;
            self.publish(app);
            return Err(err);
        }

        log::info!("Wallpaper: {} ({kind:?}, {mode:?})", wallpaper.path.display());
        self.publish(app);
        Ok(wallpaper)
    }

    pub fn stop_live(&self, app: &AppHandle) {
        let _op = lock(&self.ops);
        self.close_live_windows(app);
        let mut state = lock(&self.state);
        if state
            .current
            .as_ref()
            .is_some_and(|w| w.mode == RenderMode::Live)
        {
            state.current = None;
        }
        drop(state);
        self.publish(app);
    }

    pub fn restore(&self, app: &AppHandle) {
        let _op = lock(&self.ops);
        let Some(wallpaper) = self.current() else {
            return;
        };
        if !wallpaper.path.is_file() {
            log::warn!("Last wallpaper no longer exists: {}", wallpaper.path.display());
            lock(&self.state).current = None;
            self.publish(app);
            return;
        }
        allow_media_access(app, &wallpaper.path, wallpaper.kind);
        if wallpaper.mode == RenderMode::Live {
            let restored =
                display::list(app).and_then(|displays| self.ensure_live_windows(app, &displays));
            if let Err(err) = restored {
                log::error!("Could not restore the live wallpaper: {err}");
            }
        }
    }

    pub fn sync_live_windows(&self, app: &AppHandle, displays: &[DisplayInfo]) -> Result<()> {
        let _op = lock(&self.ops);
        let is_live = self
            .current()
            .is_some_and(|w| w.mode == RenderMode::Live);
        if !is_live || displays.is_empty() {
            return Ok(());
        }
        self.ensure_live_windows(app, displays)
    }

    fn ensure_live_windows(&self, app: &AppHandle, displays: &[DisplayInfo]) -> Result<()> {
        let generation = {
            let mut state = lock(&self.state);
            let up_to_date = state.live_displays.as_deref() == Some(displays)
                && live_windows(app, Some(state.generation)).len() == displays.len();
            if up_to_date {
                return Ok(());
            }
            state.generation += 1;
            state.generation
        };

        self.close_live_windows(app);
        for (index, display) in displays.iter().enumerate() {
            let label = format!("{LIVE_LABEL_PREFIX}{generation}-{index}");
            create_live_window(app, &label, display)?;
        }
        lock(&self.state).live_displays = Some(displays.to_vec());
        log::info!("Live wallpaper on {} display(s)", displays.len());
        Ok(())
    }

    fn close_live_windows(&self, app: &AppHandle) {
        let windows = live_windows(app, None);
        lock(&self.state).live_displays = None;
        if windows.is_empty() {
            return;
        }
        for window in windows {
            if let Err(err) = window.destroy() {
                log::warn!("Could not close {}: {err}", window.label());
            }
        }
        // Queued after the destroy messages, so it runs once the windows are gone.
        if let Err(err) = platform::on_main_thread(app, platform::after_live_windows_closed) {
            log::warn!("Could not refresh the desktop: {err}");
        }
    }

    fn publish(&self, app: &AppHandle) {
        let current = self.current();
        if let Err(err) = app.emit(WALLPAPER_CHANGED, &current) {
            log::warn!("Could not broadcast the wallpaper change: {err}");
        }
        if let Err(err) = (Settings { current }).save(&self.settings_path) {
            log::warn!("Could not save settings: {err}");
        }
    }
}

fn create_live_window(app: &AppHandle, label: &str, display: &DisplayInfo) -> Result<()> {
    let (x, y, width, height) = display.logical_rect();
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
        .build()?;

    let display = display.clone();
    platform::on_main_thread(app, move || platform::attach_live_window(&window, &display))?
}

fn live_windows(app: &AppHandle, generation: Option<u64>) -> Vec<WebviewWindow> {
    let prefix = match generation {
        Some(generation) => format!("{LIVE_LABEL_PREFIX}{generation}-"),
        None => LIVE_LABEL_PREFIX.to_string(),
    };
    app.webview_windows()
        .into_iter()
        .filter(|(label, _)| label.starts_with(&prefix))
        .map(|(_, window)| window)
        .collect()
}

fn allow_media_access(app: &AppHandle, path: &Path, kind: MediaKind) {
    let scope = app.state::<MediaScope>();
    match (kind, path.parent()) {
        (MediaKind::Web, Some(dir)) => scope.allow_dir(dir),
        _ => scope.allow_file(path),
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
