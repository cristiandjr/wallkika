use crate::{display::DisplayInfo, media::MediaKind};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

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

impl Wallpaper {
    pub fn is_live(&self) -> bool {
        self.mode == RenderMode::Live
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayoutMode {
    #[default]
    Mirror,
    PerDisplay,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Layout {
    pub mode: LayoutMode,
    pub all: Option<Wallpaper>,
    pub displays: BTreeMap<String, Wallpaper>,
}

impl Layout {
    pub fn live_for(&self, display_id: &str) -> Option<&Wallpaper> {
        match self.mode {
            LayoutMode::Mirror => self.all.as_ref().filter(|w| w.is_live()),
            LayoutMode::PerDisplay => self.displays.get(display_id),
        }
    }

    pub fn live_displays(&self, displays: &[DisplayInfo]) -> Vec<DisplayInfo> {
        displays
            .iter()
            .filter(|d| self.live_for(&d.id).is_some())
            .cloned()
            .collect()
    }

    pub fn switch_to(&mut self, mode: LayoutMode, displays: &[DisplayInfo]) {
        if mode == LayoutMode::PerDisplay && self.displays.is_empty() {
            if let Some(all) = self.all.clone().filter(Wallpaper::is_live) {
                for display in displays {
                    self.displays.insert(display.id.clone(), all.clone());
                }
            }
        }
        self.mode = mode;
    }

    pub fn clear_live(&mut self) {
        if self.all.as_ref().is_some_and(Wallpaper::is_live) {
            self.all = None;
        }
        self.displays.clear();
    }

    pub fn wallpapers(&self) -> impl Iterator<Item = &Wallpaper> {
        self.all.iter().chain(self.displays.values())
    }

    pub fn forget_missing_files(&mut self) -> bool {
        let before = self.wallpapers().count();
        if self.all.as_ref().is_some_and(|w| !w.path.is_file()) {
            self.all = None;
        }
        self.displays.retain(|_, w| w.path.is_file());
        before != self.wallpapers().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: &str) -> DisplayInfo {
        DisplayInfo {
            id: id.into(),
            name: id.into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
            primary: false,
        }
    }

    fn wallpaper(name: &str, mode: RenderMode) -> Wallpaper {
        Wallpaper {
            path: PathBuf::from(format!("/wallpapers/{name}")),
            name: name.into(),
            kind: MediaKind::Video,
            mode,
        }
    }

    #[test]
    fn mirror_shows_the_live_wallpaper_everywhere() {
        let layout = Layout {
            all: Some(wallpaper("matrix.mp4", RenderMode::Live)),
            ..Layout::default()
        };
        let displays = [display("a"), display("b")];
        assert_eq!(layout.live_displays(&displays).len(), 2);
        assert_eq!(layout.live_for("b").unwrap().name, "matrix.mp4");
    }

    #[test]
    fn mirror_with_a_native_image_needs_no_windows() {
        let layout = Layout {
            all: Some(wallpaper("photo.jpg", RenderMode::Native)),
            ..Layout::default()
        };
        assert!(layout.live_displays(&[display("a")]).is_empty());
    }

    #[test]
    fn per_display_only_covers_assigned_displays() {
        let mut layout = Layout {
            mode: LayoutMode::PerDisplay,
            ..Layout::default()
        };
        layout
            .displays
            .insert("b".into(), wallpaper("clock.html", RenderMode::Live));
        let live = layout.live_displays(&[display("a"), display("b")]);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id, "b");
        assert!(layout.live_for("a").is_none());
    }

    #[test]
    fn switching_to_per_display_keeps_the_live_wallpaper_on_every_display() {
        let mut layout = Layout {
            all: Some(wallpaper("matrix.mp4", RenderMode::Live)),
            ..Layout::default()
        };
        layout.switch_to(LayoutMode::PerDisplay, &[display("a"), display("b")]);
        assert_eq!(layout.displays.len(), 2);
        assert_eq!(layout.live_for("a").unwrap().name, "matrix.mp4");
    }

    #[test]
    fn switching_to_per_display_with_a_native_image_starts_empty() {
        let mut layout = Layout {
            all: Some(wallpaper("photo.jpg", RenderMode::Native)),
            ..Layout::default()
        };
        layout.switch_to(LayoutMode::PerDisplay, &[display("a")]);
        assert!(layout.displays.is_empty());
    }

    #[test]
    fn clear_live_keeps_native_images() {
        let mut layout = Layout {
            all: Some(wallpaper("photo.jpg", RenderMode::Native)),
            ..Layout::default()
        };
        layout
            .displays
            .insert("a".into(), wallpaper("matrix.mp4", RenderMode::Live));
        layout.clear_live();
        assert!(layout.all.is_some());
        assert!(layout.displays.is_empty());
    }
}
