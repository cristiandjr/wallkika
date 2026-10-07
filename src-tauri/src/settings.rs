use crate::layout::{Layout, Wallpaper};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub layout: Layout,
    #[serde(skip_serializing)]
    current: Option<Wallpaper>,
}

impl Settings {
    pub fn new(layout: Layout) -> Self {
        Self {
            layout,
            current: None,
        }
    }

    pub fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let mut settings: Self = serde_json::from_str(&text).unwrap_or_else(|err| {
            log::warn!("Ignoring unreadable {}: {err}", path.display());
            Self::default()
        });
        if let Some(legacy) = settings.current.take() {
            if settings.layout == Layout::default() {
                settings.layout.all = Some(legacy);
            }
        }
        settings
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        layout::{LayoutMode, RenderMode},
        media::MediaKind,
    };
    use std::path::PathBuf;

    fn temp_dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wallkika-{}-{test}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn video() -> Wallpaper {
        Wallpaper {
            path: PathBuf::from("/wallpapers/matrix.mp4"),
            name: "matrix.mp4".into(),
            kind: MediaKind::Video,
            mode: RenderMode::Live,
        }
    }

    #[test]
    fn roundtrip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("settings.json");
        let mut layout = Layout {
            mode: LayoutMode::PerDisplay,
            ..Layout::default()
        };
        layout.displays.insert("display-a".into(), video());
        let settings = Settings::new(layout);
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn migrates_the_v0_1_format() {
        let dir = temp_dir("migrate");
        let path = dir.join("settings.json");
        let legacy = r#"{ "current": { "path": "/wallpapers/matrix.mp4", "name": "matrix.mp4", "kind": "video", "mode": "live" } }"#;
        fs::write(&path, legacy).unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.layout.mode, LayoutMode::Mirror);
        assert_eq!(settings.layout.all, Some(video()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_or_corrupt_file_falls_back_to_default() {
        let dir = temp_dir("fallback");
        assert_eq!(
            Settings::load(&dir.join("missing.json")),
            Settings::default()
        );

        let path = dir.join("corrupt.json");
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        fs::remove_dir_all(dir).unwrap();
    }
}
