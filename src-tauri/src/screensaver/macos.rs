use super::config_dir;
use crate::{
    error::{Error, Result},
    platform,
};
use std::{fs, path::Path, process::Command};
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

pub const LOCK_SCREEN_SHOWS_DESKTOP: bool = true;

const BUNDLE: &str = "WallKika.saver";
const SAVER_NAME: &str = "WallKika";
const PREVIOUS_SAVER_FILE: &str = "previous-screensaver.txt";
const BUNDLE_FILES: [&str; 2] = ["Contents/MacOS/WallKika", "Contents/Info.plist"];
const SAVER_CHOICE: &str = "com.apple.wallpaper.choice.screen-saver";
const WALLPAPER_STORE: &str = "Library/Application Support/com.apple.wallpaper/Store/Index.plist";
const SETTINGS_EXTENSIONS: &str = "/System/Library/ExtensionKit/Extensions";
const SCREEN_SAVER_PANE: &str = "com.apple.ScreenSaver-Settings.extension";
const WALLPAPER_PANE: &str = "com.apple.Wallpaper-Settings.extension";

pub fn supports_screensaver() -> bool {
    true
}

pub fn install(app: &AppHandle) -> Result<()> {
    let source = app.path().resource_dir()?.join(BUNDLE);
    if !source.is_dir() {
        return Err(Error::platform(
            "This build doesn't include the WallKika screen saver",
        ));
    }
    let folder = app.path().home_dir()?.join("Library/Screen Savers");
    let target = folder.join(BUNDLE);
    let installed = BUNDLE_FILES
        .iter()
        .all(|file| fs::read(source.join(file)).ok() == fs::read(target.join(file)).ok());
    if installed {
        return Ok(());
    }

    fs::create_dir_all(&folder)?;
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    let copied = Command::new("/usr/bin/ditto")
        .arg("--noqtn")
        .arg(&source)
        .arg(&target)
        .status()?;
    if !copied.success() {
        return Err(Error::platform(
            "Could not install the WallKika screen saver",
        ));
    }
    log::info!("Screen saver installed at {}", target.display());
    Ok(())
}

pub fn set_active(app: &AppHandle, active: bool) -> Result<()> {
    if active {
        install(app)?;
        select(app)
    } else {
        restore(app)
    }
}

// System Settings has no API for the screen saver choice, but System Events still takes it by AppleScript.
fn select(app: &AppHandle) -> Result<()> {
    if selected(app) == Some(true) {
        return Ok(());
    }
    let previous = config_dir(app)?.join(PREVIOUS_SAVER_FILE);
    if let Ok(current) = current_saver() {
        if current != SAVER_NAME {
            fs::write(&previous, current)?;
        }
    }
    osascript(&select_script(SAVER_NAME))?;
    log::info!("WallKika is now the macOS screen saver");
    Ok(())
}

fn restore(app: &AppHandle) -> Result<()> {
    let previous = config_dir(app)?.join(PREVIOUS_SAVER_FILE);
    let Ok(name) = fs::read_to_string(&previous) else {
        return Ok(());
    };
    if selected(app) == Some(true) && !name.trim().is_empty() {
        osascript(&select_script(name.trim()))?;
        log::info!("Restored the previous macOS screen saver: {}", name.trim());
    }
    let _ = fs::remove_file(previous);
    Ok(())
}

fn current_saver() -> Result<String> {
    osascript("tell application \"System Events\" to get name of current screen saver")
}

fn select_script(name: &str) -> String {
    let name = name.replace('\\', "\\\\").replace('"', "\\\"");
    format!("tell application \"System Events\" to set current screen saver to screen saver named \"{name}\"")
}

fn osascript(script: &str) -> Result<String> {
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(Error::platform(format!(
            "Could not change the screen saver: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

pub fn selected(app: &AppHandle) -> Option<bool> {
    selected_in(&app.path().home_dir().ok()?)
}

fn selected_in(home: &Path) -> Option<bool> {
    let store = home.join(WALLPAPER_STORE);
    if !store.exists() {
        let saved = Command::new("/usr/bin/defaults")
            .args([
                "-currentHost",
                "read",
                "com.apple.screensaver",
                "moduleDict",
            ])
            .output()
            .ok()?;
        return Some(String::from_utf8_lossy(&saved.stdout).contains(BUNDLE));
    }
    let store = plist::Value::from_file(store).ok()?;
    let idle = store
        .as_dictionary()?
        .get("AllSpacesAndDisplays")?
        .as_dictionary()?
        .get("Idle")?;
    Some(is_wallkika(idle))
}

fn is_wallkika(idle: &plist::Value) -> bool {
    let choice = idle
        .as_dictionary()
        .and_then(|idle| {
            idle.get("Content")?
                .as_dictionary()?
                .get("Choices")?
                .as_array()?
                .first()
        })
        .and_then(plist::Value::as_dictionary);
    choice.is_some_and(|choice| {
        let provider = choice.get("Provider").and_then(plist::Value::as_string);
        let module = choice.get("Configuration").and_then(plist::Value::as_data);
        provider == Some(SAVER_CHOICE)
            && module.is_some_and(|module| module.windows(8).any(|part| part == b"WallKika"))
    })
}

pub fn open_settings(app: &AppHandle) -> Result<()> {
    let pane = if has_settings_pane(SCREEN_SAVER_PANE) {
        SCREEN_SAVER_PANE
    } else {
        WALLPAPER_PANE
    };
    app.opener()
        .open_url(format!("x-apple.systempreferences:{pane}"), None::<&str>)
        .map_err(|err| Error::platform(err.to_string()))
}

// Older macOS versions have a Screen Saver pane; newer ones moved it into Wallpaper.
fn has_settings_pane(identifier: &str) -> bool {
    let Ok(extensions) = fs::read_dir(SETTINGS_EXTENSIONS) else {
        return false;
    };
    extensions
        .flatten()
        .filter(|extension| extension.file_name().to_string_lossy().contains("Screen"))
        .any(|extension| {
            plist::Value::from_file(extension.path().join("Contents/Info.plist"))
                .ok()
                .and_then(|info| {
                    let id = info.as_dictionary()?.get("CFBundleIdentifier")?;
                    Some(id.as_string()? == identifier)
                })
                .unwrap_or(false)
        })
}

pub fn set_lock_screen(app: &AppHandle, still: &Path) -> Result<()> {
    platform::set_static_wallpaper(app, still)
}

pub fn embed(_window: &WebviewWindow, _host: u64) -> Result<()> {
    Err(Error::platform("Only X11 screen savers can host WallKika"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle(provider: &str, configuration: &[u8]) -> plist::Value {
        let mut choice = plist::Dictionary::new();
        choice.insert("Provider".into(), provider.into());
        choice.insert(
            "Configuration".into(),
            plist::Value::Data(configuration.to_vec()),
        );
        let mut content = plist::Dictionary::new();
        content.insert(
            "Choices".into(),
            vec![plist::Value::Dictionary(choice)].into(),
        );
        let mut idle = plist::Dictionary::new();
        idle.insert("Content".into(), plist::Value::Dictionary(content));
        plist::Value::Dictionary(idle)
    }

    #[test]
    fn reads_the_screen_saver_from_the_wallpaper_store() {
        let home = std::env::temp_dir().join(format!("wallkika-store-{}", std::process::id()));
        let store = home.join(WALLPAPER_STORE);
        fs::create_dir_all(store.parent().unwrap()).unwrap();
        let write = |idle: plist::Value| {
            let mut all = plist::Dictionary::new();
            all.insert("Idle".into(), idle);
            all.insert("Type".into(), "idle".into());
            let mut root = plist::Dictionary::new();
            root.insert("AllSpacesAndDisplays".into(), plist::Value::Dictionary(all));
            plist::Value::Dictionary(root)
                .to_file_binary(&store)
                .unwrap();
        };

        write(idle("com.apple.wallpaper.choice.aerials", b"assetID"));
        assert_eq!(selected_in(&home), Some(false));
        write(idle(
            SAVER_CHOICE,
            b"file:///Users/me/Library/Screen%20Savers/WallKika.saver/",
        ));
        assert_eq!(selected_in(&home), Some(true));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn quotes_screen_saver_names_for_applescript() {
        assert_eq!(
            select_script("Los Ángeles de noche"),
            "tell application \"System Events\" to set current screen saver to screen saver named \"Los Ángeles de noche\""
        );
        assert!(select_script("Say \"hi\"").contains("named \"Say \\\"hi\\\"\""));
    }

    #[test]
    fn recognizes_wallkika_as_the_chosen_screen_saver() {
        let module = b"file:///Users/me/Library/Screen%20Savers/WallKika.saver/";
        assert!(is_wallkika(&idle(SAVER_CHOICE, module)));
        assert!(!is_wallkika(&idle(
            SAVER_CHOICE,
            b"file:///Library/Screen%20Savers/Flurry.saver/"
        )));
        assert!(!is_wallkika(&idle(
            "com.apple.wallpaper.choice.aerials",
            b"{assetID}"
        )));
    }
}
