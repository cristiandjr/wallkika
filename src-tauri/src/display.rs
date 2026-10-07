use crate::{engine::Engine, error::Result, platform};
use serde::Serialize;
use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager, Monitor};

pub const DISPLAYS_CHANGED: &str = "displays-changed";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub primary: bool,
}

impl DisplayInfo {
    fn from_monitor(index: usize, monitor: &Monitor, primary: Option<&Monitor>) -> Self {
        let position = monitor.position();
        let size = monitor.size();
        Self {
            name: monitor
                .name()
                .cloned()
                .unwrap_or_else(|| format!("Display {}", index + 1)),
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            scale_factor: monitor.scale_factor(),
            primary: primary.is_some_and(|p| p.position() == position && p.size() == size),
        }
    }

    // Each display must use its own scale: with mixed DPI a shared scale puts windows on the wrong screen.
    pub fn logical_rect(&self) -> (f64, f64, f64, f64) {
        let scale = self.scale_factor;
        (
            f64::from(self.x) / scale,
            f64::from(self.y) / scale,
            f64::from(self.width) / scale,
            f64::from(self.height) / scale,
        )
    }
}

pub fn list(app: &AppHandle) -> Result<Vec<DisplayInfo>> {
    let primary = app.primary_monitor()?;
    let mut displays: Vec<DisplayInfo> = app
        .available_monitors()?
        .iter()
        .enumerate()
        .map(|(index, monitor)| DisplayInfo::from_monitor(index, monitor, primary.as_ref()))
        .collect();
    platform::name_displays(app, &mut displays);
    Ok(displays)
}

pub fn spawn_watcher(app: AppHandle) {
    let spawned = thread::Builder::new()
        .name("wallkika-displays".into())
        .spawn(move || {
            let mut last: Option<Vec<DisplayInfo>> = None;
            loop {
                thread::sleep(Duration::from_secs(2));
                let displays = match list(&app) {
                    Ok(displays) if displays.is_empty() => continue,
                    Ok(displays) => displays,
                    Err(err) => {
                        log::warn!("Could not list displays: {err}");
                        continue;
                    }
                };
                if last.as_ref() != Some(&displays) {
                    log::info!("Displays: {}", summary(&displays));
                    if last.is_some() {
                        let _ = app.emit(DISPLAYS_CHANGED, &displays);
                    }
                    last = Some(displays.clone());
                }
                if let Err(err) = app.state::<Engine>().sync_live_windows(&app, &displays) {
                    log::warn!("Could not sync wallpaper windows: {err}");
                }
            }
        });
    if let Err(err) = spawned {
        log::error!("Could not start the display watcher: {err}");
    }
}

fn summary(displays: &[DisplayInfo]) -> String {
    displays
        .iter()
        .map(|d| {
            let main = if d.primary { ", main" } else { "" };
            format!("{} {}x{} @{}x{main}", d.name, d.width, d.height, d.scale_factor)
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_rect_uses_each_display_scale() {
        let retina = DisplayInfo {
            name: "Color LCD".into(),
            x: -2880,
            y: 360,
            width: 2880,
            height: 1800,
            scale_factor: 2.0,
            primary: false,
        };
        assert_eq!(retina.logical_rect(), (-1440.0, 180.0, 1440.0, 900.0));
    }
}
