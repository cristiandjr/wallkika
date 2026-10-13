use super::config_dir;
use crate::{
    error::{Error, Result},
    platform,
};
use gtk::{gdk, prelude::*};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, Manager, WebviewWindow};

pub const LOCK_SCREEN_SHOWS_DESKTOP: bool = true;

const THEME: &str = "screensavers-wallkika";
const PREVIOUS_SAVER_FILE: &str = "previous-screensaver.json";
const XFCE_CHANNEL: &str = "xfce4-screensaver";
const MATE_SCHEMA: &str = "org.mate.screensaver";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Daemon {
    Xfce,
    Mate,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct SaverSettings {
    enabled: Option<String>,
    mode: Option<String>,
    themes: Option<String>,
}

impl Daemon {
    fn detect() -> Option<Self> {
        let desktop = env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase();
        let xfce =
            find_program("xfce4-screensaver").is_some() && find_program("xfconf-query").is_some();
        let mate =
            find_program("mate-screensaver").is_some() && find_program("gsettings").is_some();
        match (xfce, mate) {
            (true, true) if desktop.contains("mate") => Some(Self::Mate),
            (true, _) => Some(Self::Xfce),
            (false, true) => Some(Self::Mate),
            (false, false) => None,
        }
    }

    fn settings_app(self) -> &'static str {
        match self {
            Self::Xfce => "xfce4-screensaver-preferences",
            Self::Mate => "mate-screensaver-preferences",
        }
    }

    fn read(self) -> SaverSettings {
        let xfce =
            |property: &str| output("xfconf-query", &["-c", XFCE_CHANNEL, "-p", property]).ok();
        let mate = |key: &str| output("gsettings", &["get", MATE_SCHEMA, key]).ok();
        match self {
            Self::Xfce => SaverSettings {
                enabled: xfce("/saver/enabled"),
                mode: xfce("/saver/mode"),
                themes: xfce("/saver/themes/list"),
            },
            Self::Mate => SaverSettings {
                enabled: mate("idle-activation-enabled"),
                mode: mate("mode"),
                themes: mate("themes"),
            },
        }
    }

    fn shows_wallkika(self, settings: &SaverSettings) -> bool {
        let themes = settings.themes.as_deref().unwrap_or_default();
        match self {
            Self::Xfce => settings.mode.as_deref() == Some("2") && xfconf_list(themes) == [THEME],
            Self::Mate => settings.mode.as_deref() == Some("'single'") && themes.contains(THEME),
        }
    }

    fn select(self) -> Result<()> {
        match self {
            Self::Xfce => {
                xfconf_set("/saver/enabled", "bool", &["true"])?;
                xfconf_set("/saver/mode", "int", &["2"])?;
                xfconf_set("/saver/themes/list", "string", &[THEME])
            }
            Self::Mate => {
                run(
                    "gsettings",
                    &["set", MATE_SCHEMA, "idle-activation-enabled", "true"],
                )?;
                run("gsettings", &["set", MATE_SCHEMA, "mode", "single"])?;
                run(
                    "gsettings",
                    &["set", MATE_SCHEMA, "themes", &format!("['{THEME}']")],
                )
            }
        }
    }

    fn restore(self, previous: &SaverSettings) -> Result<()> {
        match self {
            Self::Xfce => {
                xfconf_restore(
                    "/saver/enabled",
                    "bool",
                    previous.enabled.as_deref().map(|value| vec![value]),
                )?;
                xfconf_restore(
                    "/saver/mode",
                    "int",
                    previous.mode.as_deref().map(|value| vec![value]),
                )?;
                xfconf_restore(
                    "/saver/themes/list",
                    "string",
                    previous.themes.as_deref().map(xfconf_list),
                )
            }
            Self::Mate => {
                let get = |value: &Option<String>, default: &'static str| {
                    value.clone().unwrap_or_else(|| default.into())
                };
                run(
                    "gsettings",
                    &[
                        "set",
                        MATE_SCHEMA,
                        "idle-activation-enabled",
                        &get(&previous.enabled, "true"),
                    ],
                )?;
                run(
                    "gsettings",
                    &[
                        "set",
                        MATE_SCHEMA,
                        "mode",
                        &get(&previous.mode, "blank-only"),
                    ],
                )?;
                run(
                    "gsettings",
                    &[
                        "set",
                        MATE_SCHEMA,
                        "themes",
                        &get(&previous.themes, "@as []"),
                    ],
                )
            }
        }
    }
}

pub fn supports_screensaver() -> bool {
    Daemon::detect().is_some()
}

pub fn selected(_app: &AppHandle) -> Option<bool> {
    let daemon = Daemon::detect()?;
    Some(daemon.shows_wallkika(&daemon.read()))
}

pub fn install(app: &AppHandle) -> Result<()> {
    let program = match env::var_os("APPIMAGE") {
        Some(appimage) => PathBuf::from(appimage),
        None => env::current_exe()?,
    };
    let entry = desktop_entry(&program);
    let path = app
        .path()
        .data_dir()?
        .join("applications/screensavers/wallkika.desktop");
    if fs::read_to_string(&path).ok().as_deref() == Some(entry.as_str()) {
        return Ok(());
    }
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::write(&path, entry)?;
    log::info!("Screen saver installed at {}", path.display());
    Ok(())
}

pub fn set_active(app: &AppHandle, active: bool) -> Result<()> {
    let Some(daemon) = Daemon::detect() else {
        return Ok(());
    };
    if active {
        install(app)?;
    }
    let current = daemon.read();
    let selected = daemon.shows_wallkika(&current);
    let previous = config_dir(app)?.join(PREVIOUS_SAVER_FILE);
    if active && !selected {
        let saved = serde_json::to_vec(&current).map_err(|err| Error::platform(err.to_string()))?;
        fs::write(&previous, saved)?;
        daemon.select()?;
        log::info!("WallKika is now the screen saver of {daemon:?}");
    } else if !active && selected {
        let saved = fs::read(&previous)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        daemon.restore(&saved)?;
        let _ = fs::remove_file(&previous);
        log::info!("Restored the previous screen saver of {daemon:?}");
    }
    Ok(())
}

pub fn open_settings(_app: &AppHandle) -> Result<()> {
    let daemon = Daemon::detect()
        .ok_or_else(|| Error::platform("This desktop has no screen saver settings"))?;
    Command::new(daemon.settings_app()).spawn()?;
    Ok(())
}

// GNOME, Cinnamon, XFCE and MATE draw the lock screen over the desktop picture; KDE keeps its own.
pub fn set_lock_screen(app: &AppHandle, still: &Path) -> Result<()> {
    let desktop = platform::set_static_wallpaper(app, still);
    let kde = env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase()
        .contains("kde");
    if kde {
        let uri = url::Url::from_file_path(still)
            .map_err(|_| Error::platform("Invalid file path"))?
            .to_string();
        let args = [
            "--file",
            "kscreenlockerrc",
            "--group",
            "Greeter",
            "--group",
            "Wallpaper",
            "--group",
            "org.kde.image",
            "--group",
            "General",
            "--key",
            "Image",
            &uri,
        ];
        if run("kwriteconfig6", &args)
            .or_else(|_| run("kwriteconfig5", &args))
            .is_err()
        {
            log::warn!("Could not set the KDE lock screen picture");
        }
    }
    desktop
}

pub fn embed(window: &WebviewWindow, host: u64) -> Result<()> {
    let gtk_window = window.gtk_window()?;
    gtk_window.realize();
    let own = gtk_window
        .window()
        .ok_or_else(|| Error::platform("The screen saver window has no X11 window"))?;
    let display = own
        .display()
        .downcast::<gdkx11::X11Display>()
        .map_err(|_| Error::platform("The screen saver needs an X11 session"))?;
    let host: gdk::Window =
        gdkx11::X11Window::foreign_new_for_display(&display, host as _).upcast();
    let (_, _, width, height) = host.geometry();
    own.reparent(&host, 0, 0);
    gtk_window.resize(width, height);
    gtk_window.show_all();
    Ok(())
}

fn find_program(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    output(program, args).map(|_| ())
}

fn output(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| Error::platform(format!("{program}: {err}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(Error::platform(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

fn xfconf_set(property: &str, kind: &str, values: &[&str]) -> Result<()> {
    let mut args = vec!["-c", XFCE_CHANNEL, "-p", property, "-n"];
    if values.len() != 1 || property.ends_with("/list") {
        args.push("-a");
    }
    for value in values {
        args.extend(["-t", kind, "-s", value]);
    }
    run("xfconf-query", &args)
}

fn xfconf_restore(property: &str, kind: &str, previous: Option<Vec<&str>>) -> Result<()> {
    match previous {
        Some(values) if !values.is_empty() => xfconf_set(property, kind, &values),
        _ => run("xfconf-query", &["-c", XFCE_CHANNEL, "-p", property, "-r"]),
    }
}

fn xfconf_list(value: &str) -> Vec<&str> {
    match value.split_once("\n\n") {
        Some((header, items)) if header.starts_with("Value is an array") => items
            .lines()
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .collect(),
        _ => vec![value.trim()]
            .into_iter()
            .filter(|item| !item.is_empty())
            .collect(),
    }
}

fn desktop_entry(program: &Path) -> String {
    let program = program.to_string_lossy();
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=WallKika\n\
         Comment=Your WallKika wallpaper as a screen saver\n\
         Exec={} --screensaver\n\
         TryExec={}\n\
         StartupNotify=false\n\
         Terminal=false\n\
         Categories=Screensaver;\n\
         OnlyShowIn=XFCE;MATE;\n",
        exec_argument(&program),
        program.replace('\\', "\\\\")
    )
}

// Desktop entries unescape strings first and then the Exec quoting, so some characters need two levels.
fn exec_argument(value: &str) -> String {
    let mut quoted = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => quoted.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_the_program_for_the_exec_key() {
        assert_eq!(
            exec_argument("/home/me/Apps/Wall Kika.AppImage"),
            "\"/home/me/Apps/Wall Kika.AppImage\""
        );
        assert_eq!(exec_argument("/tmp/a$b%c"), "\"/tmp/a\\\\$b%%c\"");
    }

    #[test]
    fn reads_xfconf_arrays_and_single_values() {
        let array =
            "Value is an array with 2 items:\n\nscreensavers-wallkika\nscreensavers-xfce-floaters";
        assert_eq!(
            xfconf_list(array),
            ["screensavers-wallkika", "screensavers-xfce-floaters"]
        );
        assert_eq!(
            xfconf_list("screensavers-wallkika"),
            ["screensavers-wallkika"]
        );
        assert!(xfconf_list("").is_empty());
    }

    #[test]
    fn knows_when_wallkika_is_the_daemon_theme() {
        let xfce = SaverSettings {
            enabled: Some("true".into()),
            mode: Some("2".into()),
            themes: Some("Value is an array with 1 items:\n\nscreensavers-wallkika".into()),
        };
        assert!(Daemon::Xfce.shows_wallkika(&xfce));
        let mate = SaverSettings {
            enabled: Some("true".into()),
            mode: Some("'single'".into()),
            themes: Some("['screensavers-wallkika']".into()),
        };
        assert!(Daemon::Mate.shows_wallkika(&mate));
        assert!(!Daemon::Mate.shows_wallkika(&SaverSettings::default()));
    }

    #[test]
    fn the_entry_lists_wallkika_as_a_screen_saver() {
        let entry = desktop_entry(Path::new("/opt/WallKika.AppImage"));
        assert!(entry.contains("Exec=\"/opt/WallKika.AppImage\" --screensaver\n"));
        assert!(entry.contains("Categories=Screensaver;\n"));
    }
}
