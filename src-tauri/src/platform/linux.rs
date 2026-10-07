use crate::{
    display::DisplayInfo,
    error::{Error, Result},
};
use gtk::prelude::*;
use std::{env, path::Path, process::Command};
use tauri::{AppHandle, WebviewWindow};

const XFCE_STYLE_ZOOMED: &str = "5";

#[derive(Debug, Clone, Copy)]
enum Setter {
    Gnome,
    Cinnamon,
    Mate,
    Kde,
    Xfce,
    LxQt,
    Lxde,
    Hyprland,
    Sway,
    Swww,
    Feh,
    Xwallpaper,
    Nitrogen,
}

pub fn prepare_process() {
    // Wayland windows can't sit behind the desktop; X11 ones (via XWayland) can on GNOME and KDE.
    if env::var_os("WAYLAND_DISPLAY").is_some() && env::var_os("GDK_BACKEND").is_none() {
        env::set_var("GDK_BACKEND", "x11");
    }
}

pub fn set_static_wallpaper(_app: &AppHandle, path: &Path) -> Result<()> {
    let desktop = env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| env::var("DESKTOP_SESSION"))
        .unwrap_or_default()
        .to_lowercase();
    let mut errors = Vec::new();
    for setter in setters_for(&desktop) {
        match apply(setter, path) {
            Ok(()) => return Ok(()),
            Err(err) => errors.push(format!("{setter:?}: {err}")),
        }
    }
    Err(Error::platform(format!(
        "Could not set the wallpaper on this desktop ({desktop}). Tried: {}",
        errors.join(" | ")
    )))
}

fn setters_for(desktop: &str) -> Vec<Setter> {
    let has = |names: &[&str]| names.iter().any(|name| desktop.contains(name));
    let mut setters = Vec::new();
    if has(&["kde", "plasma"]) {
        setters.push(Setter::Kde);
    }
    if has(&["cinnamon"]) {
        setters.push(Setter::Cinnamon);
    }
    if has(&["mate"]) {
        setters.push(Setter::Mate);
    }
    if has(&["xfce"]) {
        setters.push(Setter::Xfce);
    }
    if has(&["lxqt"]) {
        setters.push(Setter::LxQt);
    }
    if has(&["lxde"]) {
        setters.push(Setter::Lxde);
    }
    if has(&["hyprland"]) {
        setters.push(Setter::Hyprland);
    }
    if has(&["sway"]) {
        setters.push(Setter::Sway);
    }
    if has(&["gnome", "unity", "budgie", "pantheon", "cosmic"]) {
        setters.push(Setter::Gnome);
    }
    setters.extend([
        Setter::Swww,
        Setter::Feh,
        Setter::Xwallpaper,
        Setter::Nitrogen,
    ]);
    setters
}

fn apply(setter: Setter, path: &Path) -> Result<()> {
    let file = path
        .to_str()
        .ok_or_else(|| Error::platform("The file path is not valid UTF-8"))?;
    let uri = url::Url::from_file_path(path)
        .map_err(|_| Error::platform("Invalid file path"))?
        .to_string();
    match setter {
        Setter::Gnome => {
            run(
                "gsettings",
                &["set", "org.gnome.desktop.background", "picture-uri", &uri],
            )?;
            let _ = run(
                "gsettings",
                &[
                    "set",
                    "org.gnome.desktop.background",
                    "picture-uri-dark",
                    &uri,
                ],
            );
            let _ = run(
                "gsettings",
                &[
                    "set",
                    "org.gnome.desktop.background",
                    "picture-options",
                    "zoom",
                ],
            );
            Ok(())
        }
        Setter::Cinnamon => {
            run(
                "gsettings",
                &[
                    "set",
                    "org.cinnamon.desktop.background",
                    "picture-uri",
                    &uri,
                ],
            )?;
            let _ = run(
                "gsettings",
                &[
                    "set",
                    "org.cinnamon.desktop.background",
                    "picture-options",
                    "zoom",
                ],
            );
            Ok(())
        }
        Setter::Mate => {
            run(
                "gsettings",
                &["set", "org.mate.background", "picture-filename", file],
            )?;
            let _ = run(
                "gsettings",
                &["set", "org.mate.background", "picture-options", "zoom"],
            );
            Ok(())
        }
        Setter::Kde => run("plasma-apply-wallpaperimage", &[file]),
        Setter::Xfce => set_xfce(file),
        Setter::LxQt => run(
            "pcmanfm-qt",
            &[&format!("--set-wallpaper={file}"), "--wallpaper-mode=zoom"],
        ),
        Setter::Lxde => run(
            "pcmanfm",
            &[&format!("--set-wallpaper={file}"), "--wallpaper-mode=crop"],
        ),
        Setter::Hyprland => run("hyprctl", &["hyprpaper", "reload", &format!(",{file}")]),
        Setter::Sway => run("swaymsg", &["output", "*", "bg", file, "fill"]),
        Setter::Swww => run("swww", &["img", file]),
        Setter::Feh => run("feh", &["--bg-fill", file]),
        Setter::Xwallpaper => run("xwallpaper", &["--zoom", file]),
        Setter::Nitrogen => run("nitrogen", &["--set-zoom-fill", "--save", file]),
    }
}

fn set_xfce(file: &str) -> Result<()> {
    let listed = output("xfconf-query", &["-c", "xfce4-desktop", "-l"])?;
    let image_props: Vec<&str> = listed
        .lines()
        .filter(|prop| prop.ends_with("/last-image"))
        .collect();
    if image_props.is_empty() {
        return Err(Error::platform("XFCE has no backdrop properties yet"));
    }
    for prop in image_props {
        run(
            "xfconf-query",
            &["-c", "xfce4-desktop", "-p", prop, "-s", file],
        )?;
        let style = prop.replace("/last-image", "/image-style");
        let _ = run(
            "xfconf-query",
            &["-c", "xfce4-desktop", "-p", &style, "-s", XFCE_STYLE_ZOOMED],
        );
    }
    Ok(())
}

pub fn attach_live_window(window: &WebviewWindow, display: &DisplayInfo) -> Result<()> {
    let gtk_window = window.gtk_window()?;
    gtk_window.set_type_hint(gtk::gdk::WindowTypeHint::Desktop);
    gtk_window.set_skip_taskbar_hint(true);
    gtk_window.set_skip_pager_hint(true);
    gtk_window.set_keep_below(true);
    gtk_window.set_accept_focus(false);
    gtk_window.set_focus_on_map(false);
    gtk_window.stick();

    let (x, y, width, height) = display.logical_rect();
    gtk_window.move_(x.round() as i32, y.round() as i32);
    gtk_window.resize(width.round() as i32, height.round() as i32);
    gtk_window.show_all();
    Ok(())
}

pub fn after_live_windows_closed() {}

pub fn name_displays(_app: &AppHandle, _displays: &mut [DisplayInfo]) {}

fn run(program: &str, args: &[&str]) -> Result<()> {
    output(program, args).map(|_| ())
}

fn output(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| Error::platform(format!("{program}: {err}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(Error::platform(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_specific_setter_goes_first() {
        assert!(matches!(setters_for("kde")[0], Setter::Kde));
        assert!(matches!(setters_for("ubuntu:gnome")[0], Setter::Gnome));
        assert!(matches!(setters_for("x-cinnamon")[0], Setter::Cinnamon));
        assert!(matches!(setters_for("xfce")[0], Setter::Xfce));
        assert!(matches!(setters_for("hyprland")[0], Setter::Hyprland));
    }

    #[test]
    fn unknown_desktop_uses_generic_tools() {
        assert!(matches!(setters_for("i3")[0], Setter::Swww));
        assert_eq!(setters_for("").len(), 4);
    }
}
